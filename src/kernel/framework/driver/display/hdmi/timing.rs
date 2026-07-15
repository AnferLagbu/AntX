//! HDMI 时序参数 (H/V total/active/sync) 配置
//!
//! HDMI 控制器时序寄存器 (16-bit, 每项占 2 字节偏移):
//! - H_TOTAL: 总水平像素 (active + blanking)
//! - H_ACTIVE: 水平有效像素
//! - H_SYNC_OFFSET: 水平同步信号前沿 (从 blanking 开始到 sync 起始)
//! - H_SYNC_PW: 水平同步脉冲宽度
//! - V_TOTAL: 总垂直行数 (active + blanking)
//! - V_ACTIVE: 垂直有效行数
//! - V_SYNC_OFFSET: 垂直同步信号前沿
//! - V_SYNC_PW: 垂直同步脉冲宽度
//!
//! ## 厂商差异
//!
//! - **Intel IGP (HSW/SKL)**: 每项占 4 字节, 需用 32-bit 写入
//! - **AMD DCN**: DENTIST_HWITCH_H_TOTAL 等分散寄存器
//! - **通用 SoC**: 通常 16-bit 紧凑排列 (本实现)
//! - **QEMU Bochs DISPI**: 使用 VBE index/data port I/O, 不走 MMIO
//!
//! ## P0-3 DMT 精度
//!
//! 派生优先级: DMT lookup table → 简化公式 fallback.
//! lookup 覆盖 [`STANDARD_VIDEO_MODES`] 全部 10 个常见模式 (DMT 0x01/0x08/0x10/0x32/0x44/0x52/0x55/0x5F, CVT-RB v2).

use super::DriverError;
use super::VideoMode;
use crate::kernel::framework::iomem::IoMem;

// ============================================================================
// 时序寄存器偏移
// ============================================================================

/// H_TOTAL 寄存器偏移 (16-bit, 2 字节连续).
pub(super) const HDMI_H_TOTAL_REG_OFFSET: usize = 0x068;
/// H_ACTIVE 寄存器偏移 (16-bit).
pub(super) const HDMI_H_ACTIVE_REG_OFFSET: usize = 0x06A;
/// V_TOTAL 寄存器偏移 (16-bit).
pub(super) const HDMI_V_TOTAL_REG_OFFSET: usize = 0x06C;
/// V_ACTIVE 寄存器偏移 (16-bit).
pub(super) const HDMI_V_ACTIVE_REG_OFFSET: usize = 0x06E;
/// H_SYNC_OFFSET 寄存器偏移 (16-bit).
pub(super) const HDMI_H_SYNC_OFFSET_REG_OFFSET: usize = 0x070;
/// H_SYNC_PW 寄存器偏移 (16-bit).
pub(super) const HDMI_H_SYNC_PW_REG_OFFSET: usize = 0x072;
/// V_SYNC_OFFSET 寄存器偏移 (16-bit).
pub(super) const HDMI_V_SYNC_OFFSET_REG_OFFSET: usize = 0x074;
/// V_SYNC_PW 寄存器偏移 (16-bit).
pub(super) const HDMI_V_SYNC_PW_REG_OFFSET: usize = 0x076;

// ============================================================================
// VideoTiming 结构
// ============================================================================

/// HDMI 时序参数 (从 VideoMode 派生)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoTiming {
    /// 水平有效像素数 (= width)
    pub h_active: u16,
    /// 水平总像素数 (active + blanking)
    pub h_total: u16,
    /// 水平同步信号前沿 (像素数)
    pub h_sync_offset: u16,
    /// 水平同步脉冲宽度 (像素数)
    pub h_sync_pulse_width: u16,
    /// 垂直有效行数 (= height)
    pub v_active: u16,
    /// 垂直总行数 (active + blanking)
    pub v_total: u16,
    /// 垂直同步信号前沿 (行数)
    pub v_sync_offset: u16,
    /// 垂直同步脉冲宽度 (行数)
    pub v_sync_pulse_width: u16,
}

// ============================================================================
// DMT / CVT-RB 精度表
// ============================================================================

