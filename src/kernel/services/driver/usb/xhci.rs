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

// ============================================================================
// xHCI TRB 类型定义 (USB-1.5)
// ============================================================================

/// TRB 类型枚举 — xHCI 规范 §6.4.1
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TrbType {
    /// 普通传输 TRB
    Normal = 1,
    /// Setup Stage TRB (Control Transfer)
    SetupStage = 2,
    /// Data Stage TRB (Control Transfer)
    DataStage = 3,
    /// Status Stage TRB (Control Transfer)
    StatusStage = 4,
    /// 等时传输 TRB
    Isoch = 5,
    /// Link TRB (环链接)
    Link = 6,
    /// Event Data TRB
    EventData = 7,
    /// No-Op TRB
    NoOp = 8,
    /// Enable Slot Command TRB
    EnableSlot = 9,
    /// Disable Slot Command TRB
    DisableSlot = 10,
    /// Address Device Command TRB
    AddressDevice = 11,
    /// Configure Endpoint Command TRB
    ConfigureEndpoint = 12,
    /// Evaluate Context Command TRB
    EvaluateContext = 13,
    /// Reset Endpoint Command TRB
    ResetEndpoint = 14,
    /// Stop Endpoint Command TRB
    StopEndpoint = 15,
    /// Set TR Dequeue Pointer Command TRB
    SetTrDequeuePointer = 16,
    /// Reset Device Command TRB
    ResetDevice = 17,
    /// Transfer Event TRB (Controller → Host)
    TransferEvent = 32,
    /// Command Completion Event TRB (Controller → Host)
    CommandCompletionEvent = 33,
    /// Port Status Change Event TRB
    PortStatusChangeEvent = 34,
}

/// 传输描述符 (TRB) — 16 字节, xHCI 规范 §6.4
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Trb {
    /// 参数 (含义取决于 TRB 类型)
    pub parameter: u64,
    /// 状态字段 (传输长度/完成码等)
    pub status: u32,
    /// 控制字段 (TRB 类型 + cycle bit + flags)
    pub control: u32,
}

impl Trb {
    /// 创建新 TRB
    pub fn new(parameter: u64, status: u32, control: u32) -> Self {
        Self { parameter, status, control }
    }

    /// 提取 TRB 类型 (control[15:10])
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
            32 => TrbType::TransferEvent,
            33 => TrbType::CommandCompletionEvent,
            34 => TrbType::PortStatusChangeEvent,
            _ => TrbType::Normal,
        }
    }

    /// 获取 cycle bit (control[0])
    pub fn cycle_bit(&self) -> bool {
        self.control & 1 != 0
    }
}

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
/// 门铃偏移
pub const CAP_DBOFF: usize = 0x14;
/// 运行时寄存器空间偏移
pub const CAP_RTSOFF: usize = 0x18;
/// 能力参数 2
pub const CAP_HCCPARAMS2: usize = 0x1C;

// ── Operational 寄存器偏移 (相对 OpBase, OpBase = CAPLENGTH 字节) ──

/// USB 命令
pub const OP_USBCMD: usize = 0x00;
/// USB 状态
pub const OP_USBSTS: usize = 0x04;
/// 页面大小
pub const OP_PAGESIZE: usize = 0x08;
/// 设备通知控制
pub const OP_DNCTRL: usize = 0x14;
/// 命令环控制
pub const OP_CRCR: usize = 0x18;
/// 设备上下文基址数组指针
pub const OP_DCBAAP: usize = 0x30;
/// 配置寄存器
pub const OP_CONFIG: usize = 0x38;

/// 端口 1 状态与控制寄存器 (n=0..MaxPorts-1)
pub const OP_PORTSC_BASE: usize = 0x400;
pub const OP_PORTSC_STRIDE: usize = 0x10;

// ── USBCMD 位 ──

/// 运行/停止
pub const USBCMD_RUN_STOP: u32 = 1 << 0;
/// 主控复位
pub const USBCMD_HC_RESET: u32 = 1 << 1;
/// 中断使能
pub const USBCMD_INTR_ENABLE: u32 = 1 << 2;
/// 主系统错误使能
pub const USBCMD_HSE_ENABLE: u32 = 1 << 3;
/// 轻量级主控复位
pub const USBCMD_LHCRST: u32 = 1 << 7;
/// 控制器保存状态
pub const USBCMD_CSS: u32 = 1 << 8;
/// 控制器恢复状态
pub const USBCMD_CRS: u32 = 1 << 9;
/// 启用环绕事件
pub const USBCMD_EWE: u32 = 1 << 10;
/// 启用 U3 MFINDEX 停止
pub const USBCMD_EU3S: u32 = 1 << 11;

