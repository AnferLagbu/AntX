//! Vendor 特定 HDMI 子 trait (P2-2)
//!
//! 通用 [`HdmiPort`] trait 抽象跨厂商公共接口; vendor 特定 GPU 芯片
//! (Intel IGP, AMD DCN, Synopsys DesignWare 等) 有私有寄存器布局 + 算法.
//!
//! 本模块定义 vendor trait 抽象, 允许:
//! 1. **trait 组合**: `T: HdmiPort + IntelDpll` 表示"既是 HDMI 端口, 又支持 Intel DPLL 算法"
//! 2. **算法委派**: 默认 [`HdmiPort`] 走 8-bit mul/div 公式; vendor trait 可接管
//!    e.g. Intel DPLL 的 fractional-N, AMD DCN DENTIST 的 spread spectrum
//! 3. **可选实装**: 集成 GPU 主板仅实现 [`IntelDpll`], 独立显卡仅实现 [`AmdDentist`],
//!    通用 SoC 不实现任何 vendor trait (用默认 fallback)
//!
//! ## 当前实装状态
//!
//! 仅定义 trait 抽象 + stub 实现骨架; 真实 vendor 算法实装需要对应芯片手册
//! (Intel Volume 12 / AMD DCN Display Engine Programmer's Guide) 详细寄存器说明,
//! 不在本周期范围. trait 设计已就位, 后续 vendor 实装只需 impl 即可.

use crate::kernel::framework::iomem::IoMem;

// ============================================================================
// Intel IGP DPLL trait
// ============================================================================

/// Intel IGP (Integrated Graphics Processor) DPLL (Display PLL) 高级算法 trait (P2-2).
///
/// 覆盖 Intel Haswell/Skylake/Kaby Lake/Coffee Lake/Comet Lake 等 IGP 系列.
/// 真实硬件使用 Display PLL + PCH transcoder 复杂架构, 涉及:
/// - DPLL 频率合成 (N/M/frac, 而非简单 mul/div)
/// - PCH transcoder 时序配置
/// - HDMI/DVI mode select
/// - Spread spectrum clocking (SSC) 可选
///
/// 默认 [`super::HdmiController`] 用 8-bit mul/div 公式不适用于此算法;
/// Intel IGP 应同时实现本 trait 和 [`super::HdmiPort`], 通过 trait upcasting 接管.
pub trait IntelDpll {
    /// 获取 DPLL 参考时钟频率 (kHz).
    ///
    /// Intel IGP 典型参考时钟: 19.2 MHz (19200 kHz), 24 MHz (24000 kHz),
    /// 25 MHz (25000 kHz) 等, 取决于主板晶振.
    /// 默认 27 MHz (HDMI 规范) 适用于多数现代 Intel IGP.
    fn dpll_reference_clock_khz(&self) -> u32 {
        27_000 // 默认 HDMI 规范标准
    }

    /// 计算 DPLL 参数 (N, M, P) 用于目标像素时钟.
    ///
    /// 返回 `(N, M, P)` 三元组, 满足:
    /// `pixel_clock = reference_clock × N / (M × P)`
    ///
    /// 真实 Intel IGP 需考虑:
    /// - N 范围: 3..=8 (整数)
    /// - M 范围: 5..=80 (整数)
    /// - P 范围: 1, 2, 3, 4, 5, 7 (支持的小数)
    /// - 总频率误差 < 0.5%
    fn compute_dpll_params(&self, target_khz: u32) -> (u8, u8, u8) {
        let _ = target_khz;
        // Stub: 默认 8-bit mul/div 简化
        (1, 1, 1)
    }

    /// 启用 DPLL 并等待锁定.
    ///
    /// # Safety
    /// 调用方必须保证 `iomem` 已映射到 Intel IGP MMIO 区域,
    /// 且相关寄存器偏移在 `iomem` 范围内.
    unsafe fn enable_dpll_and_wait_lock(
        &mut self,
        iomem: &IoMem,
        target_khz: u32,
    ) -> Result<(), VendorError> {
        let _ = (iomem, target_khz);
        // Stub: 默认成功
        Ok(())
    }
}

// ============================================================================
// AMD DCN DENTIST trait
// ============================================================================

/// AMD DCN (Display Core Next) DENTIST clock generator trait (P2-2).
///
/// 覆盖 AMD Raven Ridge / Navi / RDNA 系列 APU 与独立显卡.
/// 真实硬件使用 DENTIST + DISPCLK 架构, 涉及:
/// - DENTIST frequency synthesis (decimal feedback divider)
/// - DISPCLK 与 DENTIST 链路
/// - HDMI 输出 PHY 配置
/// - Spread spectrum 可选
///
/// 与 Intel DPLL 不同, AMD 的算法基于 DSPCLK (Display Clock) 而非独立 PLL,
/// 实装需阅读 AMD DCN Display Engine Programmer's Guide (RDNA 3 §3.4).
pub trait AmdDentist {
    /// 获取 DISPCLK 参考频率 (kHz).
    ///
    /// AMD DCN 典型: 600 MHz (600_000 kHz) - 600_000, 由 SMU 动态调整.
    fn dispclk_khz(&self) -> u32 {
        600_000
    }

    /// 计算 DENTIST divider 用于目标像素时钟.
    ///
    /// `pixel_clock = dispclk / divider` (近似, 实际更复杂)
    fn compute_dentist_divider(&self, target_khz: u32) -> u32 {
        let _ = target_khz;
        1 // Stub
    }

