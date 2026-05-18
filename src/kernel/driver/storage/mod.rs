//! 存储子系统 (Storage Subsystem)
//!
//! 提供多种存储设备支持：
//! - **NVMe**: PCIe SSD (高性能)
//! - **AHCI/SATA**: 传统SATA SSD/HDD
//! - **ATA/IDE**: 传统IDE设备
//!
//! ## 架构
//!
//! ```text
//! Storage Subsystem
//! ├── nvme.rs    # NVMe驱动 (PCIe SSD)
//! ├── ahci.rs    # AHCI/SATA驱动
//! └── ata.rs     # ATA/IDE驱动
//! ```

pub mod nvme;
pub mod ahci;
pub mod ata;

// 导出常用类型
pub use nvme::{
    NvmeController,
    NvmeCommand,
    NvmeCompletion,
    NvmeIdentifyController,
    NvmeIdentifyNamespace,
    NvmeQueuePair,
};

pub use ahci::{
    AhciController,
    AhciPort,
    H2dFis,
    AtaCommand,
};

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化存储子系统
pub fn storage_init() -> framework::Result<()> {
    // TODO: 扫描PCI总线查找NVMe控制器
    // TODO: 扫描PCI总线查找AHCI控制器
    // TODO: 初始化找到的控制器
    
    Ok(())
}

use super::framework;
