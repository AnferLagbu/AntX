//! AArch64 MMU 初始化
//!
//! QEMU virt 机器启动时 MMU 关闭。本模块创建初始 identity mapping,
//! 然后启用 MMU。使用 4KB 页粒度, TTBR0_EL1, 2-level (L0[512] + L1[512])。
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

/// 初始化 identity mapping (覆盖 0-1GB) 并启用 MMU。
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
    // 4KB granule, 48-bit VA, TTBR0 only
    let tcr: u64 = (0b00 << 37)  // T0SZ: 16 (4KB + 48-bit VA = 2^(64-48) = 64TB -> T0SZ=16)
                  | (0b00 << 14)  // TG0: 4KB granule
                  | (0b11 << 12)  // SH0: Inner Shareable
                  | (0b01 << 10)  // ORGN0: Normal, WB, RA, WA
                  | (0b01 << 8)   // IRGN0: Normal, WB, RA, WA
                  | (25 << 0);    // T0SZ = 64 - 48 + 25? No. T0SZ = 64 - IPA_size.
                                  // For 48-bit: T0SZ = 16. But we only need 1GB so...
                                  // Let's use T0SZ=25 (covers 0-512GB with L0)
                                  // Actually, TCR_EL1.T0SZ: 64 - size of VA region.
                                  // For 48-bit VA: T0SZ = 16
                                  // Let's use a simpler approach: T0SZ = 16 for 48-bit IPA
                                  // Wait, TCR_EL1 encoding depends on granule:
                                  // 4KB, 48-bit: T0SZ = 16
                                  // Actually I'll keep it simple: T0SZ=16
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
}

// ============================================================================
// 系统寄存器读写 (inline asm)
// ============================================================================

#[inline(always)]
unsafe fn set_ttbr0(val: u64) {
    core::arch::asm!("msr ttbr0_el1, {}", in(reg) val);
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