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
//! ## 依赖声明
//!
//! framework 内部依赖: sync, mm, io, chitin, pci, net, timer, tests
//! services 依赖: services::driver (安全代理)
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

/// 网络设备驱动
pub mod net;

/// VirtIO 驱动框架 (跨架构，MMIO transport)
pub mod virtio;

/// 块设备抽象层 (BlockDevice trait + 全局注册表)
pub mod block;

/// 热插拔管理器 (设备插入/移除事件分发)
pub mod hotplug;
/// D5: 电源管理 (CpuIdle/CpuFreq/Suspend)
pub mod power;
/// D10: kexec (直接内核引导)
pub mod kexec;
/// D11: UEFI 运行时服务
pub mod uefi;

// ============================================================================
// 公共 API 导出 (便捷访问)
// ============================================================================

// --- 框架导出 ---
pub use framework::{DeviceInfo, DeviceType, Driver, DriverError, Result as DriverResult, inb, outb};

// --- 块设备导出 ---
pub use block::{BlockDevice, block_device_count, block_device_state, block_device_name, block_device_info, block_device_list, hdd_read_sector, hdd_write_sector, hdd_is_present, hdd_total_sectors};

// --- 显示设备导出 ---
pub use display::font::Font;
pub use display::framebuffer::{Color, Framebuffer, Rect, colors};
pub use display::{get_framebuffer, display_init, FB_PHYS_ADDR, FB_PHYS_SIZE};

// --- 总线驱动导出 ---
#[cfg(target_arch = "x86_64")]
pub use bus::pci;

// --- 字符设备导出 ---
#[cfg(target_arch = "x86_64")]
pub use char::{
    BaudRate, DataBits, ParityMode, SerialConfig, SerialPort, StopBits, TextAttribute,
    VgaChar, VgaDriver, SCREEN_HEIGHT, SCREEN_WIDTH,
};
#[cfg(target_arch = "x86_64")]
pub use char::vga::Color as VgaColor;
#[cfg(target_arch = "aarch64")]
pub use char::pl011::Pl011Driver;

// --- 网络设备导出 ---
// e1000 内部函数 (`e1000_probe` 等) 在 e1000.rs 中以
// `#[cfg(not(feature = "kernel_test"))]` 守卫 (kernel_test 无 PCI 总线);
// 此处 re-export 必须同步 gate, 否则 kernel_test build 失败 (P0-1 修复).
#[cfg(not(feature = "kernel_test"))]
pub use net::e1000::{e1000_probe, e1000_net_send, e1000_net_recv, e1000_net_get_mac, e1000_net_irq, take_device as e1000_take_device};
#[cfg(not(feature = "kernel_test"))]
pub use virtio::net::{virtio_net_probe, virtio_net_send, virtio_net_recv, virtio_net_get_mac, virtio_net_irq, take_device as virtio_net_take_device};

// --- 输入设备导出 ---
#[cfg(target_arch = "x86_64")]
pub use input::keyboard;

// --- 存储设备导出 ---
pub use storage::{
    AhciController, AhciPort, AtaCommand, H2dFis, NvmeCommand, NvmeCompletion, NvmeController,
};

// 为了向后兼容，保留一些直接导入
#[cfg(target_arch = "x86_64")]
pub use storage::ata::{AtaController, AtaDevice, get_io_base, get_ctrl_base, ATA_PRIMARY_IO, ATA_PRIMARY_CTRL, ATA_SECONDARY_IO, ATA_SECONDARY_CTRL, MAX_ATA_DEVICES, WORDS_PER_SECTOR};

// --- serial 内部细节 re-export (供测试使用) ---
#[cfg(target_arch = "x86_64")]
pub use char::serial::{RingBuffer, COM1_BASE, COM2_BASE, MAX_COM_PORTS, SERIAL_BUFFER_SIZE};

// --- e1000 内部细节 re-export (供测试使用) ---
#[cfg(not(feature = "kernel_test"))]
pub use net::e1000::{virt_to_phys, E1000Device, E1000RxDesc, E1000TxDesc, E1000_RX_BUFFER_SIZE, E1000_RX_RING_SIZE, E1000_TX_RING_SIZE};

