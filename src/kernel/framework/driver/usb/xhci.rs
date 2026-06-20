//! xHCI 主机控制器驱动 (xHCI Host Controller Driver)
//!
//! 实现USB 3.0 xHCI (eXtensible Host Controller Interface) 规范：
//! - **USB 3.0支持**: 5 Gbps SuperSpeed
//! - **USB 2.0兼容**: 支持高速、全速、低速设备
//! - **多端口**: 支持多达256个端口
//! - **DMA传输**: 高效的内存传输
//!
//! ## 硬件规格
//!
//! ```text
//! xHCI Registers:
//! ├── CAPLENGTH (0x00): 能力寄存器长度
//! ├── HCSPARAMS1 (0x04): 结构参数1
//! ├── HCSPARAMS2 (0x08): 结构参数2
//! ├── HCSPARAMS3 (0x0C): 结构参数3
//! ├── HCCPARAMS1 (0x10): 能力参数1
//! ├── DBOFF (0x14): 门铃寄存器偏移
//! ├── RTSOFF (0x18): 运行时寄存器偏移
//! └── Operational Registers:
//!     ├── USBCMD (0x00): USB命令寄存器
//!     ├── USBSTS (0x04): USB状态寄存器
//!     ├── PAGESIZE (0x08): 页大小
//!     └── PORTSC (0x400+): 端口状态和控制
//! ```
//!
//! # Safety
//! xHCI驱动涉及复杂的DMA操作和MMIO寄存器访问。

use super::framework::{DeviceInfo, DeviceType, Driver, DriverError, Result};
use super::usb_core::{HostController, Urb, UsbSpeed};
use crate::kernel::framework::iomem::IoMem;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr;

// ============================================================================
// xHCI 寄存器定义
// ============================================================================

/// xHCI 能力寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct XhciCapabilityRegisters {
    /// 能力寄存器长度
    pub cap_length: u8,
    /// 保留
    pub reserved: u8,
    /// xHCI版本号
    pub hci_version: u16,
    /// 结构参数1
    pub hcs_params1: u32,
    /// 结构参数2
    pub hcs_params2: u32,
    /// 结构参数3
    pub hcs_params3: u32,
    /// 能力参数1
    pub hcc_params1: u32,
    /// 数据库偏移
    pub db_off: u32,
    /// 运行时寄存器偏移
    pub rts_off: u32,
    /// 能力参数2
    pub hcc_params2: u32,
}

/// xHCI 操作寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct XhciOperationalRegisters {
    /// USB命令寄存器
    pub usb_cmd: u32,
    /// USB状态寄存器
    pub usb_sts: u32,
    /// 页大小
    pub page_size: u32,
    /// 保留
    pub reserved1: [u32; 2],
    /// 设备通知控制
    pub dn_ctrl: u32,
    /// 命令环控制
    pub cr_ctrl: u64,
    /// 保留
    pub reserved2: [u32; 4],
    /// 设备上下文基地址数组指针
    pub dcbaap: u64,
    /// 配置参数
    pub config: u32,
}

/// 端口状态和控制寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct XhciPortRegister {
    /// 端口状态和控制
    pub portsc: u32,
    /// 端口电源管理状态和控制
    pub portpmsc: u32,
    /// 端口链路信息
    pub portli: u32,
    /// 保留
    pub reserved: u32,
}

// ============================================================================
// xHCI 命令和状态位
// ============================================================================

/// xHCI USB 命令寄存器 (USBCMD) 位定义 — xHCI 规范 §5.4.1
///
/// 当前使用的位:
/// - RUN_STOP    (bit 0): 运行/停止
/// - HC_RESET    (bit 1): 控制器复位
/// - INTR_ENABLE (bit 2): 中断使能
///
/// 规范定义的全部位 (未实现部分供参考):
/// - HOST_SYSTEM_ERROR_ENABLE (bit 3)
/// - DRIVER_DEBUG (bit 4)
/// - LIGHT_HC_RESET (bit 5)
/// - CONTROLLER_SAVE_STATE (bit 6)
/// - CONTROLLER_RESTORE_STATE (bit 7)
/// - ENABLE_U3 (bit 8)
/// - ENABLE_S0IX (bit 9)
/// - WRAP_EVENT_CHECKING (bit 10)
/// - STROBE_DEBUG (bit 11)
/// - PARK_MODE_{ENABLE,SELECT} (bits 12-14)
/// - EVENT_RING_SEGMENT_TABLE_SIZE_MODE (bit 15)
/// - CONFIGURE_ENDPOINT_MAX_EXIT_LATENCY_TOO_LARGE (bit 16)
mod usb_cmd {
    pub const RUN_STOP: u32 = 1 << 0;
    pub const HC_RESET: u32 = 1 << 1;
    pub const INTR_ENABLE: u32 = 1 << 2;
}

