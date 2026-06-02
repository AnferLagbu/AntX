//! 几丁质网络设备协议 (Chitin Net Protocol)
//!
//! 定义网络设备的统一操作接口:
//! - send: 发送网络包
//! - try_receive: 尝试接收 (返回长度, 0=无数据)
//! - get_mac: 读取 MAC 地址
//! - handle_irq: 中断处理
//!
//! 用于 E1000、virtio-net 等网卡驱动。

/// 网络设备发送函数 (dst buffer owned by caller, len bytes to send)
pub type NetSendFn =
    unsafe fn(driver_data: *mut u8, data: *const u8, len: u32) -> i32;

/// 网络设备接收函数 (receive buffer owned by caller)
/// 返回接收到的字节数, 0 表示无数据, <0 表示错误
pub type NetRecvFn =
    unsafe fn(driver_data: *mut u8, buf: *mut u8, buf_len: u32) -> i32;

/// 获取 MAC 地址
pub type NetGetMacFn = unsafe fn(driver_data: *mut u8, mac: &mut [u8; 6]);

/// 中断处理
pub type NetIrqFn = unsafe fn(driver_data: *mut u8);

/// 网络设备操作表
pub struct NetOps {
    /// 发送网络包, 返回 0 成功 / -1 失败
    pub send: NetSendFn,
    /// 接收网络包, 返回长度 (0=空), buf_len 最大缓冲区大小
    pub try_receive: NetRecvFn,
    /// 获取 MAC 地址
    pub get_mac: NetGetMacFn,
    /// 中断处理
    pub handle_irq: Option<NetIrqFn>,
}
