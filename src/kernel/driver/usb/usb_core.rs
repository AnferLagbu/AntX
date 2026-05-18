//! USB 核心框架 (USB Core Framework)
//!
//! 提供USB子系统的核心功能：
//! - **设备枚举**: 自动发现和配置USB设备
//! - **URB管理**: USB请求块的管理和调度
//! - **类驱动支持**: HID、大容量存储等
//! - **热插拔**: 设备动态连接和断开
//!
//! ## 架构设计
//!
//! ```text
//! USB Core
//! ├── usb_core.rs      # 核心框架和设备管理
//! ├── xhci.rs          # xHCI 主机控制器 (USB 3.0)
//! ├── ehci.rs          # EHCI 主机控制器 (USB 2.0)
//! ├── hid.rs           # HID 类驱动 (键盘/鼠标)
//! └── mass_storage.rs  # 大容量存储类驱动
//! ```
//!
//! # Safety
//! USB驱动涉及DMA和硬件寄存器操作，需要特别小心。

use alloc::vec::Vec;
use super::framework::{Driver, DeviceType, DriverError, Result, DeviceInfo};
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// USB 常量定义
// ============================================================================

/// USB描述符类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DescriptorType {
    Device = 1,
    Configuration = 2,
    String = 3,
    Interface = 4,
    Endpoint = 5,
    DeviceQualifier = 6,
    OtherSpeedConfig = 7,
    InterfacePower = 8,
    Hid = 0x21,
    HidReport = 0x22,
    HidPhysical = 0x23,
}

/// USB传输类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

/// USB方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    Out = 0,
    In = 0x80,
}

/// USB速度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Unknown,
    Low,      // 1.5 Mbps (USB 1.0)
    Full,     // 12 Mbps (USB 1.1)
    High,     // 480 Mbps (USB 2.0)
    Super,    // 5 Gbps (USB 3.0)
    SuperPlus, // 10 Gbps (USB 3.1)
}

impl UsbSpeed {
    pub fn bandwidth_mbps(&self) -> u32 {
        match self {
            Self::Low => 1,
            Self::Full => 12,
            Self::High => 480,
            Self::Super => 5000,
            Self::SuperPlus => 10000,
            Self::Unknown => 0,
        }
    }
}

/// USB设备状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    NotAttached,
    Attached,
    Powered,
    Default,
    Addressed,
    Configured,
    Suspended,
}

// ============================================================================
// USB描述符结构
// ============================================================================

/// USB设备描述符 (18字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DeviceDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub usb_version: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub max_packet_size0: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_version: u16,
    pub manufacturer_index: u8,
    pub product_index: u8,
    pub serial_number_index: u8,
    pub num_configurations: u8,
}

impl Default for DeviceDescriptor {
    fn default() -> Self {
        Self {
            length: 18,
            descriptor_type: DescriptorType::Device as u8,
            usb_version: 0x0200,
            device_class: 0,
            device_subclass: 0,
            device_protocol: 0,
            max_packet_size0: 64,
            vendor_id: 0,
            product_id: 0,
            device_version: 0x0100,
            manufacturer_index: 0,
            product_index: 0,
            serial_number_index: 0,
            num_configurations: 1,
        }
    }
}

/// USB配置描述符 (9字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ConfigurationDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub total_length: u16,
    pub num_interfaces: u8,
    pub configuration_value: u8,
    pub configuration_index: u8,
    pub attributes: u8,
    pub max_power: u8,
}

/// USB接口描述符 (9字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct InterfaceDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub num_endpoints: u8,
    pub interface_class: u8,
    pub interface_subclass: u8,
    pub interface_protocol: u8,
    pub interface_index: u8,
}

/// USB端点描述符 (7字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct EndpointDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub endpoint_address: u8,
    pub attributes: u8,
    pub max_packet_size: u16,
    pub interval: u8,
}

