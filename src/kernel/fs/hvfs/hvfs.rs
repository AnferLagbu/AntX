use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use spin::Mutex;
use crate::kernel::fs::hvfs::bp::*;
use crate::kernel::fs::hvfs::spa::*;
use crate::kernel::fs::hvfs::txg::*;
use crate::kernel::fs::hvfs::dmu::*;
use crate::kernel::fs::hvfs::dataset::*;
use crate::kernel::fs::hvfs::snapshot::*;
use crate::kernel::fs::hvfs::zil::*;
use crate::kernel::fs::hvfs::compress;
use crate::kernel::fs::hvfs::arc::{HvArcBufType, HvArcKey};
use crate::kernel::fs::vfs::types::KernelError;

extern "C" {
    fn klog_ffi_info(msg: *const u8);
    fn pwid_get_privilege_level(pwid: u64) -> u8;
    fn pwid_has_capability(pwid: u64, domain: u16, required: u64) -> bool;
    fn timer_get_ticks() -> u64;
    fn ata_disk_present(disk: u8) -> i32;
}

fn log(s: &str) {
    unsafe { klog_ffi_info(s.as_ptr()); }
}

pub const HVFS_MAX_FDS: usize = 64;

#[derive(Debug, Clone, Copy)]
pub struct HvfsFd {
    pub fd: u32,
    pub obj_id: u64,
    pub ds_id: u64,
    pub offset: u64,
    pub flags: u32,
    pub pwid: u64,
    pub used: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HvfsMode {
    Memory = 0,
    Disk = 1,
}

pub struct HvfsData {
    pub spa: HvSpa,
    pub txg_group: Mutex<Option<HvTxgGroup>>,
    pub datasets: Mutex<Vec<HvDataset>>,
    pub snap_mgr: HvSnapshotManager,
    pub zil: HvZil,
    pub fds: Mutex<[HvfsFd; HVFS_MAX_FDS]>,
    pub next_fd: AtomicU32,
    pub current_pwid: AtomicU64,
    pub current_dir: AtomicU64,
    pub mounted: AtomicBool,
    pub initialized: AtomicBool,
    pub root_ds_id: AtomicU64,
    pub mode: AtomicU8,
    pub disk_drive: u8,
}

unsafe impl Send for HvfsData {}
unsafe impl Sync for HvfsData {}

static HVFS_DATA: spin::Mutex<Option<Box<HvfsData>>> = spin::Mutex::new(None);

pub fn get_hvfs() -> &'static HvfsData {
    let mut guard = HVFS_DATA.lock();
    if guard.is_none() {
        let data = Box::new(HvfsData {
            spa: HvSpa::new(),
            txg_group: Mutex::new(None),
            datasets: Mutex::new(Vec::new()),
            snap_mgr: HvSnapshotManager::new(),
            zil: HvZil::new(),
            fds: Mutex::new([HvfsFd { fd: 0, obj_id: 0, ds_id: 0, offset: 0, flags: 0, pwid: 0, used: false }; HVFS_MAX_FDS]),
            next_fd: AtomicU32::new(0),
            current_pwid: AtomicU64::new(0),
            current_dir: AtomicU64::new(HV_DMU_OBJ_ROOT),
            mounted: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            root_ds_id: AtomicU64::new(0),
            mode: AtomicU8::new(HvfsMode::Memory as u8),
            disk_drive: 0,
        });
        *guard = Some(data);
    }
    let ptr: *const HvfsData = guard.as_ref().unwrap().as_ref() as *const HvfsData;
    drop(guard);
    unsafe { &*ptr }
}

impl HvfsData {
    fn check_disk(&self) -> bool {
        unsafe { ata_disk_present(self.disk_drive) != 0 }
    }

    fn read_sector(&self, sector: u32, buf: &mut [u8]) -> i32 {
        extern "C" {
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }
        if buf.len() < 512 { return -1; }
        unsafe { ata_read_sector(self.disk_drive, sector, buf.as_mut_ptr()) }
    }

