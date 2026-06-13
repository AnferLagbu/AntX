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
/// Redistributor RD frame 基地址 (每个CPU独立)
const GICR_BASE: u64 = 0x080A_0000;
/// Redistributor SGI frame 基地址 (SGI/PPI registers)
const GICR_SGI_BASE: u64 = 0x080B_0000;

/// GICD 寄存器偏移
const GICD_CTLR: u64 = 0x0000; // Distributor Control
#[allow(dead_code)]
const GICD_TYPER: u64 = 0x0008; // Type
#[allow(dead_code)]
const GICD_IIDR: u64 = 0x000C; // Implementer ID
const GICD_IGROUPR: u64 = 0x0080; // Interrupt Group (0-31)
#[allow(dead_code)]
const GICD_ISENABLER: u64 = 0x0100; // Interrupt Set-Enable (0-31)
#[allow(dead_code)]
const GICD_ISPENDR: u64 = 0x0200; // Interrupt Set-Pending
const GICD_IPRIORITYR: u64 = 0x0400; // Interrupt Priority (8-bit each)
const GICD_ITARGETSR: u64 = 0x0800; // Interrupt Target
#[allow(dead_code)]
const GICD_ICFGR: u64 = 0x0C00; // Interrupt Configuration (level/edge)

/// GICR 寄存器偏移 (SGI + PPI)
const GICR_CTLR: u64 = 0x0000; // Redistributor Control
const GICR_WAKER: u64 = 0x0014; // Wake
#[allow(dead_code)]
const GICR_IGROUPR0: u64 = 0x0080; // Group for SGIs/PPIs
pub const GICR_ISENABLER0: u64 = 0x0100; // Enable for SGIs/PPIs
const GICR_IPRIORITYR: u64 = 0x0400; // Priority for SGIs/PPIs
#[allow(dead_code)]
const GICR_ICFGR1: u64 = 0x0C04; // Configuration for PPIs

/// CPU Interface 寄存器 (系统寄存器, ICC_*)
/// 通过 MRS/MSR 访问

// SPIs 范围
#[allow(dead_code)]
const PPI_BASE: u32 = 16;
#[allow(dead_code)]
const SPI_BASE: u32 = 32;

/// ARM 架构定时器 PPI (Non-secure Physical Timer)
const TIMER_PPI: u32 = 30; // CNTPNSIRQ

// ============================================================================
// 寄存器读写辅助
// ============================================================================

#[allow(dead_code)]
#[inline(always)]
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe fn gicd_read(offset: u64) -> u32 {
    core::arch::asm!("dsb sy");
    let val = read_volatile((GICD_BASE + offset) as *const u32);
    core::arch::asm!("dsb sy");
    val
}

#[inline(always)]
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe fn gicd_write(offset: u64, val: u32) {
    core::arch::asm!("dsb sy");
    write_volatile((GICD_BASE + offset) as *mut u32, val);
    core::arch::asm!("dsb sy");
}

#[inline(always)]
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe fn gicr_read(offset: u64) -> u32 {
    core::arch::asm!("dsb sy");
    let val = read_volatile((GICR_BASE + offset) as *const u32);
    core::arch::asm!("dsb sy");
    val
}

#[inline(always)]
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe fn gicr_write(offset: u64, val: u32) {
    core::arch::asm!("dsb sy");
    write_volatile((GICR_BASE + offset) as *mut u32, val);
    core::arch::asm!("dsb sy");
}

#[inline(always)]
/// 读取 GICv3 Redistributor SGI 帧寄存器。
///
/// # Safety
///
/// 调用者需确保 GICR_SGI_BASE (0x080B_0000) 已映射且 Redistributor 已唤醒。
pub unsafe fn gicr_sgi_read(offset: u64) -> u32 {
    core::arch::asm!("dsb sy");
    let val = read_volatile((GICR_SGI_BASE + offset) as *const u32);
    core::arch::asm!("dsb sy");
    val
}

#[inline(always)]
/// 写入 GICv3 Redistributor SGI 帧寄存器。
///
/// # Safety
///
/// 调用者需确保 GICR_SGI_BASE (0x080B_0000) 已映射且 Redistributor 已唤醒。
pub unsafe fn gicr_sgi_write(offset: u64, val: u32) {
    core::arch::asm!("dsb sy");
    write_volatile((GICR_SGI_BASE + offset) as *mut u32, val);
    core::arch::asm!("dsb sy");
}

// ============================================================================
// GICv3 初始化
// ============================================================================

