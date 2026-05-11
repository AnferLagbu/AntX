//! 设备驱动子系统 (Driver Subsystem)
//!
//! 提供完整的硬件驱动支持：
//! - **统一框架**: Driver Trait 和设备管理
//! - **ATA 磁盘**: ATA/IDE 硬盘驱动
//! - **键盘**: PS/2 键盘驱动
//! - **串口**: COM 端口驱动 (COM1-COM4)
//! - **PCI**: PCI 总线枚举和配置
//!
//! ## 架构设计
//!
//! ```text
//! Driver Subsystem
//! ├── framework.rs   # 统一接口和基础设施
//! ├── ata.rs         # ATA/IDE 磁盘驱动
//! ├── keyboard.rs    # PS/2 键盘驱动
//! ├── serial.rs      # 串口驱动
//! └── pci.rs         # PCI 总线驱动
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! // 初始化所有驱动
//! driver::init_all();
//!
//! // 使用 ATA 驱动读取磁盘
//! let mut buf = [0u8; 512];
//! ata::ata_read_sector(0, 0, buf.as_mut_ptr());
//!
//! // 从键盘读取字符
//! if keyboard::keyboard_has_char() > 0 {
//!     let ch = keyboard::keyboard_read_char();
//!     println!("Key: {}", ch);
//! }
//! ```

// ============================================================================
// 子模块声明
// ============================================================================

/// 统一驱动框架 (Trait, IO 操作, 错误码)
pub mod framework;

/// ATA/IDE 磁盘驱动
pub mod ata;

/// PS/2 键盘驱动
pub mod keyboard;

/// COM 串口驱动
pub mod serial;

/// PCI 总线驱动
pub mod pci;

// ============================================================================
// 公共 API 导出 (便捷访问)
// ============================================================================

// --- 框架导出 ---
pub use framework::{
    Driver,
    DeviceType,
    DeviceInfo,
    DriverError,
    Result as DriverResult,
};

// --- ATA 驱动导出 ---
pub use ata::{
    AtaController,
    AtaDevice,
};

// --- 键盘驱动导出 ---
pub use keyboard::{
    KeyboardDriver,
    ModifierState,
    SpecialKey,
};

// --- 串口驱动导出 ---
pub use serial::{
    SerialPort,
    SerialConfig,
    BaudRate,
    DataBits,
    StopBits,
    ParityMode,
};

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化所有设备驱动
///
/// 按照依赖顺序初始化各个子系统：
/// 1. PCI 总线 (发现硬件)
/// 2. ATA 控制器 (磁盘)
/// 3. PS/2 键盘
/// 4. COM1 串口 (用于调试输出)
///
/// # Returns
/// * `Ok(())` - 所有驱动初始化成功
/// * `Err(DriverError)` - 某个驱动初始化失败
pub fn init_all() -> framework::Result<()> {
    // 使用 klog 或其他方式输出初始化信息
    // (在 no_std 环境中无法使用 println!)

    // 1. 初始化 PCI (可选，用于设备发现)
    #[cfg(feature = "pci")]
    {
        let _ = pci::pci_init();
    }

    // 2. 初始化 ATA 控制器
    ata::ata_init();

    // 3. 初始化键盘
    keyboard::keyboard_init();

    // 4. 初始化 COM1 (用于内核调试输出)
    serial::serial_init(0);

    Ok(())
}

/// 关闭所有设备驱动
///
/// 按相反顺序关闭各驱动。
pub fn shutdown_all() -> framework::Result<()> {
    // 关闭顺序与初始化相反
    
    // 4. 关闭串口
    // TODO: 实现 serial_shutdown()
    
    // 3. 关闭键盘
    // TODO: 实现 keyboard_shutdown()
    
    // 2. 关闭 ATA
    // TODO: 实现 ata_shutdown()
    
    // 1. 关闭 PCI
    // TODO: 实现 pci_shutdown()

    Ok(())
}

/// 获取系统已检测到的设备列表
///
/// 返回格式化的设备信息字符串。
#[cfg(feature = "alloc")]
pub fn list_devices() -> alloc::string::String {
    use alloc::format;
    
    let mut info = alloc::string::String::from("=== Detected Devices ===\n\n");
    
    // ATA 设备
    info.push_str("ATA Devices:\n");
    unsafe {
        if let Some(ref controller) = crate::kernel::driver::ata::ATA_CONTROLLER {
            for i in 0..4 {
                if controller.disk_present(i) {
                    let channel = if i < 2 { "Primary" } else { "Secondary" };
                    let role = if i % 2 == 0 { "Master" } else { "Slave" };
                    info.push_str(&format!("  [{}] {}-{}\n", i, channel, role));
                }
            }
        }
    }
    
    // 其他设备可以继续添加...
    
    info.push('\n');
    info
}

// ============================================================================
// FFI 兼容层 (C 接口)
// ============================================================================

/// C 兼容的初始化函数
#[no_mangle]
pub extern "C" fn driver_init() {
    let _ = init_all();
}

/// C 兼容的关闭函数
#[no_mangle]
pub extern "C" fn driver_shutdown() {
    let _ = shutdown_all();
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_structure() {
        // 验证所有模块都存在且可访问
        
        // Framework
        assert_eq!(DeviceType::Block.to_string(), "Block");
        assert_eq!(DeviceType::Char.to_string(), "Char");
        
        // ATA
        let _controller = AtaController::new();
        
        // Keyboard
        let _driver = KeyboardDriver::new();
        
        // Serial
        assert!(SerialPort::new(0).is_some());
        assert!(SerialPort::new(5).is_none());
    }

    #[test]
    fn test_driver_trait_polymorphism() {
        // 测试多态性: 不同类型都可以作为 &dyn Driver 使用
        
        let ata = AtaController::new();
        let kb = KeyboardDriver::new();
        let com = SerialPort::new(0).unwrap();
        
        let drivers: Vec<&dyn Driver> = vec![&ata, &kb, &com];
        
        for driver in &drivers {
            assert!(driver.name().len() > 0);
            assert!(matches!(
                driver.device_type(),
                DeviceType::Block | DeviceType::Input | DeviceType::Char
            ));
        }
    }

    #[test]
    fn test_error_handling() {
        // 验证错误处理机制正常工作
        let err = DriverError::InvalidParameter;
        let result: DriverResult<u32> = Err(err);
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Invalid parameter");
    }

    #[test]
    fn test_device_info_creation() {
        let info = DeviceInfo::new("test", DeviceType::Other);
        
        assert!(info.id > 0);
        assert_eq!(info.name, "test");
        assert_eq!(info.device_type, DeviceType::Other);
        assert!(!info.initialized);
    }
}
