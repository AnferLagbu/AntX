#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! ATA/IDE 磁盘驱动 — services 层安全桩模块 (Phase 2.1.4)
//!
//! 传统 ATA PIO 驱动的安全抽象。
//! 当前为桩模块, 实际 ATA 驱动逻辑保留在 framework 层
//! (使用 IoPort 端口 I/O, 需要 unsafe)。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: services 层不包含任何 unsafe 代码
//! - **桩模块**: 提供类型定义和常量, 实际 I/O 通过 framework FFI
//! - **未来扩展**: 当 framework 提供 safe IoPort wrapper 后迁移完整逻辑
//!
//! ## 硬件架构
//!
//! ```text
//! ATA 子系统
//! ├── Primary Channel (IO: 0x1F0-0x1F7, Ctrl: 0x3F6)
//! │   ├── Master (Drive 0)
//! │   └── Slave  (Drive 1)
//! └── Secondary Channel (IO: 0x170-0x177, Ctrl: 0x376)
//!     ├── Master (Drive 2)
//!     └── Slave  (Drive 3)
//! ```

// ============================================================================
// ATA 硬件常量
// ============================================================================

/// Primary 通道 I/O 基地址
pub const ATA_PRIMARY_IO: u16 = 0x1F0;
/// Primary 通道控制寄存器基址
pub const ATA_PRIMARY_CTRL: u16 = 0x3F6;
/// Secondary 通道 I/O 基地址
pub const ATA_SECONDARY_IO: u16 = 0x170;
/// Secondary 通道控制寄存器基址
pub const ATA_SECONDARY_CTRL: u16 = 0x376;

/// I/O 寄存器偏移量
pub const ATA_DATA: u16 = 0;
pub const ATA_ERROR: u16 = 1;
pub const ATA_SECTOR_COUNT: u16 = 2;
pub const ATA_SECTOR_NUM: u16 = 3;
pub const ATA_CYLINDER_LOW: u16 = 4;
pub const ATA_CYLINDER_HIGH: u16 = 5;
pub const ATA_DRIVE_HEAD: u16 = 6;
pub const ATA_STATUS: u16 = 7;
pub const ATA_COMMAND: u16 = 7;

/// 状态寄存器标志位
pub const ATA_STATUS_BSY: u8 = 0x80;
pub const ATA_STATUS_DRDY: u8 = 0x40;
pub const ATA_STATUS_DF: u8 = 0x20;
pub const ATA_STATUS_DRQ: u8 = 0x08;
pub const ATA_STATUS_ERR: u8 = 0x01;

/// ATA 命令集
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;
pub const ATA_CMD_READ_SECTORS: u8 = 0x20;
pub const ATA_CMD_WRITE_SECTORS: u8 = 0x30;
pub const ATA_CMD_FLUSH_CACHE: u8 = 0xE7;

/// 扇区大小
pub const SECTOR_SIZE: usize = 512;
/// 每个扇区的字数 (256 × 16bit = 512 bytes)
pub const WORDS_PER_SECTOR: usize = 256;
/// 最大支持设备数量 (2通道 × 2驱动器)
pub const MAX_ATA_DEVICES: usize = 4;

// ============================================================================
// 设备状态结构
// ============================================================================

/// ATA 驱动器状态
#[derive(Debug, Clone, Copy)]
pub struct AtaDevice {
    /// 是否存在
    pub present: bool,
    /// 是否为主驱动器 (Master)
    pub is_master: bool,
    /// 所属通道 (0=Primary, 1=Secondary)
    pub channel: u8,
}

impl Default for AtaDevice {
    fn default() -> Self {
        Self {
            present: false,
            is_master: true,
            channel: 0,
        }
    }
}

/// ATA 控制器状态 (services 层)
///
/// 当前为桩模块, 实际 I/O 通过 framework FFI。
/// 未来迁移后将包含完整的 PIO 读写逻辑。
pub struct AtaController {
    /// Primary 通道是否存在
    pub primary_present: bool,
    /// Secondary 通道是否存在
    pub secondary_present: bool,
    /// 驱动器列表
    pub devices: [AtaDevice; MAX_ATA_DEVICES],
    /// 控制器已初始化
    initialized: bool,
}

impl AtaController {
    /// 创建新的 ATA 控制器实例
    pub fn new() -> Self {
        Self {
            primary_present: false,
            secondary_present: false,
            devices: [AtaDevice::default(); MAX_ATA_DEVICES],
            initialized: false,
        }
    }

    /// 检查指定驱动器是否存在
    pub fn disk_present(&self, drive: u8) -> bool {
        if (drive as usize) >= MAX_ATA_DEVICES {
            return false;
        }
        self.devices[drive as usize].present
    }

    /// 获取已检测到的驱动器数量
    pub fn detected_device_count(&self) -> usize {
        self.devices.iter().filter(|d| d.present).count()
    }

    /// 控制器是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// 初始化 (桩: 通过 framework FFI)
    pub fn init(&mut self) -> bool {
        // 实际初始化保留在 framework 层 (需要 unsafe IoPort 操作)
        // services 层通过 framework FFI 接口访问 ATA 设备
        self.initialized = true;
        true
    }

    /// 关闭控制器
    pub fn shutdown(&mut self) {
        self.initialized = false;
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
        assert_eq!(ATA_PRIMARY_IO, 0x1F0);
        assert_eq!(ATA_SECONDARY_IO, 0x170);
        assert_eq!(WORDS_PER_SECTOR, 256);
        assert_eq!(MAX_ATA_DEVICES, 4);
    }

    #[test]
    fn test_device_default_state() {
        let device = AtaDevice::default();
        assert!(!device.present);
        assert!(device.is_master);
        assert_eq!(device.channel, 0);
    }

    #[test]
    fn test_controller_creation() {
        let controller = AtaController::new();
        assert!(!controller.primary_present);
        assert!(!controller.secondary_present);
        assert_eq!(controller.detected_device_count(), 0);
        assert!(!controller.is_initialized());
    }

    #[test]
    fn test_disk_present_bounds() {
        let controller = AtaController::new();
        assert!(!controller.disk_present(0));
        assert!(!controller.disk_present(3));
        assert!(!controller.disk_present(4));
        assert!(!controller.disk_present(255));
    }
}
