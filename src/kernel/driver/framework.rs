//! 设备驱动框架 (Driver Framework)
//!
//! 提供统一的设备驱动接口和基础设施：
//! - **Driver Trait**: 所有驱动的统一抽象
//! - **Device Manager**: 设备注册和查找
//! - **IO Port 抽象**: 安全的端口操作
//!
//! ## 设计理念
//!
//! ```text
//! Driver Trait (多态)
//! ├── AtaDriver      (ATA/IDE 磁盘)
//! ├── KeyboardDriver  (PS/2 键盘)
//! ├── SerialDriver    (COM 串口)
//! └── PciDriver      (PCI 总线)
//!
//! Device Manager
//!   └── Registry: 驱动实例注册表
//!       ├── by_name("ata")
//!       ├── by_type(DeviceType::Block)
//!       └── by_id(0x01)  // 设备编号
//! ```

use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// IO 端口操作安全封装
// ============================================================================

/// 向指定端口写入字节 (架构无关: x86_64 → out, AArch64 → MMIO)
#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    crate::arch!(outb(port, value));
}

/// 从指定端口读入字节 (架构无关: x86_64 → in, AArch64 → MMIO)
#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    crate::arch!(inb(port))
}

/// 向指定端口写入字 (x86_64 特有, 无 Arch trait 等价方法)
#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub unsafe fn outw(port: u16, value: u16) {
    core::arch::asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") value,
        options(nomem, nostack, preserves_flags),
    );
}

/// 从指定端口读入字 (x86_64 特有, 无 Arch trait 等价方法)
#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    core::arch::asm!(
        "in ax, dx",
        out("ax") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags),
    );
    value
}

/// 向指定端口写入双字 (架构无关: x86_64 → out, AArch64 → MMIO)
#[inline(always)]
pub unsafe fn outl(port: u16, value: u32) {
    crate::arch!(outl(port, value));
}

/// 从指定端口读入双字 (架构无关: x86_64 → in, AArch64 → MMIO)
#[inline(always)]
pub unsafe fn inl(port: u16) -> u32 {
    crate::arch!(inl(port))
}

// ============================================================================
// 错误码定义
// ============================================================================

/// 驱动通用错误码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    /// 参数无效
    InvalidParameter,
    /// 设备不存在
    DeviceNotFound,
    /// 超时
    Timeout,
    /// 硬件错误
    HardwareError,
    /// 缓冲区不足
    BufferTooSmall,
    /// 不支持的操作
    UnsupportedOperation,
    /// 忙碌
    Busy,
    /// 未初始化
    NotInitialized,
}

impl core::fmt::Display for DriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidParameter => write!(f, "Invalid parameter"),
            Self::DeviceNotFound => write!(f, "Device not found"),
            Self::Timeout => write!(f, "Operation timeout"),
            Self::HardwareError => write!(f, "Hardware error"),
            Self::BufferTooSmall => write!(f, "Buffer too small"),
            Self::UnsupportedOperation => write!(f, "Unsupported operation"),
            Self::Busy => write!(f, "Device busy"),
            Self::NotInitialized => write!(f, "Not initialized"),
        }
    }
}

/// 操作结果类型别名
pub type Result<T> = core::result::Result<T, DriverError>;

// ============================================================================
// 设备类型枚举
// ============================================================================

/// 设备类型分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// 块设备 (磁盘)
    Block,
    /// 字符设备 (终端、键盘)
    Char,
    /// 网络设备
    Network,
    /// 总线控制器 (PCI, USB)
    Bus,
    /// 输入设备 (鼠标、触摸板)
    Input,
    /// 其他
    Other,
}

impl core::fmt::Display for DeviceType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Block => write!(f, "Block"),
            Self::Char => write!(f, "Char"),
            Self::Network => write!(f, "Network"),
            Self::Bus => write!(f, "Bus"),
            Self::Input => write!(f, "Input"),
            Self::Other => write!(f, "Other"),
        }
    }
}

// ============================================================================
// 统一驱动 Trait
// ============================================================================

/// 设备驱动统一接口
///
/// 所有硬件驱动都必须实现此 trait，提供标准化的初始化、
/// 控制和查询接口。
///
/// # Example
/// ```rust,ignore
/// struct MyDriver;
///
/// impl Driver for MyDriver {
///     fn name(&self) -> &'static str { "my_driver" }
///     fn device_type(&self) -> DeviceType { DeviceType::Other }
///     
///     fn init(&mut self) -> Result<()> { Ok(()) }
///     fn shutdown(&mut self) -> Result<()> { Ok(()) }
/// }
/// ```
pub trait Driver {
    /// 获取驱动名称
    fn name(&self) -> &'static str;
    
