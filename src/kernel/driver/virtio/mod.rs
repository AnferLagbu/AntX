//! VirtIO MMIO Transport Layer
//!
//! Implements VirtIO 1.0 MMIO transport for device discovery and setup.
//! Used on QEMU virt platform (aarch64 and x86_64 with -M virt).
//!
//! MMIO Register Layout (each device occupies a 0x200-byte region):
//!
//! | Offset | Name            | Width | Description                       |
//! |--------|-----------------|-------|-----------------------------------|
//! | 0x000  | MagicValue      | R     | 0x74726976 ("virt")               |
//! | 0x004  | Version         | R     | 0x2 for VirtIO 1.0               |
//! | 0x008  | DeviceID        | R     | 2=blk, 1=net, etc.               |
//! | 0x00c  | VendorID        | R     | 0x554d4551 ("QEUM")              |
//! | 0x010  | DeviceFeatures  | R     | Bits 0..31 of device features     |
//! | 0x014  | DeviceFeaturesSel| W    | Selects which 32-bit feature word |
//! | 0x020  | DriverFeatures  | W     | Bits 0..31 of driver features    |
//! | 0x024  | DriverFeaturesSel| W    | Selects which 32-bit feature word |
//! | 0x030  | QueueSel        | W     | Select virtqueue                  |
//! | 0x034  | QueueNumMax     | R     | Max size of selected queue        |
//! | 0x038  | QueueNum        | W     | Set size of selected queue        |
//! | 0x040  | QueueReady      | RW    | Mark queue as ready               |
//! | 0x050  | QueueNotify     | W     | Notify device of new descriptors  |
//! | 0x060  | InterruptStatus | R     | Interrupt reason                  |
//! | 0x064  | InterruptACK    | W     | Acknowledge interrupt             |
//! | 0x070  | Status          | RW    | Device status                     |
//! | 0x080  | QueueDescLow    | W     | Descriptor table phys addr [31:0] |
//! | 0x084  | QueueDescHigh   | W     | Descriptor table phys addr [63:32]|
//! | 0x090  | QueueDriverLow  | W     | Available ring phys addr [31:0]   |
//! | 0x094  | QueueDriverHigh | W     | Available ring phys addr [63:32]  |
//! | 0x0a0  | QueueDeviceLow  | W     | Used ring phys addr [31:0]        |
//! | 0x0a4  | QueueDeviceHigh | W     | Used ring phys addr [63:32]       |
//! | 0x0fc  | ConfigGeneration| R     | Config change counter             |
//! | 0x100+ | Config          | RW    | Device-specific configuration     |
//!
//! QEMU virt aarch64 places virtio-mmio devices starting at 0x0a000000,
//! with each device at a 0x200-byte stride.

pub mod queue;
pub mod blk;
pub mod net;

use crate::kernel::mm::KERNEL_BASE;
use crate::klog_info;
use crate::klog_warn;

// ── MMIO register offsets ──

const MAGIC_VALUE:        usize = 0x000;
const VERSION:            usize = 0x004;
const DEVICE_ID:          usize = 0x008;
const VENDOR_ID:          usize = 0x00c;
const DEVICE_FEATURES:    usize = 0x010;
const DEVICE_FEATURES_SEL: usize = 0x014;
const DRIVER_FEATURES:    usize = 0x020;
const DRIVER_FEATURES_SEL: usize = 0x024;
const QUEUE_SEL:          usize = 0x030;
const QUEUE_NUM_MAX:      usize = 0x034;
const QUEUE_NUM:          usize = 0x038;
const QUEUE_READY:        usize = 0x044;
const QUEUE_PFN:          usize = 0x040; // Legacy: QueuePFN (page number)
const QUEUE_NOTIFY:       usize = 0x050;
const INTERRUPT_STATUS:   usize = 0x060;
const INTERRUPT_ACK:      usize = 0x064;
const STATUS:             usize = 0x070;
const QUEUE_DESC_LOW:     usize = 0x080;
const QUEUE_DESC_HIGH:    usize = 0x084;
const QUEUE_DRIVER_LOW:   usize = 0x090;
const QUEUE_DRIVER_HIGH:  usize = 0x094;
const QUEUE_DEVICE_LOW:   usize = 0x0a0;
const QUEUE_DEVICE_HIGH:  usize = 0x0a4;

// ── Register magic ──

const VIRTIO_MAGIC: u32 = 0x74726976;

// ── Device status bits ──

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER:      u32 = 2;
const STATUS_DRIVER_OK:   u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_NEEDS_RESET: u32 = 0x40;
const STATUS_FAILED:      u32 = 0x80;

// ── Device IDs ──

