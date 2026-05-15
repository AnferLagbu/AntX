use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::kernel::sync::mutex::Mutex;
use crate::kernel::fs::zvfs::bp::*;
use crate::kernel::fs::zvfs::spa::*;
use crate::kernel::fs::zvfs::txg::*;
use crate::kernel::fs::zvfs::dmu::*;
use crate::kernel::fs::zvfs::dataset::*;
use crate::kernel::fs::zvfs::snapshot::*;
use crate::kernel::fs::zvfs::zil::*;
use crate::kernel::fs::zvfs::compress;
use crate::kernel::fs::zvfs::arc::{ZvArcBufType, ZvArcKey};

extern "C" {
    fn klog_ffi_info(msg: *const u8);
    fn pwid_get_privilege_level(pwid: u64) -> u8;
    fn pwid_has_capability(pwid: u64, domain: u16, required: u64) -> bool;
    fn timer_get_ticks() -> u64;
}

fn log(s: &str) {
    unsafe { klog_ffi_info(s.as_ptr()); }
}

pub const ZVFS_MAX_FDS: usize = 64;
pub const ZVFS_MAX_PATH: usize = 256;
pub const ZVFS_MAX_NAME: usize = 128;

#[derive(Debug, Clone, Copy)]
pub struct ZvfsFd {
    pub fd: u32,
    pub obj_id: u64,
    pub ds_id: u64,
    pub offset: u64,
    pub flags: u32,
    pub pwid: u64,
    pub used: bool,
}

pub struct ZvfsData {
    pub spa: ZvSpa,
    pub txg_group: Mutex<Option<ZvTxgGroup>>,
    pub datasets: Mutex<Vec<ZvDataset>>,
    pub snap_mgr: ZvSnapshotManager,
    pub zil: ZvZil,
    pub fds: Mutex<[ZvfsFd; ZVFS_MAX_FDS]>,
    pub next_fd: AtomicU32,
    pub current_pwid: AtomicU64,
    pub current_dir: AtomicU64,
    pub mounted: AtomicBool,
    pub initialized: AtomicBool,
    pub root_ds_id: AtomicU64,
}

unsafe impl Send for ZvfsData {}
unsafe impl Sync for ZvfsData {}

static mut ZVFS_DATA: Option<Box<ZvfsData>> = None;

pub fn get_zvfs() -> &'static ZvfsData {
    unsafe {
        if ZVFS_DATA.is_none() {
            let data = Box::new(ZvfsData {
                spa: ZvSpa::new(),
                txg_group: Mutex::new(None),
                datasets: Mutex::new(Vec::new()),
                snap_mgr: ZvSnapshotManager::new(),
                zil: ZvZil::new(),
                fds: Mutex::new([ZvfsFd { fd: 0, obj_id: 0, ds_id: 0, offset: 0, flags: 0, pwid: 0, used: false }; ZVFS_MAX_FDS]),
                next_fd: AtomicU32::new(0),
                current_pwid: AtomicU64::new(0),
                current_dir: AtomicU64::new(ZV_DMU_OBJ_ROOT),
                mounted: AtomicBool::new(false),
                initialized: AtomicBool::new(false),
                root_ds_id: AtomicU64::new(0),
            });
            ZVFS_DATA = Some(data);
        }
        ZVFS_DATA.as_ref().unwrap().as_ref()
    }
}