// DMT_TIMINGS lookup table (P0-3).
//
// 来源: VESA DMT 1.0 / VESA CVT-RB v2 / CTA-861-G.
//
// 数据格式: (width, height, refresh_rate, VideoTiming)
// 同步极性: 全部为 negative (默认 flags, 现代显示器).
//
// 覆盖 `STANDARD_VIDEO_MODES` 全部 10 个模式 (见 `super::STANDARD_VIDEO_MODES`),
// 真实显示器兼容性显著优于 5% blanking 公式.
pub const DMT_TIMINGS: &[(u16, u16, u8, VideoTiming)] = &[
    // 640x480@60 (DMT ID 0x01): 25.175 MHz
    (
        640, 480, 60,
        VideoTiming {
            h_active: 640, h_total: 800, h_sync_offset: 16, h_sync_pulse_width: 96,
            v_active: 480, v_total: 525, v_sync_offset: 10, v_sync_pulse_width: 2,
        },
    ),
    // 800x600@60 (DMT ID 0x08): 40.0 MHz
    (
        800, 600, 60,
        VideoTiming {
            h_active: 800, h_total: 1056, h_sync_offset: 88, h_sync_pulse_width: 128,
            v_active: 600, v_total: 628, v_sync_offset: 23, v_sync_pulse_width: 4,
        },
    ),
    // 1024x768@60 (DMT ID 0x10): 65.0 MHz
    (
        1024, 768, 60,
        VideoTiming {
            h_active: 1024, h_total: 1344, h_sync_offset: 24, h_sync_pulse_width: 136,
            v_active: 768, v_total: 806, v_sync_offset: 3, v_sync_pulse_width: 6,
        },
    ),
    // 1280x720@60 (DMT ID 0x55 RB): 74.25 MHz
    (
        1280, 720, 60,
        VideoTiming {
            h_active: 1280, h_total: 1650, h_sync_offset: 110, h_sync_pulse_width: 40,
            v_active: 720, v_total: 750, v_sync_offset: 5, v_sync_pulse_width: 5,
        },
    ),
    // 1280x1024@60 (DMT ID 0x32): 108.0 MHz
    (
        1280, 1024, 60,
        VideoTiming {
            h_active: 1280, h_total: 1688, h_sync_offset: 48, h_sync_pulse_width: 112,
            v_active: 1024, v_total: 1066, v_sync_offset: 1, v_sync_pulse_width: 3,
        },
    ),
    // 1920x1080@60 (DMT ID 0x52): 148.5 MHz
    // 注意: 这是 DMT (full blanking), 不是 CVT-RB (138.5 MHz, h=2080).
    // `STANDARD_VIDEO_MODES` 用 148.5 MHz, 故 lookup 用 DMT 值.
    (
        1920, 1080, 60,
        VideoTiming {
            h_active: 1920, h_total: 2200, h_sync_offset: 88, h_sync_pulse_width: 44,
            v_active: 1080, v_total: 1125, v_sync_offset: 4, v_sync_pulse_width: 5,
        },
    ),
    // 1920x1200@60 (DMT ID 0x44): 193.25 MHz, 全消隐
    (
        1920, 1200, 60,
        VideoTiming {
            h_active: 1920, h_total: 2592, h_sync_offset: 136, h_sync_pulse_width: 32,
            v_active: 1200, v_total: 1245, v_sync_offset: 3, v_sync_pulse_width: 6,
        },
    ),
    // 2560x1440@60 (CVT-RB v2): 241.5 MHz
    (
        2560, 1440, 60,
        VideoTiming {
            h_active: 2560, h_total: 2720, h_sync_offset: 48, h_sync_pulse_width: 32,
            v_active: 1440, v_total: 1481, v_sync_offset: 3, v_sync_pulse_width: 5,
        },
    ),
    // 2560x1600@60 (CVT-RB v2): 268.5 MHz
    (
        2560, 1600, 60,
        VideoTiming {
            h_active: 2560, h_total: 2720, h_sync_offset: 48, h_sync_pulse_width: 32,
            v_active: 1600, v_total: 1646, v_sync_offset: 3, v_sync_pulse_width: 6,
        },
    ),
    // 3840x2160@60 (DMT ID 0x5F / CVT-RB v2): 594.0 MHz
    (
        3840, 2160, 60,
        VideoTiming {
            h_active: 3840, h_total: 4400, h_sync_offset: 88, h_sync_pulse_width: 44,
            v_active: 2160, v_total: 2250, v_sync_offset: 4, v_sync_pulse_width: 5,
        },
    ),
];

/// 在 DMT lookup table 中查找精确时序参数。
///
/// 返回 `Some(VideoTiming)` 如果 (width, height, refresh_rate) 三元组精确匹配;
/// 否则返回 `None`, 调用方应 fallback 到公式派生.
///
/// `interlaced` 字段不影响查找 (lookup 当前仅覆盖 progressive).
pub fn lookup_dmt_timing(mode: &VideoMode) -> Option<VideoTiming> {
    for &(w, h, rate, timing) in DMT_TIMINGS {
        if w == mode.width && h == mode.height && rate == mode.refresh_rate {
            return Some(timing);
        }
    }
    None
}