pub const VIRTIO_ID_BLOCK:   u32 = 2;
pub const VIRTIO_ID_NET:     u32 = 1;
pub const VIRTIO_ID_GPU:     u32 = 16;

// ── MMIO region ──

/// Base address of virtio-mmio region on QEMU virt (aarch64).
/// On x86_64 with QEMU microvm, this may differ.
pub const VIRTIO_MMIO_BASE: u64 = 0x0a00_0000;
/// Stride between virtio-mmio devices (0x200 bytes).
pub const VIRTIO_MMIO_STRIDE: u64 = 0x200;
/// Maximum number of virtio-mmio devices to probe.
pub const VIRTIO_MMIO_MAX_DEVICES: u32 = 32;

// ── Feature bits ──

/// General feature: VIRTIO_F_VERSION_1 (must be acknowledged for spec compliance)
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;

/// A discovered virtio device via MMIO transport.
#[derive(Clone, Copy)]
pub struct VirtioMmioDevice {
    /// Base MMIO address of this device's register space.
    pub mmio_base: u64,
    /// Device ID (e.g. 2 for block device).
    pub device_id: u32,
    /// Number of virtqueues the device supports.
    /// For block devices, typically 1.
    pub queue_count: u32,
}

impl VirtioMmioDevice {
    /// Read a 32-bit register from the device's MMIO space.
    #[inline(always)]
    unsafe fn read32(&self, offset: usize) -> u32 {
        let addr = (self.mmio_base + KERNEL_BASE + offset as u64) as *const u32;
        core::ptr::read_volatile(addr)
    }

    /// Write a 32-bit register to the device's MMIO space.
    #[inline(always)]
    unsafe fn write32(&self, offset: usize, val: u32) {
        let addr = (self.mmio_base + KERNEL_BASE + offset as u64) as *mut u32;
        core::ptr::write_volatile(addr, val);
    }

    /// Read a 64-bit value split across Low/High registers.
    unsafe fn read64(&self, low_off: usize, high_off: usize) -> u64 {
        let lo = self.read32(low_off) as u64;
        let hi = self.read32(high_off) as u64;
        lo | (hi << 32)
    }

    /// Write a 64-bit value split across Low/High registers.
    unsafe fn write64(&self, low_off: usize, high_off: usize, val: u64) {
        self.write32(low_off, (val & 0xFFFF_FFFF) as u32);
        self.write32(high_off, (val >> 32) as u32);
    }

    /// Probe whether the device at the given MMIO base is a valid virtio device.
    pub fn probe(mmio_base: u64) -> Option<Self> {
        // Create a temporary device for register access
        let dev = VirtioMmioDevice { mmio_base, device_id: 0, queue_count: 0 };

        let magic = unsafe { dev.read32(MAGIC_VALUE) };
        if magic != VIRTIO_MAGIC {
            return None;
        }

        let version = unsafe { dev.read32(VERSION) };
        // QEMU virt uses VirtIO 1.0 (version 2) or transitional (version 1)
        if version != 1 && version != 2 {
            return None;
        }

        let device_id = unsafe { dev.read32(DEVICE_ID) };
        if device_id == 0 {
            return None; // No device attached to this slot
        }

        let vendor_id = unsafe { dev.read32(VENDOR_ID) };

        klog_info!(Driver, "virtio: found device id={} vendor={:#x} at {:#x}", device_id, vendor_id, mmio_base);

        let queue_count = if device_id == VIRTIO_ID_BLOCK { 1 } else { 2 };

        Some(VirtioMmioDevice { mmio_base, device_id, queue_count })
    }

