#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! xHCI (eXtensible Host Controller Interface) USB 3.0 驱动 — services 层安全代理 (Phase 2.1.6)
//!
//! 封装 xHCI 主机控制器的 MMIO 操作,
//! 通过 `framework::IoMem` 提供 100% safe API。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `IoMem` 由 TCB 抽象, services 层只调用 safe 方法
//! - **类型安全**: 寄存器位、TRB 类型用枚举/常量
//! - **薄包装**: 暴露 Capability/Operational/Port/Doorbell 寄存器的安全访问
//! - **可替代**: 原 `kernel/driver/usb/xhci.rs` 仍存在, 本文件是迁移目标
//!
//! ## 硬件接口
//!
//! ```text
//! xHCI MMIO 区域 (来自 PCIe BAR0):
//! ├── 0x00 CAPLENGTH:  Capability 寄存器长度
//! ├── 0x04 HCSPARAMS1: 结构参数 1 (MaxSlots 等)
//! ├── 0x08 HCSPARAMS2: 结构参数 2 (MaxPorts 等)
//! ├── 0x10 HCCPARAMS1: 能力参数 1
//! ├── 0x14 DBOFF:      Doorbell 偏移
//! ├── 0x18 RTSOFF:     Runtime 偏移
//! └── (CAPLENGTH 偏移开始) Operational 寄存器:
//!     ├── +0x00 USBCMD:  USB 命令
//!     ├── +0x04 USBSTS:  USB 状态
//!     ├── +0x08 PAGESIZE: 页大小
//!     └── +0x400 PORTSC[n]: 端口 n 状态
//! ```
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.6 任务: USB/XHCI 驱动迁移

use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::PhysAddr;

// ── Capability 寄存器偏移 (在 MMIO 起始处) ──

/// Capability Length
pub const CAP_CAPLENGTH: usize = 0x00;
/// Version (u16 @ 0x02)
pub const CAP_HCIVERSION: usize = 0x02;
/// Structural Parameters 1
pub const CAP_HCSPARAMS1: usize = 0x04;
/// Structural Parameters 2
pub const CAP_HCSPARAMS2: usize = 0x08;
/// Structural Parameters 3
pub const CAP_HCSPARAMS3: usize = 0x0C;
/// Capability Parameters 1
pub const CAP_HCCPARAMS1: usize = 0x10;
/// Doorbell Offset
pub const CAP_DBOFF: usize = 0x14;
/// Runtime Register Space Offset
pub const CAP_RTSOFF: usize = 0x18;
/// Capability Parameters 2
pub const CAP_HCCPARAMS2: usize = 0x1C;

// ── Operational 寄存器偏移 (相对 OpBase, OpBase = CAPLENGTH 字节) ──

/// USB Command
pub const OP_USBCMD: usize = 0x00;
/// USB Status
pub const OP_USBSTS: usize = 0x04;
/// Page Size
pub const OP_PAGESIZE: usize = 0x08;
/// Device Notification Control
pub const OP_DNCTRL: usize = 0x14;
/// Command Ring Control
pub const OP_CRCR: usize = 0x18;
/// Device Context Base Address Array Pointer
pub const OP_DCBAAP: usize = 0x30;
/// Configure
pub const OP_CONFIG: usize = 0x38;

/// 端口 1 状态与控制寄存器 (n=0..MaxPorts-1)
pub const OP_PORTSC_BASE: usize = 0x400;
pub const OP_PORTSC_STRIDE: usize = 0x10;

// ── USBCMD 位 ──

/// Run/Stop
pub const USBCMD_RUN_STOP: u32 = 1 << 0;
/// Host Controller Reset
pub const USBCMD_HC_RESET: u32 = 1 << 1;
/// Interrupt Enable
pub const USBCMD_INTR_ENABLE: u32 = 1 << 2;
/// Host System Error Enable
pub const USBCMD_HSE_ENABLE: u32 = 1 << 3;
/// Light Host Controller Reset
pub const USBCMD_LHCRST: u32 = 1 << 7;
/// Controller Save State
pub const USBCMD_CSS: u32 = 1 << 8;
/// Controller Restore State
pub const USBCMD_CRS: u32 = 1 << 9;
/// Enable Wrap Event
pub const USBCMD_EWE: u32 = 1 << 10;
/// Enable U3 MFINDEX Stop
pub const USBCMD_EU3S: u32 = 1 << 11;

