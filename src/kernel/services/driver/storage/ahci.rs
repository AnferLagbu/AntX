//! @SAFE: 本文件不含 unsafe 代码。
//!
//! AHCI (Advanced Host Controller Interface) SATA 驱动 — services 层安全代理 (Phase 2.1.4)
//!
//! 封装 AHCI HBA 的 MMIO 操作, 通过 `framework::IoMem` 提供 100% safe API。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `IoMem` 由 TCB 抽象, services 层只调用 safe 方法
//! - **类型安全**: 端口号、命令寄存器位用枚举/常量
//! - **薄包装**: 仅暴露 HBA + Port 的核心 MMIO 读写 + 状态查询
//! - **可替代**: 原 `kernel/driver/storage/ahci.rs` 仍存在, 本文件是迁移目标
//!
//! ## 硬件接口
//!
//! ```text
//! ABAR (SATA HBA MMIO region):
//! ├── 0x00 GHC_CAP: Host Capabilities
//! ├── 0x04 GHC_GHC: Global Host Control
//! ├── 0x08 GHC_IS:  Interrupt Status
//! ├── 0x0C GHC_PI:  Ports Implemented
//! ├── 0x10 GHC_VS:  Version
//! ├── 0x100 + n*0x80: Port n registers
//! │   ├── 0x00 PxCLB: Command List Base
//! │   ├── 0x08 PxFB:  FIS Base
//! │   ├── 0x10 PxIS:  Interrupt Status
//! │   ├── 0x14 PxIE:  Interrupt Enable
//! │   ├── 0x18 PxCMD: Command and Status
//! │   ├── 0x20 PxTFD: Task File Data
//! │   ├── 0x24 PxSIG: Signature
//! │   ├── 0x28 PxSSTS: SATA Status
//! │   └── 0x38 PxCI:  Command Issue
//! ```
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.4 任务: 存储设备 (AHCI) 迁移

use crate::kernel::framework::iomem::IoMem;
use crate::kernel::mm::PhysAddr;

// ── HBA 寄存器偏移 ──

/// Host Capabilities (R)
pub const GHC_CAP: usize = 0x00;
/// Global Host Control (RW)
pub const GHC_GHC: usize = 0x04;
/// Interrupt Status (R)
pub const GHC_IS: usize = 0x08;
/// Ports Implemented (R)
pub const GHC_PI: usize = 0x0C;
/// Version (R)
pub const GHC_VS: usize = 0x10;

// ── GHC 寄存器位 ──

/// HBA Reset
pub const GHC_HR: u32 = 1 << 0;
/// Interrupt Enable
pub const GHC_IE: u32 = 1 << 1;
/// AHCI Enable
pub const GHC_AE: u32 = 1 << 31;

// ── 端口寄存器区域 ──

/// 第一个端口寄存器偏移
pub const PORT_REG_BASE: usize = 0x100;
/// 端口寄存器步长
pub const PORT_REG_STRIDE: usize = 0x80;
/// AHCI 最大端口数
pub const AHCI_MAX_PORTS: usize = 32;

// ── 端口寄存器偏移 (相对端口基址) ──

pub const PxCLB: usize = 0x00;   // Command List Base (低)
pub const PxCLBU: usize = 0x04;  // Command List Base (高)
pub const PxFB: usize = 0x08;    // FIS Base (低)
pub const PxFBU: usize = 0x0C;   // FIS Base (高)
pub const PxIS: usize = 0x10;    // Interrupt Status
pub const PxIE: usize = 0x14;    // Interrupt Enable
pub const PxCMD: usize = 0x18;   // Command and Status
pub const PxTFD: usize = 0x20;   // Task File Data
pub const PxSIG: usize = 0x24;   // Signature
pub const PxSSTS: usize = 0x28;  // SATA Status
pub const PxSCTL: usize = 0x2C;  // SATA Control
pub const PxSERR: usize = 0x30;  // SATA Error
pub const PxSACT: usize = 0x34;  // SATA Active
pub const PxCI: usize = 0x38;    // Command Issue
pub const PxSNTF: usize = 0x3C;  // SATA Notification

// ── PxCMD 寄存器位 ──

/// Start (PxCMD.ST)
pub const PxCMD_ST: u32 = 1 << 0;
/// FIS Receive Enable (PxCMD.FRE)
pub const PxCMD_FRE: u32 = 1 << 4;
/// FIS Receive Running (PxCMD.FR)
pub const PxCMD_FR: u32 = 1 << 14;
/// Command List Running (PxCMD.CR)
pub const PxCMD_CR: u32 = 1 << 15;

