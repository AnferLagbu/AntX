//! AArch64 MMU / 页表管理
//!
//! 双地址空间:
//!   - TTBR0_EL1: 用户空间 (0x0000_0000_0000_0000 - 0x0000_FFFF_FFFF_FFFF)
//!   - TTBR1_EL1: 内核空间 (0xFFFF_0000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF)
//!
//! 使用 4KB 页粒度, 48-bit VA, 3-level 或 2-level 页表。
//!
//! QEMU virt 内存布局:
//!   - DRAM: 0x40000000 - ?
//!   - UART: 0x09000000
//!   - GIC:  0x08000000

use core::ptr;

// ============================================================================
// 页表常量 (ARMv8-A 4KB granule)
// ============================================================================

/// 页表条目常量
const PT_TYPE_TABLE: u64 = 0x3; // valid + table descriptor
const PT_TYPE_BLOCK: u64 = 0x1; // valid + block descriptor
const PT_AF: u64 = 1 << 10; // Access Flag
const PT_ATTR_NORMAL: u64 = (0b0100 << 2) | (0b0100 << 8); // Normal memory, Inner/Outer WBWA
const PT_ATTR_DEVICE: u64 = (0b0000 << 2) | (0b0000 << 8); // Device-nGnRnE memory
const PT_AP_EL1_RW: u64 = 0 << 6; // EL1 read/write
const PT_AP_ALL_RW: u64 = 1 << 6; // EL1+EL0 read/write

/// 页粒度
const PAGE_SIZE: u64 = 4096;
const L2_BLOCK_SIZE: u64 = 0x200000; // 2MB (L2 block at 4KB granule)

// ============================================================================
// 页表 (static, BSS)
// ============================================================================

/// 4KB 对齐的页表数组 (ARM 要求页表 4KB 对齐)
#[repr(align(4096))]
pub struct AlignedPageTable([u64; 512]);

/// L0 页表 (512 entries × 8 bytes = 4KB)
static mut L0_TABLE: AlignedPageTable = AlignedPageTable([0; 512]);

/// L1 页表 (512 entries × 8 bytes = 4KB), 覆盖 0-2GB (L1 块描述符, 每项 1GB)
static mut L1_IDMAP: AlignedPageTable = AlignedPageTable([0; 512]);

/// L2 页表 (512 entries × 8 bytes = 4KB), 覆盖 0-1GB (L2 块描述符, 每项 2MB)
/// 用于以 Device memory 属性映射 GIC/UART 等 MMIO 区域
static mut L2_DEVICE: AlignedPageTable = AlignedPageTable([0; 512]);

// ============================================================================
// MMU 初始化
// ============================================================================

/// TTBR1 内核 L0 页表 (独立于 identity mapping)
static mut TTBR1_L0: AlignedPageTable = AlignedPageTable([0; 512]);

/// TTBR1 内核 L1 页表 (覆盖 0xFFFF_0000_0000_0000 - 0xFFFF_0000_8000_0000, 2GB)
static mut TTBR1_L1: AlignedPageTable = AlignedPageTable([0; 512]);

