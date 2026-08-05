//! 字符设备驱动子系统 (Character Device Driver Subsystem)
//!
//! 提供字符设备支持：
//! - **串口**: UART 16550串口驱动
//! - **VGA**: VGA文本模式显示
//! - **TTY**: 终端设备 (未来)
//!
//! ## 架构
//!
//! ```text
//! Character Device Subsystem
//! ├── serial.rs  # 串口驱动
//! ├── vga.rs    # VGA驱动
//! └── tty.rs    # 终端驱动 (未来)
//! ```

pub mod serial;
#[cfg(target_arch = "x86_64")]
pub mod vga;

#[cfg(target_arch = "aarch64")]
pub mod pl011;

// 导出常用类型
pub use serial::{BaudRate, DataBits, ParityMode, SerialConfig, SerialPort, StopBits};

#[cfg(target_arch = "x86_64")]
pub use vga::{Color, SCREEN_HEIGHT, SCREEN_WIDTH, TextAttribute, VgaChar, VgaDriver};

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化字符设备子系统 (`x86_64`: VGA + 串口)
#[cfg(target_arch = "x86_64")]
pub fn char_init() {
    vga::vga_init();
    serial::serial_init(0);
    crate::kernel::framework::chitin::chitin_register_driver(
        "vga",
        crate::kernel::framework::chitin::ChitinProto::Char,
        None,
        None,
        alloc::boxed::Box::new(vga::VgaDriver::new()),
    );
    if let Some(com1) = serial::SerialPort::new(0) {
        crate::kernel::framework::chitin::chitin_register_driver_with_ops(
            "serial0",
            crate::kernel::framework::chitin::ChitinProto::Char,
            Some(0x3F8),
            Some(4),
            alloc::boxed::Box::new(com1),
            crate::kernel::framework::chitin::ChitinOps::Char(&serial::NS16550_CHAR_OPS),
        );
    }
}

/// 初始化字符设备子系统 (AArch64: PL011 UART)
#[cfg(target_arch = "aarch64")]
pub fn char_init() {
    use crate::kernel::framework::chitin::ChitinOps;

    crate::kernel::framework::chitin::chitin_register_driver_with_ops(
        "pl011",
        crate::kernel::framework::chitin::ChitinProto::Char,
        Some(crate::kernel::framework::arch::uart::base()),
        None,
        alloc::boxed::Box::new(pl011::Pl011Driver::new()),
        ChitinOps::Char(&pl011::PL011_CHAR_OPS),
    );
}
