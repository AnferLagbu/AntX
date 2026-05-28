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
pub mod ata_block;
pub mod ahci_block;
pub mod nvme_block;

// Re-export key types for driver/mod.rs convenience
pub use ahci::{AhciController, AhciPort, H2dFis, AtaCommand};
pub use nvme::{NvmeController, NvmeCommand, NvmeCompletion};

use alloc::vec::Vec;
use spin::Mutex;

use super::framework::{self, Driver};
use crate::klog_info;
#[cfg(target_arch = "x86_64")]
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
#[cfg(target_arch = "x86_64")]
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

    crate::kernel::chitin::chitin_register_driver(
        "ata_controller",
        crate::kernel::chitin::ChitinProto::Block,
        None,
        None,
        alloc::boxed::Box::new(crate::kernel::driver::storage::ata::AtaController::new()),
    );

    // Step 3.5: 将 ATA 磁盘注册到 Chitin (唯一注册入口)
    {
        use crate::kernel::driver::block::BlockDevice;
        use crate::kernel::driver::storage::ata_block::AtaBlockDevice;
        use crate::kernel::chitin::proto_block;
        for drive in 0..4u8 {
            if let Some(dev) = AtaBlockDevice::new(drive) {
                let sectors = dev.blk_total_sectors();
                let dev_name = match drive {
                    0 => "ata0", 1 => "ata1",
                    2 => "ata2", _ => "ata3",
                };
                proto_block::register_block_device(dev_name, dev, None);
                klog_info!(Driver, "ATA: drive {} registered, {} sectors ({:.1} MB)",
                    drive, sectors, (sectors * 512) as f64 / (1024.0 * 1024.0));
            }
        }
    }

    // Step 3.6: 将 AHCI 端口注册到 Chitin (唯一注册入口)
    {
        use crate::kernel::driver::block::BlockDevice;
        use crate::kernel::driver::storage::ahci_block::AhciBlockDevice;
        use crate::kernel::chitin::proto_block;

        let mut ahci_ports: Vec<(usize, usize)> = Vec::new();
        {
            let mut controllers = AHCI_CONTROLLERS.lock();
            for (ci, controller) in controllers.iter_mut().enumerate() {
                let port_count = controller.port_count();
                for pi in 0..port_count {
                    if let Some(port) = controller.get_port(pi) {
                        if port.device_present {
                            ahci_ports.push((ci, pi));
                        }
                    }
                }
            }
        }

        for (ci, pi) in ahci_ports {
            if let Some(dev) = AhciBlockDevice::new(ci, pi) {
                let sectors = dev.blk_total_sectors();
                let dev_name = alloc::format!("ahci{}-p{}", ci, pi);
                let name_leaked: &'static str = dev_name.leak();
                proto_block::register_block_device(name_leaked, dev, None);
                klog_info!(Driver, "AHCI: ctrl={} port={} registered, {} sectors ({:.1} MB)",
                    ci, pi, sectors, (sectors * 512) as f64 / (1024.0 * 1024.0));
            }
        }
    }

    // Step 3.7: 将 NVMe 命名空间注册到 Chitin (唯一注册入口)
    {
        use crate::kernel::driver::block::BlockDevice;
        use crate::kernel::driver::storage::nvme_block::NvmeBlockDevice;
        use crate::kernel::chitin::proto_block;

        let mut nvme_ns: Vec<(usize, u32)> = Vec::new();
        {
            let controllers = NVME_CONTROLLERS.lock();
            for (ci, controller) in controllers.iter().enumerate() {
                let ns_count = controller.namespace_count();
                for nsid in 1..=ns_count {
                    let size = controller.namespace_size();
                    if size > 0 {
                        nvme_ns.push((ci, nsid));
                    }
                }
            }
        }

        for (ci, nsid) in nvme_ns {
            if let Some(dev) = NvmeBlockDevice::new(ci, nsid) {
                let sectors = dev.blk_total_sectors();
                let dev_name = alloc::format!("nvme{}-ns{}", ci, nsid);
                let name_leaked: &'static str = dev_name.leak();
                proto_block::register_block_device(name_leaked, dev, None);
                klog_info!(Driver, "NVMe: ctrl={} nsid={} registered, {} sectors ({:.1} MB)",
                    ci, nsid, sectors, (sectors * 512) as f64 / (1024.0 * 1024.0));
            }
        }
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

/// AArch64 存储初始化 — 通过 virtio-mmio 发现块设备。
#[cfg(not(target_arch = "x86_64"))]
pub fn storage_init() -> framework::Result<()> {
    use crate::kernel::driver::virtio::{self, VIRTIO_ID_BLOCK};

    // 扫描 virtio-mmio 区域，寻找块设备
    let devices = virtio::probe_all();
    let mut blk_count = 0u32;

    for dev in devices {
        if dev.device_id == VIRTIO_ID_BLOCK {
            if let Some(blk) = virtio::blk::VirtioBlk::new(dev) {
                let blk_name = alloc::format!("virtio-blk{}", blk_count);
                let name_leaked: &'static str = blk_name.leak();
                let mmio_base = blk.device.mmio_base;
                crate::kernel::chitin::proto_block::register_block_device(name_leaked, blk, Some(mmio_base as u64));
                blk_count += 1;
                klog_info!(Driver, "virtio-blk: registered device #{}", blk_count);
            }
        }
    }

    klog_info!(Driver, "storage: {} virtio-blk device(s) found", blk_count);
    Ok(())
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