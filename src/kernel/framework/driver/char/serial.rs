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

use crate::kernel::framework::driver::{DeviceType, Driver, DriverError, DriverResult};
use crate::kernel::framework::ioport::IoPort;
use crate::kernel::framework::sync::IrqSpinLock;

// ============================================================================
// 硬件常量定义
// ============================================================================

/// COM 端口基址
pub const COM1_BASE: u16 = 0x3F8;
pub const COM2_BASE: u16 = 0x2F8;
const COM3_BASE: u16 = 0x3E8;
const COM4_BASE: u16 = 0x2E8;

/// 最大支持的 COM 端口数量
pub const MAX_COM_PORTS: usize = 4;

/// UART 寄存器偏移量
const UART_RBR: u16 = 0; // 接收缓冲寄存器 (只读)
const UART_THR: u16 = 0; // 发送保持寄存器 (只写)
const UART_IER: u16 = 1; // 中断使能寄存器
const UART_IIR: u16 = 2; // 中断识别寄存器 (只读)
const UART_FCR: u16 = 2; // FIFO 控制寄存器 (只写)
const UART_LCR: u16 = 3; // 线路控制寄存器
const UART_MCR: u16 = 4; // 调制解调器控制寄存器
const UART_LSR: u16 = 5; // 线路状态寄存器

/// LSR 标志位
const LSR_DATA_READY: u8 = 0x01; // 数据可读
const LSR_TRANSMIT_EMPTY: u8 = 0x20; // 发送保持寄存器空

/// FCR 命令
const FCR_ENABLE_FIFO: u8 = 0xC1; // 启用 FIFO，清除缓冲区

/// MCR 命令
const MCR_DTR: u8 = 0x01; // Data Terminal Ready
const MCR_RTS: u8 = 0x02; // Request To Send
const MCR_OUT2: u8 = 0x08; // OUT2 (中断使能)

/// LCR 命令
const LCR_DLAB: u8 = 0x80; // Divisor Latch Access Bit

/// 波特率分频值
const BAUD_9600: u16 = 12;
const BAUD_19200: u16 = 6;
const BAUD_38400: u16 = 3;
const BAUD_57600: u16 = 2;
const BAUD_115200: u16 = 1;

