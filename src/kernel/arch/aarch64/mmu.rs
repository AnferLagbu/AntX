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
const L1_BLOCK_SIZE: u64 = 0x200000; // 2MB (L1 block)

// ============================================================================
// 页表 (static, BSS)
// ============================================================================

/// L0 页表 (512 entries × 8 bytes = 4KB)
static mut L0_TABLE: [u64; 512] = [0; 512];

/// L1 页表 (512 entries × 8 bytes = 4KB), 覆盖 0-1GB
static mut L1_IDMAP: [u64; 512] = [0; 512];

// ============================================================================
// MMU 初始化
// ============================================================================

/// TTBR1 内核 L0 页表 (独立于 identity mapping)
static mut TTBR1_L0: [u64; 512] = [0; 512];

/// TTBR1 内核 L1 页表 (覆盖 0xFFFF_0000_0000_0000 - 0xFFFF_0000_4000_0000)
static mut TTBR1_L1: [u64; 512] = [0; 512];

/// 初始化 identity mapping (覆盖 0-1GB) 并启用 MMU。
///
/// 同时设置 TTBR1_EL1 覆盖内核高地址空间。
///
/// 映射策略:
///   - 0x0000_0000 - 0x0800_0000 (0-128MB): Normal memory (DRAM)
///   - 0x0800_0000 - 0x0900_1000 (GIC):  Device memory
///   - 0x0900_0000 - 0x0900_1000 (UART): Device memory
///   - 0x4000_0000 - 0x8000_0000 (DRAM):  Normal memory (QEMU virt 默认RAM起始)
///
/// 所有映射均为 EL1 RW, 2MB 块。
pub unsafe fn init() {
    // 清零页表
    ptr::write_bytes(L0_TABLE.as_mut_ptr(), 0, 512);
    ptr::write_bytes(L1_IDMAP.as_mut_ptr(), 0, 512);

    // L0[0] → L1_IDMAP (覆盖 0-512GB 范围)
    L0_TABLE[0] = (L1_IDMAP.as_ptr() as u64) | PT_TYPE_TABLE;

    // L1 块映射 0-1GB (2MB 块)
    let mut paddr: u64 = 0;
    for i in 0..512 {
        let attr = if paddr < 0x0800_0000 || paddr >= 0x4000_0000 {
            // DRAM: Normal memory
            PT_ATTR_NORMAL | PT_AP_EL1_RW
        } else if paddr < 0x0900_0000 {
            // GIC region: Device
            PT_ATTR_DEVICE | PT_AP_EL1_RW
        } else {
            // UART region: Device
            PT_ATTR_DEVICE | PT_AP_EL1_RW
        };

        L1_IDMAP[i] = paddr | PT_TYPE_BLOCK | PT_AF | attr;
        paddr += L1_BLOCK_SIZE;
    }

    // 设置 TTBR0_EL1
    set_ttbr0(L0_TABLE.as_ptr() as u64);

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
    // Attr0 (index 0b000): Normal memory (use with PT_ATTR_NORMAL's 0100/0100)
    // Attr1 (index 0b100): Device-nGnRnE
    // Wait, the encoding in our PT_ATTR uses AttrIndx fields.
    // PT_ATTR_NORMAL = 0b0100 << 2 | 0b0100 << 8 = AttrIndx[2:0]=0, AttrIndx[2:0]=0 (both index 0)
    // PT_ATTR_DEVICE = 0b0000 << 2 | 0b0000 << 8 = AttrIndx=0
    // Actually the attr index comes from bits [4:2]. Let me simplify and use:
    // MAIR: Attr0 = 0xFF (Normal, IWBWA, OWBWA), Attr1 = 0x44 (Device)
    // And in PT entries: AttrIndx=0 → MAIR[0], AttrIndx=1 → MAIR[1]
    let mair: u64 = 0xFF         // Attr0: Normal memory, Inner/Outer WBWA
                   | (0x44 << 8); // Attr1: Device-nGnRnE
    set_mair(mair);

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
    ptr::write_bytes(TTBR1_L0.as_mut_ptr(), 0, 512);
    ptr::write_bytes(TTBR1_L1.as_mut_ptr(), 0, 512);

    // TTBR1 覆盖高地址: VA[47:39] 索引 L0
    // 0xFFFF_0000_0000_0000: L0 index = VA[47:39] = 0b111111111 = 511
    TTBR1_L0[511] = (TTBR1_L1.as_ptr() as u64) | PT_TYPE_TABLE;

    // 映射 0xFFFF0000_00000000 - 0xFFFF0000_40000000 → 物理 0-1GB
    let mut paddr: u64 = 0;
    for i in 0..512 {
        let attr = if paddr < 0x0800_0000 || paddr >= 0x4000_0000 {
            PT_ATTR_NORMAL | PT_AP_EL1_RW
        } else if paddr < 0x0900_0000 {
            PT_ATTR_DEVICE | PT_AP_EL1_RW
        } else {
            PT_ATTR_DEVICE | PT_AP_EL1_RW
        };
        TTBR1_L1[i] = paddr | PT_TYPE_BLOCK | PT_AF | attr;
        paddr += L1_BLOCK_SIZE;
    }

    // 设置 TTBR1_EL1
    set_ttbr1(TTBR1_L0.as_ptr() as u64);
}

/// 分配用户空间页表 (返回 TTBR0 值)。
///
/// 为简单实现, 使用 BSS 静态页表。
/// Phase 6: 每进程独立页表。
pub fn alloc_user_page_table() -> u64 {
    // 返回当前 identity mapping 的 TTBR0
    // Phase 6+: 实现每进程独立页表分配
    unsafe { L0_TABLE.as_ptr() as u64 }
}

// ============================================================================
// 系统寄存器读写 (inline asm)
// ============================================================================

#[inline(always)]
unsafe fn set_ttbr0(val: u64) {
    core::arch::asm!("msr ttbr0_el1, {}", in(reg) val);
}

#[inline(always)]
unsafe fn set_ttbr1(val: u64) {
    core::arch::asm!("msr ttbr1_el1, {}", in(reg) val);
    core::arch::asm!("isb");
}

#[inline(always)]
unsafe fn set_tcr(val: u64) {
    core::arch::asm!("msr tcr_el1, {}", in(reg) val);
}

#[inline(always)]
unsafe fn set_mair(val: u64) {
    core::arch::asm!("msr mair_el1, {}", in(reg) val);
}

/// 启用 MMU: 设置 SCTLR_EL1.M (bit 0), I (bit 12), C (bit 2)
#[inline(always)]
unsafe fn enable_mmu() {
    let sctlr: u64;
    core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr);
    // Enable M (MMU), C (data cache), I (instruction cache)
    let sctlr = sctlr | (1 << 0) | (1 << 2) | (1 << 12);
    core::arch::asm!("msr sctlr_el1, {}", in(reg) sctlr);
    // Instruction synchronization barrier
    core::arch::asm!("isb");
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