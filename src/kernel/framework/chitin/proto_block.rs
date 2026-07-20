//! 几丁质块设备协议 (Chitin Block Protocol)
//!
//! 块设备注册路径 — 统一使用 `BlockDevice` trait, 0 thunk, 0 BlockOps.
//!
//! `chitin_blk_read/write` 直接调用 `&mut dyn BlockDevice::blk_read/blk_write`,
//! 0 unsafe, 0 间接调用.

use crate::kernel::framework::chitin::BlockDevice;

/// 注册块设备到 Chitin
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
    let leaked: &'static mut dyn BlockDevice = alloc::boxed::Box::leak(alloc::boxed::Box::new(dev));
    crate::kernel::framework::chitin::chitin_register_block_dev(name, io_base, None, leaked)
}