/// 从 VideoMode 派生时序参数 (DMT lookup 优先, 公式 fallback)。
///
/// 优先级:
/// 1. [`DMT_TIMINGS`] lookup table (覆盖 STANDARD_VIDEO_MODES 全部 10 个常见模式)
/// 2. 简化公式 fallback (未知模式)
///
/// 公式 fallback:
/// - `v_blank = max(1, v_active * 5 / 100)` (5% 垂直 blanking)
/// - `v_total = v_active + v_blank`
/// - `h_total = pixel_clock_hz / v_total / refresh_rate` (反推)
/// - `h_blank = h_total - h_active`
/// - `h_sync_offset = h_blank / 4` (典型 25% 前沿)
/// - `h_sync_pulse_width = h_blank / 8` (典型 12.5% 脉冲)
/// - `v_sync_offset = 1` (典型 1 行前沿)
/// - `v_sync_pulse_width = 3` (典型 3 行脉冲)
///
/// 公式 fallback 与 VESA DMT 标准值的偏差 (lookup 不覆盖时):
/// - 1920x1080@60Hz: v_total=1134 (DMT=1125), h_total≈2182 (DMT=2200)
/// - 误差 < 5%, 对真实显示器可能略偏, 但大多数现代显示器容忍.
///
/// 对于 refresh_rate == 0 或 pixel_clock_khz == 0 的边界情况, 使用 fallback
/// (v_total = v_active + 50, h_total = h_active + 200 整行整列扩展规则).
pub(super) fn derive_video_timing(mode: &VideoMode) -> VideoTiming {
    // P0-3 精度扩展: DMT lookup 优先
    if let Some(timing) = lookup_dmt_timing(mode) {
        return timing;
    }
    // 公式 fallback
    let v_active = mode.height;
    let h_active = mode.width;

    // 派生 v_total
    let v_total = if mode.refresh_rate > 0 && mode.pixel_clock_khz > 0 {
        // 典型 V blanking = 5% V active
        let v_blank = ((v_active as u32) * 5 / 100).max(1);
        (v_active as u32 + v_blank) as u16
    } else {
        // fallback
        v_active + 50
    };

    // 派生 h_total = pixel_clock_hz / v_total / refresh_rate
    let h_total = if mode.refresh_rate > 0 && mode.pixel_clock_khz > 0 {
        let h_total_u32 = (mode.pixel_clock_khz * 1000)
            / ((v_total as u32) * (mode.refresh_rate as u32));
        // 强制 h_total >= h_active + 1 (至少 1 像素 blanking)
        h_total_u32.max((h_active as u32) + 1) as u16
    } else {
        h_active + 200
    };

    let h_blank = h_total.saturating_sub(h_active);

    // Sync 偏移和脉冲宽度
    let h_sync_offset = h_blank / 4;
    let h_sync_pulse_width = (h_blank / 8).max(1);
    let v_sync_offset: u16 = 1;
    let v_sync_pulse_width: u16 = 3;

    VideoTiming {
        h_active,
        h_total,
        h_sync_offset,
        h_sync_pulse_width,
        v_active,
        v_total,
        v_sync_offset,
        v_sync_pulse_width,
    }
}

// ============================================================================
// 时序寄存器写入
// ============================================================================

/// 写入 16-bit 时序寄存器 (低字节 + 高字节)。
///
/// # Safety
/// 调用方必须保证 `reg_offset + 2 <= iomem.len()` (2 字节连续写入)。
#[inline]
pub(super) unsafe fn write_timing_register_u16(iomem: &IoMem, reg_offset: usize, value: u16) {
    iomem.write_u8(reg_offset, (value & 0xFF) as u8);
    iomem.write_u8(reg_offset + 1, ((value >> 8) & 0xFF) as u8);
}

/// 配置 HDMI 时序参数 (8 个 16-bit 寄存器)。
///
/// 写入顺序: H_TOTAL → H_ACTIVE → V_TOTAL → V_ACTIVE →
/// H_SYNC_OFFSET 水平同步偏移 → H_SYNC_PW 水平同步脉宽 →
/// V_SYNC_OFFSET 垂直同步偏移 → V_SYNC_PW 垂直同步脉宽
///
/// # Safety
/// 调用方必须保证:
/// - `iomem` 已映射到有效 HDMI 控制器 MMIO 区域
/// - `HDMI_V_SYNC_PW_REG_OFFSET + 2 <= iomem.len()` (最后一个寄存器结束)
pub(super) unsafe fn configure_hdmi_timing(iomem: &IoMem, timing: &VideoTiming) { unsafe {
    write_timing_register_u16(iomem, HDMI_H_TOTAL_REG_OFFSET, timing.h_total);
    write_timing_register_u16(iomem, HDMI_H_ACTIVE_REG_OFFSET, timing.h_active);
    write_timing_register_u16(iomem, HDMI_V_TOTAL_REG_OFFSET, timing.v_total);
    write_timing_register_u16(iomem, HDMI_V_ACTIVE_REG_OFFSET, timing.v_active);
    write_timing_register_u16(
        iomem,
        HDMI_H_SYNC_OFFSET_REG_OFFSET,
        timing.h_sync_offset,
    );
    write_timing_register_u16(
        iomem,
        HDMI_H_SYNC_PW_REG_OFFSET,
        timing.h_sync_pulse_width,
    );
    write_timing_register_u16(
        iomem,
        HDMI_V_SYNC_OFFSET_REG_OFFSET,
        timing.v_sync_offset,
    );
    write_timing_register_u16(
        iomem,
        HDMI_V_SYNC_PW_REG_OFFSET,
        timing.v_sync_pulse_width,
    );
}}

// 抑制未使用导入警告 (DriverError 由后续模块使用, 此处预留).
fn _ensure_driver_error_imported(_: DriverError) {}