impl EndpointDescriptor {
    pub fn direction(&self) -> Direction {
        if self.endpoint_address & 0x80 != 0 {
            Direction::In
        } else {
            Direction::Out
        }
    }
    
    pub fn number(&self) -> u8 {
        self.endpoint_address & 0x0F
    }
    
    pub fn transfer_type(&self) -> TransferType {
        match self.attributes & 0x03 {
            0 => TransferType::Control,
            1 => TransferType::Isochronous,
            2 => TransferType::Bulk,
            3 => TransferType::Interrupt,
            _ => TransferType::Control,
        }
    }
}

// ============================================================================
// USB设备类代码
// ============================================================================

/// USB设备类代码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceClass {
    Unknown = 0x00,
    Audio = 0x01,
    Communications = 0x02,
    Hid = 0x03,
    Physical = 0x05,
    Image = 0x06,
    Printer = 0x07,
    MassStorage = 0x08,
    Hub = 0x09,
    Data = 0x0A,
    SmartCard = 0x0B,
    Video = 0x0E,
    WirelessController = 0xE0,
    Miscellaneous = 0xEF,
    ApplicationSpecific = 0xFE,
    VendorSpecific = 0xFF,
}

impl From<u8> for DeviceClass {
    fn from(value: u8) -> Self {
        match value {
            0x01 => Self::Audio,
            0x02 => Self::Communications,
            0x03 => Self::Hid,
            0x05 => Self::Physical,
            0x06 => Self::Image,
            0x07 => Self::Printer,
            0x08 => Self::MassStorage,
            0x09 => Self::Hub,
            0x0A => Self::Data,
            0x0B => Self::SmartCard,
            0x0E => Self::Video,
            0xE0 => Self::WirelessController,
            0xEF => Self::Miscellaneous,
            0xFE => Self::ApplicationSpecific,
            0xFF => Self::VendorSpecific,
            _ => Self::Unknown,
        }
    }
}

// ============================================================================
// USB请求块 (URB)
// ============================================================================

/// USB标准请求
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct UsbSetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

/// 标准请求代码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StandardRequest {
    GetStatus = 0,
    ClearFeature = 1,
    SetFeature = 3,
    SetAddress = 5,
    GetDescriptor = 6,
    SetDescriptor = 7,
    GetConfiguration = 8,
    SetConfiguration = 9,
    GetInterface = 10,
    SetInterface = 11,
    SynchFrame = 12,
}

/// URB状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrbStatus {
    Idle,
    Pending,
    Completed,
    Error,
    Cancelled,
}

/// USB请求块 (URB)
pub struct Urb {
    pub id: u32,
    pub device: u8,
    pub endpoint: u8,
    pub setup: Option<UsbSetupPacket>,
    pub buffer: *mut u8,
    pub buffer_length: usize,
    pub actual_length: usize,
    pub status: UrbStatus,
    pub callback: Option<unsafe fn(&Urb)>,
}

// ============================================================================
// USB设备结构
// ============================================================================

/// USB设备
pub struct UsbDevice {
    /// 设备ID
    pub id: u32,
    /// USB地址 (1-127)
    pub address: u8,
    /// 设备速度
    pub speed: UsbSpeed,
    /// 设备状态
    pub state: DeviceState,
    /// 设备描述符
    pub descriptor: DeviceDescriptor,
    /// 当前配置
    pub configuration: Option<u8>,
    /// 接口列表
    pub interfaces: Vec<InterfaceDescriptor>,
    /// 端点列表
    pub endpoints: Vec<EndpointDescriptor>,
    /// 设备信息
    pub info: DeviceInfo,
}

impl UsbDevice {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            address: 0,
            speed: UsbSpeed::Unknown,
            state: DeviceState::NotAttached,
            descriptor: DeviceDescriptor::default(),
            configuration: None,
            interfaces: Vec::new(),
            endpoints: Vec::new(),
            info: DeviceInfo::new("usb_device", DeviceType::Other),
        }
    }
    
    pub fn device_class(&self) -> DeviceClass {
        DeviceClass::from(self.descriptor.device_class)
    }
    
    pub fn vendor_id(&self) -> u16 {
        self.descriptor.vendor_id
    }
    
    pub fn product_id(&self) -> u16 {
        self.descriptor.product_id
    }
}