/// 初始化 identity mapping (覆盖 0-2GB) 并启用 MMU。
///
/// 同时设置 TTBR1_EL1 覆盖内核高地址空间 (2GB)。
///
/// 页表层级 (4KB 粒度, 48-bit VA, 4-level):
///   L0[0] → L1_IDMAP
///   
/// L1[0] → L2_DEVICE (2MB 粒度, 全部 Device memory — QEMU virt 低 1GB 无 DRAM)
/// L1[1] → 1GB block: PA 0x40000000 (DRAM, kernel @ 0x40080000, Normal memory)
///
/// 所有映射均为 EL1 RW。
pub unsafe fn init() {
    // 清零页表
    ptr::write_bytes(L0_TABLE.0.as_mut_ptr(), 0, 512);
    ptr::write_bytes(L1_IDMAP.0.as_mut_ptr(), 0, 512);
    ptr::write_bytes(L2_DEVICE.0.as_mut_ptr(), 0, 512);

    // L0[0] → L1_IDMAP (L0 每项覆盖 512GB, 这里只需一个)
    L0_TABLE.0[0] = (L1_IDMAP.0.as_ptr() as u64) | PT_TYPE_TABLE;

    // L1[0] → L2_DEVICE: 0-1GB 用 L2 2MB 块映射, Device memory 属性
    //   因为 QEMU virt 低 1GB 无 DRAM (只有 GIC@0x08000000, UART@0x09000000),
    //   以 Normal cacheable 属性访问 MMIO 会导致数据异常/挂死。
    L1_IDMAP.0[0] = (L2_DEVICE.0.as_ptr() as u64) | PT_TYPE_TABLE;

    // L2_DEVICE: 512 个 2MB Device 块, 覆盖 0x00000000 - 0x40000000
    for i in 0..512 {
        let pa = (i as u64) * L2_BLOCK_SIZE;
        L2_DEVICE.0[i] = pa | PT_TYPE_BLOCK | PT_AF | PT_ATTR_DEVICE | PT_AP_EL1_RW;
    }

    // L1[1]: VA 1-2GB → PA 1-2GB (DRAM, kernel @ 0x40080000, Normal memory)
    L1_IDMAP.0[1] = 0x40000000 | PT_TYPE_BLOCK | PT_AF | PT_ATTR_NORMAL | PT_AP_EL1_RW;

    // 设置 TTBR0_EL1
    set_ttbr0(L0_TABLE.0.as_ptr() as u64);

    // 设置 TCR_EL1 (Translation Control Register)
    // T0SZ=16 (48-bit IPA for TTBR0), T1SZ=16 (48-bit IPA for TTBR1)
    // 4KB granule (TG0=00, TG1=10), Inner Shareable, Normal cacheable
    let tcr: u64 = (16u64 << 0)    // T0SZ: 64 - 48 = 16
                  | (16u64 << 16)   // T1SZ: 64 - 48 = 16
                  | (0b00 << 14)    // TG0: 4KB
                  | (0b10 << 30)    // TG1: 4KB
                  | (0b11 << 12)    // SH0: Inner Shareable
                  | (0b11 << 28)    // SH1: Inner Shareable
                  | (0b01 << 10)    // ORGN0: Normal, WB, RA, WA
                  | (0b01 << 26)    // ORGN1: Normal, WB, RA, WA
                  | (0b01 << 8)     // IRGN0: Normal, WB, RA, WA
                  | (0b01 << 24);   // IRGN1: Normal, WB, RA, WA
    set_tcr(tcr);

    // 设置 MAIR_EL1 (Memory Attribute Indirection Register)
    // PT_ATTR_NORMAL = (0b0100<<2)|(0b0100<<8) → AttrIndx=4
    // PT_ATTR_DEVICE = (0b0000<<2)|(0b0000<<8) → AttrIndx=0
    // MAIR[0] = 0x44 (Device-nGnRnE, for PT_ATTR_DEVICE)
    // MAIR[4] = 0xFF (Normal IWBWA OWBWA, for PT_ATTR_NORMAL)
    let mair: u64 = 0x44                     // Attr0: Device
                   | (0xFFu64 << 32);        // Attr4: Normal
    set_mair(mair);

    // 使之前的所有页表写入和系统寄存器设置可见
    core::arch::asm!("dsb sy");
    core::arch::asm!("isb");

    // 清空 TLB (确保旧映射不影响新表)
    core::arch::asm!("tlbi vmalle1");
    core::arch::asm!("dsb sy");
    core::arch::asm!("isb");

    // 启用 MMU
    enable_mmu();

    // 设置 TTBR1 内核页表 (映射高地址 → 物理地址)
    init_kernel_ttbr1();
}

