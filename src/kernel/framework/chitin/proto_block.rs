//! 几丁质块设备协议 (Chitin Block Protocol)
//!
//! 块设备注册路径 — 简化为 1 个 trait, 0 thunk, 0 BlockOps.
//!
//! T-4.1 (LEGACY-4): 旧路径 (4 个 extern "C" thunk + BlockOps 函数指针表 + Box<Box>)
//! 已被 trait dispatch 完全替代. 现在 `chitin_blk_read/write` 直接调用
//! `&mut dyn BlockDevice::blk_read/blk_write`, 0 unsafe, 0 间接调用.
//!
//! 旧 `BlockOps`/`block_device_with_ops`/`register_block_raw` API **保留**
//! 以支持尚未迁移的自定义驱动, 但新代码应使用 `register_block_device` +
//! `impl BlockDevice` 即可.

use crate::kernel::framework::chitin::{
    chitin_register_block, chitin_register_with_ops, BlockOps, ChitinOps, ChitinProto, BlockDevice,
};

/// 注册块设备到 Chitin (推荐入口, T-4.1 新版)
///
/// 直接传 `&'static mut dyn BlockDevice`, 0 thunk, 0 BlockOps:
/// - `chitin_blk_read/write` 直接 trait dispatch
/// - 无 extern "C" 间接调用
/// - 编译期类型安全
///
/// # 内存
///
/// `dev: impl BlockDevice + 'static` 会被 `Box::leak` 为静态内存, 由 Chitin 全权管理.
/// 这意味着注册的块设备**永不释放** (适用 OS 生命周期). 如需动态卸载, 请
/// 显式管理并使用 chitin_unregister + drop.
///
/// 返回设备在 CHITIN_DEVICES 中的索引 (用作 drive_id)。
pub fn register_block_device(
    name: &'static str,
    dev: impl BlockDevice + 'static,
    io_base: Option<u64>,
) -> u32 {
    // T-4.1: Box::leak → &'static mut dyn BlockDevice, 走新注册路径
    let leaked: &'static mut (dyn BlockDevice) = alloc::boxed::Box::leak(alloc::boxed::Box::new(dev));
    crate::kernel::framework::chitin::chitin_register_block_dev(name, io_base, None, leaked)
}

/// 注册块设备 (使用自定义 BlockOps) — **遗留 API, 不推荐**
///
/// 适用于需要自定义 I/O 路径的块设备驱动, 或尚未迁移到 `BlockDevice` trait
/// 的旧驱动. 新代码请使用 `register_block_device`.
#[deprecated(
    since = "0.1.0",
    note = "请使用 `register_block_device` + `impl BlockDevice`. BlockOps thunk 路径已废弃. (Phase E 待移除, 跟踪: LEGACY-4)"
)]
pub fn register_block_device_with_ops(
    name: &'static str,
    io_base: Option<u64>,
    irq: Option<u8>,
    ops: &'static BlockOps,
    driver_data: *mut u8,
) -> u32 {
    chitin_register_block(name, io_base, irq, ops, driver_data)
}

/// 注册块设备到 Chitin (使用 ChitinOps 枚举) — **遗留 API, 不推荐**
#[deprecated(
    since = "0.1.0",
    note = "请使用 `register_block_device` + `impl BlockDevice`. (Phase E 待移除, 跟踪: LEGACY-4)"
)]
pub fn register_block_raw(
    name: &'static str,
    io_base: Option<u64>,
    irq: Option<u8>,
    driver_data: *mut u8,
    ops: ChitinOps,
) -> u32 {
    chitin_register_with_ops(name, ChitinProto::Block, io_base, irq, driver_data, ops)
}
