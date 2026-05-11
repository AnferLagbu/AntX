//! Timer 子系统使用示例
//!
//! 展示 AntX 内核中 Timer 模块的典型应用场景：
//! - **系统初始化**: 启动时配置和校准
//! - **时间测量**: 性能分析和调试
//! - **延时控制**: 精确的等待机制
//! - **调度集成**: 时间片轮转
//!
//! ## 快速开始
//!
//! ```rust,no_run
//! // 1. 初始化 (kernel_init() 中自动完成)
//! timer::timer_init(1000).unwrap();  // 1ms 中断
//!
//! // 2. 获取时间
//! let ticks = timer::get_ticks();
//! println!("Uptime: {} ms", timer::get_uptime_ms());
//!
//! // 3. 睡眠/延时
//! timer::timer_sleep(100).unwrap();  // 100ms
//! ```

#![allow(dead_code)]

use crate::kernel::timer::*;

// ============================================================================
// 示例 1: 系统启动时的完整初始化流程
// ============================================================================

/// 完整的系统定时器初始化 (通常在 kernel_init() 中调用)
///
/// # Example Usage
/// ```rust,no_run
/// fn kernel_main() {
///     setup_timer_subsystem();
///     
///     // ... 其他初始化 ...
///     
///     enable_interrupts();  // 最后启用中断
/// }
/// ```
pub fn example_full_initialization() {
    println!("=== Timer Subsystem Initialization ===\n");

    // Step 1: 初始化 PIT 硬件
    match timer_init(1000) {
        Ok(actual_freq) => {
            println!("✓ PIT initialized: {} Hz (target: 1000 Hz)", actual_freq);
        },
        Err(msg) => {
            println!("✗ PIT init failed: {}", msg);
            return;
        }
    }

    // Step 2: 注册 IRQ0 handler
    match irq::register_timer_irq() {
        Ok(()) => println!("✓ IRQ0 handler registered"),
        Err(msg) => println!("✗ IRQ0 registration failed: {}", msg),
    }

    // Step 3: 校准 TSC (提高时间测量精度)
    match calibrate_tsc(20) {
        Ok(tsc_mhz) => {
            println!("✓ TSC calibrated: {} MHz", tsc_mhz);
            
            // 验证校准结果
            if let Some(freq) = get_tsc_frequency_hz() {
                println!("  Full precision: {} Hz", freq);
            }
        },
        Err(msg) => {
            println!("⚠ TSC calibration failed: {} (using approximations)", msg);
        }
    }

    // Step 4: 验证系统状态
    println!("\n--- System Status ---");
    println!("Timer initialized: {}", is_initialized());
    println!("TSC calibrated: {}", is_calibrated());
    
    if let Some(uptime) = get_uptime_ms_checked() {
        println!("Current uptime: {} ms", uptime);
    }

    println!("\n✓ Timer subsystem ready!\n");
}

// ============================================================================
// 示例 2: 高精度性能测量
// ============================================================================

/// 测量代码执行时间 (多种方法对比)
///
/// 展示不同精度级别的时间测量技术：
/// - Tick 级别 (~1ms @ 1kHz)
/// - TSC 级别 (~纳秒, 需要校准)
/// - PIT 级别 (~微秒, 使用 PIT 计数器)
pub fn example_performance_measurement() {
    println!("=== Performance Measurement Examples ===\n");

    // --- 方法 1: 基于 tick 的测量 (简单但低精度) ---
    println!("1. Tick-based measurement:");
    
    let start_ticks = get_ticks();
    
    // 执行一些工作
    let mut sum: u64 = 0;
    for i in 0..10000 {
        sum = sum.wrapping_add(i * i);
    }
    
    let end_ticks = get_ticks();
    let elapsed_ticks = end_ticks - start_ticks;
    let elapsed_ms = ticks_to_ms(elapsed_ticks);
    
    println!("   Result: {}", sum);
    println!("   Elapsed: {} ticks (≈ {} ms)", elapsed_ticks, elapsed_ms);

    // --- 方法 2: 基于 TSC 的高精度测量 ---
    println!("\n2. TSC-based measurement (high precision):");
    
    if is_calibrated() {
        let (result, duration_ns) = measure_time(|| {
            let mut sum: u64 = 0;
            for i in 0..10000 {
                sum = sum.wrapping_add(i * i);
            }
            sum
        });
        
        println!("   Result: {}", result);
        println!("   Duration: {} ns ({} μs)", 
                 duration_ns, 
                 duration_ns / 1000);
        
        // 使用绝对时间戳
        if let Some(start_ns) = get_time_ns() {
            // ... 执行工作 ...
            if let Some(end_ns) = get_time_ns() {
                let diff_ns = end_ns - start_ns;
                println!("   Wall-clock: {} ns", diff_ns);
            }
        }
    } else {
        println!("   ⚠ TSC not calibrated, skipping high-precision measurement");
    }

    // --- 方法 3: 使用 measure_time_ticks (中等精度) ---
    println!("\n3. measure_time_ticks utility:");
    
    let (result, duration_ticks) = measure_time_ticks(|| {
        let mut sum: u64 = 0;
        for i in 0..10000 {
            sum = sum.wrapping_add(i * i);
        }
        sum
    });
    
    println!("   Result: {}", result);
    println!("   Duration: {} ticks", duration_ticks);

    println!();
}

