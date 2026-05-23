//! Block Device Abstraction Layer
//!
//! Provides a unified `BlockDevice` trait implemented by all storage drivers
//! (ATA, AHCI, NVMe, virtio-blk), plus a global registry for device discovery.
//!
//! This enables HvFS and other subsystems to use any block device backend
//! without being coupled to a specific driver implementation.

use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;

// ── BlockDevice Trait ──

/// Trait implemented by all block device drivers.
///
/// Each method may fail; callers should handle errors gracefully.
/// The trait is object-safe so it can be used via `dyn BlockDevice`.
pub trait BlockDevice: Send + Sync {
    /// Read one sector (512 bytes) at the given LBA.
    /// On success, `buf[0..512]` contains the sector data.
    /// Returns negative on failure, 0 on success.
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32;

    /// Write one sector (512 bytes) at the given LBA.
    /// Returns negative on failure, 0 on success.
    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32;

    /// Check whether the device is present and usable.
    fn blk_is_present(&self) -> bool;

    /// Total number of 512-byte sectors on the device.
    fn blk_total_sectors(&self) -> u64;
}

// ── Global Registry ──

static REGISTRY: Mutex<Vec<Mutex<Box<dyn BlockDevice>>>> = Mutex::new(Vec::new());

/// Register a block device. Returns the device index.
pub fn register(dev: Box<dyn BlockDevice>) -> usize {
    let mut list = REGISTRY.lock();
    let idx = list.len();
    list.push(Mutex::new(dev));
    idx
}

/// Get mutable access to a registered block device.
/// Returns None if the index is out of range.
pub fn get(_idx: usize) -> Option<&'static Mutex<Vec<Mutex<Box<dyn BlockDevice>>>>> {
    None
}

/// Get a reference to the global registry.
pub fn registry() -> &'static Mutex<Vec<Mutex<Box<dyn BlockDevice>>>> {
    &REGISTRY
}

/// Number of registered block devices.
pub fn count() -> usize {
    REGISTRY.lock().len()
}

// ── Multi-sector helper ──

/// Read multiple consecutive sectors from a given block device.
/// Panics if buf is too small.
pub fn read_sectors(dev: &mut dyn BlockDevice, start: u64, count: u32, buf: &mut [u8]) -> i32 {
    let need = (count as u64) * 512;
    if (buf.len() as u64) < need {
        return -1;
    }
    let mut offset = 0usize;
    for i in 0..count {
        if dev.blk_read(start + i as u64, &mut buf[offset..offset + 512]) < 0 {
            return -1;
        }
        offset += 512;
    }
    0
}

/// Write multiple consecutive sectors to a given block device.
/// Panics if buf is too small.
pub fn write_sectors(dev: &mut dyn BlockDevice, start: u64, count: u32, buf: &[u8]) -> i32 {
    let need = (count as u64) * 512;
    if (buf.len() as u64) < need {
        return -1;
    }
    let mut offset = 0usize;
    for i in 0..count {
        if dev.blk_write(start + i as u64, &buf[offset..offset + 512]) < 0 {
            return -1;
        }
        offset += 512;
    }
    0
}

// ── HvFS bridge: drop-in replacement for ata_* C FFI ──
//
// These functions provide the same API as the old C FFI calls
// (ata_read_sector, ata_write_sector, ata_disk_present) but
// route through the BlockDevice registry.
//
// Drive numbers map 1:1 to registry indices.

/// Read one sector from drive `drive` at LBA `sector`.
/// Returns 0 on success, -1 on failure.
pub fn hdd_read_sector(drive: u8, sector: u64, buf: &mut [u8]) -> i32 {
    let reg = REGISTRY.lock();
    if buf.len() < 512 || (drive as usize) >= reg.len() {
        return -1;
    }
    let mut dev = reg[drive as usize].lock();
    dev.blk_read(sector, buf)
}

/// Write one sector to drive `drive` at LBA `sector`.
/// Returns 0 on success, -1 on failure.
pub fn hdd_write_sector(drive: u8, sector: u64, buf: &[u8]) -> i32 {
    let reg = REGISTRY.lock();
    if buf.len() < 512 || (drive as usize) >= reg.len() {
        return -1;
    }
    let mut dev = reg[drive as usize].lock();
    dev.blk_write(sector, buf)
}

/// Check if drive `drive` has a valid disk present.
pub fn hdd_is_present(drive: u8) -> bool {
    let reg = REGISTRY.lock();
    if (drive as usize) >= reg.len() {
        return false;
    }
    let dev = reg[drive as usize].lock();
    dev.blk_is_present()
}

/// Get total sectors for drive `drive`. Returns 0 if not present.
pub fn hdd_total_sectors(drive: u8) -> u64 {
    let reg = REGISTRY.lock();
    if (drive as usize) >= reg.len() {
        return 0;
    }
    let dev = reg[drive as usize].lock();
    dev.blk_total_sectors()
}