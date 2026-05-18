//! USB 子系统 (USB Subsystem)
//!
//! 提供完整的USB支持：
//! - **USB核心**: 设备枚举和管理
//! - **xHCI驱动**: USB 3.0主机控制器
//! - **HID类**: 人机接口设备（键盘、鼠标）
//! - **大容量存储**: USB存储设备
//!
//! ## 架构
//!
//! ```text
//! USB Subsystem
//! ├── usb_core.rs    # 核心框架
//! ├── xhci.rs        # xHCI控制器
//! ├── hid.rs         # HID类驱动
//! └── mass_storage.rs # 大容量存储
//! ```

pub mod usb_core;
pub mod xhci;

// 导出常用类型
pub use usb_core::{
    UsbCore,
    UsbDevice,
    UsbSpeed,
    DeviceState,
    DeviceClass,
    DeviceDescriptor,
    ConfigurationDescriptor,
    InterfaceDescriptor,
    EndpointDescriptor,
    Urb,
    HostController,
};

pub use xhci::XhciController;

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化USB子系统
pub fn usb_init() -> framework::Result<()> {
    // TODO: 扫描PCI总线查找xHCI控制器
    // TODO: 初始化找到的控制器
    // TODO: 枚举USB设备
    
    Ok(())
}

use super::framework;
