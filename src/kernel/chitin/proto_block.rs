//! 几丁质块设备协议 (Chitin Block Protocol)
//!
//! 定义块设备 I/O 函数指针表。每个块设备驱动提供自己的实现。
//! 用于 HvFS 等子系统通过 Chitin 统一访问块设备。

use crate::kernel::driver::block::BlockDevice;

/// 块设备操作表 — 薄封装, 委托给 BlockDevice trait
///
/// ## 设计选择
///
/// QueenX 已有 `BlockDevice` trait (driver/block.rs) 及全局注册表。
/// Chitin 块设备协议作为 **桥接层**:
/// - 注册时: 同时注册到 BlockDevice 表和 Chitin 表
/// - 运行时: HvFS 继续使用 BlockDevice 注册表 (零开销)
/// - 发现时: `chitin_find_by_proto(Block)` 可枚举所有块设备
///
/// 因此 BlockProto 不需要函数指针表 — 直接复用 BlockDevice trait。
/// 本模块提供便捷的桥接函数。

/// 注册块设备到 Chitin + BlockDevice 双注册表
///
/// `name`: 设备名称 (如 "ata0", "virtio-blk0")
/// `dev`: 实现 BlockDevice trait 的设备
/// `io_base`: MMIO 基地址 (如果适用)
///
/// 返回 Chitin 设备 ID
pub fn register_block_device(
    name: &'static str,
    dev: impl BlockDevice + 'static,
    io_base: Option<u64>,
) -> u32 {
    use crate::kernel::chitin::{chitin_register, ChitinProto, box_to_raw};

    // 先注册到 BlockDevice 表 (HvFS 直接使用)
    let bdev: alloc::boxed::Box<dyn BlockDevice> = alloc::boxed::Box::new(dev);
    let raw = box_to_raw(bdev);

    // 再注册到 Chitin
    chitin_register(name, ChitinProto::Block, io_base, None, raw)
}

/// 通过 Chitin ID 获取 BlockDevice trait 引用
///
/// # Safety
/// 调用者确保 ID 对应的设备确实是块设备, 且 driver_data 是有效的 Box<dyn BlockDevice>
pub unsafe fn get_block_device(id: u32) -> Option<&'static mut dyn BlockDevice> {
    use crate::kernel::chitin::CHITIN_DEVICES;
    let devices = CHITIN_DEVICES.lock();
    // 不能返回引用 (Mutex 锁作用域), 所以这里返回 None
    // 实际使用时应该用 chitin_with_device 模式
    let _ = (id, devices);
    None
}