    fn write_sector(&self, sector: u32, buf: &[u8]) -> i32 {
        extern "C" {
            fn ata_write_sector(disk: u8, sector: u32, buf: *const u8) -> i32;
        }
        if buf.len() < 512 { return -1; }
        unsafe { ata_write_sector(self.disk_drive, sector, buf.as_ptr()) }
    }

    pub fn init(&self) {
        log("[HvFS] Initializing...\n");
        self.spa.init("antx-pool");
        let has_disk = self.check_disk();
        self.spa.disk_present.store(has_disk, Ordering::Release);
        if has_disk {
            log("[HvFS] Disk detected, mounting from disk...\n");
            if self.mount_disk() {
                log("[HvFS] Mounted from disk successfully\n");
                return;
            }
            log("[HvFS] Mount from disk failed, formatting and starting fresh...\n");
            self.format_disk();
        } else {
            log("[HvFS] No disk, running in memory mode\n");
        }
        self.spa.add_vdev(crate::kernel::fs::hvfs::vdev::HvVdevConfig::new_disk(0, "ata0", 12));
        {
            let mut txg_guard = self.txg_group.lock();
            let mut txg_group = HvTxgGroup::new();
            txg_group.init(1);
            *txg_guard = Some(txg_group);
        }
        self.zil.init();
        {
            let mut datasets = self.datasets.lock();
            let root_ds = HvDataset::new(0, "root", 0);
            datasets.push(root_ds);
        }
        {
            let datasets = self.datasets.lock();
            datasets[0].init(0);
        }
        self.root_ds_id.store(0, Ordering::Release);
        self.current_dir.store(HV_DMU_OBJ_ROOT, Ordering::Release);
        self.mounted.store(true, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
        log("[HvFS] Initialized: pool=antx-pool\n");
    }

    pub fn format_disk(&self) {
        if !self.check_disk() {
            log("[HvFS] FORMAT: No disk present\n");
            self.spa.disk_present.store(false, Ordering::Release);
            self.spa.formatted.store(false, Ordering::Release);
            return;
        }
        log("[HvFS] FORMAT: Writing fresh HvFS v2 to disk...\n");
        self.spa.disk_present.store(true, Ordering::Release);
        let mut vdev_cfg = crate::kernel::fs::hvfs::vdev::HvVdevConfig::new_disk(0, "ata0", 12);
        let asize = crate::kernel::fs::hvfs::vdev::HvVdev::probe_disk_size(self.disk_drive);
        vdev_cfg.asize = asize;
        self.spa.add_vdev(vdev_cfg);
        self.spa.formatted.store(true, Ordering::Release);
        self.mode.store(HvfsMode::Disk as u8, Ordering::Release);
        self.spa.write_uberblock_to_disk();
        log("[HvFS] FORMAT: Complete (asize=");
        log(" bytes)\n");
    }

    pub fn mount_disk(&self) -> bool {
        if !self.check_disk() { return false; }
        log("[HvFS] MOUNT: Reading uberblock from disk...\n");
        let ub = match self.spa.read_uberblock_from_disk() {
            Some(u) => u,
            None => {
                log("[HvFS] MOUNT: No valid uberblock found\n");
                return false;
            }
        };
        log("[HvFS] MOUNT: Valid uberblock found (txg=");
        {
            let mut stored = self.spa.uberblock.lock();
            *stored = ub;
        }
        self.spa.txg_current.store(ub.txg, Ordering::Release);
        self.spa.formatted.store(true, Ordering::Release);
        self.spa.disk_present.store(true, Ordering::Release);
        self.mode.store(HvfsMode::Disk as u8, Ordering::Release);
        let mut vdev_cfg = crate::kernel::fs::hvfs::vdev::HvVdevConfig::new_disk(0, "ata0", 12);
        let asize = crate::kernel::fs::hvfs::vdev::HvVdev::probe_disk_size(self.disk_drive);
        vdev_cfg.asize = asize;
        self.spa.add_vdev(vdev_cfg);
        {
            let mut txg_guard = self.txg_group.lock();
            let mut txg_group = HvTxgGroup::new();
            txg_group.init(ub.txg);
            *txg_guard = Some(txg_group);
        }
        self.zil.init();
        {
            let mut datasets = self.datasets.lock();
            let root_ds = HvDataset::new(0, "root", 0);
            datasets.push(root_ds);
        }
        {
            let datasets = self.datasets.lock();
            datasets[0].init(0);
        }
        self.root_ds_id.store(0, Ordering::Release);
        self.current_dir.store(HV_DMU_OBJ_ROOT, Ordering::Release);
        self.mounted.store(true, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
        self.spa.state.store(HvPoolState::Active as u8, Ordering::Release);
        log("[HvFS] MOUNT: Ready (pool_guid=");
        log(")\n");
        true
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    pub fn is_disk_mode(&self) -> bool {
        self.mode.load(Ordering::Acquire) == HvfsMode::Disk as u8
    }

    fn alloc_fd(&self) -> Option<usize> {
        let mut fds = self.fds.lock();
        for i in 0..HVFS_MAX_FDS {
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
        if idx < HVFS_MAX_FDS {
            fds[idx].used = false;
            fds[idx].offset = 0;
        }
    }

    fn check_permission(&self, obj: &HvDmuObject, pwid: u64, cap: u64) -> bool {
        if pwid == 0 { return true; }
        let level = unsafe { pwid_get_privilege_level(pwid) };
        if level == 0xFF { return false; }
        if level == 0 { return true; }
        if obj.owner_pwid == pwid { return true; }
        unsafe { pwid_has_capability(pwid, 3, cap) }
    }

    pub fn open(&self, path: &str, flags: u32, pwid: u64) -> Result<i32, KernelError> {
        if !self.is_initialized() { return Err(KernelError::NotInitialized); }
        let name = path.trim_start_matches('/');
        let obj_id = {
            let datasets = self.datasets.lock();
            let ds = &datasets[0];
            match ds.lookup(name) {
                Some(id) => Some(id),
                None => {
                    if flags & 0x0100 != 0 { ds.create_file(name, pwid) } else { None }
                }
            }
        };
        let obj_id = match obj_id { Some(id) => id, None => return Err(KernelError::NotFound) };
        let obj = {
            let datasets = self.datasets.lock();
            datasets[0].objset.get_obj(obj_id)
        };
        let obj = match obj { Some(o) => o, None => return Err(KernelError::NotFound) };
        if !self.check_permission(&obj, pwid, 0x01) { return Err(KernelError::PermissionDenied); }
        let fd_idx = match self.alloc_fd() { Some(i) => i, None => return Err(KernelError::NoSpace) };
        {
            let mut fds = self.fds.lock();
            fds[fd_idx].obj_id = obj_id;
            fds[fd_idx].ds_id = self.root_ds_id.load(Ordering::Acquire);
            fds[fd_idx].offset = if flags & 0x0400 != 0 { obj.size } else { 0 };
            fds[fd_idx].flags = flags;
            fds[fd_idx].pwid = pwid;
        }
        self.zil.add_record(HvZilRecord::new_create(0, 0, name));
        Ok(fd_idx as i32)
    }

    pub fn close(&self, fd: u32) -> i32 {
        let idx = fd as usize;
        if idx >= HVFS_MAX_FDS { return -1; }
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
            if idx >= HVFS_MAX_FDS || !fds[idx].used { return -1; }
            (fds[idx].obj_id, fds[idx].offset, fds[idx].pwid)
        };
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) { Some(o) => o, None => return -1 }
        };
        if !self.check_permission(&obj, pwid, 0x01) { return -3; }
        let available = if offset < obj.size { (obj.size - offset) as usize } else { 0 };
        let to_read = (count as usize).min(available).min(buf.len());
        if to_read == 0 { return 0; }
        let block_offset = (offset / 4096) * 4096;
        let block_key = HvArcKey::new(0, block_offset, obj.birth_txg);
        if let Some(data_ptr) = self.spa.arc.lookup(&block_key) {
            let data = unsafe { core::slice::from_raw_parts(data_ptr, 4096) };
            let start = (offset - block_offset) as usize;
            let end = (start + to_read).min(4096);
            buf[..end - start].copy_from_slice(&data[start..end]);
            self.spa.arc.release(&block_key);
        } else if self.is_disk_mode() && !obj.bp.is_null() {
            let mut disk_buf = vec![0u8; obj.bp.prop.physical_size as usize];
            if self.spa.read_bp(&obj.bp, &mut disk_buf) == 0 {
                let start = offset as usize;
                let end = (start + to_read).min(disk_buf.len());
                buf[..end - start].copy_from_slice(&disk_buf[start..end]);
                let arc_key = HvArcKey::new(0, 0, obj.birth_txg);
                self.spa.arc.insert(arc_key, &disk_buf, HvArcBufType::Data);
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
            if idx >= HVFS_MAX_FDS || !fds[idx].used { return -1; }
            (fds[idx].obj_id, fds[idx].offset, fds[idx].pwid, fds[idx].flags)
        };
        if flags & 0x0001 != 0 && flags & 0x0002 == 0 { return -1; }
        let mut obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) { Some(o) => o, None => return -1 }
        };
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        let to_write = (count as usize).min(buf.len());
        if to_write == 0 { return 0; }
        let txg = self.spa.current_txg();
        let cksum_type = HvCksumType::Fletcher4;
        let comp_type = HvCompType::Off;
        let compressed = compress::compress(&buf[..to_write], comp_type);
        let write_data = compressed.as_deref().unwrap_or(&buf[..to_write]);
        let new_bp = match self.spa.allocate(write_data.len() as u64, cksum_type, comp_type, txg) {
            Some(bp) => bp, None => return -1,
        };
        if self.is_disk_mode() {
            if self.spa.write_bp(&new_bp, write_data) != 0 {
                self.spa.free(&new_bp, txg);
                return -1;
            }
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
        self.zil.add_record(HvZilRecord::new_write(txg, obj_id, offset, to_write as u32));
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
                self.zil.add_record(HvZilRecord::new_mkdir(txg, 0, name));
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
            match datasets[0].lookup(name) { Some(id) => Some(id), None => None }
        };
        let obj_id = match obj_id { Some(id) => id, None => return -2 };
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) { Some(o) => o, None => return -1 }
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
        self.zil.add_record(HvZilRecord::new_remove(txg, 0, name));
        0
    }

    pub fn stat(&self, path: &str, pwid: u64) -> Option<HvDmuObject> {
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
        {
            let ub = self.spa.uberblock.lock();
            let mut new_ub = *ub;
            new_ub.txg = txg;
            new_ub.timestamp = unsafe { timer_get_ticks() };
            drop(ub);
            {
                let mut stored = self.spa.uberblock.lock();
                *stored = new_ub;
            }
        }
        self.zil.sync(txg);
        if self.is_disk_mode() {
            self.spa.write_uberblock_to_disk();
        }
        {
            let mut txg_guard = self.txg_group.lock();
            if let Some(ref mut txg_group) = *txg_guard {
                txg_group.transition();
            }
        }
        let dirty_count = self.spa.arc.flush_dirty();
        log("[HvFS] Sync complete (txg=");
        log(", dirty=");
        log(")\n");
        let _ = dirty_count;
        0
    }

    pub fn snapshot_create(&self, name: &str) -> i32 {
        if !self.is_initialized() { return -1; }
        let datasets = self.datasets.lock();
        let ds = &datasets[0];
        let txg = self.spa.current_txg();
        match self.snap_mgr.create_snapshot(ds, name, txg) {
            Some(id) => id as i32, None => -1,
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
        let ds_id = { self.datasets.lock().len() as u64 };
        let txg = self.spa.current_txg();
        match self.snap_mgr.create_clone(snap_id, ds_id, name, txg) {
            Some(ds) => {
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
            if idx >= HVFS_MAX_FDS || !fds[idx].used { return -1; }
            (fds[idx].obj_id, fds[idx].offset)
        };
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) { Some(o) => o, None => return -1 }
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
