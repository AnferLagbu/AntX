use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::kernel::sync::mutex::Mutex;
use crate::kernel::fs::zvfs::bp::ZvBlockPointer;

pub const ZV_ZIL_MAX_RECORDS: usize = 1024;
pub const ZV_ZIL_BLOCK_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvZilRecordType {
    Create = 1,
    Remove = 2,
    Link = 3,
    Rename = 4,
    Write = 5,
    Truncate = 6,
    SetAttr = 7,
    Acl = 8,
    CreateAcl = 9,
    Mkdir = 10,
}

#[derive(Debug, Clone)]
pub struct ZvZilRecord {
    pub rec_type: ZvZilRecordType,
    pub txg: u64,
    pub obj_id: u64,
    pub parent_obj: u64,
    pub offset: u64,
    pub size: u32,
    pub name: [u8; 128],
    pub data_hash: [u64; 4],
    pub seq: u64,
}

impl ZvZilRecord {
    pub fn new_write(txg: u64, obj_id: u64, offset: u64, size: u32) -> Self {
        Self {
            rec_type: ZvZilRecordType::Write,
            txg,
            obj_id,
            parent_obj: 0,
            offset,
            size,
            name: [0; 128],
            data_hash: [0; 4],
            seq: 0,
        }
    }

    pub fn new_create(txg: u64, parent_obj: u64, name: &str) -> Self {
        let mut n = [0u8; 128];
        let b = name.as_bytes();
        let len = b.len().min(127);
        n[..len].copy_from_slice(&b[..len]);
        Self {
            rec_type: ZvZilRecordType::Create,
            txg,
            obj_id: 0,
            parent_obj,
            offset: 0,
            size: 0,
            name: n,
            data_hash: [0; 4],
            seq: 0,
        }
    }

    pub fn new_remove(txg: u64, parent_obj: u64, name: &str) -> Self {
        let mut n = [0u8; 128];
        let b = name.as_bytes();
        let len = b.len().min(127);
        n[..len].copy_from_slice(&b[..len]);
        Self {
            rec_type: ZvZilRecordType::Remove,
            txg,
            obj_id: 0,
            parent_obj,
            offset: 0,
            size: 0,
            name: n,
            data_hash: [0; 4],
            seq: 0,
        }
    }

    pub fn new_mkdir(txg: u64, parent_obj: u64, name: &str) -> Self {
        let mut n = [0u8; 128];
        let b = name.as_bytes();
        let len = b.len().min(127);
        n[..len].copy_from_slice(&b[..len]);
        Self {
            rec_type: ZvZilRecordType::Mkdir,
            txg,
            obj_id: 0,
            parent_obj,
            offset: 0,
            size: 0,
            name: n,
            data_hash: [0; 4],
            seq: 0,
        }
    }

    pub fn new_setattr(txg: u64, obj_id: u64) -> Self {
        Self {
            rec_type: ZvZilRecordType::SetAttr,
            txg,
            obj_id,
            parent_obj: 0,
            offset: 0,
            size: 0,
            name: [0; 128],
            data_hash: [0; 4],
            seq: 0,
        }
    }
}

pub struct ZvZil {
    pub records: Mutex<Vec<ZvZilRecord>>,
    pub committed_seq: AtomicU64,
    pub current_seq: AtomicU64,
    pub log_bp: Mutex<ZvBlockPointer>,
    pub itxg: AtomicU64,
    pub syncing: AtomicBool,
    pub replaying: AtomicBool,
    pub enabled: AtomicBool,
}

unsafe impl Send for ZvZil {}
unsafe impl Sync for ZvZil {}

impl ZvZil {
    pub fn new() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            committed_seq: AtomicU64::new(0),
            current_seq: AtomicU64::new(0),
            log_bp: Mutex::new(ZvBlockPointer::null()),
            itxg: AtomicU64::new(0),
            syncing: AtomicBool::new(false),
            replaying: AtomicBool::new(false),
            enabled: AtomicBool::new(true),
        }
    }

    pub fn init(&self) {
        self.committed_seq.store(0, Ordering::Release);
        self.current_seq.store(0, Ordering::Release);
        self.records.lock().clear();
        self.enabled.store(true, Ordering::Release);
    }

    pub fn add_record(&self, record: ZvZilRecord) {
        if !self.enabled.load(Ordering::Acquire) { return; }
        let seq = self.current_seq.fetch_add(1, Ordering::AcqRel) + 1;
        let mut rec = record;
        rec.seq = seq;
        self.records.lock().push(rec);
    }

    pub fn commit(&self, txg: u64) {
        let mut records = self.records.lock();
        let mut max_seq = 0u64;
        records.retain(|r| {
            if r.txg <= txg {
                max_seq = max_seq.max(r.seq);
                false
            } else {
                true
            }
        });
        self.committed_seq.store(max_seq, Ordering::Release);
    }

    pub fn sync(&self, txg: u64) {
        if self.syncing.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_err() {
            return;
        }
        self.commit(txg);
        self.syncing.store(false, Ordering::Release);
    }

    pub fn replay(&self) -> Vec<ZvZilRecord> {
        if self.replaying.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_err() {
            return Vec::new();
        }
        let records = self.records.lock().clone();
        let mut sorted = records;
        sorted.sort_by_key(|r| r.seq);
        self.replaying.store(false, Ordering::Release);
        sorted
    }

    pub fn has_uncommitted(&self) -> bool {
        let committed = self.committed_seq.load(Ordering::Acquire);
        let current = self.current_seq.load(Ordering::Acquire);
        current > committed
    }

    pub fn pending_count(&self) -> usize {
        self.records.lock().len()
    }
}
