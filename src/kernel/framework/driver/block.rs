//! 块设备抽象层
//!
//! 提供统一的 `BlockDevice` trait, 由所有存储驱动 (ATA, AHCI, NVMe, virtio-blk) 实现.
//!
//! ## Chitin 统一架构
//!
//! Chitin 是唯一的设备驱动框架. 块设备通过 `proto_block::register_block_device`
//! 注册到 Chitin, HvFS 通过 `chitin_blk_read/write` 直接 I/O.
//!
//! `BlockDevice` trait 定义在 chitin (设备框架) 中, 本模块 re-export 保持兼容.
//! 本模块保留:
//! - `BlockDevice` re-export: 保持调用方路径兼容
//! - `hdd_*` 函数: 向后兼容的 Chitin 代理
//! - SMP 安全基础设施: 用于 `safe_unregister`
//!
//! `REGISTRY` 仅在 `safe_unregister` 需要时使用, 不再是 I/O 主路径.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{fence, AtomicBool, AtomicU32, Ordering};
use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::kernel::framework::fs::{KernelError, KernelResult};

// ── BlockDevice Trait (定义在 chitin, 此处 re-export) ──

pub use crate::kernel::framework::chitin::BlockDevice;

// ── SMP 安全基础设施 ──
//
// 保留 REGISTRY 用于 safe_unregister 的设备移除协议.
// I/O 主路径已迁移至 Chitin (chitin_blk_read/write).

static REGISTRY: Mutex<Vec<Option<Mutex<Box<dyn BlockDevice>>>>> = Mutex::new(Vec::new());
static DEVICE_NAMES: Mutex<Vec<Option<&'static str>>> = Mutex::new(Vec::new());
static IO_REFS: Mutex<Vec<AtomicU32>> = Mutex::new(Vec::new());
static REMOVING: Mutex<Vec<AtomicBool>> = Mutex::new(Vec::new());

pub fn register_named(name: &'static str, dev: Box<dyn BlockDevice>) -> usize {
    let mut list = REGISTRY.lock();
    let idx = list.len();
    list.push(Some(Mutex::new(dev)));
    drop(list);
    DEVICE_NAMES.lock().push(Some(name));
    IO_REFS.lock().push(AtomicU32::new(0));
    REMOVING.lock().push(AtomicBool::new(false));
    idx
}

pub fn register(dev: Box<dyn BlockDevice>) -> usize {
    register_named("unknown", dev)
}

pub fn with_device<R>(idx: usize, f: impl FnOnce(&mut dyn BlockDevice) -> R) -> Option<R> {
    let reg = REGISTRY.lock();
    if idx >= reg.len() {
        return None;
    }
    let slot = reg[idx].as_ref()?;
    let mut dev = slot.lock();
    Some(f(&mut **dev))
}

pub fn safe_unregister(idx: usize) -> Option<Box<dyn BlockDevice>> {
    {
        let reg = REGISTRY.lock();
        if idx >= reg.len() {
            return None;
        }
        if reg[idx].is_none() {
            return None;
        }
        let removing = REMOVING.lock();
        if idx < removing.len() {
            removing[idx].store(true, Ordering::Release);
        }
    }
    fence(Ordering::SeqCst);

    loop {
        let refs = IO_REFS.lock();
        let current = if idx < refs.len() {
            refs[idx].load(Ordering::Acquire)
        } else {
            0
        };
        drop(refs);
        if current == 0 {
            break;
        }
        core::hint::spin_loop();
    }

    let removed = {
        let mut reg = REGISTRY.lock();
        if idx >= reg.len() {
            return None;
        }
        match reg[idx].take() {
            // SAFETY: 设备注册表按槽位互斥访问, 取出时槽位已置 None, 无并发持有。
            Some(m) => unsafe { m.into_inner() },
            None => return None,
        }
    };

    {
        let mut names = DEVICE_NAMES.lock();
        if idx < names.len() {
            names[idx] = None;
        }
    }

    Some(removed)
}

pub fn is_removing(idx: usize) -> bool {
    let removing = REMOVING.lock();
    if idx >= removing.len() {
        return true;
    }
    removing[idx].load(Ordering::Acquire)
}

pub fn io_refcount(idx: usize) -> u32 {
    let refs = IO_REFS.lock();
    if idx >= refs.len() {
        return 0;
    }
    refs[idx].load(Ordering::Acquire)
}

