//! HDMI 同步信号极性 + TMDS 输出使能
//!
//! 同步极性寄存器 (8-bit):
//! - bit 0: H 同步极性 (1=positive/active-high, 0=negative/active-low)
//! - bit 1: V 同步极性 (1=positive/active-high, 0=negative/active-low)
//!
//! 现代显示器/HDMI 通常使用 negative sync (bit 0=0, bit 1=0);
//! 部分老式 CEA/DMT 模式需要 positive sync (e.g. 480i, 576i).
//!
//! TMDS 输出使能寄存器 (8-bit):
//! - bit 0: TMDS 输出使能 (1=on, 0=off)
//!
//! ## 写入时序约束
//!
//! 写入 TMDS enable bit 前必须确保:
//! 1. 像素时钟 PLL 已锁定 ([`super::pixel_clock::poll_hdmi_pll_locked`])
//! 2. 时序参数已配置 ([`super::timing::configure_hdmi_timing`])
//! 3. 同步极性已配置 (本模块)
//! 否则显示器可能收到无效信号, 显示异常.
//!
//! ## 厂商差异
//!
//! - **Intel IGP**: TMDS enable 在 PCH transcoder config (HSW/HSW+), 需额外
//!   "transcoder enable" 步骤
//! - **AMD DCN**: DIG_FE_EN + DIG_BE_EN 两个独立使能
//! - **通用 SoC**: 通常单 bit TMDS enable (本实现)

use crate::kernel::framework::iomem::IoMem;

// ============================================================================
// 同步极性 + TMDS 寄存器偏移
// ============================================================================

/// HDMI 同步极性寄存器偏移 (8-bit)。
pub(super) const HDMI_SYNC_POL_REG_OFFSET: usize = 0x078;
/// 同步极性寄存器 H bit (bit 0)
pub(super) const HDMI_SYNC_POL_H_BIT: u8 = 0x01;
/// 同步极性寄存器 V bit (bit 1)
pub(super) const HDMI_SYNC_POL_V_BIT: u8 = 0x02;

/// HDMI TMDS 输出使能寄存器偏移 (8-bit)。
pub(super) const HDMI_TMDS_ENABLE_REG_OFFSET: usize = 0x079;
/// TMDS 输出使能 bit (bit 0)
pub(super) const HDMI_TMDS_ENABLE_BIT: u8 = 0x01;

// ============================================================================
// 同步极性 + TMDS 配置
// ============================================================================

/// 配置 HDMI 同步信号极性 (1 字节寄存器)。
///
/// - `hsync_positive = true`: H 同步 active-high
/// - `vsync_positive = true`: V 同步 active-high
/// - 均为 false (默认): 双同步 active-low (现代显示器常用)
///
/// # Safety
/// 调用方必须保证 `HDMI_SYNC_POL_REG_OFFSET + 1 <= iomem.len()`.
pub(super) unsafe fn configure_hdmi_sync_polarity(
    iomem: &IoMem,
    hsync_positive: bool,
    vsync_positive: bool,
) {
    let mut val = 0u8;
    if hsync_positive {
        val |= HDMI_SYNC_POL_H_BIT;
    }
    if vsync_positive {
        val |= HDMI_SYNC_POL_V_BIT;
    }
    iomem.write_u8(HDMI_SYNC_POL_REG_OFFSET, val);
}

/// 启用 HDMI TMDS 输出 (1 字节寄存器, bit 0 = enable)。
///
/// 调用时机: 必须在 `configure_hdmi_pixel_clock` + `configure_hdmi_timing` +
/// `configure_hdmi_sync_polarity` 全部完成后调用; 否则显示器会收到无效信号.
///
/// # Safety
/// 调用方必须保证 `HDMI_TMDS_ENABLE_REG_OFFSET + 1 <= iomem.len()`.
pub(super) unsafe fn enable_hdmi_tmds_output(iomem: &IoMem) {
    iomem.write_u8(HDMI_TMDS_ENABLE_REG_OFFSET, HDMI_TMDS_ENABLE_BIT);
}

/// 关闭 HDMI TMDS 输出 (1 字节寄存器, bit 0 = disable)。
///
/// 用于节能 / 切模式时的临时关闭.
///
/// # Safety
/// 调用方必须保证 `HDMI_TMDS_ENABLE_REG_OFFSET + 1 <= iomem.len()`.
#[allow(dead_code)] // 待 shutdown() / mode switch 实装时启用.
pub(super) unsafe fn disable_hdmi_tmds_output(iomem: &IoMem) {
    iomem.write_u8(HDMI_TMDS_ENABLE_REG_OFFSET, 0x00);
}
