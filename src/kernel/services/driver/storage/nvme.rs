#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! NVMe (Non-Volatile Memory Express) 驱动 — services 层安全代理 (Phase 2.1.3)
//!
//! 封装 NVMe 控制器的 PCIe BAR0 MMIO 操作,
//! 通过 `framework::IoMem` 提供 100% safe API。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `IoMem` 由 TCB 抽象, services 层只调用 safe 方法
//! - **类型安全**: 寄存器位、队列 ID、命令字典型用枚举/常量
//! - **薄包装**: 仅暴露核心 MMIO 寄存器访问, 队列/DMA 提交仍由调用方管理
//! - **可替代**: 原 `kernel/driver/storage/nvme.rs` 仍存在, 本文件是迁移目标
//!
//! ## 硬件接口
//!
//! ```text
//! BAR0 (NVMe Controller MMIO):
//! ├── 0x00 CAP:   Controller Capabilities (R, u64)
//! ├── 0x08 VS:    Version (R, u32)
//! ├── 0x0C INTMS: Interrupt Mask Set (RW, u32)
//! ├── 0x10 INTMC: Interrupt Mask Clear (RW, u32)
//! ├── 0x14 CC:    Controller Configuration (RW, u32)
//! ├── 0x1C CSTS:  Controller Status (R, u32)
//! ├── 0x24 AQA:   Admin Queue Attributes (RW, u32)
//! ├── 0x28 ASQ:   Admin SQ Base Address (RW, u64)
//! ├── 0x30 ACQ:   Admin CQ Base Address (RW, u64)
//! └── 0x1000+:   Doorbell (per-queue, 32-bit)
//! ```
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.3 任务: NVMe 存储控制器迁移

use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::PhysAddr;

// ── BAR0 寄存器偏移 ──

/// Controller Capabilities (R, u64)
pub const NVME_REG_CAP: usize = 0x00;
/// Version (R, u32)
pub const NVME_REG_VS: usize = 0x08;
/// Interrupt Mask Set (RW, u32)
pub const NVME_REG_INTMS: usize = 0x0C;
/// Interrupt Mask Clear (RW, u32)
pub const NVME_REG_INTMC: usize = 0x10;
/// Controller Configuration (RW, u32)
pub const NVME_REG_CC: usize = 0x14;
/// Controller Status (R, u32)
pub const NVME_REG_CSTS: usize = 0x1C;
/// Admin Queue Attributes (RW, u32)
pub const NVME_REG_AQA: usize = 0x24;
/// Admin SQ Base Address (RW, u64)
pub const NVME_REG_ASQ: usize = 0x28;
/// Admin CQ Base Address (RW, u64)
pub const NVME_REG_ACQ: usize = 0x30;
/// Doorbell 寄存器基址
pub const NVME_DB_BASE: usize = 0x1000;

// ── CC 寄存器位 ──

/// Enable (CC.EN)
pub const CC_EN: u32 = 1 << 0;
/// I/O Submission Queue Entry Size (CC.IOSQES, 4-bit @ bit 16)
pub const CC_IOSQES_MASK: u32 = 0xF << 16;
/// I/O Completion Queue Entry Size (CC.IOCQES, 4-bit @ bit 20)
pub const CC_IOCQES_MASK: u32 = 0xF << 20;

// ── CSTS 寄存器位 ──

/// Ready (CSTS.RDY)
pub const CSTS_RDY: u32 = 1 << 0;
/// Controller Fatal Status (CSTS.CFS)
pub const CSTS_CFS: u32 = 1 << 1;
/// Shutdown Status (CSTS.SHST, 2-bit @ bit 2)
pub const CSTS_SHST_MASK: u32 = 0x3 << 2;
/// NVM Subsystem Reset Occurred (CSTS.NSSRO)
pub const CSTS_NSSRO: u32 = 1 << 4;

// ── 队列 ID ──

/// Admin Submission/Completion Queue ID
pub const ADMIN_QID: u16 = 0;
/// 第一个 I/O Queue ID
pub const IO_QID_BASE: u16 = 1;

// ── 队列深度 ──

/// Admin / I/O 队列深度
pub const QUEUE_DEPTH: u16 = 64;
/// SQ 条目大小 (64 字节)
pub const SQ_ENTRY_SIZE: u16 = 64;
/// CQ 条目大小 (16 字节)
pub const CQ_ENTRY_SIZE: u16 = 16;
/// SQ 占用字节 = depth * entry_size
pub const SQ_SIZE_BYTES: u32 = 64 * 64;
/// CQ 占用字节 = depth * entry_size
pub const CQ_SIZE_BYTES: u32 = 64 * 16;

