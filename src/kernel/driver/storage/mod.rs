//! 存储设备驱动子系统 (Storage Driver Subsystem)
//!
//! 负责发现和初始化存储控制器：
//! - **AHCI SATA控制器**: 通过PCI扫描 (class 0x01, subclass 0x06)
//! - **NVMe 控制器**: 通过PCI扫描 (class 0x01, subclass 0x08)
//! - **ATA IDE 磁盘**: 传统IDE(PATA)磁盘支持
//!
//! ## 初始化流程
//!
//! ```text
//! storage_init()
//!   ├── PCI::scan_all_buses()
//!   ├── for each AHCI device  → AhciController::new(BAR).init()
//!   ├── for each NVMe device  → NvmeController::new(BAR).init()
//!   └── ata::detect_drives()   → 检测PATA磁盘
//! ```

pub mod ata;
pub mod ahci;
pub mod nvme;

// Re-export key types for driver/mod.rs convenience
pub use ahci::{AhciController, AhciPort, H2dFis, AtaCommand};
pub use nvme::{NvmeController, NvmeCommand, NvmeCompletion};

use alloc::vec::Vec;
use spin::Mutex;

use super::framework::{self, Driver};
use crate::klog_info;
use crate::klog_warn;

/// PCI 存储控制器类码
const PCI_CLASS_STORAGE: u8 = 0x01;
const PCI_SUBCLASS_AHCI: u8 = 0x06;
const PCI_SUBCLASS_NVME: u8 = 0x08;

/// 全局存储控制器注册表
static AHCI_CONTROLLERS: Mutex<Vec<AhciController>> = Mutex::new(Vec::new());
static NVME_CONTROLLERS: Mutex<Vec<NvmeController>> = Mutex::new(Vec::new());

/// 初始化存储子系统
///
/// 扫描 PCI 总线发现 AHCI/NVMe 控制器，然后初始化它们。
pub fn storage_init() -> framework::Result<()> {
    // Step 1: 确保 PCI 子系统已初始化
    let pci_count = crate::kernel::pci::init();
    if pci_count == 0 {
        klog_warn!(Driver, "storage_init: no PCI devices found, falling back to ATA");
    }

    // Step 2: 扫描 PCI 总线寻找存储控制器
    let devices = crate::kernel::pci::scan_all_buses();

    let mut ahci_found = 0u32;
    let mut nvme_found = 0u32;

    for dev in &devices {
        if dev.class_code != PCI_CLASS_STORAGE {
            continue;
        }

        match dev.subclass_code {
            PCI_SUBCLASS_AHCI => {
                // AHCI 控制器 - 使用 BAR5 (偏移 0x24)
                let bar = dev.bars[5].base_addr;
                if bar == 0 || bar == 0xFFFFFFFF {
                    klog_warn!(Driver, "AHCI: device {:02X}:{:02X}.{} has no valid BAR5",
                        dev.bus, dev.device, dev.function);
                    continue;
                }

                let mmio_base = (bar as usize) & !0xFFF; // 掩码低12位 (BAR类型/可预取位)
                klog_info!(Driver, "AHCI: found at {:02X}:{:02X}.{}, BAR5=0x{:X}",
                    dev.bus, dev.device, dev.function, mmio_base);

                let mut controller = AhciController::new(mmio_base);
                match controller.init_controller() {
                    Ok(()) => {
                        klog_info!(Driver, "AHCI: {:02X}:{:02X}.{} initialized ({} ports)",
                            dev.bus, dev.device, dev.function,
                            controller.port_count());
                        AHCI_CONTROLLERS.lock().push(controller);
                        ahci_found += 1;
                    }
                    Err(e) => {
                        klog_warn!(Driver, "AHCI: {:02X}:{:02X}.{} init failed: {:?}",
                            dev.bus, dev.device, dev.function, e);
                    }
                }
            }

            PCI_SUBCLASS_NVME => {
                // NVMe 控制器 - 使用 BAR0
                let bar = dev.bars[0].base_addr;
                if bar == 0 || bar == 0xFFFFFFFF {
                    klog_warn!(Driver, "NVMe: device {:02X}:{:02X}.{} has no valid BAR0",
                        dev.bus, dev.device, dev.function);
                    continue;
                }

                let mmio_base = (bar as usize) & !0xFFF;
                klog_info!(Driver, "NVMe: found at {:02X}:{:02X}.{}, BAR0=0x{:X}",
                    dev.bus, dev.device, dev.function, mmio_base);

                let mut controller = NvmeController::new(mmio_base);
                match controller.init() {
                    Ok(()) => {
                        klog_info!(Driver, "NVMe: {:02X}:{:02X}.{} initialized",
                            dev.bus, dev.device, dev.function);
                        NVME_CONTROLLERS.lock().push(controller);
                        nvme_found += 1;
                    }
                    Err(e) => {
                        klog_warn!(Driver, "NVMe: {:02X}:{:02X}.{} init failed: {:?}",
                            dev.bus, dev.device, dev.function, e);
                    }
                }
            }

            _ => {
                // 其他存储子类 (IDE, RAID等) 静默跳过
            }
        }
    }

    // Step 3: 传统 ATA 检测 (回退)
    // ATA 驱动使用内部全局单例, 通过 C FFI 接口初始化
    unsafe {
        crate::kernel::driver::storage::ata::ata_init();
    }

    klog_info!(Driver, "storage: {} AHCI, {} NVMe, ATA detected",
        ahci_found, nvme_found);

    if ahci_found > 0 || nvme_found > 0 {
        Ok(())
    } else {
        // 没有存储控制器时不算致命错误 - ATA 可能仍有设备
        klog_warn!(Driver, "storage: no PCI storage controllers, ATA-only mode");
        Ok(())
    }
}

/// 获取所有已发现的 AHCI 端口总数
pub fn ahci_port_count() -> usize {
    let mut total = 0usize;
    for ctrl in AHCI_CONTROLLERS.lock().iter() {
        total += ctrl.port_count();
    }
    total
}

/// 获取 NVMe 控制器数量
pub fn nvme_controller_count() -> usize {
    NVME_CONTROLLERS.lock().len()
}

/// 关机 — 关闭所有存储控制器
pub fn storage_shutdown() -> framework::Result<()> {
    for ctrl in AHCI_CONTROLLERS.lock().iter_mut() {
        let _ = ctrl.shutdown();
    }
    for ctrl in NVME_CONTROLLERS.lock().iter_mut() {
        let _ = ctrl.shutdown();
    }
    Ok(())
}