// ============================================================================
// USB主机控制器接口
// ============================================================================

/// USB主机控制器 Trait
pub trait HostController: Driver {
    /// 获取控制器支持的USB速度
    fn supported_speeds(&self) -> Vec<UsbSpeed>;
    
    /// 获取根集线器端口数量
    fn num_ports(&self) -> usize;
    
    /// 检测端口是否有设备连接
    fn port_has_device(&self, port: usize) -> bool;
    
    /// 复位端口
    fn reset_port(&mut self, port: usize) -> Result<()>;
    
    /// 获取端口设备速度
    fn get_port_speed(&self, port: usize) -> UsbSpeed;
    
    /// 提交URB
    fn submit_urb(&mut self, urb: &Urb) -> Result<()>;
    
    /// 取消URB
    fn cancel_urb(&mut self, urb_id: u32) -> Result<()>;
    
    /// 分配设备地址
    fn allocate_address(&mut self) -> Result<u8>;
    
    /// 释放设备地址
    fn free_address(&mut self, address: u8);
}

// ============================================================================
// USB核心管理器
// ============================================================================

/// 全局设备ID分配器
static NEXT_USB_DEVICE_ID: AtomicU32 = AtomicU32::new(1);

/// USB核心管理器
pub struct UsbCore {
    /// 已连接的设备列表
    devices: Vec<UsbDevice>,
    /// 主机控制器列表
    controllers: Vec<*mut dyn HostController>,
    /// 是否已初始化
    initialized: bool,
}

impl UsbCore {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            controllers: Vec::new(),
            initialized: false,
        }
    }
    
    /// 注册主机控制器
    pub fn register_controller(&mut self, controller: *mut dyn HostController) {
        self.controllers.push(controller);
    }
    
    /// 枚举所有设备
    pub fn enumerate_devices(&mut self) -> Result<()> {
        let controllers: Vec<*mut dyn HostController> = self.controllers.iter().copied().collect();
        for &controller_ptr in &controllers {
            let controller = unsafe { &mut *controller_ptr };
            
            for port in 0..controller.num_ports() {
                if controller.port_has_device(port) {
                    self.enumerate_port(controller, port)?;
                }
            }
        }
        
        Ok(())
    }
    
    /// 枚举单个端口
    fn enumerate_port(&mut self, controller: &mut dyn HostController, port: usize) -> Result<()> {
        // 复位端口
        controller.reset_port(port)?;
        
        // 获取设备速度
        let speed = controller.get_port_speed(port);
        
        // 创建新设备
        let device_id = NEXT_USB_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
        let mut device = UsbDevice::new(device_id);
        device.speed = speed;
        device.state = DeviceState::Powered;
        
        // 分配地址
        let address = controller.allocate_address()?;
        device.address = address;
        device.state = DeviceState::Addressed;
        
        // 获取设备描述符
        self.get_device_descriptor(controller, &mut device)?;
        
        // 配置设备
        self.configure_device(controller, &mut device)?;
        
        // 添加到设备列表
        self.devices.push(device);
        
        Ok(())
    }
    
    /// 获取设备描述符
    fn get_device_descriptor(
        &self,
        controller: &mut dyn HostController,
        device: &mut UsbDevice,
    ) -> Result<()> {
        let setup = UsbSetupPacket {
            request_type: 0x80,
            request: StandardRequest::GetDescriptor as u8,
            value: (DescriptorType::Device as u16) << 8,
            index: 0,
            length: 18,
        };
        
        let mut descriptor = DeviceDescriptor::default();
        let mut urb = Urb {
            id: 0,
            device: device.address,
            endpoint: 0,
            setup: Some(setup),
            buffer: &mut descriptor as *mut _ as *mut u8,
            buffer_length: 18,
            actual_length: 0,
            status: UrbStatus::Idle,
            callback: None,
        };
        
        controller.submit_urb(&urb)?;
        
        device.descriptor = descriptor;
        
        Ok(())
    }
    
    /// 配置设备
    fn configure_device(
        &self,
        controller: &mut dyn HostController,
        device: &mut UsbDevice,
    ) -> Result<()> {
        let setup = UsbSetupPacket {
            request_type: 0x00,
            request: StandardRequest::SetConfiguration as u8,
            value: 1,
            index: 0,
            length: 0,
        };
        
        let urb = Urb {
            id: 0,
            device: device.address,
            endpoint: 0,
            setup: Some(setup),
            buffer: core::ptr::null_mut(),
            buffer_length: 0,
            actual_length: 0,
            status: UrbStatus::Idle,
            callback: None,
        };
        
        controller.submit_urb(&urb)?;
        
        device.configuration = Some(1);
        device.state = DeviceState::Configured;
        
        Ok(())
    }
    
    /// 根据类查找设备
    pub fn find_device_by_class(&self, class: DeviceClass) -> Option<&UsbDevice> {
        self.devices.iter().find(|d| d.device_class() == class)
    }
    
    /// 根据VID/PID查找设备
    pub fn find_device_by_vid_pid(&self, vid: u16, pid: u16) -> Option<&UsbDevice> {
        self.devices.iter().find(|d| {
            d.vendor_id() == vid && d.product_id() == pid
        })
    }
    
    /// 获取设备数量
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

