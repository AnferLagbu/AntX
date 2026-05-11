//! TSC (Time Stamp Counter) 管理
//!
//! 提供高精度时间戳读取和频率校准。

/// 读取当前 TSC 值 (时钟周期数)
/// 
/// # Returns
/// 自 CPU 复位以来的时钟周期数
/// 
/// # Note
/// 在 1GHz CPU 上, 1 个周期 ≈ 1 纳秒。
/// 在 3GHz CPU 上, 1 个周期 ≈ 0.33 纳秒。
#[inline(always)]
pub fn read_tsc() -> u64 {
    let (lo, hi): (u32, u32);
    
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem, preserves_flags),
        );
    }
    
    ((hi as u64) << 32) | (lo as u64)
}

/// 读取 TSC 并附带序列化 (防止乱序执行)
/// 
/// 比 `read_tsc()` 慢, 但结果更精确。
/// 适用于性能测量场景。
#[inline(always)]
pub fn read_tsc_serialized() -> u64 {
    let (lo, hi): (u32, u32);  // ✅ 修复: r32 → u32 (类型错误)
    
    unsafe {
        core::arch::asm!(
            "cpuid",          // 序列化屏障
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem, preserves_flags),
        );
    }
    
    ((hi as u64) << 32) | (lo as u64)
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