    /// 启用 DENTIST 链路.
    ///
    /// # Safety
    /// 调用方必须保证 `iomem` 已映射到 AMD DCN MMIO 区域.
    unsafe fn enable_dentist_link(
        &mut self,
        iomem: &IoMem,
        target_khz: u32,
    ) -> Result<(), VendorError> {
        let _ = (iomem, target_khz);
        Ok(())
    }
}

// ============================================================================
// Synopsys DesignWare HDMI PHY trait
// ============================================================================

/// Synopsys DesignWare HDMI PHY trait (P2-2).
///
/// 覆盖多数 ARM SoC (i.MX8 / Rockchip / Allwinner / Amlogic 等).
/// 真实硬件使用 phy_clock + tmds_clock 寄存器架构, 实装相对简单.
pub trait SynopsysDwcHdmiPhy {
    /// 配置 PHY 时钟.
    ///
    /// # Safety
    /// 调用方必须保证 `iomem` 已映射到 Synopsys HDMI PHY 寄存器区域.
    unsafe fn configure_phy_clock(
        &self,
        iomem: &IoMem,
        target_khz: u32,
    ) -> Result<(), VendorError> {
        let _ = (iomem, target_khz);
        Ok(())
    }
}

// ============================================================================
// Vendor 错误类型
// ============================================================================

/// Vendor 特定操作错误 (P2-2).
///
/// 通用 [`super::super::framework::DriverError`] 已覆盖大多数场景;
/// VendorError 仅用于 vendor trait 特有的错误 (e.g. PLL 锁定失败, PHY 校准错误).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorError {
    /// PLL 锁定超时.
    PllLockTimeout,
    /// PHY 校准失败.
    PhyCalibrationFailed,
    /// 不支持的频率 (超出范围).
    UnsupportedFrequency(u32),
    /// 其他 vendor 错误.
    Other(&'static str),
}

impl core::fmt::Display for VendorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PllLockTimeout => write!(f, "PLL 锁定超时"),
            Self::PhyCalibrationFailed => write!(f, "PHY 校准失败"),
            Self::UnsupportedFrequency(khz) => write!(f, "不支持的频率: {} kHz", khz),
            Self::Other(msg) => write!(f, "vendor 错误: {}", msg),
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock 实现, 仅用于测试 trait API.
    struct MockIntelGpu;

    impl IntelDpll for MockIntelGpu {
        fn dpll_reference_clock_khz(&self) -> u32 {
            19_200 // Intel 典型 19.2 MHz 晶振
        }

        fn compute_dpll_params(&self, target_khz: u32) -> (u8, u8, u8) {
            // Mock: target=148500, ref=19200: 148500/19200 = 7.73, 简化为 N=8
            let _ = target_khz;
            (8, 1, 1)
        }
    }

    struct MockAmdGpu;

    impl AmdDentist for MockAmdGpu {
        fn dispclk_khz(&self) -> u32 {
            600_000
        }
    }

    #[test]
    fn test_intel_dpll_default_reference_clock() {
        // P2-2: IntelDpll 默认参考时钟 27 MHz.
        struct DefaultIntel;
        impl IntelDpll for DefaultIntel {}
        let gpu = DefaultIntel;
        assert_eq!(gpu.dpll_reference_clock_khz(), 27_000);
    }

    #[test]
    fn test_intel_dpll_custom_reference_clock() {
        // P2-2: Mock 19.2 MHz 晶振.
        let gpu = MockIntelGpu;
        assert_eq!(gpu.dpll_reference_clock_khz(), 19_200);
    }

    #[test]
    fn test_intel_dpll_compute_params() {
        // P2-2: Mock DPLL 参数计算.
        let gpu = MockIntelGpu;
        let (n, m, p) = gpu.compute_dpll_params(148_500);
        assert_eq!((n, m, p), (8, 1, 1));
    }

    #[test]
    fn test_amd_dentist_default_dispclk() {
        // P2-2: AmdDentist 默认 DISPCLK 600 MHz.
        struct DefaultAmd;
        impl AmdDentist for DefaultAmd {}
        let gpu = DefaultAmd;
        assert_eq!(gpu.dispclk_khz(), 600_000);
    }

    #[test]
    fn test_amd_dentist_custom_dispclk() {
        // P2-2: Mock DISPCLK 600 MHz.
        let gpu = MockAmdGpu;
        assert_eq!(gpu.dispclk_khz(), 600_000);
    }

    #[test]
    fn test_vendor_error_display() {
        // P2-2: VendorError Display 实现.
        assert_eq!(format!("{}", VendorError::PllLockTimeout), "PLL 锁定超时");
        assert_eq!(format!("{}", VendorError::PhyCalibrationFailed), "PHY 校准失败");
        assert_eq!(
            format!("{}", VendorError::UnsupportedFrequency(148500)),
            "不支持的频率: 148500 kHz"
        );
        assert_eq!(
            format!("{}", VendorError::Other("test error")),
            "vendor 错误: test error"
        );
    }

    #[test]
    fn test_vendor_error_equality() {
        // P2-2: VendorError 必须支持 PartialEq + Eq.
        assert_eq!(VendorError::PllLockTimeout, VendorError::PllLockTimeout);
        assert_ne!(VendorError::PllLockTimeout, VendorError::PhyCalibrationFailed);
    }
}

// 抑制未使用导入警告 (IoMem 用于 trait 默认 impl)
#[allow(dead_code)]
fn _ensure_iomem_imported(_: IoMem) {}