// ── PxTFD 寄存器位 ──

/// Error (PxTFD.ERR)
pub const PxTFD_ERR: u32 = 1 << 0;
/// DRQ (data request)
pub const PxTFD_DRQ: u32 = 1 << 3;
/// Busy
pub const PxTFD_BSY: u32 = 1 << 7;

// ── 设备签名 (PxSIG) ──

/// SATA 设备签名
pub const SATA_SIG_ATA: u32 = 0x0000_0101;
/// ATAPI 设备签名
pub const SATA_SIG_ATAPI: u32 = 0xEB14_0101;
/// SEMB 设备签名
pub const SATA_SIG_SEMB: u32 = 0xC33C_0101;
/// Port Multiplier 签名
pub const SATA_SIG_PM: u32 = 0x9669_0101;

// ============================================================================
// 设备类型
// ============================================================================

/// AHCI 端口连接的设备类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciDeviceKind {
    /// SATA 磁盘 (ATA)
    Ata,
    /// ATAPI 光驱
    Atapi,
    /// SEMB 设备
    Semb,
    /// Port Multiplier
    PortMultiplier,
    /// 无设备
    None,
}

impl AhciDeviceKind {
    /// 从 PxSIG 寄存器解析
    pub fn from_signature(sig: u32) -> Self {
        match sig {
            SATA_SIG_ATA => Self::Ata,
            SATA_SIG_ATAPI => Self::Atapi,
            SATA_SIG_SEMB => Self::Semb,
            SATA_SIG_PM => Self::PortMultiplier,
            0xFFFF_FFFF | 0x0000_0000 => Self::None,
            _ => Self::None,
        }
    }
}

// ============================================================================
// 端口状态
// ============================================================================

/// AHCI 端口 SATA 状态 (PxSSTS) 解析
#[derive(Debug, Clone, Copy)]
pub struct SataStatus {
    /// Device Detection (PxSSTS[3:0])
    /// 0=未连接, 1=已连接但未建立通信, 3=建立通信, 4=离线
    pub device_detection: u8,
    /// Interface Speed (PxSSTS[7:4])
    pub interface_speed: u8,
}

impl SataStatus {
    /// 从 PxSSTS 寄存器解析
    pub fn from_register(val: u32) -> Self {
        Self {
            device_detection: (val & 0x0F) as u8,
            interface_speed: ((val >> 4) & 0x0F) as u8,
        }
    }

    /// 设备是否已建立通信 (PxSSTS.DET == 3)
    pub fn is_connected(&self) -> bool {
        self.device_detection == 3
    }
}

// ============================================================================
// HBA 安全代理
// ============================================================================

/// AHCI HBA (Host Bus Adapter) 的安全代理 (services 层)。
///
/// 内部封装 `IoMem` 指向 ABAR MMIO 区域, 提供所有 HBA + 端口寄存器的安全访问。
pub struct AhciHba {
    mmio: IoMem,
}

impl AhciHba {
    /// 创建 AHCI HBA 实例。
    ///
    /// # 参数
    /// - `abar_phys`: ABAR MMIO 物理基地址 (来自 PCI BAR5)
    /// - `len`: ABAR MMIO 区域大小 (通常 0x2000 = 8KB 包含 32 个端口)
    ///
    /// # 返回
    /// - `Some(AhciHba)`: 初始化成功
    /// - `None`: 区域已被占用 (别名检测)
    pub fn new(abar_phys: u64, len: usize) -> Option<Self> {
        let mmio = IoMem::from_pci_bar(PhysAddr::new(abar_phys), len, "ahci-abar").ok()?;
        Some(Self { mmio })
    }

    // ── HBA 全局寄存器 ──

    /// 读 HBA 能力寄存器 (GHC_CAP)
    #[inline]
    pub fn capabilities(&self) -> u32 {
        self.mmio.read_u32(GHC_CAP)
    }

    /// 读 GHC 全局控制寄存器
    #[inline]
    pub fn ghc(&self) -> u32 {
        self.mmio.read_u32(GHC_GHC)
    }

    /// 写 GHC 全局控制寄存器
    #[inline]
    pub fn set_ghc(&self, val: u32) {
        self.mmio.write_u32(GHC_GHC, val);
    }

    /// 读中断状态 (GHC_IS)
    #[inline]
    pub fn interrupt_status(&self) -> u32 {
        self.mmio.read_u32(GHC_IS)
    }

