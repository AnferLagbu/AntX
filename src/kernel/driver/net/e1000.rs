#![allow(dead_code)]

#[cfg(not(feature = "kernel_test"))]
use core::sync::atomic::AtomicU32;

#[cfg(not(feature = "kernel_test"))]
use crate::kernel::driver::framework::DriverError;
use crate::kernel::driver::framework::{DeviceType, Driver, Result};
#[cfg(not(feature = "kernel_test"))]
use crate::klog_debug;
#[cfg(not(feature = "kernel_test"))]
use crate::klog_err;
#[cfg(not(feature = "kernel_test"))]
use crate::klog_info;
#[cfg(not(feature = "kernel_test"))]
use crate::klog_warn;
#[cfg(not(feature = "kernel_test"))]
use alloc::boxed::Box;
#[cfg(not(feature = "kernel_test"))]
use spin::Mutex;

#[cfg(not(feature = "kernel_test"))]
static POLL_COUNT: AtomicU32 = AtomicU32::new(0);

pub(crate) const E1000_TX_RING_SIZE: usize = 64;
pub(crate) const E1000_RX_RING_SIZE: usize = 128;
pub(crate) const E1000_RX_BUFFER_SIZE: usize = 2048;
const E1000_TIMEOUT: u32 = 100000;

const E1000_CTRL: u32 = 0x0000;
const E1000_CTRL_RST: u32 = 1 << 31;
const E1000_CTRL_SLU: u32 = 1 << 6;
const E1000_CTRL_ASDE: u32 = 1 << 5;
const E1000_CTRL_SPEED_1000: u32 = 2 << 8;
const E1000_CTRL_FRCDPX: u32 = 1 << 14;
const E1000_CTRL_FD: u32 = 1 << 0;
const E1000_CTRL_FRCSPD: u32 = 1 << 11;

const E1000_STATUS: u32 = 0x0008;
const E1000_STATUS_LU: u32 = 1 << 1;
const E1000_STATUS_FD: u32 = 1 << 0;
const E1000_STATUS_SPEED_1000: u32 = 2 << 6;
const E1000_STATUS_SPEED_100: u32 = 1 << 6;

const E1000_EERD: u32 = 0x0014;
const E1000_EERD_START: u32 = 1 << 0;
const E1000_EERD_DONE: u32 = 1 << 4;

const E1000_RCTL: u32 = 0x0100;
const E1000_RCTL_EN: u32 = 1 << 1;
const E1000_RCTL_SBP: u32 = 1 << 2;
const E1000_RCTL_UPE: u32 = 1 << 3;
const E1000_RCTL_MPE: u32 = 1 << 4;
const E1000_RCTL_BAM: u32 = 1 << 15;
const E1000_RCTL_SECRC: u32 = 1 << 26;
const E1000_RCTL_BSIZE_2048: u32 = 1 << 25;

const E1000_TCTL: u32 = 0x0400;
const E1000_TCTL_EN: u32 = 1 << 1;
const E1000_TCTL_PSP: u32 = 1 << 3;
const E1000_TCTL_COLD_FD: u32 = 0x40 << 12;
const E1000_TCTL_CT_FD: u32 = 0x10 << 4;

const E1000_TDBAL: u32 = 0x3800;
const E1000_TDBAH: u32 = 0x3804;
const E1000_TDLEN: u32 = 0x3808;
const E1000_TDH: u32 = 0x3810;
const E1000_TDT: u32 = 0x3818;

const E1000_RDBAL: u32 = 0x2800;
const E1000_RDBAH: u32 = 0x2804;
const E1000_RDLEN: u32 = 0x2808;
const E1000_RDH: u32 = 0x2810;
const E1000_RDT: u32 = 0x2818;

const E1000_IMC: u32 = 0x00D8;
const E1000_ICR: u32 = 0x00C0;
const E1000_IMS: u32 = 0x00D0;
const E1000_ICR_LSC: u32 = 1 << 2;
const E1000_ICR_RXDMT0: u32 = 1 << 4;
const E1000_ICR_RXO: u32 = 1 << 6;
const E1000_ICR_RXT0: u32 = 1 << 7;

const E1000_IPG: u32 = 0x00B0;

const E1000_RAL0: u32 = 0x5400;
const E1000_RAH0: u32 = 0x5404;
const E1000_RAH_AV: u32 = 1 << 31;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct E1000TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct E1000RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