// ============================================================================
// 示例 3: 延时控制策略选择
// ============================================================================

/// 不同场景下的最佳延时策略
///
/// 根据具体需求选择合适的 sleep/wait 方法：
pub fn example_sleep_strategies() {
    println!("=== Sleep Strategy Selection Guide ===\n");

    // --- 场景 1: 中断上下文 (不能调度) ---
    println!("1. Interrupt context (no scheduling allowed):");
    println!("   Use: busy_wait_us() or busy_wait_ns()");
    println!("   Example: Waiting for hardware register\n");

    // 模拟中断上下文中的短延时
    busy_wait_us(500);  // 500 微秒
    println!("   ✓ Completed 500μs busy-wait\n");

    // --- 场景 2: 用户态进程 (可以阻塞) ---
    println!("2. User process (can block efficiently):");
    println!("   Use: timer_sleep() or adaptive_sleep()");
    println!("   Example: Process waiting for I/O\n");

    // 模拟用户态长延时
    let _ = timer_sleep(10);  // 10 毫秒 (实际会阻塞并让出 CPU)
    println!("   ✓ Completed 10ms blocking sleep\n");

    // --- 场景 3: 自适应策略 (推荐) ---
    println!("3. Adaptive strategy (recommended for general use):");
    println!("   Use: adaptive_sleep()");
    println!("   Automatically selects best method based on duration\n");

    // 短延时 → 忙等待
    adaptive_sleep(1);   // 1ms: 可能使用忙等待
    println!("   ✓ Short sleep (1ms) completed\n");

    // 长延时 → 调度器阻塞
    adaptive_sleep(100); // 100ms: 会使用调度器阻塞
    println!("   ✓ Long sleep (100ms) completed\n");

    // --- 场景 4: 条件等待 (带超时) ---
    println!("4. Conditional wait with timeout:");
    println!("   Use: wait_with_timeout()");
    println!("   Example: Polling hardware status\n");

    // 模拟条件变量
    let condition_met = core::sync::atomic::AtomicBool::new(false);
    
    let result = wait_with_timeout(
        || condition_met.load(core::sync::atomic::Ordering::Relaxed),
        50  // 50ms 超时
    );
    
    match result {
        Ok(()) => println!("   ✓ Condition met within timeout"),
        Err(_) => println!("   ✗ Timeout waiting for condition"),
    }
    println!();
}

// ============================================================================
// 示例 4: 调度器和时间片集成
// ============================================================================

/// Timer 与调度器的集成模式
///
/// 展示如何在调度系统中使用 Timer：
pub fn example_scheduler_integration() {
    println!("=== Scheduler Integration Patterns ===\n");

    // --- Pattern 1: 时间片轮转 ---
    println!("1. Time-slice round-robin:");
    println!("   Each process gets a fixed time slice (e.g., 10ms)");
    println!("   Timer interrupt triggers context switch\n");

    // 模拟时间片检查
    let time_slice_ms: u64 = 10;
    let start_tick = get_ticks();
    
    // ... 进程运行 ...
    
    let current_tick = get_ticks();
    let elapsed = ticks_to_ms(current_tick - start_tick);
    
    if elapsed >= time_slice_ms {
        println!("   ⏰ Time slice expired ({} >= {}ms), yield CPU", elapsed, time_slice_ms);
        // scheduler_yield();  // 让出 CPU
    } else {
        println!("   ✓ Time slice remaining: {}ms", time_slice_ms - elapsed);
    }

    // --- Pattern 2: Sleep-based scheduling ---
    println!("\n2. Sleep-based scheduling:");
    println!("   Process voluntarily gives up CPU for a period\n");

    // 模拟 I/O 等待
    println!("   Process: Starting I/O operation...");
    let _ = timer_sleep(5);  // 等待 5ms
    println!("   Process: I/O complete, resuming\n");

    // --- Pattern 3: Priority aging ---
    println!("3. Priority aging (prevent starvation):");
    println!("   Increase priority of waiting processes over time\n");

    struct Process {
        pid: u32,
        priority: u32,
        wait_start: u64,
    }

    let mut proc = Process {
        pid: 42,
        priority: 5,
        wait_start: get_ticks(),
    };

    // 模拟等待一段时间后提升优先级
    let _ = timer_sleep(100);  // 模拟其他进程运行
    
    let wait_duration = ticks_to_ms(get_ticks() - proc.wait_start);
    if wait_duration > 50 {
        // 每 50ms 提升优先级
        let boost = (wait_duration / 50) as u32;
        proc.priority = proc.priority.saturating_add(boost);
        println!("   PID {}: boosted priority to {} (waited {}ms)",
                 proc.pid, proc.priority, wait_duration);
    }

    println!();
}

