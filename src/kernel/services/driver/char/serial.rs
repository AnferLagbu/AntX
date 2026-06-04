//! @SAFE: 本文件不含 unsafe 代码。
//!
//! 16550 UART 串口驱动 — services 层安全代理 (Phase 2.1.5)
//!
//! 封装 PC 标准 COM 端口的 PIO 操作,
//! 通过 `framework::IoPort` 提供 100% safe API。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `IoPort` 由 TCB 抽象, services 层只调用 safe 方法
//! - **类型安全**: 波特率/数据位/停止位/校验位用枚举
//! - **薄包装**: 仅暴露发送/接收/配置常用操作
//! - **可替代**: 原 `kernel/driver/char/serial.rs` 仍存在, 本文件是迁移目标
//!
//! ## 硬件接口
//!
//! ```text
//! COM1: IO=0x3F8, IRQ=4
//! COM2: IO=0x2F8, IRQ=3
//! COM3: IO=0x3E8, IRQ=4
//! COM4: IO=0x2E8, IRQ=3
//! ```
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.5 任务: 串口设备迁移

use crate::kernel::framework::ioport::IoPort;

// ── COM 端口基址 ──

pub const COM1_BASE: u16 = 0x3F8;
pub const COM2_BASE: u16 = 0x2F8;
pub const COM3_BASE: u16 = 0x3E8;
pub const COM4_BASE: u16 = 0x2E8;
pub const COM_PORT_COUNT: u16 = 8;

// ── UART 寄存器偏移 (相对基址) ──

/// 接收缓冲寄存器 (只读)
pub const UART_RBR: u16 = 0;
/// 发送保持寄存器 (只写)
pub const UART_THR: u16 = 0;
/// 中断使能寄存器
pub const UART_IER: u16 = 1;
/// 中断识别寄存器 (只读)
pub const UART_IIR: u16 = 2;
/// FIFO 控制寄存器 (只写)
pub const UART_FCR: u16 = 2;
/// 线路控制寄存器
pub const UART_LCR: u16 = 3;
/// 调制解调器控制寄存器
pub const UART_MCR: u16 = 4;
/// 线路状态寄存器
pub const UART_LSR: u16 = 5;
/// 波特率分频值低字节 (DLAB=1)
pub const UART_DLL: u16 = 0;
/// 波特率分频值高字节 (DLAB=1)
pub const UART_DLM: u16 = 1;

// ── LSR 状态位 ──

/// 数据可读
pub const LSR_DATA_READY: u8 = 0x01;
/// 发送保持寄存器空
pub const LSR_TRANSMIT_EMPTY: u8 = 0x20;
/// 发送器空闲
pub const LSR_TRANSMIT_IDLE: u8 = 0x40;

// ── 波特率分频值 (1.8432 MHz 基准) ──

const BAUD_9600: u16 = 12;
const BAUD_19200: u16 = 6;
const BAUD_38400: u16 = 3;
const BAUD_57600: u16 = 2;
const BAUD_115200: u16 = 1;

// ── 控制位 ──

/// DLAB 位 (访问分频值时置 1)
pub const LCR_DLAB: u8 = 0x80;
/// 启用 FIFO, 清空缓冲
pub const FCR_ENABLE_FIFO: u8 = 0xC1;
/// Data Terminal Ready
pub const MCR_DTR: u8 = 0x01;
/// Request To Send
pub const MCR_RTS: u8 = 0x02;
/// OUT2 (中断使能)
pub const MCR_OUT2: u8 = 0x08;

// ============================================================================
// 配置类型
// ============================================================================

/// 波特率
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaudRate {
    Baud9600,
    Baud19200,
    Baud38400,
    Baud57600,
    Baud115200,
}

impl BaudRate {
    /// 转换为分频值
    pub const fn divisor(self) -> u16 {
        match self {
            Self::Baud9600 => BAUD_9600,
            Self::Baud19200 => BAUD_19200,
            Self::Baud38400 => BAUD_38400,
            Self::Baud57600 => BAUD_57600,
            Self::Baud115200 => BAUD_115200,
        }
    }
}

impl Default for BaudRate {
    fn default() -> Self {
        Self::Baud115200
    }
}

/// 数据位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Bits5,
    Bits6,
    Bits7,
    Bits8,
}

impl DataBits {
    /// 转换为 LCR 位域
    pub const fn lcr_bits(self) -> u8 {
        match self {
            Self::Bits5 => 0x00,
            Self::Bits6 => 0x01,
            Self::Bits7 => 0x02,
            Self::Bits8 => 0x03,
        }
    }
}

impl Default for DataBits {
    fn default() -> Self {
        Self::Bits8
    }
}

/// 停止位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

impl StopBits {
    /// 转换为 LCR 位 (1 或 2)
    pub const fn lcr_bit(self) -> u8 {
        match self {
            Self::One => 0x00,
            Self::Two => 0x04,
        }
    }
}

impl Default for StopBits {
    fn default() -> Self {
        Self::One
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
    /// 转换为 LCR 位域
    pub const fn lcr_bits(self) -> u8 {
        match self {
            Self::None => 0x00,
            Self::Odd => 0x08,
            Self::Even => 0x18,
            Self::Mark => 0x28,
            Self::Space => 0x38,
        }
    }
}

impl Default for ParityMode {
    fn default() -> Self {
        Self::None
    }
}

/// 串口配置
#[derive(Debug, Clone, Copy, Default)]
pub struct SerialConfig {
    pub baud_rate: BaudRate,
    pub data_bits: DataBits,
    pub stop_bits: StopBits,
    pub parity: ParityMode,
}

impl SerialConfig {
    /// 创建默认配置 (115200 8N1)
    pub const fn default_115200_8n1() -> Self {
        Self {
            baud_rate: BaudRate::Baud115200,
            data_bits: DataBits::Bits8,
            stop_bits: StopBits::One,
            parity: ParityMode::None,
        }
    }
}

// ============================================================================
// COM 端口标识
// ============================================================================

/// COM 端口编号 (0-3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComPort {
    Com1 = 0,
    Com2 = 1,
    Com3 = 2,
    Com4 = 3,
}

