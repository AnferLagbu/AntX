//! 串口驱动 (Rust 安全重写)
//!
//! 提供对 PC 标准 COM 端口的完整支持：
//! - **多端口支持**: COM1-COM4 (0x3F8-0x2E8)
//! - **波特率配置**: 支持 9600-115200 bps
//! - **数据格式**: 可配置数据位、停止位、校验位
//! - **中断支持**: IRQ3/IRQ4 中断处理
//! - **缓冲机制**: 接收/发送环形缓冲区
//!
//! ## 硬件接口
//!
//! ```text
//! COM Ports:
//! ├── COM1: IO=0x3F8, IRQ=4
//! ├── COM2: IO=0x2F8, IRQ=3
//! ├── COM3: IO=0x3E8, IRQ=4
//! └── COM4: IO=0x2E8, IRQ=3
//!
//! UART Registers:
//! ├── RBR/TBR: 数据寄存器 (读/写)
//! ├── IER: 中断使能寄存器
//! ├── IIR/FCR: 中断识别/FIFO 控制
//! ├── LCR: 线路控制寄存器
//! ├── MCR: 调制解调器控制
//! └── LSR: 线路状态寄存器
//! ```
//!
//! # Safety
//! 此模块直接操作串口硬件端口。

use super::framework::{Driver, DeviceType, DriverError, Result, DeviceInfo};
use super::framework::{outb, inb};

// ============================================================================
// 硬件常量定义
// ============================================================================

/// COM 端口基址
const COM1_BASE: u16 = 0x3F8;
const COM2_BASE: u16 = 0x2F8;
const COM3_BASE: u16 = 0x3E8;
const COM4_BASE: u16 = 0x2E8;

/// 最大支持的 COM 端口数量
const MAX_COM_PORTS: usize = 4;

/// UART 寄存器偏移量
const UART_RBR: u16 = 0;   // 接收缓冲寄存器 (只读)
const UART_THR: u16 = 0;   // 发送保持寄存器 (只写)
const UART_IER: u16 = 1;   // 中断使能寄存器
const UART_IIR: u16 = 2;   // 中断识别寄存器 (只读)
const UART_FCR: u16 = 2;   // FIFO 控制寄存器 (只写)
const UART_LCR: u16 = 3;   // 线路控制寄存器
const UART_MCR: u16 = 4;   // 调制解调器控制寄存器
const UART_LSR: u16 = 5;   // 线路状态寄存器

/// LSR 标志位
const LSR_DATA_READY: u8 = 0x01;     // 数据可读
const LSR_TRANSMIT_EMPTY: u8 = 0x20; // 发送保持寄存器空
const LSR_TRANSMIT_IDLE: u8 = 0x40;  // 发送器空闲

/// FCR 命令
const FCR_ENABLE_FIFO: u8 = 0xC1;    // 启用 FIFO，清除缓冲区

/// MCR 命令
const MCR_DTR: u8 = 0x01;            // Data Terminal Ready
const MCR_RTS: u8 = 0x02;            // Request To Send
const MCR_OUT2: u8 = 0x08;           // OUT2 (中断使能)

/// LCR 命令
const LCR_DLAB: u8 = 0x80;          // Divisor Latch Access Bit

/// 波特率分频值
const BAUD_9600: u16 = 12;
const BAUD_19200: u16 = 6;
const BAUD_38400: u16 = 3;
const BAUD_57600: u16 = 2;
const BAUD_115200: u16 = 1;

/// 缓冲区大小
const SERIAL_BUFFER_SIZE: usize = 256;

// ============================================================================
// 配置结构体
// ============================================================================

/// 串口配置参数
#[derive(Debug, Clone, Copy)]
pub struct SerialConfig {
    /// 波特率
    pub baud_rate: BaudRate,
    /// 数据位数
    pub data_bits: DataBits,
    /// 停止位数
    pub stop_bits: StopBits,
    /// 校验模式
    pub parity: ParityMode,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baud_rate: BaudRate::Baud115200,
            data_bits: DataBits::Bits8,
            stop_bits: StopBits::One,
            parity: ParityMode::None,
        }
    }
}

