#![allow(dead_code)]

use crate::kernel::driver::block;
use crate::kernel::fs::hvfs::arc::HvArc;
use crate::kernel::fs::hvfs::bp::*;
use crate::kernel::fs::hvfs::checksum::HvChecksum;
use crate::kernel::fs::hvfs::dva::HvDva;
use crate::kernel::fs::hvfs::metaslab::*;
use crate::kernel::fs::hvfs::vdev::*;
use crate::kernel::sync::mutex::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};

pub const HV_SPA_MAGIC: u32 = 0x48564653;
pub const HV_UBERBLOCK_COUNT: usize = 128;
pub const HV_UBERBLOCK_SECTOR: u32 = 0;
pub const HV_VDEV_LABEL_SIZE: u64 = 262144;
pub const HV_POOL_MAX_NAME: usize = 64;

pub const HV_POOL_BLOCK_SIZE: u64 = 4096;
pub const HV_POOL_METASLAB_SHIFT: u8 = 24;
pub const HV_POOL_METASLAB_SIZE: u64 = 1 << HV_POOL_METASLAB_SHIFT;
pub const HV_POOL_ASIZE_DEFAULT: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HvPoolState {
    Uninit = 0,
    Active = 1,
    Exported = 2,
    Destroyed = 3,
    Suspended = 4,
    ReadOnly = 5,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvUberblock {
    pub magic: u32,
    pub txg: u64,
    pub root_bp: HvBlockPointer,
    pub timestamp: u64,
    pub root_dataset_obj: u64,
    pub pool_guid: u64,
    pub checkpoint_txg: u64,
    pub pwm_domain_id: u16,
    pub _pad: [u8; 6],
    pub checksum: [u64; 4],
}

impl HvUberblock {
    pub const fn null() -> Self {
        Self {
            magic: 0,
            txg: 0,
            root_bp: HvBlockPointer::null(),
            timestamp: 0,
            root_dataset_obj: 0,
            pool_guid: 0,
            checkpoint_txg: 0,
            pwm_domain_id: 0,
            _pad: [0; 6],
            checksum: [0; 4],
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == HV_SPA_MAGIC
    }

    pub fn compute_checksum(&mut self) {
        self.checksum = [0; 4];
        let ck = HvChecksum::compute(HvCksumType::Fletcher4, self.as_bytes());
        self.checksum = ck.value;
    }

    pub fn verify_checksum(&self) -> bool {
        let mut copy = *self;
        let saved = copy.checksum;
        copy.checksum = [0; 4];
        let ck = HvChecksum::compute(HvCksumType::Fletcher4, copy.as_bytes());
        ck.value == saved
    }

    /// Framekernel P2.2.2: 安全地将 HvUberblock 转换为字节切片
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: HvUberblock is repr(C), layout well-defined
        unsafe {
            core::slice::from_raw_parts(self as *const Self as *const u8, core::mem::size_of::<Self>())
        }
    }

    /// Framekernel P2.2.2: 从字节切片安全地反序列化 HvUberblock
    /// SAFETY: 已验证输入长度足够；使用 read_unaligned 因为缓冲区可能不满足对齐要求
    pub fn from_bytes_unaligned(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        Some(unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const HvUberblock) })
    }
}

const _UBERBLOCK_MAX_SIZE: usize = 512;
const _ASSERT_UBERBLOCK_FITS: () =
    assert!(core::mem::size_of::<HvUberblock>() <= _UBERBLOCK_MAX_SIZE);

pub struct HvSpaConfig {
    pub name: [u8; HV_POOL_MAX_NAME],
    pub guid: u64,
    pub ashift: u8,
    pub block_size: u32,
    pub max_vdevs: u16,
    pub readonly: bool,
}

impl HvSpaConfig {
    pub fn new(name: &str) -> Self {
        let mut n = [0u8; HV_POOL_MAX_NAME];
        let b = name.as_bytes();
        let len = b.len().min(HV_POOL_MAX_NAME - 1);
        n[..len].copy_from_slice(&b[..len]);
        Self {
            name: n,
            guid: 0,
            ashift: 12,
            block_size: 4096,
            max_vdevs: 8,
            readonly: false,
        }
    }
}

pub struct HvSpa {
    pub config: Mutex<HvSpaConfig>,
    pub state: AtomicU8,
    pub uberblock: Mutex<HvUberblock>,
    pub vdevs: Mutex<Vec<HvVdev>>,
    pub metaslabs: Mutex<Vec<HvMetaslab>>,
    pub arc: HvArc,
    pub txg_current: AtomicU64,
    pub txg_syncing: AtomicBool,
    pub alloc_count: AtomicU64,
    pub free_count: AtomicU64,
    pub read_count: AtomicU64,
    pub write_count: AtomicU64,
    pub initialized: AtomicBool,
    pub disk_present: AtomicBool,
    pub formatted: AtomicBool,
    pub last_sync_time: AtomicU64,
    pub scrub_in_progress: AtomicBool,
    pub scrub_last_txg: AtomicU64,
    pub partition_start: AtomicU32,
}

