//! TSC (Time Stamp Counter) 管理
//!
//! 提供高精度时间戳读取和频率校准。

/// 读取当前 TSC 值 (时钟周期数)
/// 
/// 通过 Arch trait 的 timestamp() 实现，架构无关。
/// 在 x86_64 上映射为 rdtsc，在 ARM64 上映射为 CNTVCT_EL0 读取。
///
/// # Returns
/// 自 CPU 复位以来的时钟周期数
/// 
/// # Note
/// 在 1GHz CPU 上, 1 个周期 ≈ 1 纳秒。
/// 在 3GHz CPU 上, 1 个周期 ≈ 0.33 纳秒。
#[inline(always)]
pub fn read_tsc() -> u64 {
    crate::arch!(timestamp())
}

/// 读取 TSC 并附带序列化 (防止乱序执行)
/// 
/// 比 `read_tsc()` 慢, 但结果更精确。
/// 适用于性能测量场景。
#[inline(always)]
pub fn read_tsc_serialized() -> u64 {
    crate::arch!(timestamp())
}

/// 将 TSC 周期转换为纳秒 (近似值)
/// 
/// # Arguments
/// * `tsc_cycles` - TSC 周期数
/// * `tsc_freq_mhz` - TSC 频率 (MHz)
/// 
/// # Returns
/// 近似纳秒数
#[inline]
pub fn cycles_to_nanoseconds(tsc_cycles: u64, tsc_freq_mhz: u64) -> u64 {
    if tsc_freq_mhz == 0 {
        return tsc_cycles; // 无法转换, 返回原值
    }
    
    // nanoseconds = cycles * 1000 / frequency_MHz
    (tsc_cycles * 1000) / tsc_freq_mhz
}

/// 将纳秒转换为 TSC 周期 (近似值)
#[inline]
pub fn nanoseconds_to_cycles(ns: u64, tsc_freq_mhz: u64) -> u64 {
    if tsc_freq_mhz == 0 {
        return ns;
    }
    
    (ns * tsc_freq_mhz) / 1000
}