pub fn unregister(idx: usize) -> Option<Box<dyn BlockDevice>> {
    safe_unregister(idx)
}

pub fn mark_removed(idx: usize) {
    let reg = REGISTRY.lock();
    if idx >= reg.len() {
        return;
    }
    let removing = REMOVING.lock();
    if idx < removing.len() {
        removing[idx].store(true, Ordering::Release);
    }
    fence(Ordering::SeqCst);
}

pub fn registry() -> &'static Mutex<Vec<Option<Mutex<Box<dyn BlockDevice>>>>> {
    &REGISTRY
}

pub fn count() -> usize {
    REGISTRY.lock().len()
}

// ── 多扇区辅助函数 ──

// I-20: read_sectors / write_sectors 改用 KernelResult<()>, 替代 C 风格 `return -1`.
// 现在错误类型与项目其余模块统一 (KernelError::InvalidArgument / IoError),
// 调用方能用 `?` 传播或 `.err()` 分类处理, 不再丢失错误上下文.
// 由于 Chitin 是 I/O 主路径, 这两个函数当前没有调用者; 保留以便上层
// `with_device` 风格的同步读写场景 (非中断上下文).

pub fn read_sectors(dev: &mut dyn BlockDevice, start: u64, count: u32, buf: &mut [u8]) -> KernelResult<()> {
    let need = (count as u64) * 512;
    if (buf.len() as u64) < need {
        return Err(KernelError::InvalidArgument);
    }
    let mut offset = 0usize;
    for i in 0..count {
        match dev.blk_read(start + i as u64, &mut buf[offset..offset + 512]) {
            n if n >= 0 => offset += 512,
            _ => return Err(KernelError::IoError),
        }
    }
    Ok(())
}

pub fn write_sectors(dev: &mut dyn BlockDevice, start: u64, count: u32, buf: &[u8]) -> KernelResult<()> {
    let need = (count as u64) * 512;
    if (buf.len() as u64) < need {
        return Err(KernelError::InvalidArgument);
    }
    let mut offset = 0usize;
    for i in 0..count {
        match dev.blk_write(start + i as u64, &buf[offset..offset + 512]) {
            n if n >= 0 => offset += 512,
            _ => return Err(KernelError::IoError),
        }
    }
    Ok(())
}

// ── HvFS bridge (Chitin 代理) ──
//
// 所有 hdd_* 函数现在委托给 Chitin 统一 I/O 路径。
// 这确保 HvFS 的所有块设备访问都经过 Chitin。

pub fn hdd_read_sector(drive: u8, sector: u64, buf: &mut [u8]) -> i32 {
    crate::kernel::framework::chitin::chitin_blk_read(drive, sector, buf)
}

pub fn hdd_write_sector(drive: u8, sector: u64, buf: &[u8]) -> i32 {
    crate::kernel::framework::chitin::chitin_blk_write(drive, sector, buf)
}

pub fn hdd_is_present(drive: u8) -> bool {
    crate::kernel::framework::chitin::chitin_blk_is_present(drive)
}

pub fn hdd_total_sectors(drive: u8) -> u64 {
    crate::kernel::framework::chitin::chitin_blk_total_sectors(drive)
}

pub fn block_device_name(drive: u8) -> Option<&'static str> {
    crate::kernel::framework::chitin::chitin_blk_name(drive)
}

pub fn block_device_info(drive: u8) -> (&'static str, bool, u64) {
    crate::kernel::framework::chitin::chitin_blk_info(drive)
}

pub fn block_device_count() -> usize {
    crate::kernel::framework::chitin::chitin_blk_count()
}

pub fn block_device_list() -> Vec<(usize, &'static str, u64)> {
    let devices = crate::kernel::framework::chitin::CHITIN_DEVICES.lock();
    devices
        .iter()
        .filter(|d| d.proto == crate::kernel::framework::chitin::ChitinProto::Block)
        .enumerate()
        .map(|(i, d)| {
            let sectors = match d.block_ops() {
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                Some(ops) => unsafe { (ops.total_sectors)(d.driver_data) },
                None => 0,
            };
            (i, d.name, sectors)
        })
        .collect()
}

pub fn block_device_state(drive: u8) -> (bool, bool, u32) {
    let idx = drive as usize;
    let present = hdd_is_present(drive);
    let removing = is_removing(idx);
    let io_count = io_refcount(idx);
    (present, removing, io_count)
}
