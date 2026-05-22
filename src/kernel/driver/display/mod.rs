//! 显示子系统 (Display Subsystem)
//!
//! 提供完整的显示支持：
//! - **Framebuffer**: 帧缓冲驱动
//! - **HDMI**: 高清多媒体接口
//! - **DisplayPort**: 数字显示接口
//! - **显示控制器**: 统一的显示管理
//! - **多显示器**: 支持多个显示设备
//!
//! ## 架构
//!
//! ```text
//! Display Subsystem
//! ├── framebuffer.rs  # Framebuffer驱动
//! ├── hdmi.rs         # HDMI驱动
//! ├── dp.rs           # DisplayPort驱动
//! └── controller.rs   # 显示控制器抽象
//! ```

pub mod framebuffer;
pub mod hdmi;
pub mod dp;
pub mod controller;

// 导出Framebuffer类型
pub use framebuffer::{
    Framebuffer,
    PixelFormat,
    Color,
    Point,
    Rect,
    colors,
};

// 导出HDMI类型
pub use hdmi::{
    HdmiController,
    Edid,
    VideoMode,
    VideoModeFlags,
    STANDARD_VIDEO_MODES,
};

// 导出DisplayPort类型
pub use dp::{
    DpController,
    Dpcd,
    LinkRate,
    LaneCount,
    TrainingState,
};

// 导出控制器类型
pub use controller::{
    DisplayController,
    DisplayManager,
    DisplayMode,
    DisplayOutput,
    MonitorInfo,
};

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化显示子系统
pub fn display_init() -> framework::Result<()> {
    // 1. 初始化显示管理器
    let _manager = DisplayManager::new();
    
    // 2. 扫描显示控制器
    // TODO: 扫描PCI总线查找GPU
    // TODO: 检测HDMI/DP连接
    
    // 3. 配置显示输出
    // TODO: 设置最佳分辨率
    // TODO: 启用主显示器
    
    Ok(())
}

use super::framework;
