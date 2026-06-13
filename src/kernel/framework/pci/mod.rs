#![allow(dead_code)]
//! PCI 总线子系统 (Rust 重写)
//!
//! 取代 `src/driver/pci.c` 中的 C 实现.
//! 提供 PCI 配置空间访问、总线扫描、BAR 解析、设备枚举与驱动匹配.
//!
//! ## 配置空间访问机制
//!
//! | 架构   | 机制      | 细节 |
//! |--------|-----------|------|
//! | x86_64 | 端口 I/O  | 0xCF8 (地址) / 0xCFC (数据) |
//! | aarch64| ECAM MMIO | 内存映射配置空间, 基址 `ECAM_BASE` |
//!
//! ECAM 将每个 (bus, device, function) 映射到 4KB 对齐的 MMIO 窗口:
//!   `addr = ECAM_BASE + (bus << 20) | (dev << 15) | (func << 12) | offset`
//!
//! QEMU virt aarch64: `ECAM_BASE = 0x3F000000` (无 highmem)
//! 真实 ARM 服务器: 随平台变化 (由 DTB/ACPI 驱动)
//!
//! ## 架构
//!
//! ```text
//! pci_init() → pci_scan_all_buses(None)
//!   → for bus in 0..256:
//!       pci_scan_bus(bus)
//!         → for dev in 0..32:
//!             pci_scan_device(bus, dev)
//!               → pci_check_function(bus, dev, 0)
//!               → if multifunction: for fn in 1..8: check
//! ```
//!
//! ## e1000 独立
//!
//! e1000 驱动自行执行 PCI 扫描 (经 0xCF8/0xCFC), 不复用本模块的设备列表.
//! 这是有意为之: e1000 需要特殊的 MMIO 页表设置, 与其 probe 紧耦合.
//! 详见 `src/net/driver/e1000.c` → `e1000_probe()`.

use alloc::vec::Vec;
use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock as Mutex;
use core::fmt;

pub mod api;
pub mod hotplug;
pub mod msi;

// ── 端口 I/O 原语 (仅 x86_64) ──

#[cfg(target_arch = "x86_64")]
mod port_io {
    #[inline(always)]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    pub unsafe fn outb(port: u16, val: u8) {
        crate::arch!(outb(port, val));
    }

    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    pub unsafe fn outw(port: u16, val: u16) {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack));
    }

    #[inline(always)]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    pub unsafe fn outl(port: u16, val: u32) {
        crate::arch!(outl(port, val));
    }

    #[inline(always)]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    pub unsafe fn inb(port: u16) -> u8 {
        crate::arch!(inb(port))
    }

    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    pub unsafe fn inw(port: u16) -> u16 {
        let ret: u16;
        core::arch::asm!("in ax, dx", out("ax") ret, in("dx") port, options(nomem, nostack));
        ret
    }

    #[inline(always)]
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    pub unsafe fn inl(port: u16) -> u32 {
        crate::arch!(inl(port))
    }
}

// ── Constants ──

#[cfg(target_arch = "x86_64")]
const PCI_CONFIG_ADDR: u16 = 0xCF8;
#[cfg(target_arch = "x86_64")]
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// aarch64 下的 ECAM 基址.
/// QEMU virt aarch64 (无 highmem): 0x3F000000
/// 该值必须与 MMU identity 映射保持同步.
#[cfg(target_arch = "aarch64")]
const ECAM_BASE: u64 = 0x3F00_0000;

const PCI_MAX_BUS: u8 = 255;
const PCI_MAX_DEV: u8 = 32;
const PCI_MAX_FUNC: u8 = 8;

// 厂商/设备 ID 字段偏移
const REG_VENDOR_ID: u8 = 0x00;
const REG_DEVICE_ID: u8 = 0x02;
const REG_COMMAND: u8 = 0x04;
const REG_STATUS: u8 = 0x06;
const REG_REVISION_ID: u8 = 0x08;
const REG_CLASS_CODE: u8 = 0x0B;
const REG_HEADER_TYPE: u8 = 0x0E;
const REG_BAR0: u8 = 0x10;
const REG_CAP_PTR: u8 = 0x34;
const REG_INT_LINE: u8 = 0x3C;
const REG_INT_PIN: u8 = 0x3D;

// Command bits
pub const PCI_CMD_IO_SPACE: u16 = 1 << 0;
pub const PCI_CMD_MEMORY_SPACE: u16 = 1 << 1;
pub const PCI_CMD_BUS_MASTER: u16 = 1 << 2;

// Header type
const HEADER_MULTIFUNC: u8 = 0x80;

