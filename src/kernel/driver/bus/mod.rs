//! 总线驱动子系统 (Bus Driver Subsystem)
//!
//! 提供系统总线支持：
//! - **PCI**: 外设组件互连总线
//! - **PCIe**: PCI Express总线 (未来)
//!
//! ## 架构
//!
//! ```text
//! Bus Subsystem
//! ├── pci.rs    # PCI总线驱动
//! └── pcie.rs   # PCIe总线驱动 (未来)
//! ```

pub mod pci;

use super::framework;
use crate::klog_info;

pub fn bus_init() -> framework::Result<()> {
    let count = pci::pci_init();
    if count >= 0 {
        klog_info!(Driver, "PCI bus initialized: {} device(s)", count);
        Ok(())
    } else {
        Err(framework::DriverError::HardwareError)
    }
}