// ── 命令操作码 (NVMe spec §6) ──

/// Admin Get Log Page
pub const OP_ADMIN_GET_LOG_PAGE: u8 = 0x02;
/// Admin Identify
pub const OP_ADMIN_IDENTIFY: u8 = 0x06;
/// Admin Create I/O Completion Queue
pub const OP_ADMIN_CREATE_IOCQ: u8 = 0x05;
/// Admin Create I/O Submission Queue
pub const OP_ADMIN_CREATE_IOSQ: u8 = 0x01;
/// Admin Delete I/O CQ
pub const OP_ADMIN_DELETE_IOCQ: u8 = 0x09;
/// Admin Delete I/O SQ
pub const OP_ADMIN_DELETE_IOSQ: u8 = 0x00;

/// I/O Read
pub const OP_IO_READ: u8 = 0x02;
/// I/O Write
pub const OP_IO_WRITE: u8 = 0x01;
/// I/O Flush
pub const OP_IO_FLUSH: u8 = 0x00;

// ── Identify CNS (Controller or Namespace) ──

/// Identify Controller
pub const IDENTIFY_CNS_CONTROLLER: u8 = 0x01;
/// Identify Namespace
pub const IDENTIFY_CNS_NAMESPACE: u8 = 0x00;

// ============================================================================
// 控制器状态
// ============================================================================

/// 控制器状态 (CSTS) 解析
#[derive(Debug, Clone, Copy)]
pub struct ControllerStatus {
    /// Ready
    pub ready: bool,
    /// Controller Fatal Status
    pub fatal: bool,
    /// Shutdown Status (0=normal, 1=shutdown proceeding, 2=shutdown complete)
    pub shutdown: u8,
    /// NVM Subsystem Reset Occurred
    pub nssro: bool,
}

impl ControllerStatus {
    /// 从 CSTS 寄存器解析
    pub fn from_register(val: u32) -> Self {
        Self {
            ready: val & CSTS_RDY != 0,
            fatal: val & CSTS_CFS != 0,
            shutdown: ((val & CSTS_SHST_MASK) >> 2) as u8,
            nssro: val & CSTS_NSSRO != 0,
        }
    }
}

/// 控制器能力 (CAP) 解析
#[derive(Debug, Clone, Copy)]
pub struct ControllerCapabilities {
    /// 控制器支持的最大队列深度 (CAP.MQES, 16-bit @ bit 0)
    pub max_queue_entries: u16,
    /// 控制器支持的 Doorbell 步长 (CAP.DSTRD, 4-bit @ bit 32)
    pub doorbell_stride: u8,
    /// 控制器版本 (CAP.CRTO, 2-bit @ bit 36) - 0=未实现 NVM 命令
    pub crton: u8,
    /// NVMe Subsystem Reset 支持 (CAP.CSS.NVMSRS, bit 36)
    pub nvm_subsystem_reset_supported: bool,
}

impl ControllerCapabilities {
    /// 从 CAP 寄存器解析
    pub fn from_register(val: u64) -> Self {
        let mqes = (val & 0xFFFF) as u16;
        let dstrd = ((val >> 32) & 0xF) as u8;
        let crto = ((val >> 36) & 0x3) as u8;
        Self {
            max_queue_entries: mqes,
            doorbell_stride: dstrd,
            crton: crto,
            nvm_subsystem_reset_supported: crto != 0,
        }
    }
}

// ============================================================================
// 安全代理
// ============================================================================

/// NVMe 控制器的安全代理 (services 层)。
///
/// 内部封装 `IoMem` 指向 PCIe BAR0 MMIO 区域, 提供所有 NVMe 寄存器的安全访问。
pub struct NvmeController {
    mmio: IoMem,
}

impl NvmeController {
    /// 创建 NVMe 控制器实例。
    ///
    /// # 参数
    /// - `bar0_phys`: PCIe BAR0 MMIO 物理基地址
    /// - `len`: BAR0 MMIO 区域大小 (典型 0x2000, 包含控制器寄存器 + Doorbell)
    ///
    /// # 返回
    /// - `Some(NvmeController)`: 初始化成功
    /// - `None`: 区域已被占用 (别名检测)
    pub fn new(bar0_phys: u64, len: usize) -> Option<Self> {
        let mmio = IoMem::from_pci_bar(PhysAddr::new(bar0_phys), len, "nvme-bar0").ok()?;
        Some(Self { mmio })
    }

    // ── 通用 32/64 位寄存器访问 ──