/// xHCI USB 状态寄存器 (USBSTS) 位定义 — xHCI 规范 §5.4.2
///
/// 当前使用的位:
/// - HC_HALTED        (bit 0): 控制器已停止
/// - HC_RESET_COMPLETE (bit 1): 复位完成
///
/// 规范定义的全部位 (未实现部分供参考):
/// - EVENT_RING_NOT_EMPTY (bit 2), INTR_PENDING (bit 3),
/// - HOST_SYSTEM_ERROR (bit 4), EVENT_COUNTER_OVERFLOW (bit 5),
/// - PORT_CHANGE_DETECT (bit 6), SAVE_RESTORE_COMPLETE (bit 7),
/// - RESTORE_ERROR (bit 8), CONTROLLER_NOT_READY (bit 11),
/// - HOST_CONTROLLER_ERROR (bit 12)
mod usb_sts {
    pub const HC_HALTED: u32 = 1 << 0;
    pub const HC_RESET_COMPLETE: u32 = 1 << 1;
}

/// xHCI 端口状态与控制寄存器 (PORTSC) 位定义 — xHCI 规范 §5.4.8
///
/// 当前使用的位:
/// - CURRENT_CONNECT_STATUS (bit 0): 设备已连接
/// - PORT_ENABLED           (bit 1): 端口已使能
/// - PORT_RESET             (bit 4): 端口复位
/// - PORT_POWER             (bit 9): 端口供电
///
/// 规范定义的其余位 (未实现部分供参考):
/// - PORT_LINK_STATE [5:8], PORT_SPEED [10:13], PORT_INDICATOR [14:15],
/// - CONNECT_STATUS_CHANGE (bit 16), PORT_ENABLED_DISABLED_CHANGE (bit 17),
/// - OVER_CURRENT_CHANGE (bit 19), RESET_CHANGE (bit 21),
/// - WAKE_ON_{CONNECT,DISCONNECT,OVER_CURRENT} (bits 20-22),
/// - DEVICE_REMOVABLE (bit 23), PORT_LINK_STATE_STROBE (bit 26),
/// - PORT_TEST [28:31]
mod portsc {
    pub const CURRENT_CONNECT_STATUS: u32 = 1 << 0;
    #[allow(dead_code)] // 规范定义, 待端口使能/禁用变更中断处理启用后使用。
    pub const PORT_ENABLED: u32 = 1 << 1;
    pub const PORT_RESET: u32 = 1 << 4;
    #[allow(dead_code)] // 规范定义, 待端口电源管理启用后使用。
    pub const PORT_POWER: u32 = 1 << 9;
}

// ============================================================================
// xHCI 传输描述符 (TRB)
// ============================================================================

/// TRB类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TrbType {
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Isoch = 5,
    Link = 6,
    EventData = 7,
    NoOp = 8,
    EnableSlot = 9,
    DisableSlot = 10,
    AddressDevice = 11,
    ConfigureEndpoint = 12,
    EvaluateContext = 13,
    ResetEndpoint = 14,
    StopEndpoint = 15,
    SetTrDequeuePointer = 16,
    ResetDevice = 17,
    ForceEvent = 18,
    NegotiateBandwidth = 19,
    SetLatencyToleranceValue = 20,
    GetPortBandwidth = 21,
    ForceHeader = 22,
    NoOpCommand = 23,
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
    BandwidthRequestEvent = 35,
    DoorbellEvent = 36,
    HostControllerEvent = 37,
    DeviceNotificationEvent = 38,
    MfindexWrapEvent = 39,
}