impl Driver for UsbCore {
    fn name(&self) -> &'static str {
        "USB Core"
    }
    
    fn device_type(&self) -> DeviceType {
        DeviceType::Bus
    }
    
    fn init(&mut self) -> Result<()> {
        self.enumerate_devices()?;
        self.initialized = true;
        Ok(())
    }
    
    fn shutdown(&mut self) -> Result<()> {
        self.devices.clear();
        self.initialized = false;
        Ok(())
    }
    
    fn is_ready(&self) -> bool {
        self.initialized
    }
    
    fn status(&self) -> &'static str {
        if self.initialized {
            "USB Core ready"
        } else {
            "USB Core not initialized"
        }
    }
}

impl Default for UsbCore {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_usb_speed_bandwidth() {
        assert_eq!(UsbSpeed::Low.bandwidth_mbps(), 1);
        assert_eq!(UsbSpeed::Full.bandwidth_mbps(), 12);
        assert_eq!(UsbSpeed::High.bandwidth_mbps(), 480);
        assert_eq!(UsbSpeed::Super.bandwidth_mbps(), 5000);
    }
    
    #[test]
    fn test_device_descriptor_default() {
        let desc = DeviceDescriptor::default();
        assert_eq!(desc.length, 18);
        assert_eq!(desc.descriptor_type, 1);
        assert_eq!(desc.max_packet_size0, 64);
    }
    
    #[test]
    fn test_device_class_from_u8() {
        assert_eq!(DeviceClass::from(0x03), DeviceClass::Hid);
        assert_eq!(DeviceClass::from(0x08), DeviceClass::MassStorage);
        assert_eq!(DeviceClass::from(0x09), DeviceClass::Hub);
        assert_eq!(DeviceClass::from(0xFF), DeviceClass::VendorSpecific);
    }
    
    #[test]
    fn test_usb_device_creation() {
        let device = UsbDevice::new(1);
        assert_eq!(device.id, 1);
        assert_eq!(device.address, 0);
        assert_eq!(device.state, DeviceState::NotAttached);
    }
    
    #[test]
    fn test_usb_core_creation() {
        let core = UsbCore::new();
        assert_eq!(core.device_count(), 0);
        assert!(!core.is_ready());
    }
}