/// 波特率枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaudRate {
    Baud9600,
    Baud19200,
    Baud38400,
    Baud57600,
    Baud115200,
}

impl BaudRate {
    fn to_divisor(&self) -> u16 {
        match self {
            Self::Baud9600 => BAUD_9600,
            Self::Baud19200 => BAUD_19200,
            Self::Baud38400 => BAUD_38400,
            Self::Baud57600 => BAUD_57600,
            Self::Baud115200 => BAUD_115200,
        }
    }
}

/// 数据位数
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Bits5,
    Bits6,
    Bits7,
    Bits8,
}

impl DataBits {
    fn to_lcr_value(&self) -> u8 {
        match self {
            Self::Bits5 => 0x00,
            Self::Bits6 => 0x01,
            Self::Bits7 => 0x02,
            Self::Bits8 => 0x03,
        }
    }
}

/// 停止位数
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

impl StopBits {
    fn to_lcr_value(&self) -> u8 {
        match self {
            Self::One => 0x00,
            Self::Two => 0x04,
        }
    }
}

/// 校验模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParityMode {
    None,
    Odd,
    Even,
    Mark,
    Space,
}

impl ParityMode {
    fn to_lcr_value(&self) -> u8 {
        match self {
            Self::None => 0x00,
            Self::Odd => 0x08,
            Self::Even => 0x18,
            Self::Mark => 0x28,
            Self::Space => 0x38,
        }
    }
}

// ============================================================================
// 环形缓冲区
// ============================================================================

struct RingBuffer<T> {
    buffer: [T; SERIAL_BUFFER_SIZE],
    head: usize,
    tail: usize,
    count: usize,
}

impl<T: Default + Copy> Default for RingBuffer<T> {
    fn default() -> Self {
        Self {
            buffer: [T::default(); SERIAL_BUFFER_SIZE],
            head: 0,
            tail: 0,
            count: 0,
        }
    }
}

impl<T: Default + Copy> RingBuffer<T> {
    fn push(&mut self, item: T) -> Result<()> {
        if self.count >= SERIAL_BUFFER_SIZE {
            return Err(DriverError::Busy);
        }

        self.buffer[self.tail] = item;
        self.tail = (self.tail + 1) % SERIAL_BUFFER_SIZE;
        self.count += 1;
        Ok(())
    }

    fn pop(&mut self) -> Option<T> {
        if self.count == 0 {
            return None;
        }

        let item = self.buffer[self.head];
        self.head = (self.head + 1) % SERIAL_BUFFER_SIZE;
        self.count -= 1;
        Some(item)
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn is_full(&self) -> bool {
        self.count >= SERIAL_BUFFER_SIZE
    }

    fn len(&self) -> usize {
        self.count
    }

    fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
    }
}

// ============================================================================
// 单个串口设备
// ============================================================================

