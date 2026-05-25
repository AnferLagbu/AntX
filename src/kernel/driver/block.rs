//! Block Device Abstraction Layer
//!
//! Provides a unified `BlockDevice` trait implemented by all storage drivers
//! (ATA, AHCI, NVMe, virtio-blk), plus a global registry for device discovery.
//!
//! ## SMP Safety — 引用计数 + 移除屏障
//!
//! 多核环境下，锁顺序固定为 REGISTRY → REMOVING → IO_REFS → device，
//! 统一的三段协议防止 use-after-free:
//!
//! ```text
//! I/O 路径 (持 REGISTRY 锁):      safe_unregister:
//!   lock(REGISTRY)                   lock(REGISTRY)
//!   check idx in range               set REMOVING = true
//!   lock(REMOVING)                   unlock(REGISTRY)
//!   if REMOVING: return err          fence(SeqCst)
//!   lock(IO_REFS) refs++            wait io_refs[idx]==0
//!   unlock(IO_REFS)                  lock(REGISTRY)
//!   lock(device)                     remove device
//!   do I/O                           unlock(REGISTRY)
//!   unlock(device)                   cleanup names/refs/removing
//!   unlock(REGISTRY)
//!   lock(IO_REFS) refs--
//! ```
//!
//! 关键不变量: REGISTRY 锁始终是外层锁，避免了 ABBA 死锁。

use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering, fence};

// ── BlockDevice Trait ──

pub trait BlockDevice: Send + Sync {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32;
    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32;
    fn blk_is_present(&self) -> bool;
    fn blk_total_sectors(&self) -> u64;
}

// ── Global Registry ──

static REGISTRY: Mutex<Vec<Mutex<Box<dyn BlockDevice>>>> = Mutex::new(Vec::new());
static DEVICE_NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static IO_REFS: Mutex<Vec<AtomicU32>> = Mutex::new(Vec::new());
static REMOVING: Mutex<Vec<AtomicBool>> = Mutex::new(Vec::new());

pub fn register_named(name: &'static str, dev: Box<dyn BlockDevice>) -> usize {
    let mut list = REGISTRY.lock();
    let idx = list.len();
    list.push(Mutex::new(dev));
    drop(list);
    DEVICE_NAMES.lock().push(name);
    IO_REFS.lock().push(AtomicU32::new(0));
    REMOVING.lock().push(AtomicBool::new(false));
    idx
}

pub fn register(dev: Box<dyn BlockDevice>) -> usize {
    register_named("unknown", dev)
}

pub fn with_device<R>(idx: usize, f: impl FnOnce(&mut dyn BlockDevice) -> R) -> Option<R> {
    let reg = REGISTRY.lock();
    if idx >= reg.len() { return None; }
    let mut dev = reg[idx].lock();
    Some(f(&mut **dev))
}

/// SMP-safe 设备移除。
///
/// 固定锁顺序: REGISTRY → (无锁自旋等待 refs) → REGISTRY。
/// 第一阶段在 REGISTRY 锁内设置 REMOVING 阻止新 I/O，
/// 然后释放锁等待飞行中的 I/O 完成。
pub fn safe_unregister(idx: usize) -> Option<Box<dyn BlockDevice>> {
    // 阶段 1: 持有 REGISTRY 锁, 设置 REMOVING
    {
        let reg = REGISTRY.lock();
        if idx >= reg.len() { return None; }
        let removing = REMOVING.lock();
        if idx < removing.len() {
            removing[idx].store(true, Ordering::Release);
        }
    }
    fence(Ordering::SeqCst);

    // 阶段 2: 等待飞行 I/O 完成 (自旋, 仅短暂持有 IO_REFS 用于读取)
    loop {
        let refs = IO_REFS.lock();
        let current = if idx < refs.len() { refs[idx].load(Ordering::Acquire) } else { 0 };
        drop(refs);
        if current == 0 { break; }
        core::hint::spin_loop();
    }

    // 阶段 3: 再次持有 REGISTRY 锁, 移除设备
    let removed = {
        let mut reg = REGISTRY.lock();
        if idx >= reg.len() { return None; }
        reg.remove(idx).into_inner()
    };

    // 清理并行数组
    {
        let mut names = DEVICE_NAMES.lock();
        if idx < names.len() { names.remove(idx); }
    }
    {
        let mut refs = IO_REFS.lock();
        if idx < refs.len() { refs.remove(idx); }
    }
    {
        let mut removing = REMOVING.lock();
        if idx < removing.len() { removing.remove(idx); }
    }

    Some(removed)
}

pub fn is_removing(idx: usize) -> bool {
    let removing = REMOVING.lock();
    if idx >= removing.len() { return true; }
    removing[idx].load(Ordering::Acquire)
}

pub fn io_refcount(idx: usize) -> u32 {
    let refs = IO_REFS.lock();
    if idx >= refs.len() { return 0; }
    refs[idx].load(Ordering::Acquire)
}

pub fn unregister(idx: usize) -> Option<Box<dyn BlockDevice>> {
    safe_unregister(idx)
}

pub fn mark_removed(idx: usize) {
    let reg = REGISTRY.lock();
    if idx >= reg.len() { return; }
    let removing = REMOVING.lock();
    if idx < removing.len() {
        removing[idx].store(true, Ordering::Release);
    }
    fence(Ordering::SeqCst);
}

