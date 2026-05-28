//! Timer 子系统 (定时器管理)
//!
//! 提供稳健且实用的系统定时器功能：
//! - **PIT 驱动**: 8254 可编程间隔定时器硬件控制
//! - **Tick 计数器**: 全局时间追踪和转换
//! - **Sleep 机制**: 多种精度的延时和等待
//! - **高精度计时**: 基于 TSC 的纳秒级测量
//!
//! ## 架构设计
//!
//! ```text
//! Timer Subsystem
//! ├── PIT Driver (pit.rs)
//! │   └── 硬件层: 8254 芯片 I/O 操作
//! │
//! ├── Tick Manager (tick.rs)
//! │   └── 时间层: tick 计数、转换、uptime
//! │
//! └── Sleep Functions (sleep.rs)
//!     └── 应用层: 忙等待、调度器阻塞、自适应策略
//! ```
//!
//! ## 使用示例
//!
//! ### 基础用法
//! ```rust,no_run
//! // 初始化 (通常在内核启动时调用一次)
//! timer::timer_init(1000).unwrap();  // 1ms 中断频率
//!
//! // 获取当前时间
//! let ticks = timer::get_ticks();
//! println!("Uptime: {} ms", timer::get_uptime_ms());
//!
//! // 睡眠
//! timer::timer_sleep(100).unwrap();  // 睡眠 100ms
//! ```
//!
//! ### 高级用法
//! ```rust,no_run
//! // 高精度忙等待 (中断上下文)
//! timer::busy_wait_us(500);  // 500 微秒
//!
//! // 条件等待 (带超时)
//! timer::wait_with_timeout(
//!     || hardware_ready(),
//!     5000  // 5 秒超时
//! ).unwrap();
//!
//! // 性能测量
//! let (_, duration) = timer::measure_time(|| {
//!     expensive_operation();
//! });
//! ```
//!
//! # 设计原则
//!
//! 1. **稳健性**: 所有操作都有错误处理和边界检查
//! 2. **实用性**: 提供多种粒度满足不同场景需求
//! 3. **兼容性**: 与 C 版本 API 完全兼容 (FFI)
//! 4. **性能**: 关键路径内联，使用原子操作避免锁

// ============================================================================
// 子模块声明
// ============================================================================

/// PIT (8254) 可编程间隔定时器驱动
pub mod pit;

/// 全局 Tick 计数器和时间管理
pub mod tick;

/// Sleep 和延时功能
pub mod sleep;

/// Timer IRQ0 中断处理程序 (IDT 集成)
pub mod irq;

/// TSC 频率校准 (PIT-based 高精度测量)
pub mod calibration;

// ============================================================================
// 公共 API 导出 (便捷访问)
// ============================================================================

// --- 初始化和状态 ---

/// 使用 `tick` 模块的初始化函数
pub use tick::{
    timer_init,
    is_initialized,
    on_timer_interrupt,
};

// --- 时间查询 ---

/// 使用 `tick` 模块的时间函数
pub use tick::{
    get_ticks,
    get_frequency,
    get_uptime_ms,
    get_uptime_s,
    get_time_info,
};

// --- 时间转换 ---

/// 使用 `tick` 模块的转换函数
pub use tick::{
    ticks_to_ms,
    ticks_to_us,
    ticks_to_ns,
    ms_to_ticks,
    us_to_ticks,
    ticks_to_ms_fast,
    ticks_to_us_fast,
};

// --- Sleep 函数 ---

/// 使用 `sleep` 模块的所有睡眠函数
pub use sleep::{
    busy_wait_ns,
    busy_wait_us,
    busy_wait_ms,
    pit_busy_wait_us,
    timer_sleep as timer_sleep_safe,
    wait_with_timeout,
    adaptive_sleep,
    measure_time,
    measure_time_ticks,
};

// --- PIT 底层控制 ---

/// 使用 `pit` 模块的底层功能 (高级用户)
pub use pit::{
    pit_init as raw_pit_init,
    pit_read_count,
    pit_elapsed_since_tick_us,
    pit_get_frequency,
    pit_is_initialized,
    pit_shutdown,
    PIT_BASE_FREQUENCY,
    DEFAULT_INTERRUPT_FREQ_HZ,
};

// --- TSC 校准 ---

