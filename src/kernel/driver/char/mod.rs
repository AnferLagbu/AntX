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
pub mod vga;

// 导出常用类型
pub use serial::{
    SerialPort,
    SerialConfig,
    BaudRate,
    DataBits,
    StopBits,
    ParityMode,
};

pub use vga::{
    VgaDriver,
    Color,
    TextAttribute,
    VgaChar,
    SCREEN_WIDTH,
    SCREEN_HEIGHT,
};

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化字符设备子系统
pub fn char_init() -> framework::Result<()> {
    // 初始化VGA
    vga::vga_init();
    
    // 初始化串口
    serial::serial_init(0);
    
    Ok(())
}

use super::framework;