const E1000_TXD_CMD_EOP: u8 = 1 << 0;
const E1000_TXD_CMD_IFCS: u8 = 1 << 1;
const E1000_TXD_CMD_RS: u8 = 1 << 3;
const E1000_TXD_STAT_DD: u8 = 1 << 0;
pub(crate) const E1000_RXD_STAT_DD: u8 = 1 << 0;
pub(crate) const E1000_RXD_ERR_CE: u8 = 1 << 0;
pub(crate) const E1000_RXD_ERR_SE: u8 = 1 << 1;
pub(crate) const E1000_RXD_ERR_SEQ: u8 = 1 << 2;
pub(crate) const E1000_RXD_ERR_RXE: u8 = 1 << 3;

pub struct E1000Device {
    pub bus: u8,
    pub device: u8,
    pub func: u8,
    mmio_phys: u64,
    mmio_base: *mut u8,
    pub irq: u8,
    pub mac: [u8; 6],
    tx_descs: Option<*mut E1000TxDesc>,
    tx_tail: usize,
    tx_count: u64,
    rx_descs: Option<*mut E1000RxDesc>,
    rx_buffers: [*mut u8; E1000_RX_RING_SIZE],
    rx_tail: usize,
    rx_count: u64,
    isr_count: u64,
    link_change_count: u64,
    info: crate::kernel::driver::framework::DeviceInfo,
    initialized: bool,
}

impl Default for E1000Device {
    fn default() -> Self {
        Self {
            bus: 0,
            device: 0,
            func: 0,
            mmio_phys: 0,
            mmio_base: core::ptr::null_mut(),
            irq: 0,
            mac: [0u8; 6],
            tx_descs: None,
            tx_tail: 0,
            tx_count: 0,
            rx_descs: None,
            rx_buffers: [core::ptr::null_mut(); E1000_RX_RING_SIZE],
            rx_tail: 0,
            rx_count: 0,
            isr_count: 0,
            link_change_count: 0,
            info: crate::kernel::driver::framework::DeviceInfo::new(
                "Intel E1000",
                DeviceType::Network,
            ),
            initialized: false,
        }
    }
}

#[inline(always)]
unsafe fn mmio_read32(base: *mut u8, reg: u32) -> u32 {
    let ptr = base.add(reg as usize) as *const u32;
    core::ptr::read_volatile(ptr)
}

#[inline(always)]
unsafe fn mmio_write32(base: *mut u8, reg: u32, val: u32) {
    let ptr = base.add(reg as usize) as *mut u32;
    core::ptr::write_volatile(ptr, val);
}

fn eeprom_read(dev: &E1000Device, addr: u8) -> u16 {
    unsafe {
        mmio_write32(
            dev.mmio_base,
            E1000_EERD,
            ((addr as u32) << 2) | E1000_EERD_START,
        );
        let mut timeout: u32 = 0;
        while timeout < E1000_TIMEOUT {
            let val = mmio_read32(dev.mmio_base, E1000_EERD);
            if val & E1000_EERD_DONE != 0 {
                return ((val >> 16) & 0xFFFF) as u16;
            }
            timeout += 1;
            core::hint::spin_loop();
        }
        0xFFFF
    }
}

fn read_mac_address(dev: &mut E1000Device) {
    let lo = eeprom_read(dev, 0);
    let hi = eeprom_read(dev, 1);
    dev.mac[0] = (lo & 0xFF) as u8;
    dev.mac[1] = ((lo >> 8) & 0xFF) as u8;
    dev.mac[2] = (hi & 0xFF) as u8;
    dev.mac[3] = ((hi >> 8) & 0xFF) as u8;
    let lo2 = eeprom_read(dev, 2);
    dev.mac[4] = (lo2 & 0xFF) as u8;
    dev.mac[5] = ((lo2 >> 8) & 0xFF) as u8;
}

