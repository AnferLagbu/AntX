//! HDMI (High-Definition Multimedia Interface) 控制器
//!
//! 完整实现已迁移至 `services::driver::display::hdmi` (0 unsafe).
//! 本文件仅保留 re-export 以保持向后兼容.
//!
//! # Safety
//!
//! 本文件不含 unsafe 代码. HDMI 驱动安全逻辑在 services 层.

// 重新导出 services 层的 HDMI 驱动类型
pub use crate::kernel::services::driver::display::hdmi::{
    Edid, EdidBasicDisplay, EdidColorCharacteristics, EdidDetailedTiming, EdidError,
    HPD_STATUS_BIT, HPD_STATUS_REG_OFFSET, HdmiController, HdmiError, REQUIRED_IOMEM_SIZE,
    STANDARD_VIDEO_MODES, VideoMode, VideoModeFlags, VideoTiming, compute_pixel_clock_mul_div,
    derive_video_timing, fill_mock_edid, lookup_dmt_timing,
};
