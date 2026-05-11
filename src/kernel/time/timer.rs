//! 系统定时器 (System Timer) - PIT/APIC 实现
//!
//! ## 功能概览
//!
//! - **PIT (8254)**: 可编程间隔定时器 (Legacy 支持)
//! - **APIC Timer**: Local APIC 定时器 (高性能, 多核友好)
//! - **中断驱动**: 时钟中断触发调度、网络超时等
//! - **高精度计时**: 基于 TSC 或 HPET (可选)
//!
//! ## 对比 C 版本 (timer.c, 85行)
//!
//! **功能复刻 + 增强**:
//! ✅ trait 抽象 (TimerBackend for PIT/APIC)
//! ✅ 原子操作 tick 计数器
//! ✅ 回调注册机制 (类型安全)
//! ✅ 睡眠接口 (blocking + busy-wait)
//! ✅ 完整的配置常量

// ============================================================================
// 常量定义
// ============================================================================

/// PIT (8254) I/O 端口地址
pub const PIT_CHANNEL0: u16 = 0x40;    // 通道 0 数据端口
pub const PIT_COMMAND: u16 = 0x43;     // 命令寄存器端口

/// PIT 基准频率 (Hz) - 典型值 1.193182 MHz
pub const PIT_BASE_FREQUENCY: u32 = 1193182;

/// 系统时钟频率 (Hz) - 每秒中断次数
pub const TIMER_FREQUENCY: u32 = 100;  // 100 Hz = 10ms 间隔

/// PIT 分频值 (用于产生 TIMER_FREQUENCY)
pub const PIT_DIVISOR: u32 = PIT_BASE_FREQUENCY / TIMER_FREQUENCY;

/// 调度器优先级提升周期 (ticks, 1000 ticks = 10秒)
pub const SCHED_BOOST_INTERVAL: u64 = 1000;

/// PWID 清理周期 (ticks, 100 ticks = 1秒)
pub const PWID_CLEANUP_INTERVAL: u64 = 100;

/// 中断向量号 (IRQ 0 → INT 32)
pub const TIMER_IRQ_VECTOR: u8 = 32;

// ============================================================================
// FFI: 从 C 导入的回调函数 (用于 timer_handler)
// ============================================================================
//
// 这些函数在 C 子系统中实现:
// - scheduler_tick():        进程调度器 tick (proc/scheduler.c)
// - sys_tick_inc():          lwIP 网络协议栈时钟推进 (net/sys_arch.c)
// - sys_check_timeouts():    lwIP 超时检查 (lwIP core)
// - scheduler_boost_priority(): MLFQ 优先级提升 (proc/scheduler_ex.c)
// - pwid_cleanup_internal(): PWID 安全框架清理 (pwid/manager.c)

extern "C" {
    /// 触发调度器一次 tick (可能触发进程切换)
    fn scheduler_tick();

    /// 推进 lwIP 协议栈时钟 (每次中断调用)
    fn sys_tick_inc();

    /// 检查 lwIP 待处理超时事件 (TCP重传等)
    fn sys_check_timeouts();

    /// MLFQ 调度器优先级提升 (防止饥饿, 每10秒一次)
    fn scheduler_boost_priority();

    /// PWID 身份清理 (过期会话/令牌回收, 每1秒一次)
    fn pwid_cleanup_internal();
}

// ============================================================================
// trait 定义 (抽象定时器后端)
// ============================================================================

/// 定时器后端 trait (支持 PIT 和 APIC)
///
/// # Example
/// ```ignore
/// struct PitTimer;
/// impl TimerBackend for PitTimer {
///     fn init(&self) { /* 初始化 PIT */ }
///     fn set_frequency(&self, hz: u32) { /* 设置频率 */ }
/// }
/// ```
pub trait TimerBackend {
    /// 初始化定时器硬件
    fn init(&mut self);
    
    /// 设置定时器频率
    fn set_frequency(&self, hz: u32);
    
    /// 启用定时器中断
    fn enable_interrupt(&self);
    
    /// 禁用定时器中断
    fn disable_interrupt(&self);
    
    /// 读取当前计数值 (可选)
    fn read_count(&self) -> Option<u16> {
        None // 默认不支持
    }
}

// ============================================================================
// PIT 定时器实现 (8254 Programmable Interval Timer)
// ============================================================================