impl ZvfsData {
    pub fn init(&self) {
        log("[ZvFS] Initializing...\n");
        self.spa.init("antx-pool");
        self.spa.add_vdev(crate::kernel::fs::zvfs::vdev::ZvVdevConfig::new_disk(0, "ata0", 12));
        {
            let mut txg_guard = self.txg_group.lock();
            let mut txg_group = ZvTxgGroup::new();
            txg_group.init(1);
            *txg_guard = Some(txg_group);
        }
        self.zil.init();
        {
            let mut datasets = self.datasets.lock();
            let root_ds = ZvDataset::new(0, "root", 0);
            datasets.push(root_ds);
        }
        {
            let datasets = self.datasets.lock();
            datasets[0].init(0);
        }
        self.root_ds_id.store(0, Ordering::Release);
        self.current_dir.store(ZV_DMU_OBJ_ROOT, Ordering::Release);
        self.mounted.store(true, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
        log("[ZvFS] Initialized: pool=antx-pool vdev=ata0\n");
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    fn alloc_fd(&self) -> Option<usize> {
        let mut fds = self.fds.lock();
        for i in 0..ZVFS_MAX_FDS {
            if !fds[i].used {
                let fd = self.next_fd.fetch_add(1, Ordering::AcqRel);
                fds[i].fd = fd;
                fds[i].used = true;
                return Some(i);
            }
        }
        None
    }

    fn free_fd(&self, idx: usize) {
        let mut fds = self.fds.lock();
        if idx < ZVFS_MAX_FDS {
            fds[idx].used = false;
            fds[idx].offset = 0;
        }
    }

    fn check_permission(&self, obj: &ZvDmuObject, pwid: u64, cap: u64) -> bool {
        if pwid == 0 { return true; }
        let level = unsafe { pwid_get_privilege_level(pwid) };
        if level == 0xFF { return false; }
        if level == 0 { return true; }
        if obj.owner_pwid == pwid { return true; }
        unsafe { pwid_has_capability(pwid, 3, cap) }
    }

    pub fn open(&self, path: &str, flags: u32, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let name = path.trim_start_matches('/');
        let obj_id = {
            let datasets = self.datasets.lock();
            let ds = &datasets[0];
            match ds.lookup(name) {
                Some(id) => Some(id),
                None => {
                    if flags & 0x0100 != 0 {
                        ds.create_file(name, pwid)
                    } else {
                        None
                    }
                }
            }
        };
        let obj_id = match obj_id {
            Some(id) => id,
            None => return -2,
        };
        let obj = {
            let datasets = self.datasets.lock();
            datasets[0].objset.get_obj(obj_id)
        };
        let obj = match obj {
            Some(o) => o,
            None => return -1,
        };
        if !self.check_permission(&obj, pwid, 0x01) { return -3; }
        let fd_idx = match self.alloc_fd() {
            Some(i) => i,
            None => return -4,
        };
        {
            let mut fds = self.fds.lock();
            fds[fd_idx].obj_id = obj_id;
            fds[fd_idx].ds_id = self.root_ds_id.load(Ordering::Acquire);
            fds[fd_idx].offset = if flags & 0x0400 != 0 { obj.size } else { 0 };
            fds[fd_idx].flags = flags;
            fds[fd_idx].pwid = pwid;
        }
        self.zil.add_record(ZvZilRecord::new_create(0, 0, name));
        fd_idx as i32
    }

    pub fn close(&self, fd: u32) -> i32 {
        let idx = fd as usize;
        if idx >= ZVFS_MAX_FDS { return -1; }
        {
            let fds = self.fds.lock();
            if !fds[idx].used { return -1; }
        }
        self.free_fd(idx);
        0
    }

    pub fn read(&self, fd: u32, buf: &mut [u8], count: u32) -> i32 {
        let (obj_id, offset, pwid) = {
            let fds = self.fds.lock();
            let idx = fd as usize;
            if idx >= ZVFS_MAX_FDS || !fds[idx].used { return -1; }
            (fds[idx].obj_id, fds[idx].offset, fds[idx].pwid)
        };
        let obj = {
            let datasets = self.datasets.lock();
            datasets[0].objset.get_obj(obj_id)
        };
        let obj = match obj {
            Some(o) => o,
            None => return -1,
        };
        if !self.check_permission(&obj, pwid, 0x01) { return -3; }
        let available = if offset < obj.size { (obj.size - offset) as usize } else { 0 };
        let to_read = (count as usize).min(available).min(buf.len());
        if to_read == 0 { return 0; }
        if !obj.bp.is_null() {
            let key = ZvArcKey::new(0, offset / 4096 * 4096, obj.birth_txg);
            if let Some(data_ptr) = self.spa.arc.lookup(&key) {
                let data = unsafe { core::slice::from_raw_parts(data_ptr, to_read) };
                buf[..to_read].copy_from_slice(data);
                self.spa.arc.release(&key);
            }
        }
        {
            let mut fds = self.fds.lock();
            fds[fd as usize].offset += to_read as u64;
        }
        to_read as i32
    }

    pub fn write(&self, fd: u32, buf: &[u8], count: u32) -> i32 {
        let (obj_id, offset, pwid, flags) = {
            let fds = self.fds.lock();
            let idx = fd as usize;
            if idx >= ZVFS_MAX_FDS || !fds[idx].used { return -1; }
            (fds[idx].obj_id, fds[idx].offset, fds[idx].pwid, fds[idx].flags)
        };
        if flags & 0x0001 != 0 && flags & 0x0002 == 0 { return -1; }
        let mut obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        let to_write = (count as usize).min(buf.len());
        if to_write == 0 { return 0; }
        let txg = self.spa.current_txg();
        let cksum_type = ZvCksumType::Fletcher4;
        let comp_type = ZvCompType::Off;
        let compressed = compress::compress(&buf[..to_write], comp_type);
        let write_data = compressed.as_deref().unwrap_or(&buf[..to_write]);
        let new_bp = match self.spa.allocate(write_data.len() as u64, cksum_type, comp_type, txg) {
            Some(bp) => bp,
            None => return -1,
        };
        if self.spa.write_bp(&new_bp, write_data) != 0 {
            self.spa.free(&new_bp, txg);
            return -1;
        }
        obj.cow_bp(new_bp, txg);
        obj.size = (offset + to_write as u64).max(obj.size);
        obj.mtime = unsafe { timer_get_ticks() };
        {
            let datasets = self.datasets.lock();
            datasets[0].objset.update_obj(&obj);
        }
        {
            let txg_guard = self.txg_group.lock();
            if let Some(ref txg_group) = *txg_guard {
                txg_group.add_dirty_to_open(obj.bp);
            }
        }
        self.zil.add_record(ZvZilRecord::new_write(txg, obj_id, offset, to_write as u32));
        {
            let mut fds = self.fds.lock();
            fds[fd as usize].offset += to_write as u64;
        }
        to_write as i32
    }

    pub fn mkdir(&self, path: &str, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let name = path.trim_start_matches('/');
        let datasets = self.datasets.lock();
        let ds = &datasets[0];
        match ds.create_dir(name, pwid) {
            Some(obj_id) => {
                let txg = self.spa.current_txg();
                self.zil.add_record(ZvZilRecord::new_mkdir(txg, 0, name));
                obj_id as i32
            }
            None => -1,
        }
    }

    pub fn unlink(&self, path: &str, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let name = path.trim_start_matches('/');
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(name) {
                Some(id) => Some(id),
                None => None,
            }
        };
        let obj_id = match obj_id {
            Some(id) => id,
            None => return -2,
        };
        let obj = {
            let datasets = self.datasets.lock();
            datasets[0].objset.get_obj(obj_id)
        };
        let obj = match obj {
            Some(o) => o,
            None => return -1,
        };
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        {
            let datasets = self.datasets.lock();
            if !datasets[0].unlink(name) { return -1; }
        }
        let txg = self.spa.current_txg();
        if !obj.bp.is_null() {
            self.spa.free(&obj.bp, txg);
        }
        self.zil.add_record(ZvZilRecord::new_remove(txg, 0, name));
        0
    }

    pub fn stat(&self, path: &str, pwid: u64) -> Option<ZvDmuObject> {
        if !self.is_initialized() { return None; }
        let name = path.trim_start_matches('/');
        let datasets = self.datasets.lock();
        let ds = &datasets[0];
        let obj_id = ds.lookup(name)?;
        let obj = ds.objset.get_obj(obj_id)?;
        if !self.check_permission(&obj, pwid, 0x01) { return None; }
        Some(obj)
    }

    pub fn sync(&self) -> i32 {
        if !self.is_initialized() { return -1; }
        let txg = self.spa.advance_txg();
        self.zil.sync(txg);
        self.spa.sync_uberblock();
        {
            let mut txg_guard = self.txg_group.lock();
            if let Some(ref mut txg_group) = *txg_guard {
                txg_group.transition();
            }
        }
        log("[ZvFS] Sync complete\n");
        0
    }

    pub fn snapshot_create(&self, name: &str) -> i32 {
        if !self.is_initialized() { return -1; }
        let datasets = self.datasets.lock();
        let ds = &datasets[0];
        let txg = self.spa.current_txg();
        match self.snap_mgr.create_snapshot(ds, name, txg) {
            Some(id) => id as i32,
            None => -1,
        }
    }

    pub fn snapshot_destroy(&self, snap_id: u64) -> i32 {
        if self.snap_mgr.destroy_snapshot(snap_id) { 0 } else { -1 }
    }

    pub fn snapshot_rollback(&self, snap_id: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let datasets = self.datasets.lock();
        let ds = &datasets[0];
        if self.snap_mgr.rollback(snap_id, ds) { 0 } else { -1 }
    }

    pub fn clone_create(&self, snap_id: u64, name: &str) -> i32 {
        if !self.is_initialized() { return -1; }
        let ds_id = {
            let datasets = self.datasets.lock();
            datasets.len() as u64
        };
        let txg = self.spa.current_txg();
        match self.snap_mgr.create_clone(snap_id, ds_id, name, txg) {
            Some(mut ds) => {
                ds.init(0);
                self.datasets.lock().push(ds);
                ds_id as i32
            }
            None => -1,
        }
    }

    pub fn seek(&self, fd: u32, offset: i64, whence: u32) -> i64 {
        let (obj_id, cur_offset) = {
            let fds = self.fds.lock();
            let idx = fd as usize;
            if idx >= ZVFS_MAX_FDS || !fds[idx].used { return -1; }
            (fds[idx].obj_id, fds[idx].offset)
        };
        let obj = {
            let datasets = self.datasets.lock();
            datasets[0].objset.get_obj(obj_id)
        };
        let obj = match obj {
            Some(o) => o,
            None => return -1,
        };
        let new_offset = match whence {
            0 => offset as u64,
            1 => (cur_offset as i64 + offset) as u64,
            2 => (obj.size as i64 + offset) as u64,
            _ => return -1,
        };
        {
            let mut fds = self.fds.lock();
            fds[fd as usize].offset = new_offset;
        }
        new_offset as i64
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        if !self.is_initialized() { return (0, 0, 0, 0); }
        let (allocs, frees, reads, writes, _) = self.spa.get_stats();
        (allocs, frees, reads, writes)
    }
}