/// 初始化 TTBR1_EL1 内核页表。
///
/// 将 0xFFFF_0000_0000_0000 - 0xFFFF_0000_4000_0000 (1GB) 映射到
/// 物理地址 0x0000_0000 - 0x4000_0000 (1GB)。
///
/// TTBR1 页表层级:
///   TTBR1_L0[511] → TTBR1_L1
///   TTBR1_L1[i]  → 2MB block mapping (i * 2MB → i * 2MB physical)
unsafe fn init_kernel_ttbr1() {
    ptr::write_bytes(TTBR1_L0.0.as_mut_ptr(), 0, 512);
    ptr::write_bytes(TTBR1_L1.0.as_mut_ptr(), 0, 512);

    // TTBR1 覆盖高地址: VA[47:39] 索引 L0
    // 0xFFFF_0000_0000_0000: L0 index = VA[47:39] = 0b111111111 = 511
    TTBR1_L0.0[511] = (TTBR1_L1.0.as_ptr() as u64) | PT_TYPE_TABLE;

    // L1 块映射 (每项 1GB, 4KB 粒度):
    // L1[0]: VA 0xFFFF_0000_0000_0000 → PA 0x00000000 (0-1GB)
    // L1[1]: VA 0xFFFF_0000_4000_0000 → PA 0x40000000 (1-2GB, kernel @ 0x40080000)
    TTBR1_L1.0[0] = 0x00000000 | PT_TYPE_BLOCK | PT_AF | PT_ATTR_NORMAL | PT_AP_EL1_RW;
    TTBR1_L1.0[1] = 0x40000000 | PT_TYPE_BLOCK | PT_AF | PT_ATTR_NORMAL | PT_AP_EL1_RW;

    // 设置 TTBR1_EL1
    set_ttbr1(TTBR1_L0.0.as_ptr() as u64);
}

/// 分配用户空间页表 (返回 TTBR0 值)。
///
/// 为简单实现, 使用 BSS 静态页表。
/// Phase 6: 每进程独立页表。
pub fn alloc_user_page_table() -> u64 {
    // 返回当前 identity mapping 的 TTBR0
    // Phase 6+: 实现每进程独立页表分配
    unsafe { L0_TABLE.0.as_ptr() as u64 }
}

// ============================================================================
// 系统寄存器读写 (inline asm)
// ============================================================================

#[inline(always)]
unsafe fn set_ttbr0(val: u64) {
    core::arch::asm!("msr ttbr0_el1, {}", in(reg) val);
    core::arch::asm!("isb");
}

#[inline(always)]
unsafe fn set_ttbr1(val: u64) {
    core::arch::asm!("msr ttbr1_el1, {}", in(reg) val);
    core::arch::asm!("isb");
}

#[inline(always)]
unsafe fn set_tcr(val: u64) {
    core::arch::asm!("msr tcr_el1, {}", in(reg) val);
    core::arch::asm!("isb");
}

#[inline(always)]
unsafe fn set_mair(val: u64) {
    core::arch::asm!("msr mair_el1, {}", in(reg) val);
    core::arch::asm!("isb");
}

/// 启用 MMU: 设置 SCTLR_EL1.M (bit 0)
/// 暂不启用缓存 (C bit 2, I bit 12), 后续单独处理
#[inline(never)]
unsafe fn enable_mmu() {
    // ARM ARM D5.10.2: 启用 MMU 前需要 DSB 确保所有之前操作可见,
    // 启用后需要 ISB 使 MMU 对后续指令生效.
    core::arch::asm!(
        "dsb sy",
        "mrs x0, sctlr_el1",
        "orr x0, x0, #1",    // Set M bit
        "msr sctlr_el1, x0",
        "isb",
        out("x0") _,
    );
}

// ============================================================================
// Arch trait 辅助函数 (供 Arch impl 调用)
// ============================================================================

#[inline(always)]
pub fn read_ttbr0() -> u64 {
    let val: u64;
    unsafe { core::arch::asm!("mrs {}, ttbr0_el1", out(reg) val); }
    val
}

#[inline(always)]
pub fn write_ttbr0(val: u64) {
    unsafe { core::arch::asm!("msr ttbr0_el1, {}", in(reg) val); }
    unsafe { core::arch::asm!("isb", options(nomem, nostack)); }
}

#[inline(always)]
pub fn tlbi_vaae1(vaddr: u64) {
    unsafe {
        core::arch::asm!("dsb ishst", options(nomem, nostack));
        core::arch::asm!("tlbi vaae1, {}", in(reg) (vaddr >> 12));
        core::arch::asm!("dsb ish", options(nomem, nostack));
        core::arch::asm!("isb", options(nomem, nostack));
    }
}

#[inline(always)]
pub fn tlbi_vmalle1() {
    unsafe {
        core::arch::asm!("dsb ishst", options(nomem, nostack));
        core::arch::asm!("tlbi vmalle1", options(nomem, nostack));
        core::arch::asm!("dsb ish", options(nomem, nostack));
        core::arch::asm!("isb", options(nomem, nostack));
    }
}

#[inline(always)]
pub fn read_far() -> u64 {
    let val: u64;
    unsafe { core::arch::asm!("mrs {}, far_el1", out(reg) val); }
    val
}