// SAFETY (Framekernel P2.2.2): HvSpa 全部字段 (Mutex<T>, Atomic*, Vec) 自动 Send + Sync。

impl HvSpa {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(HvSpaConfig::new("")),
            state: AtomicU8::new(HvPoolState::Uninit as u8),
            uberblock: Mutex::new(HvUberblock::null()),
            vdevs: Mutex::new(Vec::new()),
            metaslabs: Mutex::new(Vec::new()),
            arc: HvArc::new(),
            txg_current: AtomicU64::new(0),
            txg_syncing: AtomicBool::new(false),
            alloc_count: AtomicU64::new(0),
            free_count: AtomicU64::new(0),
            read_count: AtomicU64::new(0),
            write_count: AtomicU64::new(0),
            initialized: AtomicBool::new(false),
            disk_present: AtomicBool::new(false),
            formatted: AtomicBool::new(false),
            last_sync_time: AtomicU64::new(0),
            scrub_in_progress: AtomicBool::new(false),
            scrub_last_txg: AtomicU64::new(0),
            partition_start: AtomicU32::new(0),
        }
    }

    fn generate_guid() -> u64 {
        let t = crate::arch!(timestamp());
        let mut h: u64 = 14695981039346656037;
        h ^= t;
        h = h.wrapping_mul(1099511628211);
        h ^= t.rotate_left(17);
        h = h.wrapping_mul(1099511628211);
        h | 1
    }

    /// Get the drive ID of the first vdev (or 0 if no vdevs)
    fn vdev_0_drive_id(&self) -> u8 {
        self.vdevs
            .lock()
            .first()
            .map(|v| v.config.vdev_id as u8)
            .unwrap_or(0)
    }

    fn check_disk_present(&self) -> bool {
        let drive = self.vdev_0_drive_id();
        block::hdd_is_present(drive)
    }

    fn read_sector(&self, sector: u32, buf: &mut [u8]) -> i32 {
        if buf.len() < 512 {
            return -1;
        }
        let phys = sector + self.partition_start.load(Ordering::Acquire);
        let drive = self.vdev_0_drive_id();
        block::hdd_read_sector(drive, phys as u64, buf)
    }

    fn write_sector(&self, sector: u32, buf: &[u8]) -> i32 {
        if buf.len() < 512 {
            return -1;
        }
        let phys = sector + self.partition_start.load(Ordering::Acquire);
        let drive = self.vdev_0_drive_id();
        block::hdd_write_sector(drive, phys as u64, buf)
    }

    pub fn init(&self, name: &str) {
        {
            let mut cfg = self.config.lock();
            let mut new_cfg = HvSpaConfig::new(name);
            new_cfg.guid = Self::generate_guid();
            *cfg = new_cfg;
        }
        self.arc.init(256);
        self.txg_current.store(1, Ordering::Release);
        self.state
            .store(HvPoolState::Active as u8, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
        {
            let mut ub = self.uberblock.lock();
            ub.magic = HV_SPA_MAGIC;
            ub.txg = 1;
            ub.pool_guid = self.config.lock().guid;
            ub.root_dataset_obj = 0;
        }
    }

    pub fn add_vdev(&self, config: HvVdevConfig) -> bool {
        let mut vdevs = self.vdevs.lock();
        let max_vdevs = self.config.lock().max_vdevs;
        if vdevs.len() >= max_vdevs as usize {
            return false;
        }
        let mut vdev = HvVdev::new(config);
        vdev.state = HvVdevState::Healthy;
        let vdev_id = vdev.config.vdev_id;
        let asize = vdev.config.asize;
        vdevs.push(vdev);
        drop(vdevs);
        if asize > 0 {
            let mut ms_list = self.metaslabs.lock();
            let n_ms = asize.div_ceil(HV_POOL_METASLAB_SIZE) as u32;
            for i in 0..n_ms {
                let ms_start = (i as u64) * HV_POOL_METASLAB_SIZE + HV_VDEV_LABEL_SIZE;
                let ms_size = if i < n_ms - 1 {
                    HV_POOL_METASLAB_SIZE
                } else {
                    asize - ms_start + HV_VDEV_LABEL_SIZE
                };
                let ms = HvMetaslab::new(ms_list.len() as u32, vdev_id, ms_start, ms_size);
                ms_list.push(ms);
            }
        }
        true
    }

    pub fn allocate(
        &self,
        size: u64,
        kind: HvCksumType,
        comp: HvCompType,
        txg: u64,
    ) -> Option<HvBlockPointer> {
        let rounded = size.div_ceil(HV_POOL_BLOCK_SIZE) * HV_POOL_BLOCK_SIZE;
        let mut ms_list = self.metaslabs.lock();
        let mut best_vdev_id: u16 = 0;
        let mut best_weight: u64 = 0;
        let mut best_ms_idx: Option<usize> = None;
        for (i, ms) in ms_list.iter().enumerate() {
            if !ms.is_available() {
                continue;
            }
            if ms.free_space.load(Ordering::Relaxed) < rounded {
                continue;
            }
            if ms.weight > best_weight {
                best_weight = ms.weight;
                best_vdev_id = ms.vdev_id;
                best_ms_idx = Some(i);
            }
        }
        let ms_idx = best_ms_idx?;
        let offset = ms_list[ms_idx].alloc(rounded)?;
        drop(ms_list);
        let dva = HvDva::new(best_vdev_id, offset, rounded as u32);
        let mut bp = HvBlockPointer::null();
        bp.set_dva(0, dva);
        bp.prop.cksum_type = kind;
        bp.prop.comp_type = comp;
        bp.prop.logical_size = size as u32;
        bp.prop.physical_size = rounded as u32;
        bp.set_birth(txg);
        self.alloc_count.fetch_add(1, Ordering::Relaxed);
        Some(bp)
    }

    pub fn free(&self, bp: &HvBlockPointer, _txg: u64) {
        for i in 0..HV_DVA_MAX {
            if let Some(dva) = bp.get_dva(i) {
                let mut ms_list = self.metaslabs.lock();
                for ms in ms_list.iter_mut() {
                    if ms.vdev_id == dva.vdev_id {
                        let rel = dva.offset.saturating_sub(ms.start);
                        if rel < ms.size {
                            ms.free(dva.offset, dva.asize as u64);
                            break;
                        }
                    }
                }
            }
        }
        self.free_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn read_bp(&self, bp: &HvBlockPointer, buf: &mut [u8]) -> i32 {
        for i in 0..HV_DVA_MAX {
            if let Some(dva) = bp.get_dva(i) {
                let mut vdevs = self.vdevs.lock();
                if let Some(vdev) = vdevs.iter_mut().find(|v| v.config.vdev_id == dva.vdev_id) {
                    let sector = dva.offset / 512;
                    let count = dva.asize.div_ceil(512);
                    let result = vdev.read_sectors(sector, count, buf);
                    if result == 0 {
                        self.read_count.fetch_add(1, Ordering::Relaxed);
                        return 0;
                    }
                }
            }
        }
        -1
    }

    pub fn write_bp(&self, bp: &HvBlockPointer, buf: &[u8]) -> i32 {
        for i in 0..HV_DVA_MAX {
            if let Some(dva) = bp.get_dva(i) {
                let mut vdevs = self.vdevs.lock();
                if let Some(vdev) = vdevs.iter_mut().find(|v| v.config.vdev_id == dva.vdev_id) {
                    let sector = dva.offset / 512;
                    let count = dva.asize.div_ceil(512);
                    let result = vdev.write_sectors(sector, count, buf);
                    if result != 0 {
                        return result;
                    }
                }
            }
        }
        self.write_count.fetch_add(1, Ordering::Relaxed);
        0
    }

    pub fn write_uberblock_to_disk(&self) {
        let ub = self.uberblock.lock();
        if !ub.is_valid() {
            return;
        }
        let mut copy = *ub;
        copy.compute_checksum();
        let ub_bytes = copy.as_bytes();
        let ub_sector =
            (self.txg_current.load(Ordering::Relaxed) as u32) % HV_UBERBLOCK_COUNT as u32;
        let sector = HV_UBERBLOCK_SECTOR + ub_sector;
        let mut sector_buf = [0u8; 512];
        let copy_len = ub_bytes.len().min(512);
        sector_buf[..copy_len].copy_from_slice(&ub_bytes[..copy_len]);
        let _ = self.write_sector(sector, &sector_buf);
        self.last_sync_time
            .store(crate::arch!(timestamp()), Ordering::Relaxed);
    }

    pub fn read_uberblock_from_disk(&self) -> Option<HvUberblock> {
        for i in (0..HV_UBERBLOCK_COUNT as u32).rev() {
            let sector = HV_UBERBLOCK_SECTOR + i;
            let mut sector_buf = [0u8; 512];
            if self.read_sector(sector, &mut sector_buf) != 0 {
                continue;
            }
            let ub = match HvUberblock::from_bytes_unaligned(&sector_buf) {
                Some(u) => u,
                None => continue,
            };
            if ub.is_valid() && ub.verify_checksum() {
                return Some(ub);
            }
        }
        None
    }

    pub fn sync_uberblock(&self) {
        self.write_uberblock_to_disk();
    }

    pub fn load_uberblock(&self) -> bool {
        let ub = self.uberblock.lock();
        ub.is_valid() && ub.verify_checksum()
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.alloc_count.load(Ordering::Relaxed),
            self.free_count.load(Ordering::Relaxed),
            self.read_count.load(Ordering::Relaxed),
            self.write_count.load(Ordering::Relaxed),
            self.txg_current.load(Ordering::Relaxed),
        )
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    pub fn is_disk_present(&self) -> bool {
        self.disk_present.load(Ordering::Acquire)
    }

    pub fn is_formatted(&self) -> bool {
        self.formatted.load(Ordering::Acquire)
    }

    pub fn advance_txg(&self) -> u64 {
        self.txg_current.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn current_txg(&self) -> u64 {
        self.txg_current.load(Ordering::Acquire)
    }
}