// --- power/kexec/uefi 公共接口 re-export ---
pub use power::*;
pub use kexec::*;
pub use uefi::*;

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化所有设备驱动
///
/// 按照依赖顺序初始化各个子系统并注册到 Chitin 全局设备表：
/// 1. 字符设备 (VGA、串口)
/// 2. 总线驱动 (PCI)
/// 3. 存储设备 (NVMe、AHCI、ATA)
/// 4. 输入设备 (键盘)
/// 5. 显示设备 (HDMI、DP)
/// 6. USB设备
/// 7. 组合虚拟设备 (RAID0/RAID1)
pub fn init_all() {
    #[cfg(target_arch = "x86_64")]
    {
        char::char_init();
        let _ = bus::bus_init();
        let _ = storage::storage_init();
        input::input_init();
    }
    #[cfg(target_arch = "aarch64")]
    {
        char::char_init();
        let _ = storage::storage_init();
    }

    let _ = display::display_init();
    let _ = usb::usb_init();

    hotplug::hotplug_init();

    let _ = crate::kernel::framework::chitin::devtree_probe_composites();
}

/// 关闭所有设备驱动
///
/// 通过 Chitin 框架统一关闭所有注册的设备。
pub fn shutdown_all() {
    crate::kernel::framework::chitin::chitin_shutdown_all();
}

/// 获取系统已检测到的设备列表 (从 Chitin + BlockDevice 读取)
///
/// 返回格式化的设备信息字符串。
#[cfg(feature = "alloc")]
pub fn list_devices() -> alloc::string::String {
    use alloc::format;
    let mut info = alloc::string::String::from("=== Chitin Device Registry ===\n\n");

    let chitin_devs = crate::kernel::framework::chitin::chitin_list();
    if chitin_devs.is_empty() {
        info.push_str("  (no devices)\n");
    } else {
        let mut block = Vec::new();
        let mut input = Vec::new();
        let mut net = Vec::new();
        let mut char_dev = Vec::new();
        let mut other = Vec::new();

        for (id, name, proto, state) in &chitin_devs {
            let st = format!("{:?}", state);
            let line = format!("  [id={}] {} proto={:?} state={}", id, name, proto, st);
            match proto {
                crate::kernel::framework::chitin::ChitinProto::Block => block.push(line),
                crate::kernel::framework::chitin::ChitinProto::Input => input.push(line),
                crate::kernel::framework::chitin::ChitinProto::Net => net.push(line),
                crate::kernel::framework::chitin::ChitinProto::Char => char_dev.push(line),
                _ => other.push(line),
            }
        }

        if !block.is_empty() {
            info.push_str("Block:\n");
            for s in &block {
                info.push_str(s);
                info.push('\n');
            }
        }
        if !char_dev.is_empty() {
            info.push_str("Char:\n");
            for s in &char_dev {
                info.push_str(s);
                info.push('\n');
            }
        }
        if !net.is_empty() {
            info.push_str("Net:\n");
            for s in &net {
                info.push_str(s);
                info.push('\n');
            }
        }
        if !input.is_empty() {
            info.push_str("Input:\n");
            for s in &input {
                info.push_str(s);
                info.push('\n');
            }
        }
        if !other.is_empty() {
            info.push_str("Other:\n");
            for s in &other {
                info.push_str(s);
                info.push('\n');
            }
        }
    }

    let blk_count = block::block_device_count();
    if blk_count > 0 {
        let bds = block::block_device_list();
        info.push_str(&format!(
            "\nBlock Device Registry: {} device(s)\n",
            blk_count
        ));
        for (id, name, sectors) in &bds {
            info.push_str(&format!(
                "  [id={}] {} sectors={} size={}MB\n",
                id,
                name,
                sectors,
                *sectors as u64 * 512 / (1024 * 1024)
            ));
        }
    }

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
        assert_eq!(DeviceType::Block.to_string(), "Block");
        assert_eq!(DeviceType::Char.to_string(), "Char");

        let _controller = AtaController::new();

        let _driver = KeyboardDriver::new();

        assert!(SerialPort::new(0).is_some());
        assert!(SerialPort::new(5).is_none());
    }

    #[test]
    fn test_driver_trait_polymorphism() {
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
