//! 输入设备驱动子系统 (Input Device Driver Subsystem)
//!
//! 提供输入设备支持：
//! - **键盘**: PS/2键盘驱动
//! - **鼠标**: PS/2鼠标驱动 (未来)
//! - **游戏手柄**: 游戏控制器 (未来)
//!
//! ## 架构
//!
//! ```text
//! Input Device Subsystem
//! ├── keyboard.rs  # 键盘驱动
//! ├── mouse.rs    # 鼠标驱动 (未来)
//! └── joystick.rs # 游戏手柄 (未来)
//! ```

pub mod keyboard;

// 导出常用类型
pub use keyboard::{
    KeyboardDriver,
    KeyEvent,
    KeyCode,
    KeyState,
};

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化输入设备子系统
pub fn input_init() -> framework::Result<()> {
    // 初始化键盘
    keyboard::keyboard_init();
    
    Ok(())
}

use super::framework;
