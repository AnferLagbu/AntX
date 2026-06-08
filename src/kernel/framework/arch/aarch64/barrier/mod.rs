//! AArch64 栏栈 (Barrier-Stack) 实现
//!
//! ## 架构等价
//!
//! | x86_64              | aarch64               |
//! |---------------------|------------------------|
//! | `int 0x82`          | SGI 7 (via ICC_SGI1R) |
//! | IDT entry 0x82      | IRQ handler intid==7  |
//! | `iretq` 恢复        | `eret` 恢复           |
//! | IST2 专用栈         | EL1h SP (独立栈)      |
//!
//! ## 两种触发场景
//!
//! 1. **运行时恢复** (`barrier_trigger_recovery()`):
//!    代码调用 → SGI 7 触发 → IRQ handler 执行恢复 → eret 回到调用点
//!
//! 2. **Panic 恢复** (panic_handler):
//!    直接调用 recovery_try_recover_from_panic() 进行域回滚,
//!    不回现场执行 (panic 不可恢复执行流).
//!
//! ## 在 IRQ handler 中的集成
//!
//! `irq_handler()` 检测 `intid == BARRIER_RECOVERY_SGI (7)`,
//! 调用 `barrier_sgi_handler()` 执行恢复.
//!
//! ## 依赖
//!
//! - GICv3 已初始化 (SGI 7 已使能)
//! - 栏栈域已注册 (PMM / PROC)
//! - `src/kernel/barrier/` 模块可用 (跨架构通用)

/// 栏栈恢复专用的 SGI ID (Software Generated Interrupt 7)
///
/// GICv3 定义 SGI ID 0-15 可用于 IPI, 此处选取 ID 7 作为栏栈恢复专用.
/// 需确保不与调度器 IPI 等冲突.
pub const BARRIER_RECOVERY_SGI: u64 = 7;

/// 触发栏栈恢复 SGI (运行时使用)
///
/// 向自身发送 SGI 7, 将由 IRQ handler 中的 `barrier_sgi_handler()` 处理.
/// 调用后, 中断返回 (eret) 将回到本函数调用点.
///
/// # Safety
///
/// 调用者必须在中断使能的环境下调用 (IRQ 已开启).
/// SGI 会立即触发 IRQ 异常, 在 ISR 中执行恢复逻辑.
///
/// # PANIC_FLAG
///
/// 本函数不设置 PANIC_FLAG. 调用者应通过设置 PANIC_FLAG 来指示恢复需求,
/// 或由 bytes_mut 接口触发 domain 故障.
#[inline(always)]
pub fn barrier_trigger_recovery() {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        // SGI 7, 目标: 当前 CPU (Aff0), IRM=0 (不广播)
        let sgi: u64 = (BARRIER_RECOVERY_SGI << 24)   // INTID = 7
                      | (1u64 << 16); // TargetList: Aff0=0
        core::arch::asm!(
            "msr icc_sgi1r_el1, {sgi}",
            "isb",
            sgi = in(reg) sgi,
        );
    }
}

/// SGI 栏栈恢复处理函数 (由 irq_handler 调用)
///
/// 当 IRQ handler 检测到 `intid == BARRIER_RECOVERY_SGI` 时调用此函数.
/// 调用 `recovery_try_recover_from_idt()` 执行域级回滚.
///
/// # 返回值
///
/// - `0`: 恢复成功
/// - `-1`: 恢复失败 (域不可恢复)
/// - `-2`: 已尝试恢复 (防止递归)
///
/// # 注意
///
/// 此函数在 IRQ 上下文 (EL1h) 执行, 不需要额外压栈/出栈.
/// 如果恢复成功, IRQ handler 正常返回 (eret), 调用点继续执行.
pub fn barrier_sgi_handler() -> i32 {
    extern "C" {
        fn recovery_try_recover_from_idt() -> i32;
    }
    // SAFETY: `recovery_try_recover_from_idt` 是有效的 C ABI 函数指针; 参数列表与声明一致
    unsafe { recovery_try_recover_from_idt() }
}

/// 使能 GICv3 SGI 7 (栏栈恢复专用)
///
/// 在 GICv3 初始化完成后调用, 确保 SGI 7 可以触发 IRQ 中断.
/// SGI 默认使能 (GICR_ISENABLER0 bit 7 = 1 需要通过 write_volatile 设置),
/// 但为了安全性, 显式使能.
///
/// # Safety
///
/// 调用前需确保 GICv3 已初始化，Redistributor 寄存器 (GICR_SGI_BASE) 可访问。
pub unsafe fn enable_barrier_sgi() {
    // SGI 7 在 GICR_ISENABLER0 的第 7 位
    // GICv3 规范: SGI 始终使能, 但显式设置确保万无一失
    let enable_reg = super::gic::GICR_ISENABLER0;
    let current = super::gic::gicr_sgi_read(enable_reg);
    super::gic::gicr_sgi_write(enable_reg, current | (1u32 << 7));
}