// ── USBSTS 位 ──

/// Host Controller Halted
pub const USBSTS_HC_HALTED: u32 = 1 << 0;
/// Host Controller Reset Complete
pub const USBSTS_HC_RESET_COMPLETE: u32 = 1 << 1;
/// Event Ring Not Empty
pub const USBSTS_EVENT_RING_NOT_EMPTY: u32 = 1 << 2;
/// Interrupt Pending
pub const USBSTS_INTR_PENDING: u32 = 1 << 3;
/// Host System Error
pub const USBSTS_HOST_SYSTEM_ERROR: u32 = 1 << 4;
/// Event Counter Overflow
pub const USBSTS_EVENT_COUNTER_OVERFLOW: u32 = 1 << 5;
/// Port Change Detect
pub const USBSTS_PORT_CHANGE_DETECT: u32 = 1 << 6;

// ── PORTSC 位 ──

/// Current Connect Status
pub const PORTSC_CCS: u32 = 1 << 0;
/// Port Enabled
pub const PORTSC_PED: u32 = 1 << 1;
/// Port Reset
pub const PORTSC_PR: u32 = 1 << 4;
/// Port Power
pub const PORTSC_PP: u32 = 1 << 9;
/// Connect Status Change
pub const PORTSC_CSC: u32 = 1 << 16;
/// Port Enabled/Disabled Change
pub const PORTSC_PEC: u32 = 1 << 17;
/// Over-current Change
pub const PORTSC_OCC: u32 = 1 << 19;
/// Reset Change
pub const PORTSC_RC: u32 = 1 << 21;

// ── PORTSC 速度 (PORTSC[10:13]) ──

pub const PORTSC_SPEED_MASK: u32 = 0xF << 10;
pub const PORTSC_SPEED_SHIFT: u32 = 10;

/// Full Speed (12 Mbps)
pub const SPEED_FULL: u8 = 1;
/// Low Speed (1.5 Mbps)
pub const SPEED_LOW: u8 = 2;
/// High Speed (480 Mbps)
pub const SPEED_HIGH: u8 = 3;
/// Super Speed (5 Gbps)
pub const SPEED_SUPER: u8 = 4;
/// Super Speed Plus (10 Gbps)
pub const SPEED_SUPER_PLUS: u8 = 5;

// ============================================================================
// 解析类型
// ============================================================================

/// Structural Parameters 1 解析
#[derive(Debug, Clone, Copy)]
pub struct StructuralParams1 {
    /// Max Device Slots (HCSPARAMS1[3:0] - 1)
    pub max_device_slots: u8,
    /// Max Interrupters (HCSPARAMS1[18:8] - 1)
    pub max_interrupters: u16,
    /// Max Ports (HCSPARAMS1[31:24] - 1)
    pub max_ports: u8,
}

impl StructuralParams1 {
    /// 从 HCSPARAMS1 寄存器解析
    pub fn from_register(val: u32) -> Self {
        Self {
            max_device_slots: (val & 0xFF) as u8,
            max_interrupters: ((val >> 8) & 0x7FF) as u16,
            max_ports: ((val >> 24) & 0xFF) as u8,
        }
    }
}

/// 端口状态
#[derive(Debug, Clone, Copy)]
pub struct PortStatus {
    /// 设备已连接
    pub connected: bool,
    /// 端口已使能
    pub enabled: bool,
    /// 端口正在复位
    pub reset: bool,
    /// 端口已供电
    pub powered: bool,
    /// 速度 (PORTSC[10:13] 解码)
    pub speed: u8,
}

