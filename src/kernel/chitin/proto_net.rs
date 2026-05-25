//! 几丁质网络设备协议 (Chitin Net Protocol)
//!
//! 定义网络设备的统一操作接口:
//! - send: 发送网络包
//! - poll: 轮询接收队列
//! - get_mac: 读取 MAC 地址
//! - handle_irq: 中断处理
//!
//! 用于 E1000、virtio-net 等网卡驱动。

/// 网络设备发送函数
pub type NetSendFn = unsafe fn(driver_data: *mut core::ffi::c_void, data: *const u8, len: u32) -> i32;

/// 网络设备轮询函数
pub type NetPollFn = unsafe fn(driver_data: *mut core::ffi::c_void);

/// 获取 MAC 地址
pub type NetGetMacFn = unsafe fn(driver_data: *mut core::ffi::c_void, mac: &mut [u8; 6]);

/// 中断处理
pub type NetIrqFn = unsafe fn(driver_data: *mut core::ffi::c_void);

/// 网络设备操作表
pub struct NetOps {
    /// 发送网络包, 返回 0 成功 / -1 失败
    pub send: NetSendFn,
    /// 轮询接收
    pub poll: NetPollFn,
    /// 获取 MAC 地址
    pub get_mac: NetGetMacFn,
    /// 中断处理
    pub handle_irq: Option<NetIrqFn>,
}