impl ComPort {
    /// 端口基址
    pub const fn base(self) -> u16 {
        match self {
            Self::Com1 => COM1_BASE,
            Self::Com2 => COM2_BASE,
            Self::Com3 => COM3_BASE,
            Self::Com4 => COM4_BASE,
        }
    }

    /// 端口名
    pub const fn name(self) -> &'static str {
        match self {
            Self::Com1 => "COM1",
            Self::Com2 => "COM2",
            Self::Com3 => "COM3",
            Self::Com4 => "COM4",
        }
    }
}

// ============================================================================
// 安全串口驱动
// ============================================================================

/// 16550 UART 串口的安全代理 (services 层)。
///
/// 内部封装 `IoPort`, 提供类型安全的 PIO 读写。
pub struct SerialPort {
    port: IoPort,
    com: ComPort,
    config: SerialConfig,
}

impl SerialPort {
    /// 创建并初始化串口。
    ///
    /// # 参数
    /// - `com`: COM 端口标识 (Com1/Com2/Com3/Com4)
    /// - `config`: 串口配置 (波特率/数据位/停止位/校验)
    ///
    /// # 返回
    /// - `Some(SerialPort)`: 初始化成功
    /// - `None`: 端口已被占用 (IoPort 别名检测失败)
    pub fn new(com: ComPort, config: SerialConfig) -> Option<Self> {
        // SAFETY: COM1-COM4 base addresses are standard PC serial port mappings.
        // IoPort::new checks for port range validity; alias detection prevents
        // multiple registrations of the same port.
        let port = IoPort::new_safe(com.base(), COM_PORT_COUNT, "serial").ok()?;

        let mut s = Self { port, com, config };
        s.apply_config();
        Some(s)
    }

    /// 应用当前配置到硬件 (分频值/数据格式/FIFO)
    fn apply_config(&mut self) {
        // 1. 启用 DLAB 写入分频值
        self.write_lcr(LCR_DLAB | self.config.data_bits.lcr_bits()
            | self.config.stop_bits.lcr_bit()
            | self.config.parity.lcr_bits());

        // 2. 写分频值
        let div = self.config.baud_rate.divisor();
        self.port.write_u8(UART_DLL, (div & 0xFF) as u8);
        self.port.write_u8(UART_DLM, ((div >> 8) & 0xFF) as u8);

        // 3. 关闭 DLAB, 写入数据格式
        self.write_lcr(self.config.data_bits.lcr_bits()
            | self.config.stop_bits.lcr_bit()
            | self.config.parity.lcr_bits());

        // 4. 启用 FIFO
        self.port.write_u8(UART_FCR, FCR_ENABLE_FIFO);

        // 5. 启用 DTR + RTS + OUT2
        self.port.write_u8(UART_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);
    }

    /// 写 LCR 寄存器
    #[inline]
    fn write_lcr(&self, val: u8) {
        self.port.write_u8(UART_LCR, val);
    }

    /// 读 LSR 寄存器
    #[inline]
    pub fn line_status(&self) -> u8 {
        self.port.read_u8(UART_LSR)
    }

    /// 发送保持寄存器是否空 (可写下一个字节)
    #[inline]
    pub fn is_transmit_ready(&self) -> bool {
        self.line_status() & LSR_TRANSMIT_EMPTY != 0
    }

    /// 是否有数据可读
    #[inline]
    pub fn has_data(&self) -> bool {
        self.line_status() & LSR_DATA_READY != 0
    }

    /// 读一个字节 (阻塞, 等待数据)
    ///
    /// 注意: 此函数会自旋等待, 仅应在串口已初始化的驱动循环中使用。
    /// 上层应使用 `try_receive` 避免忙等。
    pub fn receive(&self) -> u8 {
        while !self.has_data() {
            core::hint::spin_loop();
        }
        self.port.read_u8(UART_RBR)
    }

    /// 尝试读一个字节 (非阻塞)
    pub fn try_receive(&self) -> Option<u8> {
        if self.has_data() {
            Some(self.port.read_u8(UART_RBR))
        } else {
            None
        }
    }

    /// 写一个字节 (阻塞, 等待发送保持寄存器空)
    pub fn send(&self, byte: u8) {
        while !self.is_transmit_ready() {
            core::hint::spin_loop();
        }
        self.port.write_u8(UART_THR, byte);
    }

    /// 写多个字节
    pub fn send_all(&self, data: &[u8]) {
        for &b in data {
            self.send(b);
        }
    }

    /// 写字符串 (UTF-8 / ASCII)
    pub fn send_str(&self, s: &str) {
        self.send_all(s.as_bytes());
    }

    /// 读 IIR (中断识别寄存器)
    #[inline]
    pub fn interrupt_id(&self) -> u8 {
        self.port.read_u8(UART_IIR)
    }

    /// 启用 IER 中的中断位
    pub fn enable_interrupts(&self, mask: u8) {
        self.port.write_u8(UART_IER, mask);
    }

    /// 禁用所有中断
    pub fn disable_interrupts(&self) {
        self.port.write_u8(UART_IER, 0);
    }

    /// 获取 COM 端口标识
    pub const fn com(&self) -> ComPort {
        self.com
    }

    /// 获取端口基址
    pub const fn base(&self) -> u16 {
        self.com.base()
    }

    /// 获取当前配置
    pub const fn config(&self) -> &SerialConfig {
        &self.config
    }
}
