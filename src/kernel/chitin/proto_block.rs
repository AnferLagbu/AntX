//! 几丁质块设备协议 (Chitin Block Protocol)
//!
//! 块设备 I/O 函数指针表与便捷注册。
//!
//! Chitin 是唯一的设备驱动框架, 块设备通过 `BlockOps` 提供四个
//! 函数指针 (read/write/is_present/total_sectors), HvFS 等子系统
//! 通过 `chitin_blk_read/write` 直接 I/O, 无需中间注册表。

use crate::kernel::chitin::{
    box_to_raw, chitin_register_block, chitin_register_with_ops, BlockOps, ChitinOps, ChitinProto,
};
use crate::kernel::driver::block::BlockDevice;
use alloc::boxed::Box;

unsafe fn blk_read_thunk(data: *mut core::ffi::c_void, sector: u64, buf: &mut [u8]) -> i32 {
    let dev: &mut Box<dyn BlockDevice> = unsafe { &mut *(data as *mut Box<dyn BlockDevice>) };
    dev.blk_read(sector, buf)
}

unsafe fn blk_write_thunk(data: *mut core::ffi::c_void, sector: u64, buf: &[u8]) -> i32 {
    let dev: &mut Box<dyn BlockDevice> = unsafe { &mut *(data as *mut Box<dyn BlockDevice>) };
    dev.blk_write(sector, buf)
}

unsafe fn blk_is_present_thunk(data: *mut core::ffi::c_void) -> bool {
    let dev: &mut Box<dyn BlockDevice> = unsafe { &mut *(data as *mut Box<dyn BlockDevice>) };
    dev.blk_is_present()
}

unsafe fn blk_total_sectors_thunk(data: *mut core::ffi::c_void) -> u64 {
    let dev: &mut Box<dyn BlockDevice> = unsafe { &mut *(data as *mut Box<dyn BlockDevice>) };
    dev.blk_total_sectors()
}

static BLOCK_DEVICE_OPS: BlockOps = BlockOps {
    read: blk_read_thunk,
    write: blk_write_thunk,
    is_present: blk_is_present_thunk,
    total_sectors: blk_total_sectors_thunk,
};

/// 注册块设备到 Chitin (唯一注册入口)
///
/// 自动将 `BlockDevice` trait 方法桥接为 `BlockOps` 函数指针表,
/// 使 HvFS 可通过 `chitin_blk_read/write` 直接 I/O。
///
/// 返回设备在 CHITIN_DEVICES 中的索引 (用作 drive_id)。
pub fn register_block_device(
    name: &'static str,
    dev: impl BlockDevice + 'static,
    io_base: Option<u64>,
) -> u32 {
    let bdev: alloc::boxed::Box<dyn BlockDevice> = alloc::boxed::Box::new(dev);
    let boxed: alloc::boxed::Box<Box<dyn BlockDevice>> = alloc::boxed::Box::new(bdev);
    let raw = box_to_raw(boxed);
    chitin_register_block(name, io_base, None, &BLOCK_DEVICE_OPS, raw)
}

/// 注册块设备 (使用自定义 BlockOps)
///
/// 适用于需要自定义 I/O 路径的块设备驱动。
pub fn register_block_device_with_ops(
    name: &'static str,
    io_base: Option<u64>,
    irq: Option<u8>,
    ops: &'static BlockOps,
    driver_data: *mut core::ffi::c_void,
) -> u32 {
    chitin_register_block(name, io_base, irq, ops, driver_data)
}

/// 注册块设备到 Chitin (使用 ChitinOps 枚举)
pub fn register_block_raw(
    name: &'static str,
    io_base: Option<u64>,
    irq: Option<u8>,
    driver_data: *mut core::ffi::c_void,
    ops: ChitinOps,
) -> u32 {
    chitin_register_with_ops(name, ChitinProto::Block, io_base, irq, driver_data, ops)
}