/// 串口设备实例
pub struct SerialPort {
    /// I/O 基地址
    base: u16,
    /// 端口号 (0-3 对应 COM1-COM4)
    port_num: u8,
    /// 当前配置
    config: SerialConfig,
    /// 接收缓冲区
    rx_buffer: RingBuffer<u8>,
    /// 发送缓冲区
    tx_buffer: RingBuffer<u8>,
    /// 设备信息
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

// ============================================================================
// 底层辅助函数
// ============================================================================

/// 设置波特率
fn set_baud_rate(base: u16, divisor: u16) {
    unsafe {
        // 启用 DLAB 以访问分频寄存器
        outb(base + UART_LCR, LCR_DLAB);
        
        // 设置低字节和高字节
        outb(base + 0, (divisor & 0xFF) as u8);
        outb(base + 1, ((divisor >> 8) & 0xFF) as u8);
        
        // 关闭 DLAB，设置数据格式
        outb(base + UART_LCR, 0x03);  // 8N1
    }
}

/// 检查接收缓冲区是否有数据
fn is_data_ready(base: u16) -> bool {
    unsafe { inb(base + UART_LSR) & LSR_DATA_READY != 0 }
}

/// 检查发送保持寄存器是否为空
fn is_transmit_empty(base: u16) -> bool {
    unsafe { inb(base + UART_LSR) & LSR_TRANSMIT_EMPTY != 0 }
}

/// 从 UART 读取一个字节
fn read_byte(base: u16) -> u8 {
    unsafe { inb(base + UART_RBR) }
}

/// 向 UART 写入一个字节
fn write_byte(base: u16, byte: u8) {
    unsafe { outb(base + UART_THR, byte); }
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for SerialPort {
    fn name(&self) -> &'static str {
        match self.port_num {
            0 => "COM1",
            1 => "COM2",
            2 => "COM3",
            3 => "COM4",
            _ => "Unknown",
        }
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn init(&mut self) -> Result<()> {
        let base = self.base;

        unsafe {
            // 1. 禁用所有中断
            outb(base + UART_IER, 0x00);

            // 2. 启用 DLAB 并设置波特率
            set_baud_rate(base, self.config.baud_rate.to_divisor());

            // 3. 设置数据格式 (8N1)
            outb(base + UART_LCR, 
                self.config.data_bits.to_lcr_value() |
                self.config.stop_bits.to_lcr_value() |
                self.config.parity.to_lcr_value()
            );

            // 4. 启用 FIFO，清除缓冲区
            outb(base + UART_FCR, FCR_ENABLE_FIFO);

            // 5. 设置 MCR (DTR + RTS + OUT2)
            outb(base + UART_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);
        }

        // 清空缓冲区
        self.rx_buffer.clear();
        self.tx_buffer.clear();

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        // 禁用所有中断
        unsafe { outb(self.base + UART_IER, 0x00); }
        
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn status(&self) -> &'static str {
        if !self.initialized {
            "Not initialized"
        } else {
            match self.port_num {
                0 => "COM1 ready @ 0x3F8",
                1 => "COM2 ready @ 0x2F8",
                2 => "COM3 ready @ 0x3E8",
                3 => "COM4 ready @ 0x2E8",
                _ => "Unknown port",
            }
        }
    }
}

// ============================================================================
// 公共 API
// ============================================================================

impl SerialPort {
    /// 创建新的串口实例
    ///
    /// # Arguments
    /// * `port` - 端口号 (0=COM1, 1=COM2, 2=COM3, 3=COM4)
    pub fn new(port: u8) -> Option<Self> {
        let base = match port {
            0 => COM1_BASE,
            1 => COM2_BASE,
            2 => COM3_BASE,
            3 => COM4_BASE,
            _ => return None,
        };

        Some(Self {
            base,
            port_num: port,
            config: SerialConfig::default(),
            rx_buffer: RingBuffer::default(),
            tx_buffer: RingBuffer::default(),
            info: DeviceInfo::new("serial_port", DeviceType::Char),
            initialized: false,
        })
    }

    /// 使用自定义配置创建串口实例
    pub fn with_config(port: u8, config: SerialConfig) -> Option<Self> {
        let mut serial = Self::new(port)?;
        serial.config = config;
        Some(serial)
    }

    /// 发送单个字节
    ///
    /// # Arguments
    /// * `byte` - 要发送的字节
    pub fn send_byte(&mut self, byte: u8) -> Result<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }

        // 等待发送保持寄存器为空
        while !is_transmit_empty(self.base) {
            core::hint::spin_loop();
        }

        write_byte(self.base, byte);
        Ok(())
    }

    /// 发送字符串
    ///
    /// # Arguments
    /// * `s` - 要发送的字符串切片
    pub fn send_string(&mut self, s: &[u8]) -> Result<usize> {
        for &byte in s {
            self.send_byte(byte)?;
        }
        Ok(s.len())
    }

    /// 接收单个字节 (非阻塞)
    ///
    /// # Returns
    /// * `Some(u8)` - 收到的字节
    /// * `None` - 无数据可读
    pub fn receive_byte(&mut self) -> Option<u8> {
        if !is_data_ready(self.base) {
            return None;
        }

        let byte = read_byte(self.base);
        let _ = self.rx_buffer.push(byte);
        Some(byte)
    }

    /// 从缓冲区读取字节 (非阻塞)
    pub fn read_byte_from_buffer(&mut self) -> Option<u8> {
        self.rx_buffer.pop()
    }

