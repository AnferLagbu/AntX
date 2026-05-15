use crate::kernel::fs::zvfs::bp::{ZvDva, ZV_DVA_MAX};
use alloc::vec::Vec;
use alloc::string::String;

pub const ZV_VDEV_MAX: usize = 8;
pub const ZV_VDEV_TYPE_DISK: u8 = 0;
pub const ZV_VDEV_TYPE_MIRROR: u8 = 1;
pub const ZV_VDEV_TYPE_RAIDZ1: u8 = 2;
pub const ZV_VDEV_TYPE_RAIDZ2: u8 = 3;
pub const ZV_VDEV_TYPE_RAIDZ3: u8 = 4;
pub const ZV_VDEV_TYPE_SPARE: u8 = 5;
pub const ZV_VDEV_TYPE_LOG: u8 = 6;
pub const ZV_VDEV_TYPE_L2CACHE: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvVdevState {
    Unknown = 0,
    Closed = 1,
    Offline = 2,
    Removed = 3,
    CantOpen = 4,
    Faulted = 5,
    Degraded = 6,
    Healthy = 7,
}

impl ZvVdevState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Closed,
            2 => Self::Offline,
            3 => Self::Removed,
            4 => Self::CantOpen,
            5 => Self::Faulted,
            6 => Self::Degraded,
            7 => Self::Healthy,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ZvVdevStats {
    pub reads: u64,
    pub writes: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_errors: u64,
    pub write_errors: u64,
    pub checksum_errors: u64,
}

impl ZvVdevStats {
    pub const fn zero() -> Self {
        Self { reads: 0, writes: 0, read_bytes: 0, write_bytes: 0, read_errors: 0, write_errors: 0, checksum_errors: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct ZvVdevConfig {
    pub vdev_id: u16,
    pub vdev_type: u8,
    pub guid: u64,
    pub path: [u8; 64],
    pub ashift: u8,
    pub asize: u64,
    pub nparity: u8,
    pub children: u16,
    pub is_log: bool,
}

impl ZvVdevConfig {
    pub fn new_disk(vdev_id: u16, path: &str, ashift: u8) -> Self {
        let mut p = [0u8; 64];
        let b = path.as_bytes();
        let len = b.len().min(63);
        p[..len].copy_from_slice(&b[..len]);
        Self {
            vdev_id,
            vdev_type: ZV_VDEV_TYPE_DISK,
            guid: 0,
            path: p,
            ashift,
            asize: 0,
            nparity: 0,
            children: 1,
            is_log: false,
        }
    }

    pub fn new_mirror(vdev_id: u16, children: u16) -> Self {
        Self {
            vdev_id,
            vdev_type: ZV_VDEV_TYPE_MIRROR,
            guid: 0,
            path: [0; 64],
            ashift: 9,
            asize: 0,
            nparity: 0,
            children,
            is_log: false,
        }
    }

    pub fn new_raidz(vdev_id: u16, nparity: u8, children: u16) -> Self {
        let vt = match nparity {
            2 => ZV_VDEV_TYPE_RAIDZ2,
            3 => ZV_VDEV_TYPE_RAIDZ3,
            _ => ZV_VDEV_TYPE_RAIDZ1,
        };
        Self {
            vdev_id,
            vdev_type: vt,
            guid: 0,
            path: [0; 64],
            ashift: 9,
            asize: 0,
            nparity,
            children,
            is_log: false,
        }
    }
}

pub struct ZvVdev {
    pub config: ZvVdevConfig,
    pub state: ZvVdevState,
    pub stats: ZvVdevStats,
    pub parent_id: Option<u16>,
    pub child_ids: Vec<u16>,
    pub ms_count: u32,
    pub ms_active: u32,
    pub initializing: bool,
    pub trim_supported: bool,
}

unsafe impl Send for ZvVdev {}
unsafe impl Sync for ZvVdev {}

impl ZvVdev {
    pub fn new(config: ZvVdevConfig) -> Self {
        Self {
            config,
            state: ZvVdevState::Closed,
            stats: ZvVdevStats::zero(),
            parent_id: None,
            child_ids: Vec::new(),
            ms_count: 0,
            ms_active: 0,
            initializing: false,
            trim_supported: false,
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.state == ZvVdevState::Healthy
    }

    pub fn is_degraded(&self) -> bool {
        self.state == ZvVdevState::Degraded
    }

    pub fn is_available(&self) -> bool {
        matches!(self.state, ZvVdevState::Healthy | ZvVdevState::Degraded)
    }

    pub fn allocate(&mut self, size: u64) -> Option<ZvDva> {
        if !self.is_available() { return None; }
        let offset = 0u64;
        self.stats.writes += 1;
        self.stats.write_bytes += size;
        Some(ZvDva::new(self.config.vdev_id, offset, size as u32))
    }

    pub fn free(&mut self, _dva: &ZvDva) {
        self.stats.reads += 1;
    }

    pub fn read_block(&self, dva: &ZvDva, buf: &mut [u8]) -> i32 {
        extern "C" {
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }
        if buf.len() < dva.asize as usize { return -1; }
        let total_sectors = (dva.asize + 511) / 512;
        let mut sector = (dva.offset / 512) as u32;
        let mut offset = 0;
        for _ in 0..total_sectors {
            let result = unsafe {
                ata_read_sector(self.config.vdev_id as u8, sector, buf[offset..].as_mut_ptr())
            };
            if result < 0 { return -1; }
            sector += 1;
            offset += 512;
            if offset >= buf.len() { break; }
        }
        0
    }

    pub fn write_block(&self, dva: &ZvDva, buf: &[u8]) -> i32 {
        extern "C" {
            fn ata_write_sector(disk: u8, sector: u32, buf: *const u8) -> i32;
        }
        let total_sectors = (dva.asize + 511) / 512;
        let mut sector = (dva.offset / 512) as u32;
        let mut offset = 0;
        for _ in 0..total_sectors {
            let result = unsafe {
                ata_write_sector(self.config.vdev_id as u8, sector, buf[offset..].as_ptr())
            };
            if result < 0 { return -1; }
            sector += 1;
            offset += 512;
            if offset >= buf.len() { break; }
        }
        0
    }

    pub fn get_path_str(&self) -> &str {
        let end = self.config.path.iter().position(|&b| b == 0).unwrap_or(64);
        core::str::from_utf8(&self.config.path[..end]).unwrap_or("")
    }
}
