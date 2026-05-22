//! GICv3 (Generic Interrupt Controller v3) 初始化
//!
//! QEMU virt 机器使用 GICv3。本模块提供:
//!   - GIC 初始化 (GICD + GICR 基础配置)
//!   - Timer 中断使能
//!   - IRQ ACK/EOI 处理

use core::ptr::{read_volatile, write_volatile};

// ============================================================================
// GICv3 寄存器地址 (QEMU virt)
// ============================================================================

/// Distributor 基地址 (每个CPU共享)
const GICD_BASE: u64 = 0x08000000;
/// Redistributor 基地址 (每个CPU独立)
const GICR_BASE: u64 = 0x080A0000;

/// GICD 寄存器偏移
const GICD_CTLR: u64 = 0x0000;      // Distributor Control
const GICD_TYPER: u64 = 0x0008;     // Type
const GICD_IIDR: u64 = 0x000C;      // Implementer ID
const GICD_IGROUPR: u64 = 0x0080;   // Interrupt Group (0-31)
const GICD_ISENABLER: u64 = 0x0100; // Interrupt Set-Enable (0-31)
const GICD_ISPENDR: u64 = 0x0200;   // Interrupt Set-Pending
const GICD_IPRIORITYR: u64 = 0x0400; // Interrupt Priority (8-bit each)
const GICD_ITARGETSR: u64 = 0x0800;  // Interrupt Target
const GICD_ICFGR: u64 = 0x0C00;      // Interrupt Configuration (level/edge)

/// GICR 寄存器偏移 (SGI + PPI)
const GICR_CTLR: u64 = 0x0000;      // Redistributor Control
const GICR_WAKER: u64 = 0x0014;     // Wake
const GICR_IGROUPR0: u64 = 0x0080;  // Group for SGIs/PPIs
const GICR_ISENABLER0: u64 = 0x0100; // Enable for SGIs/PPIs
const GICR_IPRIORITYR: u64 = 0x0400; // Priority for SGIs/PPIs
const GICR_ICFGR1: u64 = 0x0C04;    // Configuration for PPIs

/// CPU Interface 寄存器 (系统寄存器, ICC_*)
/// 通过 MRS/MSR 访问

// SPIs 范围
const PPI_BASE: u32 = 16;
const SPI_BASE: u32 = 32;

/// ARM 架构定时器 PPI
const TIMER_PPI: u32 = 27;  // EL1 Physical Timer

// ============================================================================
// 寄存器读写辅助
// ============================================================================

#[inline(always)]
unsafe fn gicd_read(offset: u64) -> u32 {
    read_volatile((GICD_BASE + offset) as *const u32)
}

#[inline(always)]
unsafe fn gicd_write(offset: u64, val: u32) {
    write_volatile((GICD_BASE + offset) as *mut u32, val);
}

#[inline(always)]
unsafe fn gicr_read(offset: u64) -> u32 {
    read_volatile((GICR_BASE + offset) as *const u32)
}

#[inline(always)]
unsafe fn gicr_write(offset: u64, val: u32) {
    write_volatile((GICR_BASE + offset) as *mut u32, val);
}

// ============================================================================
// GICv3 初始化
// ============================================================================

/// 初始化 GICv3 Distributor:
/// 1. 禁用所有中断
/// 2. 配置中断优先级 (全默认 0xA0)
/// 3. 使能 Distributor
/// 4. 使能 CPU Interface
pub unsafe fn init_distributor() {
    // 1. 禁用 Distributor
    gicd_write(GICD_CTLR, 0);

    // 2. 设置所有 SPIs 为 Group 0 (安全世界) — 跳过 SGIs/PPIs
    // GICD_IGROUPR: 32bits per register, 32 interrupts. We need regs for SPI_BASE..max
    // For simplicity, just configure the first 32+32 interrupts
    for i in 0..2 {
        // SPI_BASE(32) → register at GICD_IGROUPR + (SPI_BASE/32)*4
        // Register 0: SGIs+PPIs(0-31), Register 1: SPIs(32-63)
        gicd_write(GICD_IGROUPR + (i as u64 * 4), 0);
    }

    // 3. 设置中断优先级: 所有为 0xA0 (lowest priority = 0xFF, but we use 0x80 as default)
    // GICD_IPRIORITYR: 1 byte per interrupt, 4 per register
    // Just set first few registers
    for i in 0..32 {
        gicd_write(GICD_IPRIORITYR + (i as u64 * 4), 0xA0A0_A0A0);
    }

    // 4. 禁所有中断 (ICENABLER)
    // All interrupts start disabled; ISENABLER enables specific ones

    // 5. 设置 SPIs 为 level-sensitive (default)
    // GICD_ICFGR: 2 bits per interrupt, 16 per register
    // PPI 0-15 are edge-triggered by default
    // Timer PPI (27) should be level-sensitive (set to 0 in ICFGR1)
    // Actually, ARM Generic Timer uses level-sensitive interrupts in GICv3

    // 6. 使能 Distributor
    gicd_write(GICD_CTLR, 0x3); // Group0 + Group1 enable (bits 1:0)

    // 7. 设置 CPU interface target: all PPIs to CPU0
    // GICD_ITARGETSR: 1 byte per interrupt, 4 per register
    gicd_write(GICD_ITARGETSR, 0x0101_0101); // interrupt 0-3 → CPU0
    gicd_write(GICD_ITARGETSR + 4, 0x0101_0101); // interrupt 4-7 → CPU0
}