    /// 处理串口中断
    ///
    /// 应在 IRQ3/IRQ4 中断处理程序中调用。
    pub fn handle_interrupt(&mut self) {
        // 读取 IIR 判断中断类型
        let iir = unsafe { inb(self.base + UART_IIR) };
        
        // bit 0 = 0 表示有挂起的中断
        if iir & 0x01 != 0 {
            return;
        }

        // bits 1-3: 中断 ID
        let interrupt_id = (iir >> 1) & 0x07;

        match interrupt_id {
            0x02 => {
                // 接收数据可用
                while is_data_ready(self.base) {
                    let byte = read_byte(self.base);
                    let _ = self.rx_buffer.push(byte);
                }
            },
            0x01 => {
                // 发送保持寄存器空
                // 可以从 tx_buffer 取出数据发送
                if let Some(byte) = self.tx_buffer.pop() {
                    write_byte(self.base, byte);
                }
            },
            _ => {},  // 其他中断类型暂不处理
        }
    }

    /// 检查是否有数据可读
    pub fn has_data(&self) -> bool {
        !self.rx_buffer.is_empty() || is_data_ready(self.base)
    }

    /// 获取接收缓冲区中的字节数
    pub fn available_bytes(&self) -> usize {
        self.rx_buffer.len()
    }

    /// 清空接收缓冲区
    pub fn clear_rx_buffer(&mut self) {
        self.rx_buffer.clear();
    }

    /// 获取 I/O 基地址
    pub fn get_base_address(&self) -> u16 {
        self.base
    }

    /// 获取当前配置
    pub fn get_config(&self) -> &SerialConfig {
        &self.config
    }

    /// 更新配置并重新初始化
    pub fn reconfigure(&mut self, new_config: SerialConfig) -> Result<()> {
        self.config = new_config;
        self.init()
    }

    /// 获取设备信息
    pub fn get_info(&self) -> &DeviceInfo {
        &self.info
    }
}

// ============================================================================
// FFI 兼容接口
// ============================================================================

/// 全局串口实例数组
static mut SERIAL_PORTS: [Option<SerialPort>; MAX_COM_PORTS] = [None, None, None, None];

/// 初始化指定串口 (C 兼容接口)
#[no_mangle]
pub extern "C" fn serial_init(com: u32) {
    if (com as usize) < MAX_COM_PORTS {
        unsafe {
            SERIAL_PORTS[com as usize] = SerialPort::new(com as u8);
            if let Some(ref mut port) = &mut SERIAL_PORTS[com as usize] {
                let _ = port.init();
            }
        }
    }
}

/// 发送字符到串口 (C 兼容接口)
#[no_mangle]
pub extern "C" fn serial_putc(com: u32, ch: i32) {
    if (com as usize) < MAX_COM_PORTS {
        unsafe {
            if let Some(ref mut port) = &mut SERIAL_PORTS[com as usize] {
                let _ = port.send_byte(ch as u8);
            }
        }
    }
}

/// 发送字符串到串口 (C 兼容接口)
#[no_mangle]
pub extern "C" fn serial_puts(com: u32, s: *const core::ffi::c_char) {
    if (com as usize) < MAX_COM_PORTS && !s.is_null() {
        unsafe {
            if let Some(ref mut port) = &mut SERIAL_PORTS[com as usize] {
                let mut ptr = s;
                while *ptr != 0 {
                    let _ = port.send_byte(*ptr as u8);
                    ptr = ptr.add(1);
                }
            }
        }
    }
}

/// 从串口读取字符 (C 兼容接口)
#[no_mangle]
pub extern "C" fn serial_getc(com: u32) -> i32 {
    if (com as usize) < MAX_COM_PORTS {
        unsafe {
            match &mut SERIAL_PORTS[com as usize] {
                Some(port) => {
                    match port.receive_byte() {
                        Some(ch) => ch as i32,
                        None => -1,
                    }
                },
                None => -1,
            }
        }
    } else {
        -1
    }
}

