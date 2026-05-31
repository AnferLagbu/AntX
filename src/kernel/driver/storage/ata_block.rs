//! ATA BlockDevice 适配层
//!
//! 将 ATA C FFI (`ata_read_sector`/`ata_write_sector`/`ata_disk_present`)
//! 包装为 `BlockDevice` trait 实现，使 HvFS 可以通过统一的 BlockDevice
//! 注册表访问 ATA 磁盘，与 virtio-blk/AHCI/NVMe 统一接口。
//!
//! 仅用于 x86_64; aarch64 上此模块会被编译排除。

use crate::kernel::driver::block::BlockDevice;

/// ATA 磁盘的 BlockDevice 适配器。
///
/// 内部通过 C FFI 调用 ATA 控制器读写扇区。
/// 启动时通过二分探测确定磁盘容量。
pub struct AtaBlockDevice {
    /// ATA 驱动器编号 (0=Primary Master, 1=Primary Slave, etc.)
    drive: u8,
    /// 缓存的磁盘容量 (512字节扇区数)，启动时二分探测获得
    total_sectors_cache: u64,
}

impl AtaBlockDevice {
    /// 创建 ATA BlockDevice 适配器并探测磁盘容量。
    ///
    /// 返回 None 如果指定的驱动器上没有磁盘。
    pub fn new(drive: u8) -> Option<Self> {
        // FFI declarations
        extern "C" {
            fn ata_disk_present(disk: u8) -> i32;
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }

        // Check if disk is present
        let present = unsafe { ata_disk_present(drive) };
        if present == 0 {
            return None;
        }

        // Binary search for total sectors (same approach as HvFS probe_disk_size)
        let mut lo: u32 = 0;
        let mut hi: u32 = 0xFFFF;
        let mut buf = [0u8; 512];
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if unsafe { ata_read_sector(drive, mid, buf.as_mut_ptr()) } >= 0 {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let detected = if lo > 0 { (lo as u64) * 512 } else { 0 };
        if detected == 0 {
            return None;
        }

        Some(AtaBlockDevice {
            drive,
            total_sectors_cache: detected / 512,
        })
    }
}

// SAFETY: AtaBlockDevice wraps C FFI calls; ATA controller access is
// mediated by the global ATA_DEVICE Mutex in ata.rs. BlockDevice trait
// methods use internal locks or atomic operations for cross-CPU safety.
unsafe impl Send for AtaBlockDevice {}
unsafe impl Sync for AtaBlockDevice {}

impl BlockDevice for AtaBlockDevice {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
        if buf.len() < 512 || sector > u32::MAX as u64 {
            return -1;
        }
        extern "C" {
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }
        unsafe { ata_read_sector(self.drive, sector as u32, buf.as_mut_ptr()) }
    }

    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
        if buf.len() < 512 || sector > u32::MAX as u64 {
            return -1;
        }
        extern "C" {
            fn ata_write_sector(disk: u8, sector: u32, buf: *const u8) -> i32;
        }
        unsafe { ata_write_sector(self.drive, sector as u32, buf.as_ptr()) }
    }

    fn blk_is_present(&self) -> bool {
        extern "C" {
            fn ata_disk_present(disk: u8) -> i32;
        }
        unsafe { ata_disk_present(self.drive) != 0 }
    }

    fn blk_total_sectors(&self) -> u64 {
        self.total_sectors_cache
    }
}