/// 初始化 GICv3 Distributor:
/// 1. 禁用所有中断
/// 2. 配置中断优先级 (全默认 0xA0)
/// 3. 使能 Distributor
/// 4. 使能 CPU Interface
///
/// # Safety
///
/// 调用前需确保 GICD_BASE (0x08000000) 已正确映射，MMU 已启用。
pub unsafe fn init_distributor() {
    // 1. 禁用 Distributor
    gicd_write(GICD_CTLR, 0);

    // 2. 设置所有 SPIs 为 Group 0
    for i in 0..2 {
        gicd_write(GICD_IGROUPR + (i as u64 * 4), 0);
    }

    // 3. 设置中断优先级
    for i in 0..32 {
        gicd_write(GICD_IPRIORITYR + (i as u64 * 4), 0xA0A0_A0A0);
    }

    // 4. 使能 Distributor (Group0 + Group1)
    gicd_write(GICD_CTLR, 0x3);

    // 5. 设置 CPU interface target: PPIs to CPU0
    gicd_write(GICD_ITARGETSR, 0x0101_0101);
    gicd_write(GICD_ITARGETSR + 4, 0x0101_0101);
}

/// 初始化 GICv3 Redistributor (当前 CPU):
/// 1. 唤醒 redistributor
/// 2. 为当前核启用 SGIs/PPIs
///
/// # Safety
///
/// 调用前需确保 Distributor 已初始化，GICR_BASE 已映射。
pub unsafe fn init_redistributor() {
    // 1. 唤醒 redistributor
    let waker = gicr_read(GICR_WAKER);
    gicr_write(GICR_WAKER, waker & !(1 << 1)); // 清除 ProcessorSleep (bit 1)
                                               // 等待 ChildrenAsleep == 0
    while gicr_read(GICR_WAKER) & (1 << 2) != 0 {
        core::hint::spin_loop();
    }

    // 2. 设置 PPI 优先级 (SGI frame)
    gicr_sgi_write(GICR_IPRIORITYR, 0xA0A0_A0A0);
    gicr_sgi_write(GICR_IPRIORITYR + 4, 0xA0A0_A0A0);
    gicr_sgi_write(GICR_IPRIORITYR + 8, 0xA0A0_A0A0);

    // 3. Timer PPI 低优先级
    let prio_addr = GICR_IPRIORITYR + ((TIMER_PPI as u64 / 4) * 4);
    let prio = gicr_sgi_read(prio_addr);
    let shift = ((TIMER_PPI % 4) * 8) as u64;
    gicr_sgi_write(prio_addr, (prio & !(0xFF << shift)) | (0x40 << shift));

    // 4. Enable redistributor
    gicr_write(GICR_CTLR, 0x1); // Enable
}

/// 使能 CPU Interface (ICC_* 系统寄存器)
///
/// # Safety
///
/// 仅在 EL1 或更高特权级调用，需确保 Redistributor 已初始化。
pub unsafe fn init_cpu_interface() {
    // 设置中断优先级掩码 (PMR): 允许所有优先级
    core::arch::asm!("msr icc_pmr_el1, {}", in(reg) 0xFFu64);

    // 设置 Binary Point (BPR1): 无优先级分组
    core::arch::asm!("msr icc_bpr1_el1, {}", in(reg) 0u64);

    // 启用 Group 0 + Group 1 中断
    core::arch::asm!("msr icc_igrpen0_el1, {}", in(reg) 1u64);
    core::arch::asm!("msr icc_igrpen1_el1, {}", in(reg) 1u64);

    // EOI 模式: 直接降优先级 (ICC_CTLR_EL1.EOImode = 0)
    let ctlr: u64;
    core::arch::asm!("mrs {}, icc_ctlr_el1", out(reg) ctlr);
    core::arch::asm!("msr icc_ctlr_el1, {}", in(reg) ctlr & !(1 << 1));
}

/// 使能 Timer PPI 中断
///
/// # Safety
///
/// 调用前需确保 CPU Interface 已初始化。
pub unsafe fn enable_timer_ppi() {
    let enable_offset = GICR_ISENABLER0;
    let bit = 1u32 << (TIMER_PPI % 32);
    gicr_sgi_write(enable_offset, bit);
}

/// 获取中断 ID (IAR) — 用于 IRQ handler
pub fn acknowledge() -> u32 {
    let iar: u64;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!("mrs {}, icc_iar1_el1", out(reg) iar);
    }
    iar as u32
}

/// 中断完成 (EOI)
pub fn end_of_interrupt(intid: u32) {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!("msr icc_eoir1_el1, {}", in(reg) intid as u64);
    }
}

/// 发送 EOI 并解除优先级 (drop priority)
pub fn deactivate(intid: u32) {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!("msr icc_dir_el1, {}", in(reg) intid as u64);
    }
}

/// 完整 GIC 初始化流程
///
/// # Safety
///
/// 仅在启动阶段调用，需确保 MMU 已启用且 GIC MMIO 区域已映射。
pub unsafe fn init() {
    init_distributor();
    init_redistributor();
    init_cpu_interface();
    enable_timer_ppi();
}