#[cfg(not(feature = "kernel_test"))]
fn setup_descriptor_rings(dev: &mut E1000Device) -> Result<()> {
    extern "C" {
        fn kmalloc_align(size: u64, align: u64) -> *mut core::ffi::c_void;
    }

    let tx_size = core::mem::size_of::<E1000TxDesc>() * E1000_TX_RING_SIZE;
    let tx_ptr = unsafe { kmalloc_align(tx_size as u64, 16) };
    if tx_ptr.is_null() {
        klog_err!(Net, "e1000: TX desc alloc failed");
        return Err(DriverError::HardwareError);
    }
    let tx_descs = tx_ptr as *mut E1000TxDesc;
    for i in 0..E1000_TX_RING_SIZE {
        unsafe {
            (*tx_descs.add(i)).addr = 0;
            (*tx_descs.add(i)).length = 0;
            (*tx_descs.add(i)).cmd = 0;
            (*tx_descs.add(i)).status = E1000_TXD_STAT_DD;
        }
    }
    dev.tx_tail = 0;
    dev.tx_descs = Some(tx_descs);

    unsafe {
        let tx_phys = virt_to_phys(tx_ptr as u64);
        klog_debug!(
            Net,
            "e1000: TX ring virt=0x{:x} phys=0x{:x} len={}",
            tx_ptr as u64,
            tx_phys,
            tx_size
        );
        mmio_write32(dev.mmio_base, E1000_TDBAL, (tx_phys & 0xFFFFFFFF) as u32);
        mmio_write32(dev.mmio_base, E1000_TDBAH, (tx_phys >> 32) as u32);
        mmio_write32(dev.mmio_base, E1000_TDLEN, tx_size as u32);
        mmio_write32(dev.mmio_base, E1000_TDH, 0);
        mmio_write32(dev.mmio_base, E1000_TDT, 0);
    }

    let rx_size = core::mem::size_of::<E1000RxDesc>() * E1000_RX_RING_SIZE;
    let rx_ptr = unsafe { kmalloc_align(rx_size as u64, 16) };
    if rx_ptr.is_null() {
        klog_err!(Net, "e1000: RX desc alloc failed");
        return Err(DriverError::HardwareError);
    }
    let rx_descs = rx_ptr as *mut E1000RxDesc;

    for i in 0..E1000_RX_RING_SIZE {
        let buf_ptr = unsafe { kmalloc_align(E1000_RX_BUFFER_SIZE as u64, 16) };
        if buf_ptr.is_null() {
            klog_err!(Net, "e1000: RX buf[{}] alloc failed", i);
            return Err(DriverError::HardwareError);
        }
        dev.rx_buffers[i] = buf_ptr as *mut u8;
        let buf_phys = virt_to_phys(buf_ptr as u64);
        unsafe {
            (*rx_descs.add(i)).addr = buf_phys;
            (*rx_descs.add(i)).length = 0;
            (*rx_descs.add(i)).status = 0;
        }
        if i == 0 {
            klog_debug!(
                Net,
                "e1000: RX buf[0] virt=0x{:x} phys=0x{:x}",
                buf_ptr as u64,
                buf_phys
            );
        }
    }
    dev.rx_tail = 0;
    dev.rx_descs = Some(rx_descs);

    unsafe {
        let rx_phys = virt_to_phys(rx_ptr as u64);
        klog_debug!(
            Net,
            "e1000: RX ring virt=0x{:x} phys=0x{:x} len={}",
            rx_ptr as u64,
            rx_phys,
            rx_size
        );
        mmio_write32(dev.mmio_base, E1000_RDBAL, (rx_phys & 0xFFFFFFFF) as u32);
        mmio_write32(dev.mmio_base, E1000_RDBAH, (rx_phys >> 32) as u32);
        mmio_write32(dev.mmio_base, E1000_RDLEN, rx_size as u32);
        mmio_write32(dev.mmio_base, E1000_RDH, 0);
        mmio_write32(dev.mmio_base, E1000_RDT, (E1000_RX_RING_SIZE - 1) as u32);
    }
    dev.rx_tail = 0;

    Ok(())
}

pub(crate) fn virt_to_phys(virt: u64) -> u64 {
    const KERNEL_VMA_BASE: u64 = 0xFFFF800000000000;
    if virt >= KERNEL_VMA_BASE {
        virt - KERNEL_VMA_BASE
    } else {
        virt
    }
}