/// 传输描述符 (TRB) - 16字节
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    pub fn new(parameter: u64, status: u32, control: u32) -> Self {
        Self {
            parameter,
            status,
            control,
        }
    }

    pub fn trb_type(&self) -> TrbType {
        let ty = (self.control >> 10) & 0x3F;
        match ty {
            1 => TrbType::Normal,
            2 => TrbType::SetupStage,
            3 => TrbType::DataStage,
            4 => TrbType::StatusStage,
            5 => TrbType::Isoch,
            6 => TrbType::Link,
            7 => TrbType::EventData,
            8 => TrbType::NoOp,
            9 => TrbType::EnableSlot,
            10 => TrbType::DisableSlot,
            11 => TrbType::AddressDevice,
            12 => TrbType::ConfigureEndpoint,
            13 => TrbType::EvaluateContext,
            14 => TrbType::ResetEndpoint,
            15 => TrbType::StopEndpoint,
            16 => TrbType::SetTrDequeuePointer,
            17 => TrbType::ResetDevice,
            18 => TrbType::ForceEvent,
            19 => TrbType::NegotiateBandwidth,
            20 => TrbType::SetLatencyToleranceValue,
            21 => TrbType::GetPortBandwidth,
            22 => TrbType::ForceHeader,
            23 => TrbType::NoOpCommand,
            32 => TrbType::TransferEvent,
            33 => TrbType::CommandCompletionEvent,
            34 => TrbType::PortStatusChangeEvent,
            35 => TrbType::BandwidthRequestEvent,
            36 => TrbType::DoorbellEvent,
            37 => TrbType::HostControllerEvent,
            38 => TrbType::DeviceNotificationEvent,
            39 => TrbType::MfindexWrapEvent,
            _ => TrbType::Normal,
        }
    }

    pub fn cycle_bit(&self) -> bool {
        self.control & 1 != 0
    }
}

// ============================================================================
// xHCI 主机控制器
// ============================================================================

/// xHCI 主机控制器驱动
pub struct XhciController {
    /// MMIO 句柄 (safe access proxy)
    iomem: Option<IoMem>,
    /// 能力寄存器指针
    cap_regs: *const XhciCapabilityRegisters,
    /// 操作寄存器指针
    op_regs: *mut XhciOperationalRegisters,
    /// 端口寄存器数组指针
    port_regs: *mut XhciPortRegister,
    /// 端口数量
    num_ports: usize,
    /// 插槽数量
    num_slots: usize,
    /// 设备信息 (待驱动框架 Device trait 集成后使用)。
    #[allow(dead_code)] // 待驱动框架 Device trait 集成后使用。
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

impl XhciController {
    /// 创建新的xHCI控制器实例
    pub fn new(iomem: IoMem) -> Self {
        Self {
            iomem: Some(iomem),
            cap_regs: ptr::null(),
            op_regs: ptr::null_mut(),
            port_regs: ptr::null_mut(),
            num_ports: 0,
            num_slots: 0,
            info: DeviceInfo::new("xhci", DeviceType::Bus),
            initialized: false,
        }
    }

    /// 初始化控制器
    fn init_hardware(&mut self) -> Result<()> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let iomem = self.iomem.as_ref().ok_or(DriverError::NotInitialized)?;
            let base = iomem.virt_ptr() as usize;

            // 设置能力寄存器指针
            self.cap_regs = base as *const XhciCapabilityRegisters;

            // 读取能力寄存器
            let cap = &*self.cap_regs;

            // 计算操作寄存器地址
            let op_base = base + cap.cap_length as usize;
            self.op_regs = op_base as *mut XhciOperationalRegisters;

            // 解析结构参数
            self.num_slots = (cap.hcs_params1 & 0xFF) as usize;
            self.num_ports = ((cap.hcs_params1 >> 24) & 0xFF) as usize;

            // 计算端口寄存器地址
            self.port_regs = (op_base + 0x400) as *mut XhciPortRegister;

            // 复位控制器
            self.reset_controller()?;

            // 启动控制器
            self.start_controller()?;
        }

