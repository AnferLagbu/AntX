use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use crate::kernel::sync::mutex::Mutex;
use crate::kernel::fs::zvfs::bp::*;
use crate::kernel::fs::zvfs::dva::ZvDva;
use crate::kernel::fs::zvfs::vdev::*;
use crate::kernel::fs::zvfs::metaslab::*;
use crate::kernel::fs::zvfs::arc::ZvArc;
use crate::kernel::fs::zvfs::checksum::ZvChecksum;

pub const ZV_SPA_MAGIC: u32 = 0x5A564653;
pub const ZV_SPA_VERSION: u32 = 1;
pub const ZV_UBERBLOCK_COUNT: usize = 128;
pub const ZV_UBERBLOCK_SECTOR: u32 = 0;
pub const ZV_VDEV_LABEL_SIZE: u64 = 262144;
pub const ZV_POOL_MAX_NAME: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvPoolState {
    Uninit = 0,
    Active = 1,
    Exported = 2,
    Destroyed = 3,
    Suspended = 4,
    ReadOnly = 5,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ZvUberblock {
    pub magic: u32,
    pub version: u32,
    pub txg: u64,
    pub root_bp: ZvBlockPointer,
    pub timestamp: u64,
    pub root_dataset_obj: u64,
    pub pool_guid: u64,
    pub checkpoint_txg: u64,
    pub pwid_domain_id: u16,
    pub _pad: [u8; 6],
    pub checksum: [u64; 4],
}

impl ZvUberblock {
    pub const fn null() -> Self {
        Self {
            magic: 0, version: 0, txg: 0,
            root_bp: ZvBlockPointer::null(),
            timestamp: 0, root_dataset_obj: 0,
            pool_guid: 0, checkpoint_txg: 0,
            pwid_domain_id: 0, _pad: [0; 6],
            checksum: [0; 4],
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == ZV_SPA_MAGIC && self.version == ZV_SPA_VERSION
    }

    pub fn compute_checksum(&mut self) {
        let saved = self.checksum;
        self.checksum = [0; 4];
        let bytes = unsafe {
            core::slice::from_raw_parts(self as *const Self as *const u8, core::mem::size_of::<Self>())
        };
        let ck = ZvChecksum::compute(ZvCksumType::Fletcher4, bytes);
        self.checksum = ck.value;
        let _ = saved;
    }

    pub fn verify_checksum(&self) -> bool {
        let mut copy = *self;
        let saved = copy.checksum;
        copy.checksum = [0; 4];
        let bytes = unsafe {
            core::slice::from_raw_parts(&copy as *const Self as *const u8, core::mem::size_of::<Self>())
        };
        let ck = ZvChecksum::compute(ZvCksumType::Fletcher4, bytes);
        ck.value == saved
    }
}

pub struct ZvSpaConfig {
    pub name: [u8; ZV_POOL_MAX_NAME],
    pub guid: u64,
    pub ashift: u8,
    pub block_size: u32,
    pub max_vdevs: u16,
    pub readonly: bool,
}

impl ZvSpaConfig {
    pub fn new(name: &str) -> Self {
        let mut n = [0u8; ZV_POOL_MAX_NAME];
        let b = name.as_bytes();
        let len = b.len().min(ZV_POOL_MAX_NAME - 1);
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

    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(ZV_POOL_MAX_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

pub struct ZvSpa {
    pub config: Mutex<ZvSpaConfig>,
    pub state: AtomicU8,
    pub uberblock: Mutex<ZvUberblock>,
    pub vdevs: Mutex<Vec<ZvVdev>>,
    pub metaslabs: Mutex<Vec<ZvMetaslab>>,
    pub arc: ZvArc,
    pub txg_current: AtomicU64,
    pub txg_syncing: AtomicBool,
    pub alloc_count: AtomicU64,
    pub free_count: AtomicU64,
    pub read_count: AtomicU64,
    pub write_count: AtomicU64,
    pub initialized: AtomicBool,
    pub last_sync_time: AtomicU64,
    pub scrub_in_progress: AtomicBool,
    pub scrub_last_txg: AtomicU64,
}

unsafe impl Send for ZvSpa {}
unsafe impl Sync for ZvSpa {}

impl ZvSpa {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(ZvSpaConfig::new("")),
            state: AtomicU8::new(ZvPoolState::Uninit as u8),
            uberblock: Mutex::new(ZvUberblock::null()),
            vdevs: Mutex::new(Vec::new()),
            metaslabs: Mutex::new(Vec::new()),
            arc: ZvArc::new(),
            txg_current: AtomicU64::new(0),
            txg_syncing: AtomicBool::new(false),
            alloc_count: AtomicU64::new(0),
            free_count: AtomicU64::new(0),
            read_count: AtomicU64::new(0),
            write_count: AtomicU64::new(0),
            initialized: AtomicBool::new(false),
            last_sync_time: AtomicU64::new(0),
            scrub_in_progress: AtomicBool::new(false),
            scrub_last_txg: AtomicU64::new(0),
        }
    }

    pub fn init(&self, name: &str) {
        {
            let mut cfg = self.config.lock();
            let mut new_cfg = ZvSpaConfig::new(name);
            new_cfg.guid = Self::generate_guid();
            *cfg = new_cfg;
        }
        self.arc.init(256);
        self.txg_current.store(1, Ordering::Release);
        self.state.store(ZvPoolState::Active as u8, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
        {
            let mut ub = self.uberblock.lock();
            ub.magic = ZV_SPA_MAGIC;
            ub.version = ZV_SPA_VERSION;
            ub.txg = 1;
            ub.pool_guid = self.config.lock().guid;
            ub.root_dataset_obj = 0;
        }
    }

    fn generate_guid() -> u64 {
        extern "C" { fn timer_get_ticks() -> u64; }
        let t = unsafe { timer_get_ticks() };
        let mut h: u64 = 14695981039346656037;
        h ^= t;
        h = h.wrapping_mul(1099511628211);
        h ^= t.rotate_left(17);
        h = h.wrapping_mul(1099511628211);
        h | 1
    }

    pub fn add_vdev(&self, config: ZvVdevConfig) -> bool {
        let mut vdevs = self.vdevs.lock();
        let max_vdevs = self.config.lock().max_vdevs;
        if vdevs.len() >= max_vdevs as usize { return false; }
        let mut vdev = ZvVdev::new(config);
        vdev.state = ZvVdevState::Healthy;
        vdevs.push(vdev);
        true
    }

    pub fn remove_vdev(&self, vdev_id: u16) -> bool {
        let mut vdevs = self.vdevs.lock();
        let idx = vdevs.iter().position(|v| v.config.vdev_id == vdev_id);
        match idx {
            Some(i) => { vdevs.remove(i); true }
            None => false
        }
    }

    pub fn allocate(&self, size: u64, kind: ZvCksumType, comp: ZvCompType, txg: u64) -> Option<ZvBlockPointer> {
        let vdev_id = {
            let vdevs = self.vdevs.lock();
            let mut best_vdev_id: u16 = 0;
            let mut best_weight: u64 = 0;
            for vdev in vdevs.iter() {
                if vdev.is_available() {
                    if vdev.config.asize > best_weight {
                        best_weight = vdev.config.asize;
                        best_vdev_id = vdev.config.vdev_id;
                    }
                }
            }
            best_vdev_id
        };
        let asize = size;
        let dva = ZvDva::new(vdev_id, 0, asize as u32);
        let mut bp = ZvBlockPointer::null();
        bp.set_dva(0, dva);
        bp.prop.cksum_type = kind;
        bp.prop.comp_type = comp;
        bp.prop.logical_size = size as u32;
        bp.prop.physical_size = asize as u32;
        bp.set_birth(txg);
        self.alloc_count.fetch_add(1, Ordering::Relaxed);
        Some(bp)
    }

    pub fn free(&self, bp: &ZvBlockPointer, txg: u64) {
        let _ = txg;
        for i in 0..ZV_DVA_MAX {
            if let Some(dva) = bp.get_dva(i) {
                let mut vdevs = self.vdevs.lock();
                if let Some(vdev) = vdevs.iter_mut().find(|v| v.config.vdev_id == dva.vdev_id) {
                    vdev.free(dva);
                }
            }
        }
        self.free_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn read_bp(&self, bp: &ZvBlockPointer, buf: &mut [u8]) -> i32 {
        for i in 0..ZV_DVA_MAX {
            if let Some(dva) = bp.get_dva(i) {
                let vdevs = self.vdevs.lock();
                if let Some(vdev) = vdevs.iter().find(|v| v.config.vdev_id == dva.vdev_id) {
                    let result = vdev.read_block(dva, buf);
                    if result == 0 {
                        self.read_count.fetch_add(1, Ordering::Relaxed);
                        if bp.prop.cksum_type != ZvCksumType::Off {
                            let ck = ZvChecksum::compute(bp.prop.cksum_type, &buf[..bp.prop.physical_size as usize]);
                            if ck.value != bp.checksum {
                                return -3;
                            }
                        }
                        return 0;
                    }
                }
            }
        }
        -1
    }

    pub fn write_bp(&self, bp: &ZvBlockPointer, buf: &[u8]) -> i32 {
        for i in 0..ZV_DVA_MAX {
            if let Some(dva) = bp.get_dva(i) {
                let vdevs = self.vdevs.lock();
                if let Some(vdev) = vdevs.iter().find(|v| v.config.vdev_id == dva.vdev_id) {
                    let result = vdev.write_block(dva, buf);
                    if result != 0 { return result; }
                }
            }
        }
        self.write_count.fetch_add(1, Ordering::Relaxed);
        0
    }

    pub fn sync_uberblock(&self) {
        let mut ub = self.uberblock.lock();
        ub.compute_checksum();
        self.last_sync_time.store(
            unsafe { extern "C" { fn timer_get_ticks() -> u64; } timer_get_ticks() },
            Ordering::Relaxed
        );
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

    pub fn advance_txg(&self) -> u64 {
        self.txg_current.fetch_add(1, Ordering::AcqRel)
    }

    pub fn current_txg(&self) -> u64 {
        self.txg_current.load(Ordering::Acquire)
    }
}
