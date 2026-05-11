//! TSC (Time Stamp Counter) 管理
//!
//! 提供高精度时间戳读取和频率校准功能。
//!
//! ## 使用示例
//!
//! ```
//! let start = tsc::read();
//! // ... 执行操作 ...
//! let elapsed = tsc::read() - start;
//! ```

use core::arch::asm;

/// 读取 TSC 时间戳 (64-bit)
///
/// 返回自 CPU 启动以来的时钟周期数。
/// 频率通常在 1-4 GHz 范围 (取决于 CPU)。
#[inline(always)]
pub fn read() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// 校准 TSC 频率 (MHz)
///
/// 通过测量已知时间段内的 TSC 计数值来估算频率。
/// 通常使用 PIT (Programmable Interval Timer) 作为参考。
///
/// # Returns
///
/// TSC 频率 (单位: MHz), 或 0 如果校准失败
pub fn calibrate() -> u64 {
    // TODO: 实现 PIT-based TSC 校准算法
    // 这里返回一个估算值 (常见现代 CPU 约 2000-4000 MHz)
    // 实际实现需要 PIT 定时器配合
    
    // 临时方案: 使用固定值 (将在 cpu/init 中完善)
    2500 // 假设 2.5 GHz
}

/// 将 TSC 周期转换为纳秒 (近似值)
///
/// 注意: 这是一个估算值, 在 TSC 未精确校准时误差较大.
#[inline(always)]
pub fn cycles_to_ns(cycles: u64) -> u64 {
    // 假设 2500 MHz → 0.4 ns/cycle
    (cycles * 4) / 10
}