    /// Initialize the device:
    /// 1. Reset
    /// 2. Acknowledge
    /// 3. Negotiate features
    /// 4. Set DRIVER_OK
    pub fn init(&self) -> Result<(), ()> {
        unsafe {
            // Step 1: Reset
            self.write32(STATUS, 0);
            // Ensure device observes reset
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

            // Step 2: ACKNOWLEDGE
            self.write32(STATUS, STATUS_ACKNOWLEDGE);

            // Step 3: DRIVER
            self.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

            // Step 4: Feature negotiation
            // Read device features
            self.write32(DEVICE_FEATURES_SEL, 0);
            let _dev_features_lo = self.read32(DEVICE_FEATURES);
            self.write32(DEVICE_FEATURES_SEL, 1);
            let _dev_features_hi = self.read32(DEVICE_FEATURES);

            // Acknowledge VIRTIO_F_VERSION_1
            self.write32(DRIVER_FEATURES_SEL, 1);
            self.write32(DRIVER_FEATURES, (VIRTIO_F_VERSION_1 >> 32) as u32);
            self.write32(DRIVER_FEATURES_SEL, 0);
            self.write32(DRIVER_FEATURES, 0);

            // Step 5: FEATURES_OK
            self.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);

            // Verify FEATURES_OK was accepted
            let status = self.read32(STATUS);
            if status & STATUS_FEATURES_OK == 0 {
                klog_warn!(Driver, "virtio: FEATURES_OK rejected at {:#x}", self.mmio_base);
                return Err(());
            }

            // Step 6: DRIVER_OK (final step - device is live)
            // Moved to set_driver_ok() — caller must call it after queue setup.
            Ok(())
        }
    }

    /// Set DRIVER_OK (device goes live). Must be called after all virtqueues are configured.
    pub fn set_driver_ok(&self) {
        unsafe {
            self.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK);
        }
    }

    /// Configure a virtqueue on this device.
    pub fn setup_vq(&self, vq_index: u16, vq: &queue::VirtQueue) -> Result<(), ()> {
        unsafe {
            // Select the virtqueue
            self.write32(QUEUE_SEL, vq_index as u32);

            // Check max queue size
            let max_size = self.read32(QUEUE_NUM_MAX);
            if vq.queue_size as u32 > max_size {
                klog_warn!(Driver, "virtio: queue size {} exceeds max {}", vq.queue_size, max_size);
            }
            klog_info!(Driver, "virtio: vq{} max_size={}", vq_index, max_size);

            // Set queue size
            self.write32(QUEUE_NUM, vq.queue_size as u32);
            klog_info!(Driver, "virtio: vq{} QUEUE_NUM set, writing desc={:#x}", vq_index, vq.desc_paddr());

            // Set physical addresses of the three ring parts
            self.write64(QUEUE_DESC_LOW, QUEUE_DESC_HIGH, vq.desc_paddr());
            klog_info!(Driver, "virtio: vq{} desc written", vq_index);
            self.write64(QUEUE_DRIVER_LOW, QUEUE_DRIVER_HIGH, vq.avail_paddr());
            klog_info!(Driver, "virtio: vq{} avail written", vq_index);
            self.write64(QUEUE_DEVICE_LOW, QUEUE_DEVICE_HIGH, vq.used_paddr());
            klog_info!(Driver, "virtio: vq{} used written", vq_index);

            // Mark queue as ready
            self.write32(QUEUE_READY, 1);
            klog_info!(Driver, "virtio: vq{} ready", vq_index);

            Ok(())
        }
    }

    /// Configure a virtqueue using legacy QueuePFN interface (VirtIO 0.9.5).
    /// Used when VIRTIO_F_VERSION_1 is NOT negotiated (transitional/legacy devices).
    pub fn setup_vq_legacy(&self, vq_index: u16, vq: &queue::VirtQueue) -> Result<(), ()> {
        unsafe {
            self.write32(QUEUE_SEL, vq_index as u32);

            let max_size = self.read32(QUEUE_NUM_MAX);
            if vq.queue_size as u32 > max_size {
                klog_warn!(Driver, "virtio: legacy queue size {} exceeds max {}", vq.queue_size, max_size);
            }

            self.write32(QUEUE_NUM, vq.queue_size as u32);

            // Legacy: write guest-physical page number of the queue
            // The queue (desc + avail + used) is laid out contiguously within one page
            let pfn = (vq.desc_paddr() >> 12) as u32;
            self.write32(QUEUE_PFN, pfn);

            klog_info!(Driver, "virtio: legacy vq{} pfn={:#x} (desc={:#x})", vq_index, pfn, vq.desc_paddr());
            Ok(())
        }
    }

    /// Notify the device that new descriptors are available on a virtqueue.
    pub fn notify(&self, vq_index: u16) {
        unsafe {
            self.write32(QUEUE_NOTIFY, vq_index as u32);
        }
    }

    /// Read from device-specific config space (offset relative to 0x100).
    pub fn read_config32(&self, offset: usize) -> u32 {
        unsafe { self.read32(0x100 + offset) }
    }

    pub fn read_config64(&self, offset: usize) -> u64 {
        unsafe { self.read64(0x100 + offset, 0x100 + offset + 4) }
    }
}

/// Scan the virtio-mmio region for devices.
/// Returns a Vec of discovered devices.
pub fn probe_all() -> alloc::vec::Vec<VirtioMmioDevice> {
    let mut devices = alloc::vec::Vec::new();

    // Check if the virtio-mmio region is accessible before probing.
    // On platforms without virtio-mmio (e.g. QEMU x86_64 with pc machine type),
    // the first read will return 0xFFFFFFFF or cause a fault.
    for i in 0..VIRTIO_MMIO_MAX_DEVICES {
        let base = VIRTIO_MMIO_BASE + (i as u64) * VIRTIO_MMIO_STRIDE;
        if let Some(dev) = VirtioMmioDevice::probe(base) {
            devices.push(dev);
        }
    }

    devices
}