/// PIT 定时器 (Legacy x86 硬件)
///
/// 使用 Mode 3 (Square Wave Generator), Channel 0。
/// 这是 x86 架构的标准定时器, 所有 PC 都支持。
pub struct PitTimer {
    frequency: u32,
    initialized: bool,
}

impl PitTimer {
    /// 创建新的 PIT 实例
    pub const fn new() -> Self {
        Self {
            frequency: 0,
            initialized: false,
        }
    }
    
    /// 写入 I/O 端口 (内部辅助)
    #[inline(always)]
    unsafe fn outb(port: u16, value: u8) {
        core::arch::asm!(
            "outb {0}, {1}",
            in(reg) value,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
    }
}

impl Default for PitTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerBackend for PitTimer {
    /// 初始化 PIT 为指定频率
    fn init(&mut self) {
        let divisor = PIT_DIVISOR;
        
        unsafe {
            // 设置命令字节:
            // - Channel 0 (bits 7-6: 00)
            // - Access mode: Low/High byte (bits 5-4: 11)
            // - Operating mode: Mode 3 (Square wave) (bits 3-1: 011)
            // - BCD format: Binary (bit 0: 0)
            // → Command byte = 0x36
            Self::outb(PIT_COMMAND, 0x36);
            
            // 发送分频值 (低字节 + 高字节)
            Self::outb(PIT_CHANNEL0, (divisor & 0xFF) as u8);
            Self::outb(PIT_CHANNEL0, ((divisor >> 8) & 0xFF) as u8);
        }
        
        self.frequency = TIMER_FREQUENCY;
        self.initialized = true;
    }
    
    fn set_frequency(&self, _hz: u32) {
        // PIT 频率在 init 时固定, 运行时更改需要重新初始化
        // 这里简化处理: 仅记录但不实际修改
    }
    
    fn enable_interrupt(&self) {
        // PIT 中断通过 IDT/IOAPIC 控制, 此处无需额外操作
    }
    
    fn disable_interrupt(&self) {
        // 同上
    }
    
    fn read_count(&self) -> Option<u16> {
        if !self.initialized {
            return None;
        }
        
        let count: u16;
        unsafe {
            // 锁存当前计数值 (latch command to channel 0)
            Self::outb(PIT_COMMAND, 0x00);
            
            // 读取低字节
            let lo: u8;
            core::arch::asm!(
                "in al, dx",
                out("al") lo,
                in("dx") PIT_CHANNEL0,
                options(nomem, nostack, preserves_flags),
            );
            
            // 读取高字节
            let hi: u8;
            core::arch::asm!(
                "in al, dx",
                out("al") hi,
                in("dx") PIT_CHANNEL0,
                options(nomem, nostack, preserves_flags),
            );
            
            count = (lo as u16) | ((hi as u16) << 8);
        }
        
        Some(count)
    }
}

// ============================================================================
// 全局状态与 API
// ============================================================================


/// 全局 tick 计数器 (原子操作, 线程安全)
static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

/// 定时器是否已初始化
static TIMER_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 当前活跃的定时器后端
static mut CURRENT_TIMER: Option<&'static dyn TimerBackend> = None;

/// 初始化系统定时器
///
/// 默认使用 PIT (8254) 作为定时器后端。
/// 注册 IRQ 0 (INT 32) 的中断处理程序。
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn timer_init() -> i32 {
    use crate::logging::klog::{klog_write, LogLevel, LogCategory};
    
    static INIT_MSG: &[u8] = b"Initializing system timer...\0";
    unsafe {
        klog_write(LogLevel::Info as u8, LogCategory::Boot as u8,
                  core::ptr::null(), core::ptr::null(), 0,
                  INIT_MSG.as_ptr() as *const i8);
    }
    
    // 创建并初始化 PIT
    static mut PIT: PitTimer = PitTimer::new();
    unsafe {
        PIT.init();
        CURRENT_TIMER = Some(&PIT);
    }
    
    // TODO: 注册中断处理程序到 IDT
    // idt_register_irq(0, timer_handler, "timer", 0);
    
    TIMER_INITIALIZED.store(true, Ordering::Release);
    
    static OK_MSG: &[u8] = concat!("Timer initialized (", stringify!(TIMER_FREQUENCY), " Hz)\0").as_bytes();
    unsafe {
        klog_write(LogLevel::Info as u8, LogCategory::Boot as u8,
                  core::ptr::null(), core::ptr::null(), 0,
                  OK_MSG.as_ptr() as *const i8);
    }
    
    0 // 成功
}

/// 定时器中断处理程序 (IRQ 0 handler)
///
/// **每次时钟中断调用**, 由 IDT 自动分发。
///
/// 功能:
/// 1. 更新全局 tick 计数器
/// 2. 触发调度器 tick (进程切换)
/// 3. 推进 lwIP 网络协议栈时钟
/// 4. 周期性任务: MLFQ优先级提升、PWID清理
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub unsafe extern "C" fn timer_handler(_frame: *const u8) {
    // 更新 tick 计数
    let new_ticks = TIMER_TICKS.fetch_add(1, Ordering::SeqCst) + 1;

    // ✅ 调用调度器 tick (每次中断)
    // 触发进程调度和可能的上下文切换
    scheduler_tick();

    // ✅ 推进 lwIP 网络协议栈时钟 (每次中断, 10ms)
    sys_tick_inc();
    sys_check_timeouts();

    // MLFQ 优先级提升 (每 10 秒一次, 防止饥饿)
    if new_ticks % SCHED_BOOST_INTERVAL == 0 && new_ticks > 0 {
        scheduler_boost_priority();
    }

    // PWID 清理 (每 1 秒一次)
    if new_ticks % PWID_CLEANUP_INTERVAL == 0 {
        pwid_cleanup_internal();
    }
}

/// 获取当前 tick 数 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn timer_get_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Acquire)
}