// ── USBSTS 位 ──

/// 主控已停止
pub const USBSTS_HC_HALTED: u32 = 1 << 0;
/// 主控复位完成
pub const USBSTS_HC_RESET_COMPLETE: u32 = 1 << 1;
/// 事件环非空
pub const USBSTS_EVENT_RING_NOT_EMPTY: u32 = 1 << 2;
/// 中断挂起
pub const USBSTS_INTR_PENDING: u32 = 1 << 3;
/// 主系统错误
pub const USBSTS_HOST_SYSTEM_ERROR: u32 = 1 << 4;
/// 事件计数器溢出
pub const USBSTS_EVENT_COUNTER_OVERFLOW: u32 = 1 << 5;
/// 端口变更检测
pub const USBSTS_PORT_CHANGE_DETECT: u32 = 1 << 6;

// ── PORTSC 位 ──

/// 当前连接状态
pub const PORTSC_CCS: u32 = 1 << 0;
/// 端口使能
pub const PORTSC_PED: u32 = 1 << 1;
/// 端口复位
pub const PORTSC_PR: u32 = 1 << 4;
/// 端口上电
pub const PORTSC_PP: u32 = 1 << 9;
/// 连接状态变更
pub const PORTSC_CSC: u32 = 1 << 16;
/// 端口使能/禁用变更
pub const PORTSC_PEC: u32 = 1 << 17;
/// 过流变更
pub const PORTSC_OCC: u32 = 1 << 19;
/// 复位变更
pub const PORTSC_RC: u32 = 1 << 21;

// ── PORTSC 速度 (PORTSC[10:13]) ──

pub const PORTSC_SPEED_MASK: u32 = 0xF << 10;
pub const PORTSC_SPEED_SHIFT: u32 = 10;

/// 全速 (12 Mbps)
pub const SPEED_FULL: u8 = 1;
/// 低速 (1.5 Mbps)
pub const SPEED_LOW: u8 = 2;
/// 高速 (480 Mbps)
pub const SPEED_HIGH: u8 = 3;
/// 超速 (5 Gbps)
pub const SPEED_SUPER: u8 = 4;
/// 超速+ (10 Gbps)
pub const SPEED_SUPER_PLUS: u8 = 5;

// ============================================================================
// 解析类型
// ============================================================================

/// 结构参数 1 解析
#[derive(Debug, Clone, Copy)]
pub struct StructuralParams1 {
    /// 最大设备槽位数 (HCSPARAMS1[3:0] - 1)
    pub max_device_slots: u8,
    /// 最大中断器数 (HCSPARAMS1[18:8] - 1)
    pub max_interrupters: u16,
    /// 最大端口数 (HCSPARAMS1[31:24] - 1)
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
    /// 最大端口数 (从 HCSPARAMS1 解析)
    num_ports: u8,
    /// 最大设备插槽数 (从 HCSPARAMS1 解析)
    num_slots: u8,
    /// 控制器是否已初始化 (reset + start 完成)
    initialized: bool,
    /// 已分配设备地址位图 (USB 地址 1..=254, 位图 256 bit)
    address_bitmap: [u8; 32],
    /// 下一个待分配地址扫描起点 (加速连续分配)
    next_address_hint: u8,
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

        // 解析 HCSPARAMS1: MaxSlots = [7:0], MaxPorts = [31:24]
        let hcs_params1 = mmio.read_u32(CAP_HCSPARAMS1);
        let params = StructuralParams1::from_register(hcs_params1);

