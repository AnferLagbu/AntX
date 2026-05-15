use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use crate::kernel::sync::mutex::Mutex;
use crate::kernel::fs::zvfs::bp::ZvBlockPointer;
use crate::kernel::fs::zvfs::dmu::{ZvObjSet, ZvObjType, ZV_DMU_OBJ_ROOT};
use crate::kernel::fs::zvfs::zap::ZvZap;

pub const ZV_DS_MAX_NAME: usize = 128;
pub const ZV_DS_MAX_DATASETS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvDsState {
    Uninit = 0,
    Creating = 1,
    Active = 2,
    Destroying = 3,
    Suspended = 4,
    ReadOnly = 5,
}

#[derive(Debug, Clone)]
pub struct ZvDsProps {
    pub record_size: u32,
    pub compression: u8,
    pub checksum: u8,
    pub atime: bool,
    pub sync: u8,
    pub sensitivity: u8,
    pub owner_pwid: u64,
    pub quota: u64,
    pub reservation: u64,
    pub ref_quota: u64,
    pub ref_reservation: u64,
}

impl ZvDsProps {
    pub fn default() -> Self {
        Self {
            record_size: 131072,
            compression: 0,
            checksum: 2,
            atime: true,
            sync: 0,
            sensitivity: 0,
            owner_pwid: 0,
            quota: 0,
            reservation: 0,
            ref_quota: 0,
            ref_reservation: 0,
        }
    }
}

pub struct ZvDataset {
    pub ds_id: u64,
    pub name: [u8; ZV_DS_MAX_NAME],
    pub state: AtomicU8,
    pub props: ZvDsProps,
    pub objset: ZvObjSet,
    pub dir_zap: ZvZap,
    pub xattr_zap: ZvZap,
    pub parent_id: Option<u64>,
    pub child_ids: Vec<u64>,
    pub used_space: AtomicU64,
    pub ref_count: AtomicU64,
    pub birth_txg: AtomicU64,
    pub root_bp: Mutex<ZvBlockPointer>,
    pub is_snapshot: bool,
    pub snapshot_origin: Option<u64>,
    pub prev_snap: Option<u64>,
    pub next_snap: Option<u64>,
    pub snap_name: [u8; ZV_DS_MAX_NAME],
    pub writeable: bool,
    pub mounted: AtomicBool,
}

unsafe impl Send for ZvDataset {}
unsafe impl Sync for ZvDataset {}

impl ZvDataset {
    pub fn new(ds_id: u64, name: &str, owner_pwid: u64) -> Self {
        let mut n = [0u8; ZV_DS_MAX_NAME];
        let b = name.as_bytes();
        let len = b.len().min(ZV_DS_MAX_NAME - 1);
        n[..len].copy_from_slice(&b[..len]);
        let mut props = ZvDsProps::default();
        props.owner_pwid = owner_pwid;
        Self {
            ds_id,
            name: n,
            state: AtomicU8::new(ZvDsState::Creating as u8),
            props,
            objset: ZvObjSet::new(),
            dir_zap: ZvZap::new(),
            xattr_zap: ZvZap::new(),
            parent_id: None,
            child_ids: Vec::new(),
            used_space: AtomicU64::new(0),
            ref_count: AtomicU64::new(1),
            birth_txg: AtomicU64::new(0),
            root_bp: Mutex::new(ZvBlockPointer::null()),
            is_snapshot: false,
            snapshot_origin: None,
            prev_snap: None,
            next_snap: None,
            snap_name: [0; ZV_DS_MAX_NAME],
            writeable: true,
            mounted: AtomicBool::new(false),
        }
    }

    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(ZV_DS_MAX_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    pub fn init(&self, owner_pwid: u64) {
        self.objset.init(owner_pwid);
        self.state.store(ZvDsState::Active as u8, Ordering::Release);
        self.mounted.store(true, Ordering::Release);
    }

    pub fn is_active(&self) -> bool {
        self.state.load(Ordering::Acquire) == ZvDsState::Active as u8
    }

    pub fn is_writeable(&self) -> bool {
        self.writeable && self.state.load(Ordering::Acquire) == ZvDsState::Active as u8
    }

    pub fn create_file(&self, name: &str, owner_pwid: u64) -> Option<u64> {
        if !self.is_writeable() { return None; }
        let obj_id = self.objset.alloc_obj(ZvObjType::File, owner_pwid)?;
        self.dir_zap.insert_u64(name, obj_id);
        Some(obj_id)
    }

    pub fn create_dir(&self, name: &str, owner_pwid: u64) -> Option<u64> {
        if !self.is_writeable() { return None; }
        let obj_id = self.objset.alloc_obj(ZvObjType::Dir, owner_pwid)?;
        self.dir_zap.insert_u64(name, obj_id);
        Some(obj_id)
    }

    pub fn lookup(&self, name: &str) -> Option<u64> {
        self.dir_zap.lookup_u64(name)
    }

    pub fn unlink(&self, name: &str) -> bool {
        if !self.is_writeable() { return false; }
        if let Some(obj_id) = self.dir_zap.lookup_u64(name) {
            self.objset.free_obj(obj_id);
            self.dir_zap.remove(name);
            true
        } else {
            false
        }
    }

    pub fn list_entries(&self) -> Vec<(String, u64)> {
        let keys = self.dir_zap.keys();
        let mut result = Vec::new();
        for key in keys {
            if let Some(obj_id) = self.dir_zap.lookup_u64(&key) {
                result.push((key, obj_id));
            }
        }
        result
    }

    pub fn get_used(&self) -> u64 {
        self.used_space.load(Ordering::Relaxed)
    }

    pub fn get_ref_count(&self) -> u64 {
        self.ref_count.load(Ordering::Relaxed)
    }
}