impl PortStatus {
    /// 从 PORTSC 寄存器解析
    pub fn from_register(val: u32) -> Self {
        Self {
            connected: val & PORTSC_CCS != 0,
            enabled: val & PORTSC_PED != 0,
            reset: val & PORTSC_PR != 0,
            powered: val & PORTSC_PP != 0,
            speed: ((val & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8,
        }
    }
}

// ============================================================================
// 安全代理
// ============================================================================

/// xHCI 主机控制器的安全代理 (services 层)。
///
/// 内部封装 `IoMem` 指向 xHCI MMIO 区域, 暴露 Capability/Operational/Port 寄存器的安全访问。
pub struct XhciController {
    mmio: IoMem,
    /// Capability 长度 (OpBase = CAPLENGTH 字节)
    cap_length: u8,
    /// Doorbell 偏移
    db_off: u32,
    /// Runtime 偏移
    rts_off: u32,
}

impl XhciController {
    /// 创建 xHCI 控制器实例。
    ///
    /// # 参数
    /// - `mmio_phys`: PCIe BAR0 MMIO 物理基地址
    /// - `len`: MMIO 区域大小 (典型 0x1000 包含 Operational + 端口)
    ///
    /// # 返回
    /// - `Some(XhciController)`: 初始化成功
    /// - `None`: 区域已被占用 (别名检测)
    pub fn new(mmio_phys: u64, len: usize) -> Option<Self> {
        let mmio = IoMem::from_pci_bar(PhysAddr::new(mmio_phys), len, "xhci-bar0").ok()?;

        // 读取 CAPLENGTH/DBOFF/RTSOFF
        let cap_length = mmio.read_u8(CAP_CAPLENGTH);
        let db_off = mmio.read_u32(CAP_DBOFF);
        let rts_off = mmio.read_u32(CAP_RTSOFF);

        Some(Self {
            mmio,
            cap_length,
            db_off,
            rts_off,
        })
    }

    /// Operational 寄存器基址 (绝对偏移)
    #[inline]
    fn op_base(&self) -> usize {
        self.cap_length as usize
    }

    /// 通用 32 位读
    #[inline]
    pub fn read32(&self, offset: usize) -> u32 {
        self.mmio.read_u32(offset)
    }

    /// 通用 32 位写
    #[inline]
    pub fn write32(&self, offset: usize, val: u32) {
        self.mmio.write_u32(offset, val);
    }

    // ── Capability 寄存器 ──

    /// 读 CAPLENGTH
    pub fn cap_length(&self) -> u8 {
        self.cap_length
    }

    /// 读 HCI 版本
    pub fn hci_version(&self) -> u16 {
        self.mmio.read_u16(CAP_HCIVERSION)
    }

    /// 读 HCSPARAMS1
    pub fn hcs_params1(&self) -> StructuralParams1 {
        StructuralParams1::from_register(self.mmio.read_u32(CAP_HCSPARAMS1))
    }

    /// 读 HCSPARAMS2
    pub fn hcs_params2(&self) -> u32 {
        self.mmio.read_u32(CAP_HCSPARAMS2)
    }

    /// 读 HCSPARAMS3
    pub fn hcs_params3(&self) -> u32 {
        self.mmio.read_u32(CAP_HCSPARAMS3)
    }

    /// 读 HCCPARAMS1
    pub fn hcc_params1(&self) -> u32 {
        self.mmio.read_u32(CAP_HCCPARAMS1)
    }

    /// 读 DBOFF
    pub fn db_off(&self) -> u32 {
        self.db_off
    }

    /// 读 RTSOFF
    pub fn rts_off(&self) -> u32 {
        self.rts_off
    }

    // ── Operational 寄存器 ──

    /// 读 USBCMD
    pub fn usb_cmd(&self) -> u32 {
        self.mmio.read_u32(self.op_base() + OP_USBCMD)
    }

    /// 写 USBCMD
    pub fn set_usb_cmd(&self, val: u32) {
        self.mmio.write_u32(self.op_base() + OP_USBCMD, val);
    }

    /// 读 USBSTS
    pub fn usb_sts(&self) -> u32 {
        self.mmio.read_u32(self.op_base() + OP_USBSTS)
    }

    /// 写 USBSTS (write-1-to-clear)
    pub fn ack_usb_sts(&self, val: u32) {
        self.mmio.write_u32(self.op_base() + OP_USBSTS, val);
    }

    /// 读 PAGESIZE
    pub fn page_size(&self) -> u32 {
        self.mmio.read_u32(self.op_base() + OP_PAGESIZE)
    }

    /// 读 CRCR
    pub fn crcr(&self) -> u64 {
        self.mmio.read_u64(self.op_base() + OP_CRCR)
    }

    /// 写 CRCR
    pub fn set_crcr(&self, val: u64) {
        self.mmio.write_u64(self.op_base() + OP_CRCR, val);
    }

    /// 读 DCBAAP
    pub fn dcbaap(&self) -> u64 {
        self.mmio.read_u64(self.op_base() + OP_DCBAAP)
    }

    /// 写 DCBAAP
    pub fn set_dcbaap(&self, val: u64) {
        self.mmio.write_u64(self.op_base() + OP_DCBAAP, val);
    }

    /// 读 CONFIG
    pub fn config(&self) -> u32 {
        self.mmio.read_u32(self.op_base() + OP_CONFIG)
    }

    /// 写 CONFIG
    pub fn set_config(&self, val: u32) {
        self.mmio.write_u32(self.op_base() + OP_CONFIG, val);
    }

    // ── USBCMD / USBSTS 操作 ──

    /// 软复位 (USBCMD.HC_RESET)
    pub fn reset(&self) {
        let val = self.usb_cmd();
        self.set_usb_cmd(val | USBCMD_HC_RESET);
        // 等待 USBSTS.HC_RESET_COMPLETE 置位
        let mut timeout = 100_000u32;
        while self.usb_sts() & USBSTS_HC_RESET_COMPLETE == 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
        // 应答
        self.ack_usb_sts(USBSTS_HC_RESET_COMPLETE);
    }

    /// 启动控制器 (USBCMD.RUN_STOP)
    pub fn start(&self) {
        let val = self.usb_cmd();
        self.set_usb_cmd(val | USBCMD_RUN_STOP);
    }

    /// 停止控制器
    pub fn stop(&self) {
        let val = self.usb_cmd();
        self.set_usb_cmd(val & !USBCMD_RUN_STOP);
        // 等待 USBSTS.HC_HALTED 置位
        let mut timeout = 100_000u32;
        while self.usb_sts() & USBSTS_HC_HALTED == 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
    }

    /// 启用全局中断 (USBCMD.INTR_ENABLE)
    pub fn enable_interrupts(&self) {
        let val = self.usb_cmd();
        self.set_usb_cmd(val | USBCMD_INTR_ENABLE);
    }

    /// 禁用全局中断
    pub fn disable_interrupts(&self) {
        let val = self.usb_cmd();
        self.set_usb_cmd(val & !USBCMD_INTR_ENABLE);
    }

    /// 控制器是否已停止
    pub fn is_halted(&self) -> bool {
        self.usb_sts() & USBSTS_HC_HALTED != 0
    }

    /// 是否有端口状态变化 (USBSTS.PCD)
    pub fn has_port_change(&self) -> bool {
        self.usb_sts() & USBSTS_PORT_CHANGE_DETECT != 0
    }

    // ── 端口操作 ──

    /// 计算端口 n 的 PORTSC 偏移
    fn portsc_offset(&self, port: u8) -> usize {
        self.op_base() + OP_PORTSC_BASE + (port as usize) * OP_PORTSC_STRIDE
    }

    /// 读端口 n 的 PORTSC
    pub fn portsc(&self, port: u8) -> u32 {
        self.mmio.read_u32(self.portsc_offset(port))
    }

    /// 写端口 n 的 PORTSC
    pub fn set_portsc(&self, port: u8, val: u32) {
        self.mmio.write_u32(self.portsc_offset(port), val);
    }

    /// 读端口状态
    pub fn port_status(&self, port: u8) -> PortStatus {
        PortStatus::from_register(self.portsc(port))
    }

    /// 端口是否有设备连接
    pub fn port_connected(&self, port: u8) -> bool {
        self.portsc(port) & PORTSC_CCS != 0
    }

    /// 端口是否已使能
    pub fn port_enabled(&self, port: u8) -> bool {
        self.portsc(port) & PORTSC_PED != 0
    }

    /// 复位端口 (写 PORTSC.PR = 1, 等待 0)
    pub fn reset_port(&self, port: u8) {
        let val = self.portsc(port);
        self.set_portsc(port, val | PORTSC_PR);
        // 等待 PR 清零
        let mut timeout = 100_000u32;
        while self.portsc(port) & PORTSC_PR != 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
    }

    /// 应答端口变化 (写 PORTSC.CSC|PEC|RC)
    pub fn ack_port_change(&self, port: u8, bits: u32) {
        let val = self.portsc(port);
        self.set_portsc(port, val | bits);
    }

    // ── Doorbell 操作 ──

    /// 写 Doorbell 寄存器
    ///
    /// # 参数
    /// - `slot`: 设备插槽 ID (1-based)
    /// - `value`: 32-bit 门铃值 (低 8 位 = endpoint, 16 位 = stream ID)
    pub fn ring_doorbell(&self, slot: u8, value: u32) {
        let offset = self.db_off as usize + (slot as usize) * 4;
        self.mmio.write_u32(offset, value);
    }
}
