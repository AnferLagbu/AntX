//! 设备驱动子系统 (Driver Subsystem)
//!
//! 提供完整的硬件驱动支持，按功能模块化组织：
//! - **统一框架**: Driver Trait 和设备管理
//! - **总线驱动**: PCI、PCIe等总线支持
//! - **字符设备**: 串口、VGA等字符设备
//! - **输入设备**: 键盘、鼠标等输入设备
//! - **存储设备**: NVMe、AHCI、ATA等存储设备
//! - **显示设备**: HDMI、DisplayPort等显示接口
//! - **USB设备**: USB主机控制器和设备
//!
//! ## 架构设计
//!
//! ```text
//! Driver Subsystem
//! ├── framework.rs   # 统一接口和基础设施
//! ├── bus/           # 总线驱动
//! │   └── pci.rs     # PCI总线驱动
//! ├── char/          # 字符设备驱动
//! │   ├── serial.rs  # 串口驱动
//! │   └── vga.rs     # VGA驱动
//! ├── input/         # 输入设备驱动
//! │   └── keyboard.rs # 键盘驱动
//! ├── storage/       # 存储设备驱动
//! │   ├── nvme.rs    # NVMe驱动
//! │   ├── ahci.rs    # AHCI/SATA驱动
//! │   └── ata.rs     # ATA/IDE驱动
//! ├── display/       # 显示设备驱动
//! │   ├── hdmi.rs    # HDMI驱动
//! │   └── dp.rs      # DisplayPort驱动
//! └── usb/           # USB子系统
//!     ├── usb_core.rs # USB核心
//!     └── xhci.rs    # xHCI控制器
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! // 初始化所有驱动
//! driver::init_all();
//!
//! // 使用存储驱动读取数据
//! let mut buf = [0u8; 512];
//! storage::ata::ata_read_sector(0, 0, buf.as_mut_ptr());
//!
//! // 从键盘读取字符
//! if input::keyboard::keyboard_has_char() > 0 {
//!     let ch = input::keyboard::keyboard_read_char();
//!     println!("Key: {}", ch);
//! }
//! ```

// ============================================================================
// 子模块声明
// ============================================================================

/// 统一驱动框架 (Trait, IO 操作, 错误码)
pub mod framework;

/// 总线驱动子系统
pub mod bus;

/// 字符设备驱动子系统
pub mod char;

/// 输入设备驱动子系统
pub mod input;

/// 存储设备驱动子系统
pub mod storage;

/// 显示设备驱动子系统
pub mod display;

/// USB 子系统
pub mod usb;

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

// --- 总线驱动导出 ---
pub use bus::pci;

// --- 字符设备导出 ---
pub use char::{
    SerialPort,
    SerialConfig,
    BaudRate,
    DataBits,
    StopBits,
    ParityMode,
    VgaDriver,
    Color,
    TextAttribute,
    VgaChar,
    SCREEN_WIDTH,
    SCREEN_HEIGHT,
};

// --- 输入设备导出 ---
pub use input::keyboard;

// --- 存储设备导出 ---
pub use storage::{
    NvmeController,
    NvmeCommand,
    NvmeCompletion,
    AhciController,
    AhciPort,
    H2dFis,
    AtaCommand,
};

// 为了向后兼容，保留一些直接导入
pub use storage::ata::{
    AtaController,
    AtaDevice,
};

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化所有设备驱动
///
/// 按照依赖顺序初始化各个子系统：
/// 1. 字符设备 (VGA、串口)
/// 2. 总线驱动 (PCI)
/// 3. 存储设备 (NVMe、AHCI、ATA)
/// 4. 输入设备 (键盘)
/// 5. 显示设备 (HDMI、DP)
/// 6. USB设备
///
/// # Returns
/// * `Ok(())` - 所有驱动初始化成功
/// * `Err(DriverError)` - 某个驱动初始化失败
pub fn init_all() -> framework::Result<()> {
    // 1. 初始化字符设备 (显示和调试输出)
    char::char_init()?;
    
    // 2. 初始化总线驱动 (设备发现)
    #[cfg(feature = "pci")]
    {
        let _ = bus::bus_init();
    }
    
    // 3. 初始化存储设备
    let _ = storage::storage_init();
    
    // 4. 初始化输入设备
    input::input_init()?;
    
    // 5. 初始化显示设备
    let _ = display::display_init();
    
    // 6. 初始化USB
    let _ = usb::usb_init();
    
    Ok(())
}

/// 关闭所有设备驱动
///
/// 按相反顺序关闭各驱动。
pub fn shutdown_all() -> framework::Result<()> {
    // 关闭顺序与初始化相反
    
    // 6. 关闭USB
    // TODO: 实现 usb_shutdown()
    
    // 5. 关闭显示设备
    // TODO: 实现 display_shutdown()
    
    // 4. 关闭输入设备
    // TODO: 实现 input_shutdown()
    
    // 3. 关闭存储设备
    // TODO: 实现 storage_shutdown()
    
    // 2. 关闭总线驱动
    // TODO: 实现 bus_shutdown()
    
    // 1. 关闭字符设备
    // TODO: 实现 char_shutdown()
    
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
    info.push_str("Storage Devices:\n");
    unsafe {
        if let Some(ref controller) = crate::kernel::driver::storage::ata::ATA_CONTROLLER {
            for i in 0..4 {
                if controller.disk_present(i) {
                    let channel = if i < 2 { "Primary" } else { "Secondary" };
                    let role = if i % 2 == 0 { "Master" } else { "Slave" };
                    info.push_str(&format!("  ATA [{}] {}-{}\n", i, channel, role));
                }
            }
        }
    }
    
    // NVMe 设备
    info.push_str("  NVMe: (scan PCI bus for controllers)\n");
    
    // AHCI 设备
    info.push_str("  AHCI: (scan PCI bus for controllers)\n");
    
    info.push_str("\nInput Devices:\n");
    info.push_str("  Keyboard: PS/2\n");
    
    info.push_str("\nDisplay Devices:\n");
    info.push_str("  VGA: Text Mode (80x25)\n");
    info.push_str("  HDMI: (detect via HPD)\n");
    info.push_str("  DisplayPort: (detect via HPD)\n");
    
    info.push_str("\nUSB Devices:\n");
    info.push_str("  Controllers: (scan PCI bus for xHCI/EHCI)\n");
    
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
        
        // Storage - ATA
        let _controller = AtaController::new();
        
        // Input - Keyboard
        let _driver = KeyboardDriver::new();
        
        // Char - Serial
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