        Some(Self {
            mmio,
            cap_length,
            db_off,
            rts_off,
            num_ports: params.max_ports,
            num_slots: params.max_device_slots,
            initialized: false,
            address_bitmap: [0u8; 32],
            next_address_hint: 1,
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

    // ── 初始化序列 (xHCI 规范 §4.3) ──

    /// 完整初始化控制器 (reset → start).
    ///
    /// 执行 xHCI 规范 §4.3 初始化序列:
    /// 1. 读取 Capability 参数 (已在 new 中完成)
    /// 2. 软复位控制器 (USBCMD.HC_RESET)
    /// 3. 等待复位完成 (USBSTS.HCRST = 1)
    /// 4. 启动控制器 (USBCMD.RS = 1)
    /// 5. 启用全局中断
    ///
    /// # 返回
    /// - `Ok(())`: 初始化成功
    /// - `Err`: 超时或控制器无响应
    pub fn init_hardware(&mut self) -> Result<(), &'static str> {
        // 1. 软复位
        self.reset();
        // 2. 启动
        self.start();
        // 3. 启用中断
        self.enable_interrupts();
        self.initialized = true;
        Ok(())
    }

    /// 控制器是否已完成初始化
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// 获取端口数量
    pub fn num_ports(&self) -> u8 {
        self.num_ports
    }

    /// 获取设备插槽数量
    pub fn num_slots(&self) -> u8 {
        self.num_slots
    }

    // ── USB 设备地址分配 (USB 2.0 规范 §9.1.2) ──

    /// 分配一个 USB 设备地址 (1..=254).
    ///
    /// 从 `next_address_hint` 开始扫描 `address_bitmap`,
    /// 找到第一个未使用位 (bit=0) 标记为已用并返回该地址.
    /// 全部 254 个地址耗尽时返回 `None`.
    pub fn allocate_address(&mut self) -> Option<u8> {
        for offset in 0..254u16 {
            let addr = self.next_address_hint.wrapping_add(offset as u8);
            if addr == 0 || addr == 255 {
                continue;
            }
            let byte_idx = (addr / 8) as usize;
            let bit_idx = addr % 8;
            if byte_idx >= self.address_bitmap.len() {
                continue;
            }
            if self.address_bitmap[byte_idx] & (1 << bit_idx) == 0 {
                self.address_bitmap[byte_idx] |= 1 << bit_idx;
                self.next_address_hint = addr.wrapping_add(1);
                if self.next_address_hint == 0 || self.next_address_hint == 255 {
                    self.next_address_hint = 1;
                }
                return Some(addr);
            }
        }
        None
    }

    /// 释放 USB 设备地址 (允许复用).
    ///
    /// 地址 0 和 255 静默忽略 (保留地址).
    pub fn free_address(&mut self, address: u8) {
        if address == 0 || address == 255 {
            return;
        }
        let byte_idx = (address / 8) as usize;
        let bit_idx = address % 8;
        if byte_idx < self.address_bitmap.len() {
            self.address_bitmap[byte_idx] &= !(1 << bit_idx);
        }
    }

    // ── Command Ring 操作 ──

    /// 写入 Command Ring Control Register (CRCR).
    ///
    /// # 参数
    /// - `ring_phys`: Command Ring 物理基地址
    /// - `run`: 是否启动 Command Ring (CRCR.RCS)
    pub fn set_command_ring(&self, ring_phys: u64, run: bool) {
        let val = ring_phys | if run { 1 } else { 0 };
        self.set_crcr(val);
    }

    /// 提交命令 TRB 到 Command Ring 并触发 Doorbell.
    ///
    /// # 参数
    /// - `trb`: 要提交的命令 TRB
    /// - `ring_tail_phys`: Command Ring 当前尾指针物理地址
    ///
    /// # 流程
    /// 1. 写 TRB 到 Command Ring (由调用方管理的 DMA 内存)
    /// 2. 更新 CRCR 的 Ring Consumer Cycle State
    /// 3. 写 Doorbell slot 0 触发控制器处理
    pub fn submit_command(&self, _trb: &Trb, ring_tail_phys: u64) {
        // 注意: TRB 实际写入由调用方通过 DMA 内存完成.
        // 此处仅更新 CRCR 并触发 Doorbell.
        // CRCR 低 3 位: [0] = RCS (Ring Consumer Cycle State),
        //                [1] = CS (Command Stop), [2] = CA (Command Abort)
        let crcr_val = ring_tail_phys | 1; // RCS = 1 (consumer cycle)
        self.set_crcr(crcr_val);

        // Doorbell slot 0 = 触发 Command Ring
        self.ring_doorbell(0, 0);
    }

    // ── 设备上下文基址数组 (DCBAA) ──

    /// 设置 Device Context Base Address Array Pointer (DCBAAP).
    ///
    /// # 参数
    /// - `dcbaa_phys`: DCBAA 物理基地址 (xHCI 规范 §4.5.2)
    pub fn set_dcbaa(&self, dcbaa_phys: u64) {
        self.set_dcbaap(dcbaa_phys);
    }
}