/// 缓冲区大小
pub const SERIAL_BUFFER_SIZE: usize = 256;

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
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub(crate) fn to_divisor(&self) -> u16 {
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
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub(crate) fn to_lcr_value(&self) -> u8 {
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
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub(crate) fn to_lcr_value(&self) -> u8 {
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
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub(crate) fn to_lcr_value(&self) -> u8 {
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

pub struct RingBuffer<T> {
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
    pub(crate) fn push(&mut self, item: T) -> DriverResult<()> {
        if self.count >= SERIAL_BUFFER_SIZE {
            return Err(DriverError::Busy);
        }

        self.buffer[self.tail] = item;
        self.tail = (self.tail + 1) % SERIAL_BUFFER_SIZE;
        self.count += 1;
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.count == 0 {
            return None;
        }

        let item = self.buffer[self.head];
        self.head = (self.head + 1) % SERIAL_BUFFER_SIZE;
        self.count -= 1;
        Some(item)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 缓冲区是否已满.
    ///
    /// 当 `count` 达到 `SERIAL_BUFFER_SIZE` 时视为已满,
    /// 后续 `push` 将返回 `Err(DriverError::Busy)`.
    #[cfg(all(target_arch = "x86_64", feature = "kernel_test"))]
    pub(crate) fn is_full(&self) -> bool {
        self.count >= SERIAL_BUFFER_SIZE
    }

    /// 当前缓冲区元素数量.
    #[cfg(all(target_arch = "x86_64", feature = "kernel_test"))]
    pub(crate) fn len(&self) -> usize {
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
    /// I/O 端口句柄 (safe PIO proxy)
    io: Option<IoPort>,
    /// 端口号 (0-3 对应 COM1-COM4)
    port_num: u8,
    /// 当前配置
    config: SerialConfig,
    /// 接收缓冲区
    rx_buffer: RingBuffer<u8>,
    /// 发送缓冲区
    tx_buffer: RingBuffer<u8>,
    /// 是否已初始化
    initialized: bool,
}

// ============================================================================
// 底层辅助函数
// ============================================================================

/// 设置波特率
fn set_baud_rate(io: &IoPort, divisor: u16) {
    // 启用 DLAB 以访问分频寄存器
    io.write_u8(UART_LCR, LCR_DLAB);

    // 设置低字节和高字节
    io.write_u8(0, (divisor & 0xFF) as u8);
    io.write_u8(1, ((divisor >> 8) & 0xFF) as u8);

    // 关闭 DLAB，设置数据格式
    io.write_u8(UART_LCR, 0x03); // 8N1
}

/// 检查接收缓冲区是否有数据
fn is_data_ready(io: &IoPort) -> bool {
    io.read_u8(UART_LSR) & LSR_DATA_READY != 0
}

/// 检查发送保持寄存器是否为空
fn is_transmit_empty(io: &IoPort) -> bool {
    io.read_u8(UART_LSR) & LSR_TRANSMIT_EMPTY != 0
}

/// 从 UART 读取一个字节
fn read_byte(io: &IoPort) -> u8 {
    io.read_u8(UART_RBR)
}

/// 向 UART 写入一个字节
fn write_byte(io: &IoPort, byte: u8) {
    io.write_u8(UART_THR, byte);
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

    fn init(&mut self) -> DriverResult<()> {
        let io = self.io.as_ref().ok_or(DriverError::HardwareError)?;

        // 1. 禁用所有中断
        io.write_u8(UART_IER, 0x00);

        // 2. 启用 DLAB 并设置波特率
        set_baud_rate(io, self.config.baud_rate.to_divisor());

        // 3. 设置数据格式 (8N1)
        io.write_u8(
            UART_LCR,
            self.config.data_bits.to_lcr_value()
                | self.config.stop_bits.to_lcr_value()
                | self.config.parity.to_lcr_value(),
        );

        // 4. 启用 FIFO，清除缓冲区
        io.write_u8(UART_FCR, FCR_ENABLE_FIFO);

        // 5. 设置 MCR (DTR + RTS + OUT2)
        io.write_u8(UART_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);

        // 清空缓冲区
        self.rx_buffer.clear();
        self.tx_buffer.clear();

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> DriverResult<()> {
        // 禁用所有中断
        if let Some(io) = &self.io {
            io.write_u8(UART_IER, 0x00);
        }

        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn status(&self) -> &'static str {
        if self.initialized {
            match self.port_num {
                0 => "COM1 ready @ 0x3F8",
                1 => "COM2 ready @ 0x2F8",
                2 => "COM3 ready @ 0x3E8",
                3 => "COM4 ready @ 0x2E8",
                _ => "Unknown port",
            }
        } else {
            "Not initialized"
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

        // SAFETY: COM1-COM4 base addresses are standard PC serial port mappings
        let io = unsafe { IoPort::new(base, 8, "serial").ok()? };

        Some(Self {
            io: Some(io),
            port_num: port,
            config: SerialConfig::default(),
            rx_buffer: RingBuffer::default(),
            tx_buffer: RingBuffer::default(),
            initialized: false,
        })
    }

    /// 发送单个字节
    ///
    /// # Arguments
    /// * `byte` - 要发送的字节
    /// # Errors
    /// 串口未初始化或 I/O 端口资源缺失时返回 Err。
    pub fn send_byte(&mut self, byte: u8) -> DriverResult<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }
        let io = self.io.as_ref().ok_or(DriverError::HardwareError)?;

        // 等待发送保持寄存器为空
        while !is_transmit_empty(io) {
            core::hint::spin_loop();
        }

        write_byte(io, byte);
        Ok(())
    }

    /// 接收单个字节 (非阻塞)
    ///
    /// # Returns
    /// * `Some(u8)` - 收到的字节
    /// * `None` - 无数据可读
    pub fn receive_byte(&mut self) -> Option<u8> {
        let io = self.io.as_ref()?;

        if !is_data_ready(io) {
            return None;
        }

        let byte = read_byte(io);
        let _ = self.rx_buffer.push(byte);
        Some(byte)
    }

    #[expect(
        clippy::manual_let_else,
        reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
    )]
    /// 处理串口中断
    ///
    /// 应在 IRQ3/IRQ4 中断处理程序中调用。
    pub fn handle_interrupt(&mut self) {
        // 读取 IIR 判断中断类型
        let io = match &self.io {
            Some(io) => io,
            None => return,
        };
        let iir = io.read_u8(UART_IIR);

        // bit 0 = 0 表示有挂起的中断
        if iir & 0x01 != 0 {
            return;
        }

        // bits 1-3: 中断 ID
        let interrupt_id = (iir >> 1) & 0x07;

        match interrupt_id {
            0x02 => {
                // 接收数据可用
                while is_data_ready(io) {
                    let byte = read_byte(io);
                    let _ = self.rx_buffer.push(byte);
                }
            }
            0x01 => {
                // 发送保持寄存器空
                // 可以从 tx_buffer 取出数据发送
                if let Some(byte) = self.tx_buffer.pop() {
                    write_byte(io, byte);
                }
            }
            _ => {} // 其他中断类型暂不处理
        }
    }

    /// 检查是否有数据可读
    pub fn has_data(&self) -> bool {
        if !self.rx_buffer.is_empty() {
            return true;
        }
        if let Some(io) = &self.io {
            return is_data_ready(io);
        }
        false
    }
}

// ============================================================================
// FFI 兼容接口
// ============================================================================

/// 全局串口实例数组
static SERIAL_PORTS: IrqSpinLock<[Option<SerialPort>; MAX_COM_PORTS]> =
    IrqSpinLock::new([None, None, None, None]);

/// 初始化指定串口 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
// 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
#[expect(clippy::cast_possible_truncation)]
pub extern "C" fn serial_init(com: u32) {
    if (com as usize) < MAX_COM_PORTS {
        SERIAL_PORTS.with_mut(|ports| {
            ports[com as usize] = SerialPort::new(com as u8);
            if let Some(port) = &mut ports[com as usize] {
                let _ = port.init();
            }
        });
    }
}

/// 发送字符到串口 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
// 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
#[expect(clippy::cast_possible_truncation)]
pub extern "C" fn serial_putc(com: u32, ch: i32) {
    if (com as usize) < MAX_COM_PORTS {
        SERIAL_PORTS.with_mut(|ports| {
            if let Some(port) = &mut ports[com as usize] {
                let _ = port.send_byte(ch as u8);
            }
        });
    }
}

/// 发送字符串到串口 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn serial_puts(com: u32, s: *const u8) {
    if (com as usize) < MAX_COM_PORTS && !s.is_null() {
        // SAFETY: s 是调用方保证的有效 C 字符串指针
        let mut ptr = s;
        while unsafe { *ptr != 0 } {
            let ch = unsafe { *ptr };
            SERIAL_PORTS.with_mut(|ports| {
                if let Some(port) = &mut ports[com as usize] {
                    let _ = port.send_byte(ch as u8);
                }
            });
            // SAFETY: 指针操作在有效范围内，调用方保证指针有效性
            ptr = unsafe { ptr.add(1) };
        }
    }
}

/// 从串口读取字符 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn serial_getc(com: u32) -> i32 {
    if (com as usize) < MAX_COM_PORTS {
        SERIAL_PORTS.with_mut(|ports| {
            ports[com as usize]
                .as_mut()
                .map_or(-1, |port| port.receive_byte().map_or(-1, i32::from))
        })
    } else {
        -1
    }
}

/// 检查串口是否有数据 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn serial_has_char(com: u32) -> i32 {
    if (com as usize) < MAX_COM_PORTS {
        SERIAL_PORTS.with(|ports| {
            ports[com as usize]
                .as_ref()
                .map_or(0, |port| i32::from(port.has_data()))
        })
    } else {
        0
    }
}

/// 处理串口中断 (C 兼容接口)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn serial_irq_handler(com: u32) {
    if (com as usize) < MAX_COM_PORTS {
        SERIAL_PORTS.with_mut(|ports| {
            if let Some(port) = &mut ports[com as usize] {
                port.handle_interrupt();
            }
        });
    }
}