    /// 读 32 位寄存器
    #[inline]
    pub fn read32(&self, offset: usize) -> u32 {
        self.mmio.read_u32(offset)
    }

    /// 写 32 位寄存器
    #[inline]
    pub fn write32(&self, offset: usize, val: u32) {
        self.mmio.write_u32(offset, val);
    }

    /// 读 64 位寄存器
    #[inline]
    pub fn read64(&self, offset: usize) -> u64 {
        self.mmio.read_u64(offset)
    }

    /// 写 64 位寄存器
    #[inline]
    pub fn write64(&self, offset: usize, val: u64) {
        self.mmio.write_u64(offset, val);
    }

    // ── 高层寄存器访问 (语义化封装) ──

    /// 读 CAP (Controller Capabilities)
    pub fn capabilities(&self) -> ControllerCapabilities {
        ControllerCapabilities::from_register(self.read64(NVME_REG_CAP))
    }

    /// 读 VS (Version)
    pub fn version(&self) -> u32 {
        self.read32(NVME_REG_VS)
    }

    /// 设置中断掩码 (INTMS)
    pub fn enable_interrupts(&self, mask: u32) {
        self.write32(NVME_REG_INTMS, mask);
    }

    /// 清除中断掩码 (INTMC)
    pub fn disable_interrupts(&self, mask: u32) {
        self.write32(NVME_REG_INTMC, mask);
    }

    /// 读 CC (Controller Configuration)
    pub fn cc(&self) -> u32 {
        self.read32(NVME_REG_CC)
    }

    /// 写 CC
    pub fn set_cc(&self, val: u32) {
        self.write32(NVME_REG_CC, val);
    }

    /// 启用控制器 (CC.EN = 1)
    pub fn enable(&self) {
        let val = self.cc();
        self.set_cc(val | CC_EN);
    }

    /// 禁用控制器 (CC.EN = 0)
    pub fn disable(&self) {
        let val = self.cc();
        self.set_cc(val & !CC_EN);
    }

    /// 读 CSTS (Controller Status)
    pub fn status(&self) -> ControllerStatus {
        ControllerStatus::from_register(self.read32(NVME_REG_CSTS))
    }

    /// 控制器是否就绪
    pub fn is_ready(&self) -> bool {
        self.status().ready
    }

    /// 等待控制器就绪 (轮询)
    ///
    /// # 参数
    /// - `timeout`: 迭代次数上限 (粗略超时)
    pub fn wait_ready(&self, timeout: u32) -> bool {
        let mut t = timeout;
        while !self.is_ready() && t > 0 {
            t -= 1;
            core::hint::spin_loop();
        }
        self.is_ready()
    }

    /// 等待控制器禁用 (CC.EN 清零后 CSTS.RDY 也清零)
    pub fn wait_disabled(&self, timeout: u32) -> bool {
        let mut t = timeout;
        while self.is_ready() && t > 0 {
            t -= 1;
            core::hint::spin_loop();
        }
        !self.is_ready()
    }

    // ── Admin 队列配置 ──

    /// 写 AQA (Admin Queue Attributes)
    ///
    /// ASQS @ bit 0-15, ACQS @ bit 16-31
    pub fn set_aqa(&self, asqs: u16, acqs: u16) {
        let val = (asqs as u32) | ((acqs as u32) << 16);
        self.write32(NVME_REG_AQA, val);
    }

    /// 写 ASQ (Admin SQ Base Address)
    pub fn set_asq(&self, paddr: u64) {
        self.write64(NVME_REG_ASQ, paddr);
    }

    /// 写 ACQ (Admin CQ Base Address)
    pub fn set_acq(&self, paddr: u64) {
        self.write64(NVME_REG_ACQ, paddr);
    }

    // ── Doorbell ──

    /// 写 Doorbell 寄存器
    ///
    /// # 参数
    /// - `queue_id`: 队列 ID
    /// - `is_completion`: true=CQ head doorbell, false=SQ tail doorbell
    /// - `value`: 32-bit 门铃值
    pub fn write_doorbell(&self, queue_id: u16, is_completion: bool, value: u32) {
        let dstrd = self.capabilities().doorbell_stride as u16;
        let offset = NVME_DB_BASE
            + (queue_id as usize) * 8
            + (if is_completion { 4 } else { 0 })
            + (dstrd as usize) * 4;
        self.write32(offset, value);
    }

    /// 提交 Admin SQ Doorbell (写 tail)
    pub fn ring_admin_sq(&self, tail: u32) {
        self.write_doorbell(ADMIN_QID, false, tail);
    }
}
