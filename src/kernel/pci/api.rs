//! PCI 总线 API 层
//!
//! PCI/PCIe 配置空间访问、总线扫描、BAR 解析、设备枚举的统一入口。
//!
//! ## 调用方契约
//! - `driver::bus::pci` —— 设备初始化阶段的配置空间读写
//! - `chitin::devtree` —— 设备树生成时枚举 PCI 设备
//! - `driver::net::e1000` —— E1000 网卡 probe (但 E1000 目前走自己的直接扫描)
//! - `driver::storage::nvme` —— NVMe SSD 初始化
//! - `driver::storage::ahci` —— AHCI SATA 控制器初始化
//! - `driver::usb::xhci` —— XHCI USB 控制器初始化
//!
//! ## 内部接口
//! - `mod.rs` —— `read/write_config_byte/word/dword`, `pci_scan_all_buses`, `probe_device`
//! - `hotplug.rs` —— PCIe 热插拔支持
//!
//! ## 安全约束
//! - `read_config_*` / `write_config_*` 包含 `unsafe` (Port I/O / volatile MMIO)
//! - `pci_scan_all_buses()` 必须在启动早期单线程调用
//! - `DEVICE_LIST` 由 `spin::Mutex` 保护, 线程安全
//! - BAR 地址不可跨设备共享 (由 chitin 的 IoMem 别名检测保证)
//!
//! ## 性能特征
//! - 配置空间访问: x86_64 Port I/O (O(1)), aarch64 ECAM MMIO (O(1))
//! - 总线扫描: O(bus × dev × func) ≈ O(65536) 最坏, 实际 < 1000
//! - 设备查找: Mutex + Vec 线性扫描, O(N) (N ≤ 256)

pub use super::{
    BarType, PciBar, PciDevice,
    read_config_byte, read_config_word, read_config_dword,
    write_config_byte, write_config_word, write_config_dword,
    CLASS_STORAGE, CLASS_NETWORK, CLASS_DISPLAY, CLASS_BRIDGE,
    PCI_CMD_IO_SPACE, PCI_CMD_MEMORY_SPACE, PCI_CMD_BUS_MASTER,
};