    /// 写中断状态 (GHC_IS) 应答
    #[inline]
    pub fn ack_interrupt(&self, val: u32) {
        self.mmio.write_u32(GHC_IS, val);
    }

    /// 读已实现端口位图 (GHC_PI)
    #[inline]
    pub fn ports_implemented(&self) -> u32 {
        self.mmio.read_u32(GHC_PI)
    }

    /// 读 HBA 版本 (GHC_VS)
    #[inline]
    pub fn version(&self) -> u32 {
        self.mmio.read_u32(GHC_VS)
    }

    /// 启用 AHCI 模式 (GHC.AE = 1)
    pub fn enable_ahci(&self) {
        let val = self.ghc();
        self.set_ghc(val | GHC_AE);
    }

    /// 启用全局中断 (GHC.IE = 1)
    pub fn enable_interrupts(&self) {
        let val = self.ghc();
        self.set_ghc(val | GHC_IE);
    }

    /// 禁用全局中断 (GHC.IE = 0)
    pub fn disable_interrupts(&self) {
        let val = self.ghc();
        self.set_ghc(val & !GHC_IE);
    }

    /// 软重置 HBA (GHC.HR = 1)
    pub fn reset(&self) {
        let val = self.ghc();
        self.set_ghc(val | GHC_HR);
        // 等待重置完成
        let mut timeout = 100_000u32;
        while self.ghc() & GHC_HR != 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
    }

    /// 列出已实现的端口索引 (按位遍历 GHC_PI)
    pub fn implemented_ports(&self) -> alloc::vec::Vec<u8> {
        let pi = self.ports_implemented();
        let mut ports = alloc::vec::Vec::new();
        for i in 0..AHCI_MAX_PORTS {
            if pi & (1 << i) != 0 {
                ports.push(i as u8);
            }
        }
        ports
    }

    // ── 端口 MMIO 访问 (通过偏移) ──

    /// 计算端口 n 的 MMIO 基址
    fn port_offset(&self, port: u8) -> usize {
        PORT_REG_BASE + (port as usize) * PORT_REG_STRIDE
    }

    /// 读端口 n 的 32 位寄存器
    pub fn port_read32(&self, port: u8, offset: usize) -> u32 {
        let off = self.port_offset(port) + offset;
        self.mmio.read_u32(off)
    }

    /// 写端口 n 的 32 位寄存器
    pub fn port_write32(&self, port: u8, offset: usize, val: u32) {
        let off = self.port_offset(port) + offset;
        self.mmio.write_u32(off, val);
    }

    /// 读 PxCLB (Command List Base Address, 32-bit)
    pub fn port_cmd_list_base(&self, port: u8) -> u32 {
        self.port_read32(port, PxCLB)
    }

    /// 写 PxCLB
    pub fn set_port_cmd_list_base(&self, port: u8, val: u32) {
        self.port_write32(port, PxCLB, val);
    }

    /// 读 PxCLBU (Command List Base Upper)
    pub fn port_cmd_list_base_upper(&self, port: u8) -> u32 {
        self.port_read32(port, PxCLBU)
    }

    /// 写 PxCLBU
    pub fn set_port_cmd_list_base_upper(&self, port: u8, val: u32) {
        self.port_write32(port, PxCLBU, val);
    }

    /// 设置 Command List 64-bit 物理地址
    pub fn set_port_cmd_list(&self, port: u8, paddr: u64) {
        self.set_port_cmd_list_base(port, (paddr & 0xFFFF_FFFF) as u32);
        self.set_port_cmd_list_base_upper(port, ((paddr >> 32) & 0xFFFF_FFFF) as u32);
    }

    /// 读 PxFB (FIS Base Address, 32-bit)
    pub fn port_fis_base(&self, port: u8) -> u32 {
        self.port_read32(port, PxFB)
    }

    /// 写 PxFB
    pub fn set_port_fis_base(&self, port: u8, val: u32) {
        self.port_write32(port, PxFB, val);
    }

    /// 读 PxFBU
    pub fn port_fis_base_upper(&self, port: u8) -> u32 {
        self.port_read32(port, PxFBU)
    }

    /// 写 PxFBU
    pub fn set_port_fis_base_upper(&self, port: u8, val: u32) {
        self.port_write32(port, PxFBU, val);
    }

    /// 设置 FIS 64-bit 物理地址
    pub fn set_port_fis(&self, port: u8, paddr: u64) {
        self.set_port_fis_base(port, (paddr & 0xFFFF_FFFF) as u32);
        self.set_port_fis_base_upper(port, ((paddr >> 32) & 0xFFFF_FFFF) as u32);
    }