impl Driver for E1000Device {
    fn name(&self) -> &'static str {
        "Intel E1000 Gigabit Ethernet"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Network
    }

    #[cfg(not(feature = "kernel_test"))]
    fn init(&mut self) -> Result<()> {
        if self.mmio_base.is_null() || self.mmio_base.is_null() {
            return Err(DriverError::NotInitialized);
        }

        let base = self.mmio_base;

        unsafe {
            mmio_write32(base, E1000_CTRL, E1000_CTRL_RST);
        }
        for _ in 0..100000 {
            let ctrl = unsafe { mmio_read32(base, E1000_CTRL) };
            if ctrl & E1000_CTRL_RST == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        unsafe {
            mmio_write32(base, E1000_IMC, 0xFFFFFFFF);
        }

        {
            let ctrl = unsafe { mmio_read32(base, E1000_CTRL) };
            let new_ctrl = (ctrl & !(E1000_CTRL_RST))
                | E1000_CTRL_SLU
                | E1000_CTRL_ASDE
                | E1000_CTRL_FRCSPD
                | E1000_CTRL_SPEED_1000
                | E1000_CTRL_FRCDPX
                | E1000_CTRL_FD;
            unsafe {
                mmio_write32(base, E1000_CTRL, new_ctrl);
            }
        }

        let mut link_ready = false;
        for _ in 0..500000 {
            let status = unsafe { mmio_read32(base, E1000_STATUS) };
            if status & E1000_STATUS_LU != 0 {
                link_ready = true;
                break;
            }
            core::hint::spin_loop();
        }

        if !link_ready {
            klog_warn!(Net, "e1000: link not ready, continuing anyway");
        } else {
            let status = unsafe { mmio_read32(base, E1000_STATUS) };
            let speed = if status & E1000_STATUS_SPEED_1000 != 0 {
                "1000"
            } else if status & E1000_STATUS_SPEED_100 != 0 {
                "100"
            } else {
                "10"
            };
            let duplex = if status & E1000_STATUS_FD != 0 {
                "FD"
            } else {
                "HD"
            };
            klog_info!(Net, "e1000: NIC Link is Up {} Mbps Full Duplex", speed);
            let _ = duplex;
        }

        setup_descriptor_rings(self)?;

        let tctl = E1000_TCTL_EN | E1000_TCTL_PSP | E1000_TCTL_COLD_FD | E1000_TCTL_CT_FD;
        unsafe {
            mmio_write32(base, E1000_TCTL, tctl);
        }

        let rctl = E1000_RCTL_EN
            | E1000_RCTL_SBP
            | E1000_RCTL_UPE
            | E1000_RCTL_MPE
            | E1000_RCTL_BAM
            | E1000_RCTL_SECRC
            | E1000_RCTL_BSIZE_2048;
        unsafe {
            mmio_write32(base, E1000_RCTL, rctl);
        }

        {
            let ral = (self.mac[0] as u32)
                | ((self.mac[1] as u32) << 8)
                | ((self.mac[2] as u32) << 16)
                | ((self.mac[3] as u32) << 24);
            let rah = (self.mac[4] as u32) | ((self.mac[5] as u32) << 8) | E1000_RAH_AV;
            unsafe {
                mmio_write32(base, E1000_RAL0, ral);
                mmio_write32(base, E1000_RAH0, rah);
            }
            klog_info!(
                Net,
                "e1000: MAC={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                self.mac[0],
                self.mac[1],
                self.mac[2],
                self.mac[3],
                self.mac[4],
                self.mac[5]
            );
        }

        unsafe {
            mmio_write32(base, E1000_RDT, (E1000_RX_RING_SIZE - 1) as u32);
        }
        self.rx_tail = 0;

        unsafe {
            mmio_write32(base, E1000_IPG, 0x0060200A);
        }

        unsafe {
            mmio_write32(
                base,
                E1000_IMS,
                E1000_ICR_RXT0 | E1000_ICR_RXDMT0 | E1000_ICR_LSC,
            );
        }

        unsafe {
            let ctrl = mmio_read32(base, E1000_CTRL);
            klog_info!(
                Net,
                "e1000: initialized (CTRL=0x{:x} RDLEN=0x{:x})",
                ctrl,
                mmio_read32(base, E1000_RDLEN)
            );
        }

        self.initialized = true;
        Ok(())
    }

    #[cfg(feature = "kernel_test")]
    fn init(&mut self) -> Result<()> {
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        if !self.initialized || self.mmio_base.is_null() {
            self.initialized = false;
            return Ok(());
        }
        unsafe {
            let base = self.mmio_base;
            let mut ctrl = mmio_read32(base, E1000_CTRL);
            ctrl &= !(E1000_CTRL_SLU | E1000_CTRL_FD);
            mmio_write32(base, E1000_CTRL, ctrl);
            mmio_write32(base, E1000_RCTL, 0);
            mmio_write32(base, E1000_TCTL, 0);
        }
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized && !self.mmio_base.is_null()
    }

    fn status(&self) -> &'static str {
        if !self.initialized {
            "Not initialized"
        } else if self.mmio_base.is_null() {
            "MMIO not mapped"
        } else {
            "Link ready"
        }
    }
}

impl E1000Device {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn probe(&mut self) -> Result<()> {
        extern "C" {
            fn pci_read_config_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16;
            fn pci_read_config_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32;
            fn pci_write_config_dword(bus: u8, dev: u8, func: u8, offset: u8, val: u32);
        }

