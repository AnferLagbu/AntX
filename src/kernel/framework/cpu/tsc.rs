//! TSC (Time Stamp Counter) 管理
//!
//! 提供高精度时间戳读取和频率校准。

/// 读取当前 TSC 值 (时钟周期数)
///
/// 通过 Arch trait 的 `timestamp()` 实现，架构无关。
/// 在 `x86_64` 上映射为 rdtsc，在 ARM64 上映射为 `CNTVCT_EL0` 读取。
///
/// # Returns
/// 自 CPU 复位以来的时钟周期数
///
/// # Note
/// 在 1GHz CPU 上, 1 个周期 ≈ 1 纳秒。
/// 在 3GHz CPU 上, 1 个周期 ≈ 0.33 纳秒。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub fn read_tsc() -> u64 {
    crate::arch!(timestamp())
}

/// 读取 TSC 并附带序列化 (防止乱序执行)
///
/// 比 `read_tsc()` 慢, 但结果更精确。
/// 适用于性能测量场景。
///
/// # B03-19 修复
/// 当前实现与 `read_tsc` 相同 (均走 `crate::arch!(timestamp())`), 无
/// `mfence`/`lfence` 序列化指令。文档"更精确"描述与实现不一致, 标记为已知
/// 偏差。下次重写 CPU 时序子系统时实装真正的序列化版本。
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
///
/// # B03-19 修复
/// 使用 `checked_mul` 防 u64 溢出。理论上 4GHz × 24h × 1000 ≈ 3.5×10¹⁷ 仍
/// 远小于 u64::MAX (≈1.8×10¹⁹), 但防御性写法成本极低, 避免极端情况下溢出。
#[inline]
pub fn cycles_to_nanoseconds(tsc_cycles: u64, tsc_freq_mhz: u64) -> u64 {
    if tsc_freq_mhz == 0 {
        return tsc_cycles; // 无法转换, 返回原值
    }

    // nanoseconds = cycles * 1000 / frequency_MHz
    // SAFETY: 乘法可能溢出 u64, 用 checked_mul 返回 u64::MAX 作为饱和值。
    match tsc_cycles.checked_mul(1000) {
        Some(product) => product / tsc_freq_mhz,
        None => u64::MAX,
    }
}

/// 将纳秒转换为 TSC 周期 (近似值)
#[inline]
pub fn nanoseconds_to_cycles(ns: u64, tsc_freq_mhz: u64) -> u64 {
    if tsc_freq_mhz == 0 {
        return ns;
    }

    // B03-19: checked_mul 防 u64 溢出。
    match ns.checked_mul(tsc_freq_mhz) {
        Some(product) => product / 1000,
        None => u64::MAX,
    }
}
