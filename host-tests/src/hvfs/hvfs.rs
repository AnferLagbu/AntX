#![allow(unused_variables, unused_assignments)]

use std::boxed::Box;
use std::vec::Vec;
use std::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use crate::kernel::sync::mutex::Mutex;
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

pub const HVFS_MAX_FDS: usize = 256;

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
    pub partition_start: AtomicU32,
}

unsafe impl Send for HvfsData {}
unsafe impl Sync for HvfsData {}

pub static HVFS_DATA: Mutex<Option<Box<HvfsData>>> = Mutex::new(None);

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
            partition_start: AtomicU32::new(0),
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

    pub fn init(&self) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }
        log("[HvFS] Initializing...\n");
        self.spa.init("antx-pool");
        let has_disk = self.check_disk();
        self.spa.disk_present.store(has_disk, Ordering::Release);
        if has_disk {
            log("[HvFS] Disk detected, mounting from disk...\n");
            if self.mount_disk() {
                log("[HvFS] Initialized: pool=antx-pool (disk)\n");
                return;
            }
            log("[HvFS] Mount from disk failed, formatting and starting fresh...\n");
            self.format_disk();
            // format_disk 已设置 mode=Disk 并添加 vdev，初始化上层结构
        } else {
            log("[HvFS] No disk, running in memory mode\n");
            self.spa.add_vdev(crate::kernel::fs::hvfs::vdev::HvVdevConfig::new_disk(0, "ata0", 12));
        }
        self.setup_zil_datasets();
        self.root_ds_id.store(0, Ordering::Release);
        self.current_dir.store(HV_DMU_OBJ_ROOT, Ordering::Release);
        self.mounted.store(true, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
        if !self.is_disk_mode() {
            self.mode.store(HvfsMode::Memory as u8, Ordering::Release);
        }
        log("[HvFS] Initialized: pool=antx-pool (memory)\n");
    }

    fn setup_zil_datasets(&self) {
        {
            let mut txg_guard = self.txg_group.lock();
            let mut txg_group = HvTxgGroup::new();
            txg_group.init(1);
            *txg_guard = Some(txg_group);
        }
        self.zil.init();
        let mut has_persisted = false;
        {
            let mut datasets = self.datasets.lock();
            let root_ds = HvDataset::new(0, "root", 0);
            datasets.push(root_ds);
        }
        {
            let datasets = self.datasets.lock();
            let ub_copy = *self.spa.uberblock.lock();
            if !ub_copy.root_bp.is_null() {
                if self.deserialize_dataset_metadata(&ub_copy.root_bp) {
                    has_persisted = true;
                }
            }
            if !has_persisted {
                datasets[0].init(0);
            }
        }
    }

    pub fn format_disk(&self) {
        if !self.check_disk() {
            log("[HvFS] FORMAT: No disk present\n");
            self.spa.disk_present.store(false, Ordering::Release);
            self.spa.formatted.store(false, Ordering::Release);
            return;
        }
        // 读取 ANTX 配置扇区获取 HvFS 分区起始 LBA
        let part_start = self.read_partition_start();
        self.partition_start.store(part_start, Ordering::Release);
        self.spa.partition_start.store(part_start, Ordering::Release);
        log("[HvFS] FORMAT: Writing fresh HvFS v2 to disk (partition @LBA ");
        log(")...\n");
        self.spa.disk_present.store(true, Ordering::Release);
        let mut vdev_cfg = crate::kernel::fs::hvfs::vdev::HvVdevConfig::new_disk(0, "ata0", 12);
        vdev_cfg.asize = self.probe_partition_size(part_start);
        vdev_cfg.partition_start = part_start;
        self.spa.add_vdev(vdev_cfg);
        self.spa.formatted.store(true, Ordering::Release);
        self.mode.store(HvfsMode::Disk as u8, Ordering::Release);
        self.spa.write_uberblock_to_disk();
        log("[HvFS] FORMAT: Complete\n");
    }

    fn read_partition_start(&self) -> u32 {
        extern "C" {
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }
        let mut cfg = [0u8; 512];
        let r = unsafe { ata_read_sector(self.disk_drive, 2046, cfg.as_mut_ptr()) };
        if r >= 0 && cfg[0] == b'A' && cfg[1] == b'N' && cfg[2] == b'T' && cfg[3] == b'X' {
            u32::from_le_bytes([cfg[4], cfg[5], cfg[6], cfg[7]])
        } else {
            // 回退: 使用默认 BOOT_PART_SECTORS
            16384
        }
    }

    fn probe_partition_size(&self, part_start: u32) -> u64 {
        extern "C" {
            fn ata_disk_present(disk: u8) -> i32;
            fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
        }
        if unsafe { ata_disk_present(self.disk_drive) } == 0 { return 0; }
        let mut lo: u32 = part_start;
        let mut hi: u32 = 0xFFFF;
        let mut buf = [0u8; 512];
        let mut last_ok = lo;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if unsafe { ata_read_sector(self.disk_drive, mid, buf.as_mut_ptr()) } >= 0 {
                last_ok = mid;
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if last_ok > part_start {
            (last_ok as u64 - part_start as u64) * 512
        } else {
            crate::kernel::fs::hvfs::vdev::HvVdev::probe_disk_size(self.disk_drive)
        }
    }

    pub fn mount_disk(&self) -> bool {
        if !self.check_disk() { return false; }
        // 读取 ANTX 配置获取分区起始 LBA
        let part_start = self.read_partition_start();
        self.partition_start.store(part_start, Ordering::Release);
        self.spa.partition_start.store(part_start, Ordering::Release);
        log("[HvFS] MOUNT: Reading uberblock from disk (partition @LBA ");
        log(")...\n");
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
        vdev_cfg.asize = self.probe_partition_size(part_start);
        vdev_cfg.partition_start = part_start;
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
            let root_bp = { self.spa.uberblock.lock().root_bp };
            if !root_bp.is_null() {
                if self.deserialize_dataset_metadata(&root_bp) {
                    log("[HvFS] MOUNT: Restored dataset from uberblock\n");
                } else {
                    let datasets = self.datasets.lock();
                    datasets[0].init(0);
                }
            } else {
                let datasets = self.datasets.lock();
                datasets[0].init(0);
            }
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
        if pwid == 0 { return false; }
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
            fds[fd_idx].offset = 0;
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
        let block_key = HvArcKey::new(0, (obj_id << 40) | block_offset, obj.birth_txg);
        let mut bytes_read: usize = 0;
        if let Some(data_ptr) = self.spa.arc.lookup(&block_key) {
            let data = unsafe { core::slice::from_raw_parts(data_ptr, 4096) };
            let start = (offset - block_offset) as usize;
            let end = (start + to_read).min(4096);
            buf[..end - start].copy_from_slice(&data[start..end]);
            self.spa.arc.release(&block_key);
            bytes_read = end - start;
        } else if self.is_disk_mode() && !obj.bp.is_null() {
            let mut disk_buf = vec![0u8; obj.bp.prop.physical_size as usize];
            if self.spa.read_bp(&obj.bp, &mut disk_buf) == 0 {
                let start = offset as usize;
                let end = (start + to_read).min(disk_buf.len());
                buf[..end - start].copy_from_slice(&disk_buf[start..end]);
                let arc_key = HvArcKey::new(0, (obj_id << 40) | block_offset, obj.birth_txg);
                self.spa.arc.insert(arc_key, &disk_buf, HvArcBufType::Data);
                bytes_read = end - start;
            }
        }
        {
            let mut fds = self.fds.lock();
            fds[fd as usize].offset += bytes_read as u64;
        }
        bytes_read as i32
    }

    pub fn write(&self, fd: u32, buf: &[u8], count: u32) -> i32 {
        let (obj_id, mut offset, pwid, flags) = {
            let fds = self.fds.lock();
            let idx = fd as usize;
            if idx >= HVFS_MAX_FDS || !fds[idx].used { return -1; }
            (fds[idx].obj_id, fds[idx].offset, fds[idx].pwid, fds[idx].flags)
        };
        let to_write = (count as usize).min(buf.len());
        if to_write == 0 { return 0; }
        if flags & 0x0001 != 0 && flags & 0x0002 == 0 { return -1; }
        let mut obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) { Some(o) => o, None => return -1 }
        };
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        if flags & 0x0400 != 0 {
            offset = obj.size;
            let mut fds = self.fds.lock();
            fds[fd as usize].offset = offset;
        }
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
        if !self.is_disk_mode() {
            let block_offset = (offset / 4096) * 4096;
            let arc_key = HvArcKey::new(0, (obj_id << 40) | block_offset, txg);
            let mut block_buf = [0u8; 4096];
            let start = (offset - block_offset) as usize;
            if start + to_write <= 4096 {
                block_buf[start..start + to_write].copy_from_slice(&buf[..to_write]);
            }
            self.spa.arc.insert(arc_key, &block_buf, HvArcBufType::Data);
        }
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

    pub fn chmod(&self, path: &str, mode: u16, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let name = path.trim_start_matches('/');
        
        let mut datasets = self.datasets.lock();
        let ds = &mut datasets[0];
        let obj_id = match ds.lookup(name) {
            Some(id) => id,
            None => return -1,
        };
        
        let mut obj = match ds.objset.get_obj(obj_id) {
            Some(o) => o,
            None => return -1,
        };
        
        // Permission check: only owner or privileged user can change permissions
        if obj.owner_pwid != pwid {
            let level = unsafe { pwid_get_privilege_level(pwid) };
            if level != 0 {
                return -1;
            }
        }
        
        obj.pwid_perm = mode;
        obj.ctime = unsafe { timer_get_ticks() };
        obj.dirty = true;
        
        if ds.objset.update_obj(&obj) {
            return 0;
        }
        
        -1
    }

    pub fn chown(&self, path: &str, owner_pwid: u64, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let name = path.trim_start_matches('/');
        
        let mut datasets = self.datasets.lock();
        let ds = &mut datasets[0];
        let obj_id = match ds.lookup(name) {
            Some(id) => id,
            None => return -1,
        };
        
        let mut obj = match ds.objset.get_obj(obj_id) {
            Some(o) => o,
            None => return -1,
        };
        
        let level = unsafe { pwid_get_privilege_level(pwid) };
        if level != 0 && obj.owner_pwid != pwid {
            return -1;
        }
        
        obj.owner_pwid = owner_pwid;
        obj.ctime = unsafe { timer_get_ticks() };
        obj.dirty = true;
        
        if ds.objset.update_obj(&obj) {
            return 0;
        }
        
        -1
    }

    pub fn rename(&self, old_path: &str, new_path: &str, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let old_name = old_path.trim_start_matches('/');
        let new_name = new_path.trim_start_matches('/');
        
        // 查找源文件
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(old_name) {
                Some(id) => id,
                None => return -2,
            }
        };
        
        // 检查权限
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        
        // 检查目标是否已存在
        {
            let datasets = self.datasets.lock();
            if datasets[0].lookup(new_name).is_some() {
                return -4; // 目标已存在
            }
        }
        
        // 执行重命名
        {
            let datasets = self.datasets.lock();
            let ds = &datasets[0];
            ds.dir_zap.remove(old_name);
            ds.dir_zap.insert_u64(new_name, obj_id);
        }
        
        // 记录到ZIL
        let txg = self.spa.current_txg();
        self.zil.add_record(HvZilRecord::new_rename(txg, 0, old_name, new_name));
        
        0
    }

    pub fn symlink(&self, target: &str, linkpath: &str, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let link_name = linkpath.trim_start_matches('/');
        
        // 检查链接路径是否已存在
        {
            let datasets = self.datasets.lock();
            if datasets[0].lookup(link_name).is_some() {
                return -2; // 已存在
            }
        }
        
        // 创建符号链接对象
        let obj_id = {
            let mut datasets = self.datasets.lock();
            let ds = &mut datasets[0];
            
            // 分配新对象
            match ds.objset.alloc_obj(HvObjType::Symlink, pwid) {
                Some(id) => id,
                None => return -1,
            }
        };
        
        // 设置目标路径
        {
            let mut datasets = self.datasets.lock();
            let ds = &mut datasets[0];
            
            if let Some(mut obj) = ds.objset.get_obj_mut(obj_id) {
                obj.obj_type = HvObjType::Symlink;
                obj.size = target.len() as u64;
                obj.dirty = true;
                ds.objset.update_obj(&obj);
            }
            
            let target_bytes = target.as_bytes();
            let txg = self.spa.current_txg();
            let cksum_type = HvCksumType::Fletcher4;
            let comp_type = HvCompType::Off;
            
            if let Some(new_bp) = self.spa.allocate(target_bytes.len() as u64, cksum_type, comp_type, txg) {
                if let Some(mut obj) = ds.objset.get_obj_mut(obj_id) {
                    obj.bp = new_bp;
                    ds.objset.update_obj(&obj);
                }
            }
            
            // 添加到目录
            if !ds.link(link_name, obj_id) {
                return -1;
            }
        }
        
        // 记录到ZIL
        let txg = self.spa.current_txg();
        self.zil.add_record(HvZilRecord::new_symlink(txg, 0, link_name, target));
        
        0
    }

    pub fn link(&self, old_path: &str, new_path: &str, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let old_name = old_path.trim_start_matches('/');
        let new_name = new_path.trim_start_matches('/');
        
        // 查找源文件
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(old_name) {
                Some(id) => id,
                None => return -2,
            }
        };
        
        // 检查源文件类型（不能是目录）
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        if obj.obj_type == HvObjType::Dir {
            return -3; // 不能创建目录的硬链接
        }
        
        // 检查权限
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        
        // 检查目标是否已存在
        {
            let datasets = self.datasets.lock();
            if datasets[0].lookup(new_name).is_some() {
                return -4; // 目标已存在
            }
        }
        
        // 创建硬链接
        {
            let mut datasets = self.datasets.lock();
            let ds = &mut datasets[0];
            
            // 增加链接计数
            if let Some(mut obj) = ds.objset.get_obj_mut(obj_id) {
                obj.link_count += 1;
                obj.dirty = true;
                ds.objset.update_obj(&obj);
            }
            
            // 添加到目录
            if !ds.link(new_name, obj_id) {
                return -1;
            }
        }
        
        // 记录到ZIL
        let txg = self.spa.current_txg();
        self.zil.add_record(HvZilRecord::new_link(txg, 0, new_name, obj_id));
        
        0
    }

    pub fn readlink(&self, path: &str, buf: &mut [u8], pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let name = path.trim_start_matches('/');
        
        // 查找符号链接
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(name) {
                Some(id) => id,
                None => return -2,
            }
        };
        
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        
        // 检查是否为符号链接
        if obj.obj_type != HvObjType::Symlink {
            return -3; // 不是符号链接
        }
        
        // 检查权限
        if !self.check_permission(&obj, pwid, 0x01) { return -3; }
        
        // 读取目标路径
        if obj.bp.is_null() {
            return 0;
        }
        
        let target_len = obj.size as usize;
        let to_read = target_len.min(buf.len());
        
        // 从ARC读取数据
        let block_key = HvArcKey::new(0, 0, obj.birth_txg);
        if let Some(data_ptr) = self.spa.arc.lookup(&block_key) {
            let data = unsafe { core::slice::from_raw_parts(data_ptr, target_len) };
            buf[..to_read].copy_from_slice(&data[..to_read]);
            return to_read as i32;
        }
        
        -1
    }

    pub fn setxattr(&self, path: &str, name: &str, value: &[u8], pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let obj_name = path.trim_start_matches('/');
        
        // 查找对象
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(obj_name) {
                Some(id) => id,
                None => return -2,
            }
        };
        
        // 检查权限
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        
        // 设置扩展属性
        {
            let mut datasets = self.datasets.lock();
            let ds = &mut datasets[0];
            
            if let Some(mut obj) = ds.objset.get_obj_mut(obj_id) {
                let name_hash = Self::hash_xattr_name(name);
                if name_hash < 4 {
                    let mut hash = [0u64; 4];
                    hash.copy_from_slice(&obj.data_hash);
                    hash[name_hash] = Self::hash_xattr_value(value);
                    obj.data_hash = hash;
                    obj.dirty = true;
                    ds.objset.update_obj(&obj);
                    return 0;
                }
            }
        }
        
        -1
    }

    pub fn getxattr(&self, path: &str, name: &str, buf: &mut [u8], pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let obj_name = path.trim_start_matches('/');
        
        // 查找对象
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(obj_name) {
                Some(id) => id,
                None => return -2,
            }
        };
        
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        
        // 检查权限
        if !self.check_permission(&obj, pwid, 0x01) { return -3; }
        
        // 获取扩展属性
        let name_hash = Self::hash_xattr_name(name);
        if name_hash < 4 {
            let value_hash = obj.data_hash[name_hash];
            if value_hash != 0 {
                // 简单实现：返回哈希值作为数据
                let hash_bytes = value_hash.to_le_bytes();
                let to_copy = hash_bytes.len().min(buf.len());
                buf[..to_copy].copy_from_slice(&hash_bytes[..to_copy]);
                return to_copy as i32;
            }
        }
        
        -1
    }

    pub fn listxattr(&self, path: &str, buf: &mut [u8], pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let obj_name = path.trim_start_matches('/');
        
        // 查找对象
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(obj_name) {
                Some(id) => id,
                None => return -2,
            }
        };
        
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        
        // 检查权限
        if !self.check_permission(&obj, pwid, 0x01) { return -3; }
        
        // 列出扩展属性
        let mut offset = 0;
        for i in 0..4 {
            if obj.data_hash[i] != 0 {
                let attr_name = format!("user.attr{}\0", i);
                let name_bytes = attr_name.as_bytes();
                if offset + name_bytes.len() <= buf.len() {
                    buf[offset..offset+name_bytes.len()].copy_from_slice(name_bytes);
                    offset += name_bytes.len();
                }
            }
        }
        
        offset as i32
    }

    pub fn removexattr(&self, path: &str, name: &str, pwid: u64) -> i32 {
        if !self.is_initialized() { return -1; }
        let obj_name = path.trim_start_matches('/');
        
        // 查找对象
        let obj_id = {
            let datasets = self.datasets.lock();
            match datasets[0].lookup(obj_name) {
                Some(id) => id,
                None => return -2,
            }
        };
        
        // 检查权限
        let obj = {
            let datasets = self.datasets.lock();
            match datasets[0].objset.get_obj(obj_id) {
                Some(o) => o,
                None => return -1,
            }
        };
        if !self.check_permission(&obj, pwid, 0x02) { return -3; }
        
        // 删除扩展属性
        {
            let mut datasets = self.datasets.lock();
            let ds = &mut datasets[0];
            
            if let Some(mut obj) = ds.objset.get_obj_mut(obj_id) {
                let name_hash = Self::hash_xattr_name(name);
                if name_hash < 4 {
                    let mut hash = [0u64; 4];
                    hash.copy_from_slice(&obj.data_hash);
                    hash[name_hash] = 0;
                    obj.data_hash = hash;
                    obj.dirty = true;
                    ds.objset.update_obj(&obj);
                    return 0;
                }
            }
        }
        
        -1
    }

    fn hash_xattr_name(name: &str) -> usize {
        let mut hash: u64 = 5381;
        for byte in name.bytes() {
            hash = ((hash << 5).wrapping_add(hash)).wrapping_add(byte as u64);
        }
        (hash % 4) as usize
    }

    fn hash_xattr_value(value: &[u8]) -> u64 {
        let mut hash: u64 = 5381;
        for byte in value {
            hash = ((hash << 5).wrapping_add(hash)).wrapping_add(*byte as u64);
        }
        hash
    }

    pub fn sync(&self) -> i32 {
        if !self.is_initialized() { return -1; }
        let txg = self.spa.advance_txg();
        let meta_bp = self.serialize_dataset_metadata(txg);
        {
            let mut ub = self.spa.uberblock.lock();
            ub.txg = txg;
            ub.timestamp = unsafe { timer_get_ticks() };
            if let Some(bp) = meta_bp {
                ub.root_bp = bp;
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
        self.spa.arc.flush_dirty();
        0
    }

    fn serialize_dataset_metadata(&self, txg: u64) -> Option<HvBlockPointer> {
        const BP_BYTES: usize = 128;
        let (objects, dir_entries, next_id) = {
            let datasets = self.datasets.lock();
            let ds = &datasets[0];
            let objs = ds.objset.objects.lock();
            let obj_clones: Vec<HvDmuObject> = objs.iter().filter(|o| o.used).cloned().collect();
            let obj_count = obj_clones.len();
            let dir_list = ds.dir_zap.entries();
            let next = ds.objset.next_obj_id.load(Ordering::Acquire);
            (obj_clones, dir_list, next)
        };

        let mut total = 32u32;
        for _ in &objects { total += 222; }
        for (name, _) in &dir_entries {
            total += 2 + name.len() as u32 + 8;
        }

        let mut buf = vec![0u8; total as usize];
        buf[0] = b'H'; buf[1] = b'V'; buf[2] = b'M'; buf[3] = b'1';
        Self::write_le32(&mut buf, 4, 1);
        Self::write_le32(&mut buf, 8, objects.len() as u32);
        Self::write_le32(&mut buf, 12, dir_entries.len() as u32);
        Self::write_le64(&mut buf, 16, next_id);
        Self::write_le32(&mut buf, 24, total);

        let mut off = 32usize;
        for obj in &objects {
            Self::write_le64(&mut buf, off, obj.obj_id); off += 8;
            buf[off] = obj.obj_type as u8; off += 1;
            Self::write_le32(&mut buf, off, obj.block_size); off += 4;
            Self::write_le64(&mut buf, off, obj.nblocks); off += 8;
            Self::write_le64(&mut buf, off, obj.size); off += 8;
            let bp_bytes = unsafe {
                core::slice::from_raw_parts(&obj.bp as *const HvBlockPointer as *const u8, BP_BYTES)
            };
            buf[off..off+BP_BYTES].copy_from_slice(bp_bytes); off += BP_BYTES;
            Self::write_le64(&mut buf, off, obj.atime); off += 8;
            Self::write_le64(&mut buf, off, obj.mtime); off += 8;
            Self::write_le64(&mut buf, off, obj.ctime); off += 8;
            Self::write_le64(&mut buf, off, obj.owner_pwid); off += 8;
            Self::write_le64(&mut buf, off, obj.group_pwid); off += 8;
            buf[off] = obj.sensitivity; off += 1;
            Self::write_le16(&mut buf, off, obj.pwid_perm); off += 2;
            Self::write_le32(&mut buf, off, obj.link_count); off += 4;
            Self::write_le32(&mut buf, off, obj.flags); off += 4;
            Self::write_le64(&mut buf, off, obj.birth_txg); off += 8;
            buf[off] = if obj.used { 1 } else { 0 }; off += 1;
            off += 1;
        }

        for (name, _value) in &dir_entries {
            let name_bytes = name.as_bytes();
            Self::write_le16(&mut buf, off, name_bytes.len() as u16); off += 2;
            buf[off..off+name_bytes.len()].copy_from_slice(name_bytes); off += name_bytes.len();
            Self::write_le64(&mut buf, off, 0);
        }

        if !self.is_disk_mode() { return None; }
        let bp = self.spa.allocate(buf.len() as u64, HvCksumType::Fletcher4, HvCompType::Off, txg)?;
        if self.spa.write_bp(&bp, &buf) != 0 {
            self.spa.free(&bp, txg);
            return None;
        }
        Some(bp)
    }

    fn deserialize_dataset_metadata(&self, bp: &HvBlockPointer) -> bool {
        const BP_BYTES: usize = 128;
        if bp.is_null() || !self.is_disk_mode() { return false; }
        let mut buf = vec![0u8; bp.prop.physical_size as usize];
        if self.spa.read_bp(bp, &mut buf) != 0 { return false; }
        if buf.len() < 32 { return false; }
        if buf[0] != b'H' || buf[1] != b'V' || buf[2] != b'M' || buf[3] != b'1' { return false; }
        let obj_count = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
        let zap_count = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]) as usize;
        let next_obj_id = u64::from_le_bytes([buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23]]);

        let mut off = 32usize;
        {
            let ds = &self.datasets.lock()[0];
            let mut objs = ds.objset.objects.lock();
            objs.clear();
            for _ in 0..obj_count {
                if off + 222 > buf.len() { return false; }
                let obj_id = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]);
                off += 8;
                let obj_type = HvObjType::from_u8(buf[off]); off += 1;
                let _block_size = u32::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3]]); off += 4;
                let nblocks = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let size = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let mut bp = HvBlockPointer::null();
                let bp_slice = unsafe {
                    core::slice::from_raw_parts_mut(&mut bp as *mut HvBlockPointer as *mut u8, BP_BYTES)
                };
                bp_slice.copy_from_slice(&buf[off..off+BP_BYTES]); off += BP_BYTES;
                let atime = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let mtime = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let ctime = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let owner_pwid = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let group_pwid = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let sensitivity = buf[off]; off += 1;
                let pwid_perm = u16::from_le_bytes([buf[off], buf[off+1]]); off += 2;
                let link_count = u32::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3]]); off += 4;
                let flags = u32::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3]]); off += 4;
                let birth_txg = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                let used = buf[off] != 0; off += 1;
                off += 1;
                let obj = HvDmuObject {
                    obj_id, obj_type, block_size: 4096, nblocks, size, bp,
                    atime, mtime, ctime, owner_pwid, group_pwid,
                    sensitivity, pwid_perm, link_count, flags,
                    birth_txg, data_hash: [0; 4], fill: 0,
                    dirty: false, used,
                };
                objs.push(obj);
            }
            ds.objset.next_obj_id.store(next_obj_id, Ordering::Release);

            ds.dir_zap.clear();
            for _ in 0..zap_count {
                if off + 2 > buf.len() { return false; }
                let name_len = u16::from_le_bytes([buf[off], buf[off+1]]) as usize; off += 2;
                if off + name_len + 8 > buf.len() { return false; }
                let name = core::str::from_utf8(&buf[off..off+name_len]).unwrap_or("?");
                off += name_len;
                let obj_id = u64::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3], buf[off+4], buf[off+5], buf[off+6], buf[off+7]]); off += 8;
                ds.dir_zap.insert_u64(name, obj_id);
            }
        }
        true
    }

    fn write_le16(buf: &mut [u8], off: usize, v: u16) {
        let b = v.to_le_bytes();
        buf[off] = b[0]; buf[off+1] = b[1];
    }

    fn write_le32(buf: &mut [u8], off: usize, v: u32) {
        let b = v.to_le_bytes();
        buf[off] = b[0]; buf[off+1] = b[1]; buf[off+2] = b[2]; buf[off+3] = b[3];
    }

    fn write_le64(buf: &mut [u8], off: usize, v: u64) {
        let b = v.to_le_bytes();
        buf[off] = b[0]; buf[off+1] = b[1]; buf[off+2] = b[2]; buf[off+3] = b[3];
        buf[off+4] = b[4]; buf[off+5] = b[5]; buf[off+6] = b[6]; buf[off+7] = b[7];
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