// Class codes
pub const CLASS_STORAGE: u8 = 0x01;
pub const CLASS_NETWORK: u8 = 0x02;
pub const CLASS_DISPLAY: u8 = 0x03;
pub const CLASS_BRIDGE: u8 = 0x06;

// ── Data types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarType {
    None = 0,
    Io = 1,
    Memory32 = 2,
    Memory64 = 3,
}

#[derive(Debug, Clone, Copy)]
pub struct PciBar {
    pub base_addr: u64,
    pub size: u64,
    pub bar_type: BarType,
    pub prefetchable: bool,
    pub is_64bit: bool,
}

impl PciBar {
    const fn empty() -> Self {
        Self {
            base_addr: 0,
            size: 0,
            bar_type: BarType::None,
            prefetchable: false,
            is_64bit: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass_code: u8,
    pub prog_if: u8,
    pub revision_id: u8,
    pub header_type: u8,
    pub command: u16,
    pub status: u16,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub bars: [PciBar; 6],
    pub bar_count: usize,
    pub capabilities_ptr: u8,
}

// ── Global state ──

static DEVICE_LIST: Mutex<Vec<PciDevice>> = Mutex::new(Vec::new());
static PCI_INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

// ── 配置空间访问 ──

/// 计算给定 (bus, device, function, offset) 对应的 ECAM MMIO 地址.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn ecam_addr(bus: u8, dev: u8, func: u8, offset: u8) -> u64 {
    ECAM_BASE
        | ((bus as u64) << 20)
        | (((dev & 0x1F) as u64) << 15)
        | (((func & 0x07) as u64) << 12)
        | (offset as u64 & 0xFFF)
}

/// 计算 x86 端口 I/O 配置空间地址.
#[cfg(target_arch = "x86_64")]
fn make_config_addr(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000u32
        | ((bus as u32) << 16)
        | (((device & 0x1F) as u32) << 11)
        | (((function & 0x07) as u32) << 8)
        | ((offset & 0xFC) as u32)
}

pub fn read_config_byte(bus: u8, dev: u8, func: u8, offset: u8) -> u8 {
    #[cfg(target_arch = "x86_64")]
    {
        let addr = make_config_addr(bus, dev, func, offset);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            port_io::outl(PCI_CONFIG_ADDR, addr);
            let val = port_io::inl(PCI_CONFIG_DATA);
            ((val >> ((offset & 3) * 8)) & 0xFF) as u8
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let addr = ecam_addr(bus, dev, func, offset) as *const u8;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe { core::ptr::read_volatile(addr) }
    }
}

pub fn read_config_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    #[cfg(target_arch = "x86_64")]
    {
        let addr = make_config_addr(bus, dev, func, offset);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            port_io::outl(PCI_CONFIG_ADDR, addr);
            let val = port_io::inl(PCI_CONFIG_DATA);
            ((val >> ((offset & 2) * 8)) & 0xFFFF) as u16
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let addr = ecam_addr(bus, dev, func, offset) as *const u16;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe { core::ptr::read_volatile(addr) }
    }
}

pub fn read_config_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        let addr = make_config_addr(bus, dev, func, offset);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            port_io::outl(PCI_CONFIG_ADDR, addr);
            port_io::inl(PCI_CONFIG_DATA)
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let addr = ecam_addr(bus, dev, func, offset) as *const u32;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe { core::ptr::read_volatile(addr) }
    }
}

pub fn write_config_byte(bus: u8, dev: u8, func: u8, offset: u8, val: u8) {
    #[cfg(target_arch = "x86_64")]
    {
        let addr = make_config_addr(bus, dev, func, offset);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            port_io::outl(PCI_CONFIG_ADDR, addr);
            let old = port_io::inl(PCI_CONFIG_DATA);
            let shift = (offset & 3) * 8;
            let mask = !(0xFFu32 << shift);
            let new = (old & mask) | ((val as u32) << shift);
            port_io::outl(PCI_CONFIG_ADDR, addr);
            port_io::outl(PCI_CONFIG_DATA, new);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let addr = ecam_addr(bus, dev, func, offset) as *mut u8;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_volatile(addr, val);
        }
    }
}

pub fn write_config_word(bus: u8, dev: u8, func: u8, offset: u8, val: u16) {
    #[cfg(target_arch = "x86_64")]
    {
        let addr = make_config_addr(bus, dev, func, offset);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            port_io::outl(PCI_CONFIG_ADDR, addr);
            let old = port_io::inl(PCI_CONFIG_DATA);
            let shift = (offset & 2) * 8;
            let mask = !(0xFFFFu32 << shift);
            let new = (old & mask) | ((val as u32) << shift);
            port_io::outl(PCI_CONFIG_ADDR, addr);
            port_io::outl(PCI_CONFIG_DATA, new);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let addr = ecam_addr(bus, dev, func, offset) as *mut u16;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_volatile(addr, val);
        }
    }
}