// ============================================================================
// 示例 5: 高级功能演示
// ============================================================================

/// 展示 Timer 的高级功能和实用工具
pub fn example_advanced_features() {
    println!("=== Advanced Timer Features ===\n");

    // --- Feature 1: Uptime 格式化显示 ---
    println!("1. System uptime display:");
    
    let uptime_ms = get_uptime_ms();
    let uptime_s = get_uptime_s();
    
    println!("   Raw: {} ms / {} s", uptime_ms, uptime_s);
    
    #[cfg(feature = "alloc")]
    {
        use crate::kernel::timer::format_duration;
        let formatted = format_duration(uptime_ms);
        println!("   Formatted: {}\n", formatted);
    }

    // --- Feature 2: 时间转换往返测试 ---
    println!("2. Conversion round-trip test:");
    
    let original_ms: u64 = 1234567890;  // ~14.3 days
    
    // ms → ticks → ms
    let ticks = ms_to_ticks(original_ms);
    let back_to_ms = ticks_to_ms(ticks);
    
    let error = if back_to_ms > original_ms {
        back_to_ms - original_ms
    } else {
        original_ms - back_to_ms
    };
    
    println!("   Original: {} ms", original_ms);
    println!("   Converted: {} ms (error: {} ms)\n", back_to_ms, error);

    // --- Feature 3: TSC 高精度时间 (如果已校准) ---
    println!("3. High-precision timing (TSC):");
    
    if is_calibrated() {
        if let Some(tsc_mhz) = get_tsc_frequency_mhz() {
            println!("   TSC frequency: {} MHz", tsc_mhz);
            
            // 测量极短时间间隔
            let tsc_start = crate::kernel::cpu::tsc::read_tsc();
            
            // 执行空循环 (几个周期)
            for _ in 0..100 {
                core::hint::spin_loop();
            }
            
            let tsc_end = crate::kernel::cpu::tsc::read_tsc();
            let cycles = tsc_end - tsc_start;
            
            if let Some(ns) = tsc_to_nanoseconds(cycles) {
                println!("   Empty loop: {} cycles ≈ {} ns", cycles, ns);
            }
        }
    } else {
        println!("   ⚠ TSC not calibrated, using tick-based timing only");
    }

    // --- Feature 4: PIT 底层访问 (高级用户) ---
    println!("\n4. Low-level PIT access:");
    
    if pit_is_initialized() {
        if let Some(count) = pit_read_count() {
            println!("   Current PIT count: {} (of max {})", count, 65535);
            
            if let Some(us) = pit_elapsed_since_tick_us() {
                println!("   Time since last tick: {} μs", us);
            }
        }
        
        if let Some(freq) = pit_get_frequency() {
            println!("   Configured frequency: {} Hz", freq);
        }
    } else {
        println!("   PIT not initialized");
    }

    println!();

    // --- Feature 5: 详细诊断信息 ---
    println!("5. Diagnostic information:");
    
    let (ticks, freq, uptime, ns_per_tick, us_per_tick) = get_time_info();
    println!("   Total ticks: {}", ticks);
    println!("   Timer frequency: {} Hz", freq);
    println!("   Uptime: {} ms", uptime);
    println!("   NS per tick: {}", ns_per_tick);
    println!("   US per tick: {}", us_per_tick);
    
    if is_calibrated() {
        let (mhz, hz, range) = get_calibration_info();
        println!("   TSC freq: {} MHz / {} Hz", 
                 mhz.unwrap_or(0), 
                 hz.unwrap_or(0));
        println!("   Calibration range: {} cycles", 
                 range.unwrap_or(0));
    }

    println!();
}

// ============================================================================
// 主入口: 运行所有示例
// ============================================================================

/// 运行所有 Timer 使用示例
///
/// # Safety
/// 此函数应在内核初始化完成后、用户进程启动前调用。
pub fn run_all_timer_examples() {
    println!("╔════════════════════════════════════════════╗");
    println!("║     AntX Timer Subsystem - Usage Examples   ║");
    println!("╚════════════════════════════════════════════╝\n");

    example_full_initialization();
    example_performance_measurement();
    example_sleep_strategies();
    example_scheduler_integration();
    example_advanced_features();

    println!("╔════════════════════════════════════════════╗");
    println("║           All examples completed ✓          ║");
    println!("╚════════════════════════════════════════════╝\n");
}

/// 辅助函数: 安全获取 uptime (处理未初始化情况)
fn get_uptime_ms_checked() -> Option<u64> {
    if is_initialized() {
        Some(get_uptime_ms())
    } else {
        None
    }
}