/// C 兼容别名: `serial_has_data`
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn serial_has_data(com: i32) -> bool {
    serial_has_char(com as u32) != 0
}

/// C 兼容别名: `serial_write(buf`, count 参数交换以匹配旧 C API)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// 串口已通过 `serial_init()` 初始化。仅在内核上下文中有效。
// 有意窄化: 用户内存代理, 指针/长度上下文保证
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::ptr_as_ptr,
    reason = "指针类型 cast 不变 constness (e.g. *mut T → *mut U); 改 .cast() 是机械替换不治根, 当前优先 expect 兑底"
)]
pub unsafe extern "C" fn serial_write(com: i32, buf: *const u8, count: u64) {
    unsafe {
        let bytes = core::slice::from_raw_parts(buf as *const u8, count as usize);
        for &b in bytes {
            serial_putc(com as u32, i32::from(b));
        }
    }
}

// ============================================================================
// CharOps 桥接 — 供 Chitin 统一字符设备 I/O
// ============================================================================

use crate::kernel::framework::chitin::CharOps;

#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
extern "C" fn serial_char_write(driver_data: *mut u8, buf: *const u8, len: usize) -> usize {
    if driver_data.is_null() || buf.is_null() {
        return 0;
    }
    // SAFETY: driver_data 由 Chitin CharOps 契约保证有效, buf 在调用期间有效。
    let port = unsafe { &*(driver_data as *const SerialPort) };
    let io = match &port.io {
        Some(io) => io,
        None => return 0,
    };
    let slice = unsafe { core::slice::from_raw_parts(buf, len) };
    for &byte in slice {
        if byte == b'\n' {
            write_byte(io, b'\r');
        }
        while !is_transmit_empty(io) {
            core::hint::spin_loop();
        }
        write_byte(io, byte);
    }
    slice.len()
}

#[expect(
    clippy::cast_ptr_alignment,
    reason = "cast_ptr_alignment: 指针类型转换对齐假设已知安全 (例如硬件 MMIO 寄存器地址已知对齐; 当前优先 expect"
)]
#[expect(
    clippy::manual_let_else,
    reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底"
)]
extern "C" fn serial_char_read(driver_data: *mut u8, buf: *mut u8, len: usize) -> usize {
    if driver_data.is_null() || buf.is_null() {
        return 0;
    }
    // SAFETY: driver_data 由 Chitin CharOps 契约保证有效, buf 至少 len 字节可写。
    let port = unsafe { &*(driver_data as *const SerialPort) };
    let io = match &port.io {
        Some(io) => io,
        None => return 0,
    };
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    let mut count = 0;
    for byte in slice.iter_mut() {
        if is_data_ready(io) {
            *byte = read_byte(io);
            count += 1;
        } else {
            break;
        }
    }
    count
}

pub static NS16550_CHAR_OPS: CharOps = CharOps {
    read: serial_char_read,
    write: serial_char_write,
    ioctl: None,
};