        for bus in 0..255u8 {
            let vendor_id = unsafe { pci_read_config_word(bus, 0, 0, 0x00) };
            if vendor_id == 0xFFFF || vendor_id == 0x0000 {
                if bus > 0 {
                    continue;
                }
            }

            for dev_idx in 0..32u8 {
                for func in 0..8u8 {
                    let vid = unsafe { pci_read_config_word(bus, dev_idx, func, 0x00) };
                    if vid == 0xFFFF || vid == 0x0000 {
                        if func == 0 {
                            break;
                        }
                        continue;
                    }

                    let _did = unsafe { pci_read_config_word(bus, dev_idx, func, 0x02) };
                    let class_code = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x08) };
                    let base_class = ((class_code >> 24) & 0xFF) as u8;

                    if vid == 0x8086 && base_class == 0x02 {
                        self.bus = bus;
                        self.device = dev_idx;
                        self.func = func;

                        let bar0_lo = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x10) };
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x10, 0xFFFFFFFF) };
                        let _bar_size_mask =
                            unsafe { pci_read_config_dword(bus, dev_idx, func, 0x10) };
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x10, bar0_lo) };

                        let is_io = (bar0_lo & 0x01) != 0;
                        if is_io {
                            return Err(DriverError::UnsupportedOperation);
                        }

                        self.mmio_phys = (bar0_lo & 0xFFFFFFF0) as u64;

                        let irq_reg = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x3C) };
                        self.irq = (irq_reg & 0xFF) as u8;

                        let mut cmd = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x04) };
                        cmd |= 0x06;
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x04, cmd) };

                        unsafe {
                            extern "C" {
                                fn vmm_map_huge_page(
                                    virt: u64,
                                    phys: u64,
                                    flags: u64,
                                    size_type: u8,
                                ) -> i32;
                            }
                            let mmio_aligned = self.mmio_phys & !0x1FFFFF;
                            let flags: u64 = 0x13;
                            let ret1 = vmm_map_huge_page(mmio_aligned, mmio_aligned, flags, 1);
                            let ret2 = vmm_map_huge_page(
                                mmio_aligned + 0x200000,
                                mmio_aligned + 0x200000,
                                flags,
                                1,
                            );
                            if ret1 != 0 || ret2 != 0 {
                                klog_err!(
                                    Net,
                                    "e1000: MMIO mapping failed ret1={} ret2={}",
                                    ret1,
                                    ret2
                                );
                                return Err(DriverError::HardwareError);
                            }
                            self.mmio_base = self.mmio_phys as *mut u8;
                            klog_info!(
                                Net,
                                "e1000: MMIO phys=0x{:x} base={:p} IRQ={}",
                                self.mmio_phys,
                                self.mmio_base,
                                self.irq
                            );
                        }

                        read_mac_address(self);

                        return Ok(());
                    }

                    if func == 0 && (vid & 0x8000) == 0 {
                        break;
                    }
                }
            }
        }

        Err(DriverError::DeviceNotFound)
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn send_packet(&mut self, data: &[u8]) -> Result<usize> {
        if !self.is_ready() {
            return Err(DriverError::NotInitialized);
        }

        let tx_descs = match self.tx_descs {
            Some(descs) => descs,
            None => return Err(DriverError::NotInitialized),
        };

        let tail = self.tx_tail;
        let desc = unsafe { &mut *tx_descs.add(tail) };

        let mut timeout: u32 = E1000_TIMEOUT;
        while desc.status & E1000_TXD_STAT_DD == 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }

        if timeout == 0 {
            return Err(DriverError::Timeout);
        }

        let total_len = data.len().min(2048);
        let phys = virt_to_phys(data.as_ptr() as u64);

        desc.addr = phys;
        desc.length = total_len as u16;
        desc.cmd = E1000_TXD_CMD_EOP | E1000_TXD_CMD_IFCS | E1000_TXD_CMD_RS;
        desc.status = 0;

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        self.tx_tail = (tail + 1) % E1000_TX_RING_SIZE;
        unsafe {
            mmio_write32(self.mmio_base, E1000_TDT, self.tx_tail as u32);
        }

        self.tx_count += 1;
        Ok(total_len)
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn process_rx_packets(&mut self) {
        if !self.is_ready() {
            return;
        }

        let rx_descs = match self.rx_descs {
            Some(descs) => descs,
            None => return,
        };

        let mut processed = 0u32;

        loop {
            let rdh = unsafe { mmio_read32(self.mmio_base, E1000_RDH) as usize };
            if self.rx_tail == rdh {
                break;
            }

            let desc = unsafe { &mut *rx_descs.add(self.rx_tail) };
            if desc.status & E1000_RXD_STAT_DD == 0 {
                klog_warn!(
                    Net,
                    "e1000: rx_tail={} != rdh={} but DD=0, errors=0x{:x}",
                    self.rx_tail,
                    rdh,
                    desc.errors
                );
                break;
            }

            let len = desc.length as usize;

            if desc.errors
                & (E1000_RXD_ERR_CE | E1000_RXD_ERR_SE | E1000_RXD_ERR_SEQ | E1000_RXD_ERR_RXE)
                == 0
            {
                processed += 1;
            } else {
                klog_warn!(
                    Net,
                    "e1000: RX desc[{}] errors=0x{:x} len={}",
                    self.rx_tail,
                    desc.errors,
                    len
                );
            }

            desc.status = 0;
            self.rx_count += 1;

            let prev = self.rx_tail;
            self.rx_tail = (self.rx_tail + 1) % E1000_RX_RING_SIZE;

            unsafe {
                mmio_write32(self.mmio_base, E1000_RDT, prev as u32);
            }
        }

        if processed > 0 {
            klog_debug!(
                Net,
                "e1000: RX processed {} packets (total={})",
                processed,
                self.rx_count
            );
        }
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn try_receive(&mut self, buffer: &mut [u8]) -> Option<usize> {
        if !self.is_ready() {
            return None;
        }

        let rx_descs = self.rx_descs?;

        loop {
            let rdh = unsafe { mmio_read32(self.mmio_base, E1000_RDH) as usize };
            if self.rx_tail == rdh {
                return None;
            }

            let desc = unsafe { &mut *rx_descs.add(self.rx_tail) };
            if desc.status & E1000_RXD_STAT_DD == 0 {
                break;
            }

            let len = desc.length as usize;

            if desc.errors
                & (E1000_RXD_ERR_CE | E1000_RXD_ERR_SE | E1000_RXD_ERR_SEQ | E1000_RXD_ERR_RXE)
                != 0
            {
                klog_warn!(
                    Net,
                    "e1000: try_receive skip error desc[{}] errors=0x{:x}",
                    self.rx_tail,
                    desc.errors
                );
                desc.status = 0;
                let prev = self.rx_tail;
                self.rx_tail = (self.rx_tail + 1) % E1000_RX_RING_SIZE;
                unsafe {
                    mmio_write32(self.mmio_base, E1000_RDT, prev as u32);
                }
                continue;
            }

            let copy_len = len.min(buffer.len());
            if !self.rx_buffers[self.rx_tail].is_null() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        self.rx_buffers[self.rx_tail],
                        buffer.as_mut_ptr(),
                        copy_len,
                    );
                }
            }

            desc.status = 0;
            self.rx_count += 1;

            let prev = self.rx_tail;
            self.rx_tail = (self.rx_tail + 1) % E1000_RX_RING_SIZE;
            unsafe {
                mmio_write32(self.mmio_base, E1000_RDT, prev as u32);
            }

            return Some(copy_len);
        }

        None
    }

    #[cfg(not(feature = "kernel_test"))]
    pub fn handle_interrupt(&mut self) {
        if !self.is_ready() {
            return;
        }

        let icr = unsafe { mmio_read32(self.mmio_base, E1000_ICR) };
        if icr == 0 {
            return;
        }

        self.isr_count += 1;

        if self.isr_count <= 5 {
            klog_debug!(Net, "e1000: ISR icr=0x{:x}", icr);
        }

        if icr & E1000_ICR_LSC != 0 {
            self.link_change_count += 1;
            klog_info!(Net, "e1000: link status change");
        }

        if icr & (E1000_ICR_RXT0 | E1000_ICR_RXDMT0) != 0 {
            if self.isr_count <= 5 {
                klog_debug!(Net, "e1000: RX interrupt");
            }
            self.process_rx_packets();
        }
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.isr_count,
            self.rx_count,
            self.tx_count,
            self.link_change_count,
        )
    }

    pub fn get_info(&self) -> &crate::kernel::driver::framework::DeviceInfo {
        &self.info
    }
}