pub fn write_config_dword(bus: u8, dev: u8, func: u8, offset: u8, val: u32) {
    #[cfg(target_arch = "x86_64")]
    {
        let addr = make_config_addr(bus, dev, func, offset);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            port_io::outl(PCI_CONFIG_ADDR, addr);
            port_io::outl(PCI_CONFIG_DATA, val);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let addr = ecam_addr(bus, dev, func, offset) as *mut u32;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_volatile(addr, val);
        }
    }
}

// ── BAR parsing ──

fn parse_bars(bus: u8, dev: u8, func: u8) -> ([PciBar; 6], usize) {
    let mut bars = [PciBar::empty(); 6];
    let mut count = 0;
    let mut i = 0;

    while i < 6 {
        let offset = REG_BAR0 + (i as u8) * 4;
        let bar_lo = read_config_dword(bus, dev, func, offset);
        if bar_lo == 0 || bar_lo == 0xFFFF_FFFF {
            i += 1;
            continue;
        }

        // 写全 1 以确定 BAR 尺寸
        write_config_dword(bus, dev, func, offset, 0xFFFF_FFFF);
        let size_mask = read_config_dword(bus, dev, func, offset);
        // 恢复原值
        write_config_dword(bus, dev, func, offset, bar_lo);

        if bar_lo & 1 != 0 {
            // I/O BAR
            bars[count].base_addr = (bar_lo & !0x03u32) as u64;
            bars[count].size = (!(size_mask & !0x03u32)).wrapping_add(1) as u64;
            bars[count].bar_type = BarType::Io;
            count += 1;
        } else {
            // Memory BAR
            let mem_type = (bar_lo >> 1) & 0x03;
            bars[count].prefetchable = (bar_lo >> 3) & 1 != 0;

            if mem_type == 0x02 {
                // 64 位 BAR: 占用两个槽位
                bars[count].is_64bit = true;
                let bar_hi = read_config_dword(bus, dev, func, offset + 4);
                bars[count].base_addr = ((bar_lo & !0x0Fu32) as u64) | ((bar_hi as u64) << 32);
                write_config_dword(bus, dev, func, offset + 4, 0xFFFF_FFFF);
                let hi_mask = read_config_dword(bus, dev, func, offset + 4);
                write_config_dword(bus, dev, func, offset + 4, bar_hi);
                let size =
                    (!(((size_mask & !0x0Fu32) as u64) | ((hi_mask as u64) << 32))).wrapping_add(1);
                bars[count].size = size;
                bars[count].bar_type = BarType::Memory64;
                i += 1; // skip next slot
            } else {
                bars[count].base_addr = (bar_lo & !0x0Fu32) as u64;
                bars[count].size = (!(size_mask & !0x0Fu32)).wrapping_add(1) as u64;
                bars[count].bar_type = BarType::Memory32;
            }
            count += 1;
        }
        i += 1;
    }
    (bars, count)
}

// ── Scanning ──

fn probe_device(bus: u8, dev: u8, func: u8) -> Option<PciDevice> {
    let vendor = read_config_word(bus, dev, func, REG_VENDOR_ID);
    if vendor == 0xFFFF || vendor == 0x0000 {
        return None;
    }

    let device_id = read_config_word(bus, dev, func, REG_DEVICE_ID);
    let class_raw = read_config_dword(bus, dev, func, REG_REVISION_ID);
    let revision_id = (class_raw & 0xFF) as u8;
    let prog_if = ((class_raw >> 8) & 0xFF) as u8;
    let subclass_code = ((class_raw >> 16) & 0xFF) as u8;
    let class_code = ((class_raw >> 24) & 0xFF) as u8;
    let header_type = read_config_byte(bus, dev, func, REG_HEADER_TYPE) & !HEADER_MULTIFUNC;
    let command = read_config_word(bus, dev, func, REG_COMMAND);
    let status = read_config_word(bus, dev, func, REG_STATUS);
    let int_line = read_config_byte(bus, dev, func, REG_INT_LINE);
    let int_pin = read_config_byte(bus, dev, func, REG_INT_PIN);
    let cap_ptr = read_config_byte(bus, dev, func, REG_CAP_PTR);

    let (bars, bar_count) = parse_bars(bus, dev, func);

    Some(PciDevice {
        bus,
        device: dev,
        function: func,
        vendor_id: vendor,
        device_id,
        class_code,
        subclass_code,
        prog_if,
        revision_id,
        header_type,
        command,
        status,
        interrupt_line: int_line,
        interrupt_pin: int_pin,
        bars,
        bar_count,
        capabilities_ptr: cap_ptr,
    })
}