        Ok(())
    }

    /// 复位控制器
    fn reset_controller(&mut self) -> Result<()> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let op = &mut *self.op_regs;

            // 设置复位位
            op.usb_cmd |= usb_cmd::HC_RESET;

            // 等待复位完成 (最多等待1秒)
            let mut timeout = 1_000_000;
            while timeout > 0 {
                if op.usb_sts & usb_sts::HC_RESET_COMPLETE != 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }

            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
        }

        Ok(())
    }

    /// 启动控制器
    fn start_controller(&mut self) -> Result<()> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let op = &mut *self.op_regs;

            // 设置运行位和中断使能
            op.usb_cmd |= usb_cmd::RUN_STOP | usb_cmd::INTR_ENABLE;

            // 等待控制器就绪
            let mut timeout = 1_000_000;
            while timeout > 0 {
                if op.usb_sts & usb_sts::HC_HALTED == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }

            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
        }

        Ok(())
    }

    /// 获取端口寄存器
    fn get_port_reg(&self, port: usize) -> Option<&XhciPortRegister> {
        if port >= self.num_ports {
            return None;
        }

        // SAFETY: `self` 由调用方保证为有效指针; 只读访问
        unsafe { Some(&*self.port_regs.add(port)) }
    }

    /// 获取端口寄存器 (可变)
    fn get_port_reg_mut(&mut self, port: usize) -> Option<&mut XhciPortRegister> {
        if port >= self.num_ports {
            return None;
        }

        // SAFETY: `self` 由调用方保证为有效指针; 只读访问
        unsafe { Some(&mut *self.port_regs.add(port)) }
    }

    /// 端点错误恢复 (待 USB 错误恢复路径启用后使用)。
    #[allow(dead_code)] // 待 USB 错误恢复路径启用后使用。
    fn recover_endpoint(&mut self, slot_id: u8, ep_id: u8) -> Result<()> {
        let _ = (slot_id, ep_id);
        self.reset_controller()?;
        self.start_controller()
    }
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for XhciController {
    fn name(&self) -> &'static str {
        "xHCI Controller"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Bus
    }

    fn init(&mut self) -> Result<()> {
        self.init_hardware()?;
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let op = &mut *self.op_regs;
            op.usb_cmd &= !usb_cmd::RUN_STOP;
        }

        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn status(&self) -> &'static str {
        if self.initialized {
            "xHCI running"
        } else {
            "xHCI stopped"
        }
    }
}

// ============================================================================
// HostController Trait 实现
// ============================================================================

impl HostController for XhciController {
    fn supported_speeds(&self) -> Vec<UsbSpeed> {
        vec![
            UsbSpeed::Super,
            UsbSpeed::High,
            UsbSpeed::Full,
            UsbSpeed::Low,
        ]
    }

    fn num_ports(&self) -> usize {
        self.num_ports
    }

    fn port_has_device(&self, port: usize) -> bool {
        if let Some(port_reg) = self.get_port_reg(port) {
            port_reg.portsc & portsc::CURRENT_CONNECT_STATUS != 0
        } else {
            false
        }
    }

    fn reset_port(&mut self, port: usize) -> Result<()> {
        let port_reg = self
            .get_port_reg_mut(port)
            .ok_or(DriverError::InvalidParameter)?;

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            // 设置复位位
            port_reg.portsc |= portsc::PORT_RESET;

            // 等待复位完成
            let mut timeout = 1_000_000;
            while timeout > 0 {
                if port_reg.portsc & portsc::PORT_RESET == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }

            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
        }

        Ok(())
    }

    fn get_port_speed(&self, port: usize) -> UsbSpeed {
        if let Some(port_reg) = self.get_port_reg(port) {
            let speed = (port_reg.portsc >> 10) & 0xF;

            match speed {
                1 => UsbSpeed::Full,
                2 => UsbSpeed::Low,
                3 => UsbSpeed::High,
                4 => UsbSpeed::Super,
                _ => UsbSpeed::Unknown,
            }
        } else {
            UsbSpeed::Unknown
        }
    }

    fn submit_urb(&mut self, _urb: &Urb) -> Result<()> {
        // TODO(TRACK-688EA7): 实现URB提交
        Err(DriverError::UnsupportedOperation)
    }

    fn cancel_urb(&mut self, _urb_id: u32) -> Result<()> {
        Err(DriverError::UnsupportedOperation)
    }

    fn allocate_address(&mut self) -> Result<u8> {
        // TODO(TRACK-2E0EB0): 实现地址分配
        Ok(1)
    }

    fn free_address(&mut self, _address: u8) {
        // TODO(TRACK-1F75C1): 实现地址释放
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xhci_controller_creation() {
        // SAFETY: 测试用固定 MMIO 地址, identity-mapped in test environment
        let iomem = unsafe {
            IoMem::new(crate::kernel::framework::mm::PhysAddr(0xFE000000), 0x10000, "xhci-test")
                .expect("test IoMem")
        };
        let ctrl = XhciController::new(iomem);
        assert_eq!(ctrl.name(), "xHCI Controller");
        assert_eq!(ctrl.device_type(), DeviceType::Bus);
        assert!(!ctrl.is_ready());
    }

    #[test]
    fn test_trb_creation() {
        let trb = Trb::new(0x12345678, 0, 0x12345678);
        assert_eq!(trb.parameter, 0x12345678);
        assert_eq!(trb.status, 0);
        assert_eq!(trb.control, 0x12345678);
    }

    #[test]
    fn test_portsc_bits() {
        assert_eq!(portsc::CURRENT_CONNECT_STATUS, 1);
        assert_eq!(portsc::PORT_ENABLED, 2);
        assert_eq!(portsc::PORT_POWER, 512);
    }
}
