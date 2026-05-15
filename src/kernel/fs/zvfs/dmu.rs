use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::kernel::sync::mutex::Mutex;
use crate::kernel::fs::zvfs::bp::*;

pub const ZV_DMU_OBJ_NUM: u64 = 0;
pub const ZV_DMU_OBJ_META: u64 = 1;
pub const ZV_DMU_OBJ_ROOT: u64 = 2;
pub const ZV_DMU_MAX_BLOCKPTR: usize = 16;
pub const ZV_DMU_MAX_NAME: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvObjType {
    None = 0,
    File = 1,
    Dir = 2,
    Snapshot = 3,
    Zap = 4,
    ZapMicro = 5,
    Volume = 6,
    SpaceMap = 7,
    ObjSet = 8,
}

impl ZvObjType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::File,
            2 => Self::Dir,
            3 => Self::Snapshot,
            4 => Self::Zap,
            5 => Self::ZapMicro,
            6 => Self::Volume,
            7 => Self::SpaceMap,
            8 => Self::ObjSet,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ZvDmuObject {
    pub obj_id: u64,
    pub obj_type: ZvObjType,
    pub block_size: u32,
    pub nblocks: u64,
    pub size: u64,
    pub bp: ZvBlockPointer,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub owner_pwid: u64,
    pub group_pwid: u64,
    pub sensitivity: u8,
    pub pwid_perm: u16,
    pub link_count: u32,
    pub flags: u32,
    pub birth_txg: u64,
    pub fill: u64,
    pub dirty: bool,
    pub used: bool,
}

impl ZvDmuObject {
    pub fn new_file(obj_id: u64, owner_pwid: u64) -> Self {
        Self {
            obj_id,
            obj_type: ZvObjType::File,
            block_size: 4096,
            nblocks: 0,
            size: 0,
            bp: ZvBlockPointer::null(),
            atime: 0, mtime: 0, ctime: 0,
            owner_pwid,
            group_pwid: 0,
            sensitivity: 0,
            pwid_perm: 0o644,
            link_count: 1,
            flags: 0,
            birth_txg: 0,
            fill: 0,
            dirty: false,
            used: true,
        }
    }

    pub fn new_dir(obj_id: u64, owner_pwid: u64) -> Self {
        Self {
            obj_id,
            obj_type: ZvObjType::Dir,
            block_size: 4096,
            nblocks: 0,
            size: 0,
            bp: ZvBlockPointer::null(),
            atime: 0, mtime: 0, ctime: 0,
            owner_pwid,
            group_pwid: 0,
            sensitivity: 0,
            pwid_perm: 0o755,
            link_count: 2,
            flags: 0,
            birth_txg: 0,
            fill: 0,
            dirty: false,
            used: true,
        }
    }

    pub fn new_zap(obj_id: u64) -> Self {
        Self {
            obj_id,
            obj_type: ZvObjType::Zap,
            block_size: 4096,
            nblocks: 0,
            size: 0,
            bp: ZvBlockPointer::null(),
            atime: 0, mtime: 0, ctime: 0,
            owner_pwid: 0,
            group_pwid: 0,
            sensitivity: 0,
            pwid_perm: 0,
            link_count: 1,
            flags: 0,
            birth_txg: 0,
            fill: 0,
            dirty: false,
            used: true,
        }
    }

    pub fn is_file(&self) -> bool { self.obj_type == ZvObjType::File }
    pub fn is_dir(&self) -> bool { self.obj_type == ZvObjType::Dir }
    pub fn is_zap(&self) -> bool { self.obj_type == ZvObjType::Zap || self.obj_type == ZvObjType::ZapMicro }
    pub fn is_snapshot(&self) -> bool { self.obj_type == ZvObjType::Snapshot }

    pub fn mark_dirty(&mut self, txg: u64) {
        self.dirty = true;
        self.birth_txg = txg;
    }

    pub fn cow_bp(&mut self, new_bp: ZvBlockPointer, txg: u64) {
        self.bp = new_bp;
        self.birth_txg = txg;
        self.dirty = true;
    }
}

pub struct ZvObjSet {
    pub objects: Mutex<Vec<ZvDmuObject>>,
    pub next_obj_id: AtomicU64,
    pub root_obj: u64,
    pub initialized: AtomicBool,
}

unsafe impl Send for ZvObjSet {}
unsafe impl Sync for ZvObjSet {}

impl ZvObjSet {
    pub fn new() -> Self {
        Self {
            objects: Mutex::new(Vec::new()),
            next_obj_id: AtomicU64::new(ZV_DMU_OBJ_ROOT + 1),
            root_obj: ZV_DMU_OBJ_ROOT,
            initialized: AtomicBool::new(false),
        }
    }

    pub fn init(&self, owner_pwid: u64) {
        let mut objs = self.objects.lock();
        objs.clear();
        let mut root = ZvDmuObject::new_dir(ZV_DMU_OBJ_ROOT, owner_pwid);
        root.birth_txg = 1;
        objs.push(root);
        let zap = ZvDmuObject::new_zap(ZV_DMU_OBJ_META);
        objs.push(zap);
        self.next_obj_id.store(ZV_DMU_OBJ_ROOT + 2, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
    }

    pub fn alloc_obj(&self, obj_type: ZvObjType, owner_pwid: u64) -> Option<u64> {
        let obj_id = self.next_obj_id.fetch_add(1, Ordering::AcqRel);
        let obj = match obj_type {
            ZvObjType::File => ZvDmuObject::new_file(obj_id, owner_pwid),
            ZvObjType::Dir => ZvDmuObject::new_dir(obj_id, owner_pwid),
            ZvObjType::Zap | ZvObjType::ZapMicro => ZvDmuObject::new_zap(obj_id),
            _ => return None,
        };
        self.objects.lock().push(obj);
        Some(obj_id)
    }

    pub fn free_obj(&self, obj_id: u64) -> bool {
        let mut objs = self.objects.lock();
        if let Some(obj) = objs.iter_mut().find(|o| o.obj_id == obj_id) {
            obj.used = false;
            obj.link_count = obj.link_count.saturating_sub(1);
            if obj.link_count == 0 {
                obj.used = false;
            }
            true
        } else {
            false
        }
    }

    pub fn get_obj(&self, obj_id: u64) -> Option<ZvDmuObject> {
        let objs = self.objects.lock();
        objs.iter().find(|o| o.obj_id == obj_id && o.used).cloned()
    }

    pub fn update_obj(&self, obj: &ZvDmuObject) -> bool {
        let mut objs = self.objects.lock();
        if let Some(existing) = objs.iter_mut().find(|o| o.obj_id == obj.obj_id) {
            *existing = obj.clone();
            true
        } else {
            false
        }
    }

    pub fn get_root(&self) -> Option<ZvDmuObject> {
        self.get_obj(self.root_obj)
    }

    pub fn obj_count(&self) -> u64 {
        self.objects.lock().iter().filter(|o| o.used).count() as u64
    }
}