fn scan_device(bus: u8, dev: u8) -> Vec<PciDevice> {
    let mut devices = Vec::new();
    if let Some(d0) = probe_device(bus, dev, 0) {
        let is_multifunc = read_config_byte(bus, dev, 0, REG_HEADER_TYPE) & HEADER_MULTIFUNC != 0;
        devices.push(d0);
        if is_multifunc {
            for func in 1..PCI_MAX_FUNC {
                if let Some(df) = probe_device(bus, dev, func) {
                    devices.push(df);
                }
            }
        }
    }
    devices
}

fn scan_bus(bus: u8) -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for dev in 0..PCI_MAX_DEV {
        devices.append(&mut scan_device(bus, dev));
    }
    devices
}

pub fn scan_all_buses() -> Vec<PciDevice> {
    let mut all = Vec::new();
    for bus in 0..=PCI_MAX_BUS {
        let vendor = read_config_word(bus, 0, 0, REG_VENDOR_ID);
        if vendor == 0xFFFF || vendor == 0x0000 {
            continue;
        }
        all.append(&mut scan_bus(bus));
    }
    all
}

// ── Public API ──

pub fn init() -> usize {
    if PCI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst) {
        return DEVICE_LIST.lock().len();
    }

    let devices = scan_all_buses();
    let count = devices.len();
    *DEVICE_LIST.lock() = devices;
    PCI_INITIALIZED.store(true, core::sync::atomic::Ordering::SeqCst);
    count
}

}

/// 当 `pci::init()` 至少被调用过一次时返回 true.
pub fn is_initialized() -> bool {
    PCI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst)
}

pub fn get_device_list() -> Vec<PciDevice> {
    DEVICE_LIST.lock().clone()
}

pub fn find_by_vendor(vendor_id: u16) -> Vec<PciDevice> {
    DEVICE_LIST
        .lock()
        .iter()
        .filter(|d| d.vendor_id == vendor_id)
        .cloned()
        .collect()
}

pub fn find_by_class(class_code: u8) -> Vec<PciDevice> {
    DEVICE_LIST
        .lock()
        .iter()
        .filter(|d| d.class_code == class_code)
        .cloned()
        .collect()
}

pub fn find_device(vendor_id: u16, device_id: u16) -> Option<PciDevice> {
    let list = DEVICE_LIST.lock();
    if vendor_id == 0xFFFF && device_id == 0xFFFF {
        list.first().cloned()
    } else {
        list.iter()
            .find(|d| {
                (vendor_id == 0xFFFF || d.vendor_id == vendor_id)
                    && (device_id == 0xFFFF || d.device_id == device_id)
            })
            .cloned()
    }
}

pub fn device_count() -> usize {
    DEVICE_LIST.lock().len()
}

impl fmt::Display for PciDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PCI {:02X}:{:02X}.{} [{:04X}:{:04X}] class={:02X}.{:02X}",
            self.bus,
            self.device,
            self.function,
            self.vendor_id,
            self.device_id,
            self.class_code,
            self.subclass_code
        )
    }
}

// ── C FFI ──

/// C FFI: Rust PCI 初始化. 成功返回 0.
/// 由 `kernel_main()` 调用, 替代原 C `pci_init()`.
/// 原 C 版 `pci_init()` 保留在 `pci.c` 中, 仅供测试套件使用.
#[no_mangle]
pub extern "C" fn pci_rust_init() -> i32 {
    let count = init();
    extern "C" {
        fn klog_ffi_info(msg: *const u8);
    }
    let msg = alloc::format!("PCI: Rust subsystem initialised — {} device(s)", count);
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        klog_ffi_info(msg.as_ptr());
    }
    0
}

/// C FFI: get device count
#[no_mangle]
pub extern "C" fn pci_get_device_count() -> i32 {
    device_count() as i32
}

/// C FFI: read config word (for e1000 fallback compatibility)
#[no_mangle]
pub extern "C" fn pci_read_config_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    read_config_word(bus, dev, func, offset)
}

/// C FFI: read config dword
#[no_mangle]
pub extern "C" fn pci_read_config_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    read_config_dword(bus, dev, func, offset)
}

/// C FFI: write config word
#[no_mangle]
pub extern "C" fn pci_write_config_word(bus: u8, dev: u8, func: u8, offset: u8, val: u16) {
    write_config_word(bus, dev, func, offset, val)
}

/// C FFI: write config dword
#[no_mangle]
pub extern "C" fn pci_write_config_dword(bus: u8, dev: u8, func: u8, offset: u8, val: u32) {
    write_config_dword(bus, dev, func, offset, val)
}
