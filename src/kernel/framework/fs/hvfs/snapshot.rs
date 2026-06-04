use crate::kernel::framework::fs::hvfs::bp::HvBlockPointer;
use crate::kernel::framework::fs::hvfs::dataset::HvDataset;
use crate::kernel::framework::sync_tcb_legacy::mutex::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

pub const HV_SNAP_MAX: usize = 64;

pub struct HvSnapshotManager {
    pub snapshots: Mutex<Vec<HvSnapshot>>,
    pub next_snap_id: AtomicU64,
}

// SAFETY (Framekernel P2.2.2): HvSnapshotManager 全部字段 (Mutex<T>, AtomicU64) 自动 Send + Sync。

#[derive(Debug, Clone)]
pub struct HvSnapshot {
    pub snap_id: u64,
    pub ds_id: u64,
    pub name: [u8; 128],
    pub root_bp: HvBlockPointer,
    pub birth_txg: u64,
    pub used_space: u64,
    pub ref_count: u64,
    pub is_clone: bool,
    pub origin_snap: Option<u64>,
}

impl HvSnapshot {
    pub fn new(snap_id: u64, ds_id: u64, name: &str, root_bp: HvBlockPointer, txg: u64) -> Self {
        let mut n = [0u8; 128];
        let b = name.as_bytes();
        let len = b.len().min(127);
        n[..len].copy_from_slice(&b[..len]);
        Self {
            snap_id,
            ds_id,
            name: n,
            root_bp,
            birth_txg: txg,
            used_space: 0,
            ref_count: 1,
            is_clone: false,
            origin_snap: None,
        }
    }

    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(128);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

impl HvSnapshotManager {
    pub fn new() -> Self {
        Self {
            snapshots: Mutex::new(Vec::new()),
            next_snap_id: AtomicU64::new(1),
        }
    }

    pub fn create_snapshot(&self, ds: &HvDataset, name: &str, txg: u64) -> Option<u64> {
        let snap_id = self.next_snap_id.fetch_add(1, Ordering::AcqRel);
        let root_bp = *ds.root_bp.lock();
        let mut snap = HvSnapshot::new(snap_id, ds.ds_id, name, root_bp, txg);
        snap.used_space = ds.get_used();
        self.snapshots.lock().push(snap);
        Some(snap_id)
    }

    pub fn destroy_snapshot(&self, snap_id: u64) -> bool {
        let mut snaps = self.snapshots.lock();
        let idx = snaps.iter().position(|s| s.snap_id == snap_id);
        match idx {
            Some(i) => {
                let snap = &snaps[i];
                if snap.ref_count > 1 {
                    return false;
                }
                snaps.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn create_clone(
        &self,
        snap_id: u64,
        ds_id: u64,
        name: &str,
        txg: u64,
    ) -> Option<HvDataset> {
        let mut snaps = self.snapshots.lock();
        let snap = snaps.iter_mut().find(|s| s.snap_id == snap_id)?;
        let root_bp = snap.root_bp;
        let mut ds = HvDataset::new(ds_id, name, 0);
        *ds.root_bp.lock() = root_bp;
        ds.birth_txg.store(txg, Ordering::Release);
        ds.is_snapshot = false;
        ds.snapshot_origin = Some(snap_id);
        ds.writeable = true;
        snap.ref_count += 1;
        Some(ds)
    }

    pub fn get_snapshot(&self, snap_id: u64) -> Option<HvSnapshot> {
        self.snapshots
            .lock()
            .iter()
            .find(|s| s.snap_id == snap_id)
            .cloned()
    }

    pub fn list_snapshots(&self, ds_id: u64) -> Vec<HvSnapshot> {
        self.snapshots
            .lock()
            .iter()
            .filter(|s| s.ds_id == ds_id)
            .cloned()
            .collect()
    }

    pub fn rollback(&self, snap_id: u64, ds: &HvDataset) -> bool {
        let snaps = self.snapshots.lock();
        let snap = match snaps.iter().find(|s| s.snap_id == snap_id) {
            Some(s) => s,
            None => return false,
        };
        if snap.ds_id != ds.ds_id {
            return false;
        }
        *ds.root_bp.lock() = snap.root_bp;
        ds.used_space.store(snap.used_space, Ordering::Release);
        true
    }

    pub fn snapshot_count(&self) -> usize {
        self.snapshots.lock().len()
    }
}
