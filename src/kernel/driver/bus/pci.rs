//! PCI 总线驱动 (PCI Bus Driver)
//!
//! 对接真实的 PCI 子系统 (crate::kernel::pci)，
//! 提供设备枚举、配置空间访问和 C FFI 导出。
//!
//! 通过 Chitin 框架注册为 Bus 类型设备。

#![cfg(target_arch = "x86_64")]

use crate::kernel::driver::framework::{DeviceType, Driver, DriverError};
use crate::klog_info;

struct PciBusDriver;

impl Driver for PciBusDriver {
    fn name(&self) -> &'static str {
        "pci-bus"
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Bus
    }
    fn init(&mut self) -> core::result::Result<(), DriverError> {
        Ok(())
    }
    fn shutdown(&mut self) -> core::result::Result<(), DriverError> {
        Ok(())
    }
    fn is_ready(&self) -> bool {
        true
    }
}

/// 初始化 PCI 子系统并注册到 Chitin
///
/// 调用内核 PCI 模块执行总线扫描和设备枚举,
/// 然后将 PCI 总线注册到 Chitin 框架。
/// 返回发现的设备数量。
pub fn pci_init() -> i32 {
    let count = crate::kernel::pci::init() as i32;

    crate::kernel::chitin::chitin_register_driver(
        "pci-bus",
        crate::kernel::chitin::ChitinProto::Bus,
        Some(0xCF8),
        None,
        alloc::boxed::Box::new(PciBusDriver),
    );

    count
}

/// 扫描所有 PCI 总线并返回设备列表
pub fn pci_scan() {
    let devices = crate::kernel::pci::scan_all_buses();
    for dev in &devices {
        klog_info!(
            Driver,
            "PCI {:02X}:{:02X}.{} {:04X}:{:04X} class {:02X}.{:02X}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.class_code,
            dev.subclass_code
        );
    }
}

/// 获取已发现的 PCI 设备数量
pub fn pci_device_count() -> usize {
    crate::kernel::pci::device_count()
}

#[no_mangle]
pub extern "C" fn pci_init_c() -> i32 {
    pci_init()
}

#[no_mangle]
pub extern "C" fn pci_read_config_word_c(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    crate::kernel::pci::read_config_word(bus, dev, func, offset)
}

#[no_mangle]
pub extern "C" fn pci_write_config_word_c(bus: u8, dev: u8, func: u8, offset: u8, val: u16) {
    crate::kernel::pci::write_config_word(bus, dev, func, offset, val);
}