/// 初始化 GICv3 Redistributor (当前 CPU):
/// 1. Wake redistributor
/// 2. Enable SGIs/PPIs for current core
pub unsafe fn init_redistributor() {
    // 1. Wake redistributor
    let waker = gicr_read(GICR_WAKER);
    gicr_write(GICR_WAKER, waker & !(1 << 1)); // Clear ProcessorSleep (bit 1)
    // Wait for ChildrenAsleep == 0
    while gicr_read(GICR_WAKER) & (1 << 2) != 0 {
        core::hint::spin_loop();
    }

    // 2. 设置 PPI 优先级
    gicr_write(GICR_IPRIORITYR, 0xA0A0_A0A0);
    gicr_write(GICR_IPRIORITYR + 4, 0xA0A0_A0A0);
    gicr_write(GICR_IPRIORITYR + 8, 0xA0A0_A0A0); // Timer PPI is at PPI_BASE+11=27, byte 3 of reg 6

    // 3. Timer PPI 低优先级
    let prio_addr = GICR_IPRIORITYR + ((TIMER_PPI as u64 / 4) * 4);
    let prio = gicr_read(prio_addr);
    let shift = ((TIMER_PPI % 4) * 8) as u64;
    gicr_write(prio_addr, (prio & !(0xFF << shift)) | (0x40 << shift));

    // 4. Enable redistributor
    gicr_write(GICR_CTLR, 0x1); // Enable
}

/// 使能 CPU Interface (ICC_* 系统寄存器)
pub unsafe fn init_cpu_interface() {
    // 设置中断优先级掩码 (PMR): 允许所有优先级
    core::arch::asm!("msr icc_pmr_el1, {}", in(reg) 0xFFu64);

    // 设置 Binary Point (BPR1): 无优先级分组
    core::arch::asm!("msr icc_bpr1_el1, {}", in(reg) 0u64);

    // 启用 Group 0 + Group 1 中断
    // ICC_IGRPEN0_EL1: Group 0 enable
    core::arch::asm!("msr icc_igrpen0_el1, {}", in(reg) 1u64);
    // ICC_IGRPEN1_EL1: Group 1 enable  
    core::arch::asm!("msr icc_igrpen1_el1, {}", in(reg) 1u64);

    // EOI mode: drop priority (ICC_CTLR_EL1.EOImode = 0)
    let ctlr: u64;
    core::arch::asm!("mrs {}, icc_ctlr_el1", out(reg) ctlr);
    core::arch::asm!("msr icc_ctlr_el1, {}", in(reg) ctlr & !(1 << 1));
}

/// 使能 Timer PPI 中断
pub unsafe fn enable_timer_ppi() {
    // 1. 在 redistributor 中使能 PPI 27 (Timer)
    let enable_offset = GICR_ISENABLER0; // SGIs + PPIs 0-31
    let bit = 1u32 << (TIMER_PPI % 32);
    gicr_write(enable_offset, bit);

    // 2. 在 distributor 中确保不使能 SPI (timer PPI 不在 distributor ISENABLER 中)
    // Timer PPI is managed by redistributor, not distributor.
}

/// 获取中断 ID (IAR) — 用于 IRQ handler
pub fn acknowledge() -> u32 {
    let iar: u64;
    unsafe { core::arch::asm!("mrs {}, icc_iar1_el1", out(reg) iar); }
    iar as u32
}

/// 中断完成 (EOI)
pub fn end_of_interrupt(intid: u32) {
    unsafe {
        core::arch::asm!("msr icc_eoir1_el1, {}", in(reg) intid as u64);
    }
}

/// 发送 EOI 并解除优先级 (drop priority)
pub fn deactivate(intid: u32) {
    unsafe {
        core::arch::asm!("msr icc_dir_el1, {}", in(reg) intid as u64);
    }
}

/// 完整 GIC 初始化流程
pub unsafe fn init() {
    init_distributor();
    init_redistributor();
    init_cpu_interface();
    enable_timer_ppi();
}