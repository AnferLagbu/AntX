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

use crate::kernel::idt::types::InterruptFrame;
use core::sync::atomic::Ordering;

/// Timer IRQ0 中断处理程序
///
/// 每次定时器中断时由 IDT 系统调用。
///
/// # Arguments
/// * `frame` - 中断帧指针 (包含寄存器状态)
///
/// # Safety
/// 此函数从中断上下文调用，必须快速执行。
#[no_mangle]
pub extern "C" fn timer_irq0_handler(_frame: *mut InterruptFrame) {
    crate::kernel::timer::on_timer_interrupt();

    #[cfg(not(feature = "kernel_test"))]
    {
        crate::kernel::net::types::sys_tick_inc();

        if crate::kernel::net::types::NET_READY.load(Ordering::Acquire) {
            extern "C" {
                fn sys_check_timeouts();
                fn e1000_poll_rx();
            }
            unsafe { sys_check_timeouts(); }

            let t = crate::kernel::timer::get_ticks();
            if t % 10 == 0 {
                unsafe { e1000_poll_rx(); }
            }
        }
    }

    // 5. 触发调度器 tick (主调度器 + 线程调度器)
    extern "C" {
        fn scheduler_tick_mlfq();
        fn scheduler_tick();
    }
    unsafe {
        scheduler_tick_mlfq();
        scheduler_tick();
    }
}

/// 注册 Timer IRQ0 handler 到 IDT 系统
///
/// 应在内核初始化早期调用，在启用中断之前。
///
/// # Returns
/// * `Ok(())` - 注册成功
/// * `Err(&str)` - 注册失败
pub fn register_timer_irq() -> Result<(), &'static str> {
    use crate::kernel::idt::IdtManager;
    
    let manager = IdtManager::instance();
    
    // 注册 IRQ0 handler
    manager.register_irq(
        0,  // IRQ0 = PIT Timer
        timer_irq0_handler,
        "PIT Timer",
        0,  // flags
    )?;

    // 启用 IRQ0
    manager.enable_irq(0);

    Ok(())
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
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

#[cfg(feature = "kernel_test")]
pub fn register_timer_irq_tests() {
    use crate::kernel::tests::{runner, TestFn, TestResult};

    fn timer_irq0_handler_signature() -> TestResult {
        let _handler: extern "C" fn(*mut InterruptFrame) = timer_irq0_handler;
        let _ = _handler;
        TestResult::Pass
    }

    let r = runner();
    r.register("timer::irq", "handler_signature", timer_irq0_handler_signature as TestFn);
}
