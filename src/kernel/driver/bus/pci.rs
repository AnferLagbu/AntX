//! PCI 总线驱动 (PCI Bus Driver)
//!
//! 对接真实的 PCI 子系统 (crate::kernel::pci)，
//! 提供设备枚举、配置空间访问和 C FFI 导出。

use crate::klog_info;

/// 初始化 PCI 子系统
///
/// 调用内核 PCI 模块执行总线扫描和设备枚举。
/// 返回发现的设备数量。
pub fn pci_init() -> i32 {
    // 调用真实的 PCI Rust 子系统
    crate::kernel::pci::init() as i32
}

/// 扫描所有 PCI 总线并返回设备列表
pub fn pci_scan() {
    let devices = crate::kernel::pci::scan_all_buses();
    for dev in &devices {
        klog_info!(Driver,
            "PCI {:02X}:{:02X}.{} {:04X}:{:04X} class {:02X}.{:02X}",
            dev.bus, dev.device, dev.function,
            dev.vendor_id, dev.device_id,
            dev.class_code, dev.subclass_code
        );
    }
}

/// 获取已发现的 PCI 设备数量
pub fn pci_device_count() -> usize {
    crate::kernel::pci::device_count()
}

/// C 兼容的 PCI 初始化函数
#[no_mangle]
pub extern "C" fn pci_init_c() -> i32 {
    pci_init()
}

/// C 兼容的 PCI 配置读取 (word)
#[no_mangle]
pub extern "C" fn pci_read_config_word_c(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    crate::kernel::pci::read_config_word(bus, dev, func, offset)
}

/// C 兼容的 PCI 配置写入 (word)
#[no_mangle]
pub extern "C" fn pci_write_config_word_c(bus: u8, dev: u8, func: u8, offset: u8, val: u16) {
    crate::kernel::pci::write_config_word(bus, dev, func, offset, val);
}