    /// 获取设备类型
    fn device_type(&self) -> DeviceType;
    
    /// 初始化驱动和硬件
    ///
    /// # Returns
    /// - `Ok(())` - 初始化成功
    /// - `Err(DriverError)` - 初始化失败
    fn init(&mut self) -> Result<()>;
    
    /// 关闭驱动并释放资源
    fn shutdown(&mut self) -> Result<()>;
    
    /// 检查设备是否就绪
    #[inline]
    fn is_ready(&self) -> bool {
        true
    }
    
    /// 重置设备
    #[inline]
    fn reset(&mut self) -> Result<()> {
        Err(DriverError::UnsupportedOperation)
    }
    
    /// 获取设备状态信息
    ///
    /// 返回人类可读的状态字符串
    fn status(&self) -> &'static str {
        "Ready"
    }
}

// ============================================================================
// 设备管理器 (Registry)
// ============================================================================

/// 全局设备 ID 分配器
static NEXT_DEVICE_ID: AtomicU32 = AtomicU32::new(1);

/// 分配新的设备 ID
pub(crate) fn allocate_device_id() -> u32 {
    NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed)
}

/// 设备描述信息
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// 唯一设备 ID
    pub id: u32,
    /// 设备名称
    pub name: &'static str,
    /// 设备类型
    pub device_type: DeviceType,
    /// 是否已初始化
    pub initialized: bool,
    /// I/O 基地址 (如果有)
    pub io_base: Option<u16>,
    /// IRQ 号 (如果有)
    pub irq: Option<u8>,
}

impl DeviceInfo {
    /// 创建新的设备信息
    pub fn new(name: &'static str, device_type: DeviceType) -> Self {
        Self {
            id: allocate_device_id(),
            name,
            device_type,
            initialized: false,
            io_base: None,
            irq: None,
        }
    }

    /// 设置 I/O 基地址
    pub fn with_io_base(mut self, base: u16) -> Self {
        self.io_base = Some(base);
        self
    }

    /// 设置 IRQ
    pub fn with_irq(mut self, irq: u8) -> Self {
        self.irq = Some(irq);
        self
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert_eq!(DriverError::InvalidParameter.to_string(), "Invalid parameter");
        assert_eq!(DriverError::Timeout.to_string(), "Operation timeout");
        assert_ne!(DriverError::Busy, DriverError::NotInitialized);
    }

    #[test]
    fn test_device_types() {
        assert_eq!(DeviceType::Block.to_string(), "Block");
        assert_eq!(DeviceType::Char.to_string(), "Char");
        assert_eq!(DeviceType::Network.to_string(), "Network");
        
        // 测试 Display trait
        let block_str = format!("{}", DeviceType::Block);
        assert_eq!(block_str, "Block");
    }

    #[test]
    fn test_device_info_creation() {
        let info = DeviceInfo::new("test_device", DeviceType::Other);
        
        assert!(info.id > 0);
        assert_eq!(info.name, "test_device");
        assert!(!info.initialized);
        assert!(info.io_base.is_none());
        assert!(info.irq.is_none());
    }

    #[test]
    fn test_device_info_builder() {
        let info = DeviceInfo::new("serial0", DeviceType::Char)
            .with_io_base(0x3F8)
            .with_irq(4);
        
        assert_eq!(info.io_base, Some(0x3F8));
        assert_eq!(info.irq, Some(4));
    }

    #[test]
    fn test_id_allocation() {
        let id1 = allocate_device_id();
        let id2 = allocate_device_id();
        let id3 = allocate_device_id();
        
        // IDs 应该是单调递增的
        assert!(id2 > id1);
        assert!(id3 > id2);
        
        // 差值应该为 1
        assert_eq!(id2 - id1, 1);
        assert_eq!(id3 - id2, 1);
    }

    #[test]
    fn test_result_type_alias() {
        // 验证 Result 类型别名工作正常
        fn returns_ok() -> Result<u32> {
            Ok(42)
        }
        
        fn returns_err() -> Result<u32> {
            Err(DriverError::DeviceNotFound)
        }
        
        assert!(returns_ok().is_ok());
        assert!(returns_err().is_err());
        assert_eq!(returns_ok().unwrap(), 42);
    }
}
