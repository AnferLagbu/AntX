//! Timer IRQ0 中断处理程序
//!
//! 提供与 IDT 系统集成的定时器中断处理：
//! - **IRQ0 Handler**: 定时器中断入口点
//! - **Tick 更新**: 递增全局计数器
//! - **调度触发**: 支持时间片轮转调度
//!
//! ## 集成方式
//!
//! ```text
//! Hardware IRQ0 (PIT)
//!   ↓
//! [isr.asm] → irq_handler()
//!   ↓
//! [IdtManager::handle_irq()]
//!   ↓
//! [timer_irq0_handler()]  ← 本模块
//!   ├── timer::on_timer_interrupt()
//!   └── scheduler::tick() (可选)
//! ```

#[cfg(target_arch = "x86_64")]
use crate::kernel::idt::types::InterruptFrame;
#[cfg(all(target_arch = "x86_64", not(feature = "kernel_test")))]
use core::sync::atomic::Ordering;

/// Timer IRQ0 中断处理程序 (仅 x86_64)
/// aarch64 定时器中断由 exception.rs 的 irq_handler_el1 处理
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub extern "C" fn timer_irq0_handler(_frame: *mut InterruptFrame) {
    crate::kernel::timer::on_timer_interrupt();

    #[cfg(not(feature = "kernel_test"))]
    {
        #[cfg(target_arch = "x86_64")]
        {
            crate::kernel::net::types::sys_tick_inc();

            // smoltcp: 始终轮询
            unsafe {
                crate::kernel::net::init::poll_network();
            }
        }
    }

    // 5. 触发调度器 tick (统一入口: MLFQ 进程调度器负责线程记账 + 调度决策)
    // ✅ 安全检查: 仅当调度器已初始化时才触发 tick (与 ARM 版本一致, 避免竞态崩溃)
    if crate::kernel::proc::scheduler::SCHEDULER_READY.load(core::sync::atomic::Ordering::Acquire) {
        extern "C" {
            fn scheduler_tick_mlfq();
        }
        unsafe {
            scheduler_tick_mlfq();
        }
    }
}

/// 注册 Timer IRQ0 handler 到 IDT 系统 (仅 x86_64)
#[cfg(target_arch = "x86_64")]
pub fn register_timer_irq() -> Result<(), &'static str> {
    use crate::kernel::idt::IdtManager;

    let manager = IdtManager::instance();

    // 注册 IRQ0 handler
    manager.register_irq(
        0, // IRQ0 = PIT Timer
        timer_irq0_handler,
        "PIT Timer",
        0, // flags
    )?;

    // 启用 IRQ0
    manager.enable_irq(0);

    Ok(())
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;

    #[test]
    fn test_timer_irq0_handler_exists() {
        // 验证函数存在且可调用 (不会 panic)
        // 注意: 实际调用需要有效的 InterruptFrame

        // 函数指针类型检查
        let _handler: extern "C" fn(*mut InterruptFrame) = timer_irq0_handler;

        // 如果编译通过，说明函数签名正确
    }

    #[test]
    fn test_register_timer_irq_interface() {
        // 测试注册接口存在
        // 实际注册需要在 IDT 初始化后进行

        // 函数签名验证
        let result = register_timer_irq();

        // 可能成功或失败（取决于 IDT 状态），但不应该 panic
        let _ = result;
    }
}

#[cfg(all(feature = "kernel_test", target_arch = "x86_64"))]
pub fn register_timer_irq_tests() {
    use crate::kernel::tests::{runner, TestFn, TestResult};

    fn timer_irq0_handler_signature() -> TestResult {
        let _handler: extern "C" fn(*mut InterruptFrame) = timer_irq0_handler;
        let _ = _handler;
        TestResult::Pass
    }

    let r = runner();
    r.register(
        "timer::irq",
        "handler_signature",
        timer_irq0_handler_signature as TestFn,
    );
}