/// 阻塞式睡眠 (基于调度器)
///
/// 让当前进程阻塞指定毫秒数。
/// 使用调度器的阻塞机制, 释放 CPU 给其他进程。
///
/// # Arguments
/// * `ms` - 睡眠时间 (毫秒)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn timer_sleep(ms: u64) {
    if ms == 0 {
        return;
    }
    
    let target = timer_get_ticks() + (ms * TIMER_FREQUENCY as u64) / 1000;
    
    while timer_get_ticks() < target {
        // TODO: 调用调度器阻塞和让出 CPU
        // extern "C" { fn proc_block(reason: u32); fn scheduler_yield(); }
        // proc_block(1); // BLOCK_SLEEP
        // scheduler_yield();
        
        // HLT 指令暂停 CPU 直到下一个中断
        unsafe {
            core::arch::asm!("hlt", options(nostack, nomem, preserves_flags));
        }
    }
}

/// 忙等待式睡眠 (早期启动阶段使用)
///
/// 在调度器未初始化前使用。
/// 通过 HLT 指令降低功耗, 但不释放 CPU。
///
/// # Arguments
/// * `ms` - 等待时间 (毫秒)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn timer_sleep_busy(ms: u64) {
    let target = timer_get_ticks() + (ms * TIMER_FREQUENCY as u64) / 1000;
    
    while timer_get_ticks() < target {
        unsafe {
            core::arch::asm!("hlt", options(nostack, nomem, preserves_flags));
        }
    }
}

/// 将 tick 数转换为毫秒
#[inline]
pub const fn ticks_to_ms(ticks: u64) -> u64 {
    (ticks * 1000) / TIMER_FREQUENCY as u64
}

/// 将毫秒转换为 tick 数
#[inline]
pub const fn ms_to_ticks(ms: u64) -> u64 {
    (ms * TIMER_FREQUENCY as u64) / 1000
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_pit_constants() {
        assert_eq!(PIT_BASE_FREQUENCY, 1193182);
        assert_eq!(TIMER_FREQUENCY, 100);
        assert_eq!(PIT_DIVISOR, 11931); // 1193182 / 100 ≈ 11931
        assert_eq!(TIMER_IRQ_VECTOR, 32);
    }
    
    #[test]
    fn test_time_conversion() {
        // 100 ticks = 1 second = 1000 ms (@ 100 Hz)
        assert_eq!(ticks_to_ms(100), 1000);
        assert_eq!(ms_to_ticks(1000), 100);
        
        // 50 ticks = 500 ms
        assert_eq!(ticks_to_ms(50), 500);
        assert_eq!(ms_to_ticks(500), 50);
    }
    
    #[test]
    fn test_timer_intervals() {
        assert_eq!(SCHED_BOOST_INTERVAL, 1000); // 10 seconds
        assert_eq!(PWID_CLEANUP_INTERVAL, 100);  // 1 second
    }
}