    /// 读 PxIS (Interrupt Status)
    pub fn port_interrupt_status(&self, port: u8) -> u32 {
        self.port_read32(port, PxIS)
    }

    /// 应答端口中断 (PxIS write-1-to-clear)
    pub fn ack_port_interrupt(&self, port: u8, val: u32) {
        self.port_write32(port, PxIS, val);
    }

    /// 读 PxIE (Interrupt Enable)
    pub fn port_interrupt_enable(&self, port: u8) -> u32 {
        self.port_read32(port, PxIE)
    }

    /// 写 PxIE
    pub fn set_port_interrupt_enable(&self, port: u8, val: u32) {
        self.port_write32(port, PxIE, val);
    }

    /// 读 PxCMD
    pub fn port_cmd(&self, port: u8) -> u32 {
        self.port_read32(port, PxCMD)
    }

    /// 写 PxCMD
    pub fn set_port_cmd(&self, port: u8, val: u32) {
        self.port_write32(port, PxCMD, val);
    }

    /// 启用端口 (PxCMD.ST + PxCMD.FRE)
    pub fn port_start(&self, port: u8) {
        // 1. 启用 FRE
        let cmd = self.port_cmd(port);
        self.set_port_cmd(port, cmd | PxCMD_FRE);
        // 2. 等待 FR 置位
        let mut timeout = 100_000u32;
        while self.port_cmd(port) & PxCMD_FR == 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
        // 3. 启用 ST
        let cmd = self.port_cmd(port);
        self.set_port_cmd(port, cmd | PxCMD_ST);
        // 4. 等待 CR 置位
        let mut timeout = 100_000u32;
        while self.port_cmd(port) & PxCMD_CR == 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
    }

    /// 禁用端口 (PxCMD.ST = 0, PxCMD.FRE = 0)
    pub fn port_stop(&self, port: u8) {
        // 1. 清除 ST
        let cmd = self.port_cmd(port);
        self.set_port_cmd(port, cmd & !PxCMD_ST);
        // 2. 等待 CR 清零
        let mut timeout = 100_000u32;
        while self.port_cmd(port) & PxCMD_CR != 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
        // 3. 清除 FRE
        let cmd = self.port_cmd(port);
        self.set_port_cmd(port, cmd & !PxCMD_FRE);
        // 4. 等待 FR 清零
        let mut timeout = 100_000u32;
        while self.port_cmd(port) & PxCMD_FR != 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
    }

    /// 读 PxTFD (Task File Data)
    pub fn port_tfd(&self, port: u8) -> u32 {
        self.port_read32(port, PxTFD)
    }

    /// 读 PxSIG (设备签名)
    pub fn port_signature(&self, port: u8) -> u32 {
        self.port_read32(port, PxSIG)
    }

    /// 解析端口连接的设备类型
    pub fn port_device_kind(&self, port: u8) -> AhciDeviceKind {
        AhciDeviceKind::from_signature(self.port_signature(port))
    }

    /// 读 PxSSTS
    pub fn port_sata_status(&self, port: u8) -> SataStatus {
        SataStatus::from_register(self.port_read32(port, PxSSTS))
    }

    /// 读 PxSERR
    pub fn port_sata_error(&self, port: u8) -> u32 {
        self.port_read32(port, PxSERR)
    }

    /// 清 PxSERR (write-1-to-clear)
    pub fn ack_port_sata_error(&self, port: u8, val: u32) {
        self.port_write32(port, PxSERR, val);
    }

    /// 读 PxCI (Command Issue)
    pub fn port_cmd_issue(&self, port: u8) -> u32 {
        self.port_read32(port, PxCI)
    }

    /// 写 PxCI (发布命令)
    pub fn set_port_cmd_issue(&self, port: u8, val: u32) {
        self.port_write32(port, PxCI, val);
    }

    /// 端口是否忙碌 (PxTFD.BSY)
    pub fn port_is_busy(&self, port: u8) -> bool {
        self.port_tfd(port) & PxTFD_BSY != 0
    }

    /// 端口是否有数据请求 (PxTFD.DRQ)
    pub fn port_has_drq(&self, port: u8) -> bool {
        self.port_tfd(port) & PxTFD_DRQ != 0
    }

    /// 端口是否有错误 (PxTFD.ERR)
    pub fn port_has_error(&self, port: u8) -> bool {
        self.port_tfd(port) & PxTFD_ERR != 0
    }
}
