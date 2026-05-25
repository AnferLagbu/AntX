//! 几丁质字符设备协议 (Chitin Char Protocol)
//!
//! 定义字符设备的统一操作接口:
//! - read: 从设备读取字节
//! - write: 向设备写入字节
//! - ioctl: 设备控制 (预留)
//!
//! 用于 VGA、串口、TTY 等字符设备。

/// 字符设备读操作
pub type CharReadFn = unsafe fn(driver_data: *mut core::ffi::c_void, buf: &mut [u8]) -> usize;

/// 字符设备写操作
pub type CharWriteFn = unsafe fn(driver_data: *mut core::ffi::c_void, buf: &[u8]) -> usize;

/// 字符设备操作表
pub struct CharOps {
    /// 读取字节, 返回实际读取数
    pub read: CharReadFn,
    /// 写入字节, 返回实际写入数
    pub write: CharWriteFn,
    /// (预留) ioctl
    pub ioctl: Option<unsafe fn(driver_data: *mut core::ffi::c_void, cmd: u32, arg: usize) -> i32>,
}