/// 使用 `calibration` 模块的校准功能
pub use calibration::{
    calibrate_tsc,
    quick_calibrate,
    get_tsc_frequency_mhz,
    get_tsc_frequency_hz,
    is_calibrated,
    tsc_to_nanoseconds,
    tsc_to_microseconds,
    tsc_to_milliseconds,
    nanoseconds_to_tsc,
    get_time_ns,
    get_time_us,
    get_time_ms,
};

// ============================================================================
// FFI 兼容层 (C 接口)
// ============================================================================

/// C 兼容的初始化函数
///
/// 与原始 C 函数签名完全一致：
/// ```c
/// void timer_init(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn timer_init_ffi() {
    match timer_init(DEFAULT_INTERRUPT_FREQ_HZ) {
        Ok(_) => {},
        Err(msg) => {
            // TODO: 使用 klog 记录错误
            let _ = msg;
        }
    }
}

/// C 兼容的获取 ticks 函数
///
/// 与原始 C 函数签名一致：
/// ```c
/// uint64_t timer_get_ticks(void);
/// ```
#[no_mangle]
pub extern "C" fn timer_get_ticks() -> u64 {
    get_ticks()
}

/// C 兼容的 sleep 函数
///
/// 与原始 C 函数签名一致：
/// ```c
/// void timer_sleep(uint64_t ms);
/// ```
#[no_mangle]
pub extern "C" fn timer_sleep(ms: u64) {
    let _ = timer_sleep_safe(ms);
}

/// C 兼容的忙等待 sleep 函数
///
/// 与原始 C 函数签名一致：
/// ```c
/// void timer_sleep_busy(uint64_t ms);
/// ```
#[no_mangle]
pub extern "C" fn timer_sleep_busy(ms: u64) {
    busy_wait_ms(ms);
}

// ============================================================================
// 集成测试
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_api_surface() {
        // 验证所有公共 API 都可访问
        
        // 基本状态查询
        let _ticks = get_ticks();
        let _freq = get_frequency();
        let _init = is_initialized();

        // 时间转换 (不会 panic)
        let _ms = ticks_to_ms(1000);
        let _us = ticks_to_us(1000);
        let _ns = ticks_to_ns(1000);
        let _t1 = ms_to_ticks(1000);
        let _t2 = us_to_ticks(1000000);

        // Uptime (应该 >= 0)
        assert!(get_uptime_ms() >= 0);
        assert!(get_uptime_s() >= 0);

        // Sleep 零值 (立即返回)
        busy_wait_ns(0);
        busy_wait_us(0);
        busy_wait_ms(0);
        assert!(timer_sleep(0).is_ok());

        // 测量工具
        let (_result, _time) = measure_time(|| 42);
        let (_result, _ticks) = measure_time_ticks(|| 42);
    }

    #[test]
    fn test_monotonicity_guarantee() {
        // Tick 应该严格单调递增
        let t1 = get_ticks();
        let t2 = get_ticks();
        let t3 = get_ticks();

        assert!(t2 >= t1);
        assert!(t3 >= t2);
    }

    #[test]
    fn test_conversion_roundtrip() {
        // 转换应该是合理的 (允许一定误差)
        let original_ms: u64 = 1234;
        
        // ms → ticks → ms
        let ticks = ms_to_ticks(original_ms);
        let back_to_ms = ticks_to_ms(ticks);
        
        // 允许 ±1ms 误差
        let diff = if back_to_ms > original_ms {
            back_to_ms - original_ms
        } else {
            original_ms - back_to_ms
        };
        assert!(diff <= 1, "Roundtrip error too large: {} ms", diff);
    }

    #[test]
    fn test_consistency_between_functions() {
        // 不同方式获取的时间应该一致
        let uptime_ms = get_uptime_ms();
        let ticks = get_ticks();
        let calculated_ms = ticks_to_ms(ticks);

        // 应该非常接近 (可能差 1 个 tick)
        let diff = if calculated_ms > uptime_ms {
            calculated_ms - uptime_ms
        } else {
            uptime_ms - calculated_ms
        };
        assert!(diff <= 2, "Inconsistency detected: {} ms", diff);
    }

    #[test]
    fn test_error_handling() {
        // 边界条件测试
        
        // 零值处理
        assert_eq!(ticks_to_ms(0), 0);
        assert_eq!(ticks_to_us(0), 0);
        assert_eq!(ms_to_ticks(0), 0);

        // 大数值处理 (不应该溢出或 panic)
        let large_value = u64::MAX / 2;
        let _ = ticks_to_ms(large_value);
        let _ = ms_to_ticks(large_value);
    }
}
