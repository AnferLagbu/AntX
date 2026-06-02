//! 几丁质输入设备协议 (Chitin Input Protocol)
//!
//! 定义输入设备的统一操作接口:
//! - read_char: 读取一个字符 (非阻塞)
//! - has_char: 检查缓冲区是否有数据
//! - handle_irq: 中断处理
//!
//! 用于键盘、鼠标等输入设备。

/// 输入设备读字符
pub type InputReadFn = unsafe fn(driver_data: *mut u8) -> Option<u8>;

/// 输入设备检查可读
pub type InputHasDataFn = unsafe fn(driver_data: *mut u8) -> bool;

/// 输入设备中断处理
pub type InputIrqFn = unsafe fn(driver_data: *mut u8);

/// 输入设备操作表
pub struct InputOps {
    /// 读取一个字符, None 表示无数据
    pub read_char: InputReadFn,
    /// 检查是否有可读数据
    pub has_char: InputHasDataFn,
    /// 中断处理 (键盘 IRQ1)
    pub handle_irq: InputIrqFn,
}