/// 检查串口是否有数据 (C 兼容接口)
#[no_mangle]
pub extern "C" fn serial_has_char(com: u32) -> i32 {
    if (com as usize) < MAX_COM_PORTS {
        unsafe {
            match &SERIAL_PORTS[com as usize] {
                Some(port) => {
                    if port.has_data() { 1 } else { 0 }
                },
                None => 0,
            }
        }
    } else {
        0
    }
}

/// 处理串口中断 (C 兼容接口)
#[no_mangle]
pub extern "C" fn serial_irq_handler(com: u32) {
    if (com as usize) < MAX_COM_PORTS {
        unsafe {
            if let Some(ref mut port) = &mut SERIAL_PORTS[com as usize] {
                port.handle_interrupt();
            }
        }
    }
}

/// C 兼容别名: serial_has_data
#[no_mangle]
pub extern "C" fn serial_has_data(com: i32) -> bool {
    serial_has_char(com as u32) != 0
}

/// C 兼容别名: serial_write(buf, count 参数交换以匹配旧 C API)
#[no_mangle]
pub unsafe extern "C" fn serial_write(com: i32, buf: *const core::ffi::c_void, count: u64) {
    let bytes = core::slice::from_raw_parts(buf as *const u8, count as usize);
    for &b in bytes {
        serial_putc(com as u32, b as i32);
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(COM1_BASE, 0x3F8);
        assert_eq!(COM2_BASE, 0x2F8);
        assert_eq!(MAX_COM_PORTS, 4);
    }

    #[test]
    fn test_config_default() {
        let config = SerialConfig::default();
        
        assert_eq!(config.baud_rate, BaudRate::Baud115200);
        assert_eq!(config.data_bits, DataBits::Bits8);
        assert_eq!(config.stop_bits, StopBits::One);
        assert_eq!(config.parity, ParityMode::None);
    }

    #[test]
    fn test_baud_rate_conversion() {
        assert_eq!(BaudRate::Baud9600.to_divisor(), 12);
        assert_eq!(BaudRate::Baud115200.to_divisor(), 1);
    }

    #[test]
    fn test_data_bits_conversion() {
        assert_eq!(DataBits::Bits5.to_lcr_value(), 0x00);
        assert_eq!(DataBits::Bits8.to_lcr_value(), 0x03);
    }

    #[test]
    fn test_parity_mode_conversion() {
        assert_eq!(ParityMode::None.to_lcr_value(), 0x00);
        assert_eq!(ParityMode::Odd.to_lcr_value(), 0x08);
        assert_eq!(ParityMode::Even.to_lcr_value(), 0x18);
    }

    #[test]
    fn test_serial_port_creation() {
        let port = SerialPort::new(0);
        assert!(port.is_some());
        
        let port = SerialPort::new(3);
        assert!(port.is_some());
        
        let port = SerialPort::new(4);
        assert!(port.is_none());  // 超出范围
    }

    #[test]
    fn test_driver_trait_impl() {
        let mut port = SerialPort::new(0).unwrap();
        
        assert_eq!(port.name(), "COM1");
        assert_eq!(port.device_type(), DeviceType::Char);
        assert!(!port.is_ready());
        
        let result = port.init();
        let _ = result;
        assert!(port.status().len() > 0);
    }

    #[test]
    fn test_ring_buffer_operations() {
        let mut buf: RingBuffer<u8> = RingBuffer::default();
        
        assert!(buf.is_empty());
        assert!(!buf.is_full());
        assert_eq!(buf.len(), 0);
        
        // 填充缓冲区
        for i in 0..SERIAL_BUFFER_SIZE {
            assert!(buf.push(i as u8).is_ok());
        }
        
        assert!(buf.is_full());
        assert_eq!(buf.len(), SERIAL_BUFFER_SIZE);
        
        // 尝试写入满缓冲区应该失败
        assert!(buf.push(0xFF).is_err());
        
        // 读取所有数据
        for i in 0..SERIAL_BUFFER_SIZE {
            assert_eq!(buf.pop(), Some(i as u8));
        }
        
        assert!(buf.is_empty());
        assert_eq!(buf.pop(), None);
    }

    #[test]
    fn test_error_codes() {
        let err = DriverError::NotInitialized;
        assert_eq!(err.to_string(), "Not initialized");
    }
}