#[cfg(not(feature = "kernel_test"))]
static E1000_DEVICE: Mutex<Option<Box<E1000Device>>> = Mutex::new(None);

pub fn take_device() -> Option<Box<E1000Device>> {
    E1000_DEVICE.lock().take()
}

pub unsafe fn e1000_net_send(driver_data: *mut core::ffi::c_void, data: *const u8, len: u32) -> i32 {
    if driver_data.is_null() || data.is_null() { return -1; }
    let dev = &mut *(driver_data as *mut E1000Device);
    match dev.send_packet(core::slice::from_raw_parts(data, len as usize)) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

pub unsafe fn e1000_net_recv(driver_data: *mut core::ffi::c_void, buf: *mut u8, buf_len: u32) -> i32 {
    if driver_data.is_null() || buf.is_null() { return -1; }
    let dev = &mut *(driver_data as *mut E1000Device);
    let buf_slice = core::slice::from_raw_parts_mut(buf, buf_len as usize);
    match dev.try_receive(buf_slice) {
        Some(n) => n as i32,
        None => 0,
    }
}

pub unsafe fn e1000_net_get_mac(driver_data: *mut core::ffi::c_void, mac: &mut [u8; 6]) {
    if driver_data.is_null() { return; }
    let dev = &*(driver_data as *const E1000Device);
    *mac = dev.mac;
}

pub unsafe fn e1000_net_irq(driver_data: *mut core::ffi::c_void) {
    if driver_data.is_null() { return; }
    let dev = &mut *(driver_data as *mut E1000Device);
    dev.handle_interrupt();
}

#[cfg(not(feature = "kernel_test"))]
#[no_mangle]
pub extern "C" fn e1000_irq_entry(_frame: *mut core::ffi::c_void) {
    // IRQ 上下文使用 try_lock 避免与主代码路径死锁
    if let Some(mut guard) = E1000_DEVICE.try_lock() {
        if let Some(ref mut dev) = *guard {
            dev.handle_interrupt();
        }
    }
}

#[cfg(not(feature = "kernel_test"))]
#[no_mangle]
pub extern "C" fn e1000_probe() -> i32 {
    let mut need_probe = false;
    {
        let guard = E1000_DEVICE.lock();
        if guard.is_none() {
            need_probe = true;
        }
    }

    if need_probe {
        let mut dev = Box::new(E1000Device::new());
        match dev.probe() {
            Ok(()) => {
                let raw_ptr: *mut E1000Device = &mut *dev;
                static E1000_NET_OPS: crate::kernel::chitin::proto_net::NetOps =
                    crate::kernel::chitin::proto_net::NetOps {
                        send: e1000_net_send,
                        try_receive: e1000_net_recv,
                        get_mac: e1000_net_get_mac,
                        handle_irq: Some(e1000_net_irq),
                    };
                let _id = crate::kernel::chitin::chitin_register_with_ops(
                    "e1000",
                    crate::kernel::chitin::ChitinProto::Net,
                    Some(dev.mmio_phys),
                    Some(dev.irq),
                    raw_ptr as *mut core::ffi::c_void,
                    crate::kernel::chitin::ChitinOps::Net(&E1000_NET_OPS),
                );
                *E1000_DEVICE.lock() = Some(dev);
                return 0;
            }
            Err(_) => return -1,
        }
    }

    // 已存在
    match &*E1000_DEVICE.lock() {
        Some(_) => 0,
        None => -1,
    }
}

#[cfg(not(feature = "kernel_test"))]
#[no_mangle]
pub extern "C" fn get_e1000_instance() -> *mut core::ffi::c_void {
    match &mut *E1000_DEVICE.lock() {
        Some(ref mut dev) => dev as *mut _ as *mut core::ffi::c_void,
        None => core::ptr::null_mut(),
    }
}

#[cfg(not(feature = "kernel_test"))]
#[no_mangle]
pub extern "C" fn e1000_dump_regs() {
    #[cfg(feature = "e1000-verbose")]
    {
        let guard = E1000_DEVICE.lock();
        if let Some(ref dev) = *guard {
            let base = dev.mmio_base;
            if base.is_null() {
                return;
            }
            unsafe {
                let ctrl = mmio_read32(base, E1000_CTRL);
                let status = mmio_read32(base, E1000_STATUS);
                let tctl = mmio_read32(base, E1000_TCTL);
                let rctl = mmio_read32(base, E1000_RCTL);
                let icr = mmio_read32(base, E1000_ICR);
                let ims = mmio_read32(base, E1000_IMS);
                let tdh = mmio_read32(base, E1000_TDH);
                let tdt = mmio_read32(base, E1000_TDT);
                let rdh = mmio_read32(base, E1000_RDH);
                let rdt = mmio_read32(base, E1000_RDT);
                let rdbal = mmio_read32(base, E1000_RDBAL);
                let rdbah = mmio_read32(base, E1000_RDBAH);
                let rdlen = mmio_read32(base, E1000_RDLEN);
                klog_info!(Net, "=== E1000 Register Dump ===");
                klog_info!(Net, "CTRL=0x{:x} STATUS=0x{:x}", ctrl, status);
                klog_info!(Net, "TCTL=0x{:x} RCTL=0x{:x}", tctl, rctl);
                klog_info!(Net, "ICR=0x{:x} IMS=0x{:x}", icr, ims);
                klog_info!(Net, "TDH={} TDT={}", tdh, tdt);
                klog_info!(Net, "RDH={} RDT={} rx_tail={}", rdh, rdt, dev.rx_tail);
                klog_info!(
                    Net,
                    "RDBAL=0x{:x} RDBAH=0x{:x} RDLEN={}",
                    rdbal,
                    rdbah,
                    rdlen
                );
                klog_info!(
                    Net,
                    "tx_count={} rx_count={} isr_count={}",
                    dev.tx_count,
                    dev.rx_count,
                    dev.isr_count
                );
            }
        }
    }
    #[cfg(not(feature = "e1000-verbose"))]
    let _ = &();
}

#[cfg(not(feature = "kernel_test"))]
#[no_mangle]
pub extern "C" fn e1000_dump_stats() {
    #[cfg(feature = "e1000-verbose")]
    {
        let guard = E1000_DEVICE.lock();
        if let Some(ref dev) = *guard {
            klog_info!(
                Net,
                "e1000 stats: tx={} rx={} isr={} link_chg={}",
                dev.tx_count,
                dev.rx_count,
                dev.isr_count,
                dev.link_change_count
            );
        }
    }
    let _ = &();
}

// SAFETY: 单核内核, E1000 操作序列化在 Mutex 后
#[cfg(not(feature = "kernel_test"))]
unsafe impl Send for E1000Device {}
#[cfg(not(feature = "kernel_test"))]
unsafe impl Sync for E1000Device {}

#[cfg(not(feature = "kernel_test"))]
#[repr(C, align(4096))]
struct AlignedKallocBuf {
    data: [u8; 1048576],
}

#[cfg(not(feature = "kernel_test"))]
static mut KALLOC_BUF: AlignedKallocBuf = AlignedKallocBuf { data: [0; 1048576] };
#[cfg(not(feature = "kernel_test"))]
static mut KALLOC_OFF: usize = 0;

#[cfg(not(feature = "kernel_test"))]
#[no_mangle]
///
/// # Safety
///
/// `reg` is a valid MMIO register offset within the BAR0 region. The device has been probed and MMIO region mapped.
pub unsafe extern "C" fn kmalloc_align(size: u64, align: u64) -> *mut core::ffi::c_void {
    let s = size as usize;
    let a = if align == 0 { 1 } else { align as usize };
    let base = KALLOC_BUF.data.as_mut_ptr() as usize;
    let current = base + KALLOC_OFF;
    let aligned = (current + a - 1) & !(a - 1);
    let padding = aligned - current;
    if KALLOC_OFF + padding + s > KALLOC_BUF.data.len() {
        return core::ptr::null_mut();
    }
    KALLOC_OFF += padding;
    let ptr = KALLOC_BUF.data.as_mut_ptr().add(KALLOC_OFF) as *mut core::ffi::c_void;
    KALLOC_OFF += s;
    ptr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_creation() {
        let dev = E1000Device::new();
        assert_eq!(dev.bus, 0);
        assert_eq!(dev.device, 0);
        assert!(!dev.is_ready());
        assert_eq!(dev.name(), "Intel E1000 Gigabit Ethernet");
        assert_eq!(dev.device_type(), DeviceType::Network);
    }

    #[test]
    fn test_constants() {
        assert_eq!(E1000_TX_RING_SIZE, 64);
        assert_eq!(E1000_RX_RING_SIZE, 128);
        assert_eq!(E1000_RX_BUFFER_SIZE, 2048);
    }

    #[test]
    fn test_descriptor_sizes() {
        assert_eq!(core::mem::size_of::<E1000TxDesc>(), 16);
        assert_eq!(core::mem::size_of::<E1000RxDesc>(), 16);
    }

    #[test]
    fn test_virt_to_phys_conversion() {
        let high_addr: u64 = 0xFFFF800000000000;
        assert_eq!(virt_to_phys(high_addr), 0);
        assert_eq!(virt_to_phys(0x12345678), 0x12345678);
    }
}