pub fn registry() -> &'static Mutex<Vec<Mutex<Box<dyn BlockDevice>>>> {
    &REGISTRY
}

pub fn count() -> usize {
    REGISTRY.lock().len()
}

// ── Multi-sector helper ──

pub fn read_sectors(dev: &mut dyn BlockDevice, start: u64, count: u32, buf: &mut [u8]) -> i32 {
    let need = (count as u64) * 512;
    if (buf.len() as u64) < need { return -1; }
    let mut offset = 0usize;
    for i in 0..count {
        if dev.blk_read(start + i as u64, &mut buf[offset..offset + 512]) < 0 {
            return -1;
        }
        offset += 512;
    }
    0
}

pub fn write_sectors(dev: &mut dyn BlockDevice, start: u64, count: u32, buf: &[u8]) -> i32 {
    let need = (count as u64) * 512;
    if (buf.len() as u64) < need { return -1; }
    let mut offset = 0usize;
    for i in 0..count {
        if dev.blk_write(start + i as u64, &buf[offset..offset + 512]) < 0 {
            return -1;
        }
        offset += 512;
    }
    0
}

// ── HvFS bridge ──
//
// 锁顺序: REGISTRY → REMOVING → IO_REFS → device，与 safe_unregister 一致。

pub fn hdd_read_sector(drive: u8, sector: u64, buf: &mut [u8]) -> i32 {
    if buf.len() < 512 { return -1; }
    let reg = REGISTRY.lock();
    let idx = drive as usize;
    if idx >= reg.len() { return -1; }

    let removing = REMOVING.lock();
    if idx < removing.len() && removing[idx].load(Ordering::Acquire) {
        return -1;
    }
    drop(removing);

    let refs = IO_REFS.lock();
    if idx < refs.len() { refs[idx].fetch_add(1, Ordering::Acquire); }
    drop(refs);

    let mut dev = reg[idx].lock();
    let result = dev.blk_read(sector, buf);
    drop(dev);
    drop(reg);

    let refs = IO_REFS.lock();
    if idx < refs.len() { refs[idx].fetch_sub(1, Ordering::Release); }
    result
}

pub fn hdd_write_sector(drive: u8, sector: u64, buf: &[u8]) -> i32 {
    if buf.len() < 512 { return -1; }
    let reg = REGISTRY.lock();
    let idx = drive as usize;
    if idx >= reg.len() { return -1; }

    let removing = REMOVING.lock();
    if idx < removing.len() && removing[idx].load(Ordering::Acquire) {
        return -1;
    }
    drop(removing);

    let refs = IO_REFS.lock();
    if idx < refs.len() { refs[idx].fetch_add(1, Ordering::Acquire); }
    drop(refs);

    let mut dev = reg[idx].lock();
    let result = dev.blk_write(sector, buf);
    drop(dev);
    drop(reg);

    let refs = IO_REFS.lock();
    if idx < refs.len() { refs[idx].fetch_sub(1, Ordering::Release); }
    result
}

pub fn hdd_is_present(drive: u8) -> bool {
    let reg = REGISTRY.lock();
    let idx = drive as usize;
    if idx >= reg.len() { return false; }
    let removing = REMOVING.lock();
    if idx < removing.len() && removing[idx].load(Ordering::Acquire) { return false; }
    drop(removing);
    let dev = reg[idx].lock();
    dev.blk_is_present()
}

pub fn hdd_total_sectors(drive: u8) -> u64 {
    let reg = REGISTRY.lock();
    let idx = drive as usize;
    if idx >= reg.len() { return 0; }
    let dev = reg[idx].lock();
    dev.blk_total_sectors()
}

pub fn block_device_name(drive: u8) -> Option<&'static str> {
    let names = DEVICE_NAMES.lock();
    if (drive as usize) >= names.len() { return None; }
    Some(names[drive as usize])
}

pub fn block_device_info(drive: u8) -> (&'static str, bool, u64) {
    let mut cfg = [0u8; 512];
    let is_present = hdd_is_present(drive);
    let total_sectors = hdd_total_sectors(drive);
    let has_antx = if is_present && hdd_read_sector(drive, 2046, &mut cfg) >= 0 {
        cfg[0] == b'A' && cfg[1] == b'N' && cfg[2] == b'T' && cfg[3] == b'X'
    } else { false };
    let name = block_device_name(drive).unwrap_or("unknown");
    (name, has_antx, total_sectors)
}

pub fn block_device_count() -> usize { count() }

pub fn block_device_list() -> Vec<(usize, &'static str, u64)> {
    let reg = REGISTRY.lock();
    let names = DEVICE_NAMES.lock();
    reg.iter().enumerate().map(|(i, dev_lock)| {
        let dev = dev_lock.lock();
        let name = names.get(i).copied().unwrap_or("unknown");
        (i, name, dev.blk_total_sectors())
    }).collect()
}

pub fn block_device_state(drive: u8) -> (bool, bool, u32) {
    let idx = drive as usize;
    let present = hdd_is_present(drive);
    let removing = is_removing(idx);
    let io_count = io_refcount(idx);
    (present, removing, io_count)
}
