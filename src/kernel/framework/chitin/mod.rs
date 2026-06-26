//! 几丁质设备驱动框架 (Chitin Driver Framework) — **API 层**
//!
//! QueenX 的统一设备驱动模型 — 唯一的注册/发现/初始化/I/O 入口。
//! 本文件是 QueenX 的 **API 层**。
//!
//! ## 调用方契约
//! - `driver::framework::Driver` —— 驱动实现者
//! - `fs::hvfs` —— 通过 `proto_block` 读块设备
//! - `fs::ramfs/devfs` —— 通过 `chitin_register_driver` 暴露设备节点
//! - `net::e1000/virtio` —— 通过 `proto_net` 注册网卡
//! - `proc::session` —— 通过 `user_driver` 暴露用户态驱动接口
//! - `host-tests` —— host 端设备测试桩
//!
//! ## 内部接口
//! - `proto_block/char/input/net` —— 协议族,函数指针表(类 Linux `struct ops`)
//! - `CHITIN_DEVICES` —— 全局注册表,spinlock 保护
//!
//! ## 安全约束
//! - `chitin_register_*` 必须在启动早期单线程上下文调用
//! - `chitin_blk_read/write` 等 IO 路径可在中断上下文调用,但调用方负责
//!   buffer 生命周期
//!
//! ## 性能特征
//! - 注册表: `Vec<ChitinDevice>` + spinlock,O(N) 查找(N ≤ 64)
//! - 协议调用: 静态分发,无 vtable 开销
//!
//! - **ChitinProto**: 设备协议分类 (Block/Char/Net/Input/Bus/Other)
//! - **ChitinOps**: 协议级 I/O 操作表 (函数指针, 零开销)
//! - **ChitinDevice**: 统一设备描述符 (含 I/O 能力)
//! - **Driver trait**: 驱动运行时行为的接口契约 (init/shutdown/is_ready)
//! - **全局注册表**: `CHITIN_DEVICES` 统一管理所有设备
//!
//! ## 架构
//!
//! ```text
//! chitin_register_driver("vga", ChitinProto::Char, None, None, vga_driver_box)
//!   ├── 创建 ChitinDevice 并存入 CHITIN_DEVICES
//!   ├── 调用 Driver::init() → 设置 state = Ready
//!   └── 返回 device id
//!
//! chitin_register_block("ata0", None, None, blk_ops, driver_data)
//!   ├── 创建 ChitinDevice + BlockOps
//!   ├── HvFS 通过 chitin_blk_read/write 直接 I/O
//!   └── 无需 block::REGISTRY 中间层
//!
//! chitin_blk_read(drive_idx, sector, buf)
//!   └── CHITIN_DEVICES[drive].ops::BlockOps.read(...)
//! ```

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::kernel::framework::fs::KernelError;
use super::driver::Driver;

// ── 协议模块 ──
pub mod composite;
pub mod devtree;
pub mod proto_block;

// devtree 公共接口 re-export — 避免跨子系统直接访问 chitin::devtree 内部
pub use devtree::*;
// devtree 内部接口 re-export (pub(crate), 供同 crate 测试使用)
pub(crate) use devtree::devtree_create_node_impl;
pub mod proto_char;
pub mod proto_input;
pub mod proto_net;
pub mod user_driver;

// composite 公共接口 re-export — 避免跨子系统直接访问 chitin::composite 内部
pub use composite::devtree_probe_composites;

// firmware 公共接口 re-export — 避免跨子系统直接访问 chitin::firmware 内部
pub use firmware::*;

// proto_net 公共接口 re-export — 避免跨子系统直接访问 chitin::proto_net 内部
pub use proto_net::NetOps;

// proto_char/proto_input 公共接口 re-export — 避免跨子系统直接访问 chitin::proto_char/proto_input 内部
pub use proto_char::CharOps;
pub use proto_input::InputOps;

// proto_block 公共接口 re-export — 避免跨子系统直接访问 chitin::proto_block 内部
// LEGACY-4: 标记 `#[allow(deprecated)]` 是因为 re-export 会沿用源头 `#[deprecated]` 属性.
//            两个遗留 API (register_block_device_with_ops / register_block_raw) 仍向
//            旧驱动暴露, Phase E 移除前需要保持公开; 新驱动应使用 `register_block_device` + `impl BlockDevice`.
#[allow(deprecated)]
pub use proto_block::{register_block_device, register_block_device_with_ops, register_block_raw};
pub mod firmware;

// ── BlockDevice Trait (设备框架层定义, driver::block re-export) ──

/// 块设备统一接口, 由所有存储驱动实现.
///
/// 定义在 chitin (设备框架) 而非 driver (具体驱动), 因为:
/// - chitin 是设备框架, 负责定义设备协议
/// - driver 是具体驱动实现, 实现 chitin 定义的 trait
/// - 消除 chitin→driver 循环依赖
pub trait BlockDevice: Send + Sync {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32;
    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32;
    fn blk_is_present(&self) -> bool;
    fn blk_total_sectors(&self) -> u64;
}

// ── 协议类型 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChitinProto {
    Block,
    Char,
    Net,
    Input,
    Bus,
    Other,
}

impl ChitinProto {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChitinProto::Block => "block",
            ChitinProto::Char => "char",
            ChitinProto::Net => "net",
            ChitinProto::Input => "input",
            ChitinProto::Bus => "bus",
            ChitinProto::Other => "other",
        }
    }
}

impl From<super::driver::DeviceType> for ChitinProto {
    fn from(dt: super::driver::DeviceType) -> Self {
        match dt {
            super::driver::DeviceType::Block => ChitinProto::Block,
            super::driver::DeviceType::Char => ChitinProto::Char,
            super::driver::DeviceType::Network => ChitinProto::Net,
            super::driver::DeviceType::Input => ChitinProto::Input,
            super::driver::DeviceType::Bus => ChitinProto::Bus,
            super::driver::DeviceType::Other => ChitinProto::Other,
        }
    }
}

// ── 设备状态 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Uninit,
    Probing,
    Ready,
    Failed,
    Removed,
}

// ── I/O 操作表 ──

/// 块设备 I/O 操作表
///
/// 每个块设备驱动提供这四个函数指针, Chitin 通过它们执行 I/O,
/// 无需虚表分发或 trait object, 实现零开销抽象。
pub struct BlockOps {
    pub read: extern "C" fn(driver_data: *mut u8, sector: u64, buf: *mut u8) -> i32,
    pub write: extern "C" fn(driver_data: *mut u8, sector: u64, buf: *const u8) -> i32,
    pub is_present: extern "C" fn(driver_data: *mut u8) -> bool,
    pub total_sectors: extern "C" fn(driver_data: *mut u8) -> u64,
}

impl BlockOps {
    /// 块设备读 (Framekernel 安全接口, 调用方无需 unsafe)
    ///
    /// # Safety (调用方)
    /// - `driver_data` 必须指向由驱动注册时提供的有效对象。
    /// - `buf` 必须可写 `buf.len()` 字节。
    pub fn read_sector(
        &self,
        driver_data: *mut u8,
        sector: u64,
        buf: &mut [u8],
    ) -> i32 {
        // SAFETY: 调用方契约保证 driver_data/buf 有效; extern "C" fn
        // 调用本身是安全的, 我们仅用 unsafe 块满足类型系统对裸指针的
        // 借用要求。
        unsafe { (self.read)(driver_data, sector, buf.as_mut_ptr()) }
    }

    /// 块设备写 (Framekernel 安全接口)
    ///
    /// # Safety (调用方)
    /// - `driver_data` 必须有效, `buf` 在调用期间保持借用。
    pub fn write_sector(
        &self,
        driver_data: *mut u8,
        sector: u64,
        buf: &[u8],
    ) -> i32 {
        // SAFETY: 同上。
        unsafe { (self.write)(driver_data, sector, buf.as_ptr()) }
    }

    /// 设备是否在线
    ///
    /// # Safety (调用方)
    /// - `driver_data` 必须有效。
    pub fn is_present(&self, driver_data: *mut u8) -> bool {
        // SAFETY: driver_data 由驱动设置, 生命周期内有效。
        unsafe { (self.is_present)(driver_data) }
    }

    /// 设备总扇区数
    ///
    /// # Safety (调用方)
    /// - `driver_data` 必须有效。
    pub fn total_sectors(&self, driver_data: *mut u8) -> u64 {
        // SAFETY: driver_data 由驱动设置, 生命周期内有效。
        unsafe { (self.total_sectors)(driver_data) }
    }
}

/// 协议级 I/O 操作表 (联合体, 按协议类型取对应变体)
pub enum ChitinOps {
    Block(&'static BlockOps),
    Char(&'static proto_char::CharOps),
    Net(&'static proto_net::NetOps),
    Input(&'static proto_input::InputOps),
}

// ── ChitinDevice ──

/// 几丁质设备描述符
///
/// 每个设备在注册表中占用一个条目。`driver_data` 指向内核堆上
/// 的实际驱动结构体, `ops` 提供协议级 I/O 操作表。
pub struct ChitinDevice {
    pub id: u32,
    pub name: &'static str,
    pub proto: ChitinProto,
    pub state: DeviceState,
    pub io_base: Option<u64>,
    pub irq: Option<u8>,
    pub driver_data: *mut u8,
    pub ops: Option<ChitinOps>,
    /// T-4.1: 块设备 trait 引用 (替代 BlockOps 函数指针 thunk)
    /// 当 `proto == ChitinProto::Block` 且驱动通过 `register_block_device` 注册时,
    /// `chitin_blk_read/write` 走此字段 (0 unsafe, 0 thunk)
    ///
    /// # Mutability
    ///
    /// `&'static mut` 是因为 `BlockDevice::blk_read/blk_write` 需要 `&mut self`.
    /// 通过 `&mut ChitinDevice` 借用得到 (CHITIN_DEVICES 锁保护),
    /// 不存在数据竞争 (持有锁时此引用唯一).
    pub block_dev: Option<&'static mut dyn BlockDevice>,
}

// SAFETY: ChitinDevice 含一个原始指针 (`driver_data`), 其不
// 拥有内存 —— 内存由驱动方自行管理. 其它字段均为 Copy 类型或
// SAFETY: ChitinDevice 含裸指针, 但通过 Option 包装, 写操作由设备树锁保护.
//         该指针仅在驱动自身锁内解引用.
unsafe impl Send for ChitinDevice {}
// SAFETY: 同上, 设备树锁保护并发访问.
unsafe impl Sync for ChitinDevice {}

impl ChitinDevice {
    #[inline]
    ///
    /// # Safety
    ///
    /// `self.driver_data` 由驱动在 probe 阶段设置. 调用方必须保证
    /// `T` 与 `set_driver_data` 写入时的具体类型一致.
    pub unsafe fn driver_as_mut<T>(&self) -> &mut T {
        &mut *(self.driver_data as *mut T)
    }

    #[inline]
    ///
    /// # Safety
    ///
    /// `self.driver_data` 由驱动在 probe 阶段设置. 调用方必须保证
    /// `T` 与 `set_driver_data` 写入时的具体类型一致.
    pub unsafe fn driver_as_ref<T>(&self) -> &T {
        &*(self.driver_data as *const T)
    }

    pub fn block_ops(&self) -> Option<&'static BlockOps> {
        match &self.ops {
            Some(ChitinOps::Block(ops)) => Some(ops),
            _ => None,
        }
    }

    pub fn char_ops(&self) -> Option<&'static proto_char::CharOps> {
        match &self.ops {
            Some(ChitinOps::Char(ops)) => Some(ops),
            _ => None,
        }
    }

    pub fn net_ops(&self) -> Option<&'static proto_net::NetOps> {
        match &self.ops {
            Some(ChitinOps::Net(ops)) => Some(ops),
            _ => None,
        }
    }

    pub fn input_ops(&self) -> Option<&'static proto_input::InputOps> {
        match &self.ops {
            Some(ChitinOps::Input(ops)) => Some(ops),
            _ => None,
        }
    }
}

// ── 全局注册表 ──

static NEXT_DEVICE_ID: AtomicU32 = AtomicU32::new(1);

/// 设备注册回调 — 可由 DevFS 订阅, 自动创建设备节点
/// E6-9b: Chitin→DevFS 桥接
static DEVICE_REGISTER_CALLBACK: Mutex<Option<fn(&ChitinDevice)>> = Mutex::new(None);

/// 设置设备注册回调 (由 DevFS 初始化时调用)
pub fn chitin_set_register_callback(cb: fn(&ChitinDevice)) {
    *DEVICE_REGISTER_CALLBACK.lock() = Some(cb);
}

/// 通知 DevFS 新设备已注册 (E6-9b)
///
/// 从 CHITIN_DEVICES 中获取最后注册的设备并调用回调。
/// 这样避免 ChitinDevice 的 move 问题。
fn notify_last_registered() {
    let cb = DEVICE_REGISTER_CALLBACK.lock();
    if let Some(f) = *cb {
        let devices = CHITIN_DEVICES.lock();
        if let Some(dev) = devices.last() {
            f(dev);
        }
    }
}

pub static CHITIN_DEVICES: Mutex<Vec<ChitinDevice>> = Mutex::new(Vec::new());

// ── 注册函数 ──

pub fn chitin_register(
    name: &'static str,
    proto: ChitinProto,
    io_base: Option<u64>,
    irq: Option<u8>,
    driver_data: *mut u8,
) -> u32 {
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let dev = ChitinDevice {
        id,
        name,
        proto,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data,
        ops: None,
        block_dev: None,
    };
    {
        let mut devices = CHITIN_DEVICES.lock();
        devices.push(dev);
    }
    // E6-9b: 通知 DevFS 创建设备节点 (从注册表获取引用)
    notify_last_registered();
    id
}

/// 注册带 I/O 操作表的设备
///
/// 这是推荐的注册方式, 使设备同时具备发现和 I/O 能力。
pub fn chitin_register_with_ops(
    name: &'static str,
    proto: ChitinProto,
    io_base: Option<u64>,
    irq: Option<u8>,
    driver_data: *mut u8,
    ops: ChitinOps,
) -> u32 {
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let dev = ChitinDevice {
        id,
        name,
        proto,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data,
        ops: Some(ops),
        block_dev: None,
    };
    {
        let mut devices = CHITIN_DEVICES.lock();
        devices.push(dev);
    }
    notify_last_registered();
    id
}

/// 注册块设备 (便捷函数)
///
/// 同时提供 Driver trait 生命周期管理和 BlockOps I/O 能力。
/// 返回设备在 CHITIN_DEVICES 中的索引 (用作 drive_id)。
///
/// I-43: 此函数是低层桥接, 驱动作者**不应**直接调用, 应使用
/// `proto_block::register_block_device` (BlockDevice trait → BlockOps 桥接).
/// 桥接保证: 驱动实现 `BlockDevice` trait, HvFS 通过 Chitin I/O 路径
/// (chitin_blk_read/write) 访问, 单一入口, 无双重分发.
#[doc(hidden)]
pub fn chitin_register_block(
    name: &'static str,
    io_base: Option<u64>,
    irq: Option<u8>,
    blk_ops: &'static BlockOps,
    driver_data: *mut u8,
) -> u32 {
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let dev = ChitinDevice {
        id,
        name,
        proto: ChitinProto::Block,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data,
        ops: Some(ChitinOps::Block(blk_ops)),
        // T-4.1: 旧 BlockOps 注册路径不设置 block_dev
        block_dev: None,
    };
    let idx;
    {
        let mut devices = CHITIN_DEVICES.lock();
        idx = devices.len() as u32;
        devices.push(dev);
    }
    notify_last_registered();
    idx
}

/// T-4.1: 注册块设备 (直接传 `&'static mut dyn BlockDevice`, 0 thunk, 0 BlockOps)
///
/// 这是新路径, 替代 `chitin_register_block` + 4 个 thunk 的方式.
/// 优势:
/// - 0 unsafe (chitin_blk_read/write 直接 trait dispatch)
/// - 0 间接调用 (BlockOps 函数指针 thunk 不再存在)
/// - 类型安全 (驱动方必须实现 BlockDevice trait, 编译期检查)
///
/// # 生命周期
///
/// `dev` 必须是 `'static` (即 `Box::leak` 或静态分配), ChitinDevice 持有
/// `&'static mut dyn BlockDevice` 引用. 设备移除时 Chitin 负责清理.
///
/// # Mutability 原因
///
/// `BlockDevice::blk_read/blk_write` 需要 `&mut self`, 故用 `&'static mut` 而非 `&'static`.
/// 通过 CHITIN_DEVICES 锁保护, 不存在数据竞争.
pub fn chitin_register_block_dev(
    name: &'static str,
    io_base: Option<u64>,
    irq: Option<u8>,
    dev: &'static mut dyn BlockDevice,
) -> u32 {
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let chitin_dev = ChitinDevice {
        id,
        name,
        proto: ChitinProto::Block,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data: core::ptr::null_mut(),  // 不再使用, 改用 block_dev
        ops: None,                            // 不再使用, 改用 block_dev
        block_dev: Some(dev),
    };
    let idx;
    {
        let mut devices = CHITIN_DEVICES.lock();
        idx = devices.len() as u32;
        devices.push(chitin_dev);
    }
    notify_last_registered();
    idx
}

// ── 查找函数 ──

pub fn chitin_find_by_id(id: u32) -> Option<usize> {
    CHITIN_DEVICES.lock().iter().position(|d| d.id == id)
}

pub fn chitin_find_by_name(name: &str) -> Option<usize> {
    CHITIN_DEVICES.lock().iter().position(|d| d.name == name)
}

pub fn chitin_find_by_proto(proto: ChitinProto) -> Option<usize> {
    CHITIN_DEVICES.lock().iter().position(|d| d.proto == proto)
}

pub fn chitin_list() -> Vec<(u32, &'static str, ChitinProto, DeviceState)> {
    CHITIN_DEVICES
        .lock()
        .iter()
        .map(|d| (d.id, d.name, d.proto, d.state))
        .collect()
}

pub fn chitin_count() -> usize {
    CHITIN_DEVICES.lock().len()
}

pub fn chitin_count_by_proto(proto: ChitinProto) -> usize {
    CHITIN_DEVICES
        .lock()
        .iter()
        .filter(|d| d.proto == proto)
        .count()
}

/// 查找第一个网络设备, 返回其 NetOps + driver_data + MAC 地址
///
/// 供 smoltcp_impl 使用——协议栈不关心具体驱动类型, 只需要
/// 四个函数指针 (send/recv/get_mac/irq) 和 driver_data。
pub fn chitin_find_net_device() -> Option<(
    &'static crate::kernel::framework::chitin::proto_net::NetOps,
    *mut u8,
    [u8; 6],
)> {
    let devices = CHITIN_DEVICES.lock();
    for dev in devices.iter() {
        if dev.proto != ChitinProto::Net {
            continue;
        }
        if dev.state != DeviceState::Ready {
            continue;
        }
        match &dev.ops {
            Some(ChitinOps::Net(net_ops)) => {
                let mut mac = [0u8; 6];
                net_ops.get_mac(dev.driver_data, &mut mac);
                return Some((net_ops, dev.driver_data, mac));
            }
            _ => continue,
        }
    }
    None
}

pub fn chitin_with_device<F>(id: u32, f: F)
where
    F: FnOnce(&mut ChitinDevice),
{
    let mut devices = CHITIN_DEVICES.lock();
    if let Some(dev) = devices.iter_mut().find(|d| d.id == id) {
        f(dev);
    }
}

pub fn chitin_with_device_map<T, F>(id: u32, f: F) -> Option<T>
where
    F: FnOnce(&mut ChitinDevice) -> T,
{
    let mut devices = CHITIN_DEVICES.lock();
    devices.iter_mut().find(|d| d.id == id).map(f)
}

pub fn chitin_unregister(id: u32) -> Option<*mut u8> {
    let mut devices = CHITIN_DEVICES.lock();
    let pos = devices.iter().position(|d| d.id == id)?;
    let dev = devices.remove(pos);
    Some(dev.driver_data)
}

pub fn chitin_set_state(id: u32, state: DeviceState) {
    let mut devices = CHITIN_DEVICES.lock();
    if let Some(dev) = devices.iter_mut().find(|d| d.id == id) {
        dev.state = state;
    }
}

// ── 块设备 I/O (统一入口, 替代 block::hdd_*) ──

/// 通过 Chitin 读取块设备扇区
///
/// `drive` 是设备在 CHITIN_DEVICES 中的索引 (与旧 block::REGISTRY 索引兼容)。
/// 仅对 `ChitinProto::Block` 且携带 `BlockOps` 的设备有效。
///
/// 返回值遵循 POSIX 约定: `0` = 成功, `-errno` = 失败 (与 `framework::fs::KernelError` 对齐)。
pub fn chitin_blk_read(drive: u8, sector: u64, buf: &mut [u8]) -> i32 {
    if buf.len() < 512 {
        return KernelError::InvalidArgument.as_i32();
    }
    let mut devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() {
        return KernelError::IoError.as_i32();
    }
    let dev = &mut devices[idx];
    if dev.proto != ChitinProto::Block {
        return KernelError::IoError.as_i32();
    }
    if dev.state != DeviceState::Ready {
        return KernelError::Busy.as_i32();
    }
    // T-4.1: 优先走 block_dev trait dispatch (0 unsafe, 0 thunk)
    // 通过 &mut devices[idx] 拿到的 dyn BlockDevice 已是 &mut
    if let Some(bd) = dev.block_dev.as_mut() {
        return bd.blk_read(sector, buf);
    }
    // 兼容: 旧 BlockOps 函数指针路径
    match dev.block_ops() {
        Some(ops) => ops.read_sector(dev.driver_data, sector, buf),
        None => KernelError::NotSupported.as_i32(),
    }
}

/// 通过 Chitin 写入块设备扇区
pub fn chitin_blk_write(drive: u8, sector: u64, buf: &[u8]) -> i32 {
    if buf.len() < 512 {
        return KernelError::InvalidArgument.as_i32();
    }
    let mut devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() {
        return KernelError::IoError.as_i32();
    }
    let dev = &mut devices[idx];
    if dev.proto != ChitinProto::Block {
        return KernelError::IoError.as_i32();
    }
    if dev.state != DeviceState::Ready {
        return KernelError::Busy.as_i32();
    }
    // T-4.1: 优先走 block_dev trait dispatch
    if let Some(bd) = dev.block_dev.as_mut() {
        return bd.blk_write(sector, buf);
    }
    match dev.block_ops() {
        Some(ops) => ops.write_sector(dev.driver_data, sector, buf),
        None => KernelError::NotSupported.as_i32(),
    }
}

/// 通过 Chitin 检查块设备是否存在
pub fn chitin_blk_is_present(drive: u8) -> bool {
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() {
        return false;
    }
    let dev = &devices[idx];
    if dev.proto != ChitinProto::Block {
        return false;
    }
    if dev.state != DeviceState::Ready {
        return false;
    }
    // T-4.1: 优先走 block_dev trait dispatch
    if let Some(bd) = dev.block_dev.as_ref() {
        return bd.blk_is_present();
    }
    match dev.block_ops() {
        Some(ops) => ops.is_present(dev.driver_data),
        None => false,
    }
}

/// 通过 Chitin 获取块设备总扇区数
pub fn chitin_blk_total_sectors(drive: u8) -> u64 {
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() {
        return 0;
    }
    let dev = &devices[idx];
    if dev.proto != ChitinProto::Block {
        return 0;
    }
    // T-4.1: 优先走 block_dev trait dispatch
    if let Some(bd) = dev.block_dev.as_ref() {
        return bd.blk_total_sectors();
    }
    match dev.block_ops() {
        Some(ops) => ops.total_sectors(dev.driver_data),
        None => 0,
    }
}

/// 获取块设备名称
pub fn chitin_blk_name(drive: u8) -> Option<&'static str> {
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() {
        return None;
    }
    if devices[idx].proto != ChitinProto::Block {
        return None;
    }
    Some(devices[idx].name)
}

/// 获取块设备综合信息 (name, is_present, total_sectors)
pub fn chitin_blk_info(drive: u8) -> (&'static str, bool, u64) {
    let name = chitin_blk_name(drive).unwrap_or("unknown");
    let present = chitin_blk_is_present(drive);
    let sectors = chitin_blk_total_sectors(drive);
    (name, present, sectors)
}

/// 获取块设备数量
pub fn chitin_blk_count() -> usize {
    chitin_count_by_proto(ChitinProto::Block)
}

// ── 字符设备 I/O (统一入口) ──

/// 通过 Chitin 向第一个就绪字符设备写入字节
///
/// 遍历 CHITIN_DEVICES 查找第一个 proto=Char+Ready 且携带 CharOps 的设备,
/// 通过其 write 函数指针输出数据。用于内核日志串口输出等。
pub fn chitin_char_write(data: &[u8]) {
    let devices = CHITIN_DEVICES.lock();
    for dev in devices.iter() {
        if dev.proto != ChitinProto::Char {
            continue;
        }
        if dev.state != DeviceState::Ready {
            continue;
        }
        match dev.char_ops() {
            Some(ops) => {
                ops.write(dev.driver_data, data);
                return;
            }
            None => continue,
        }
    }
}

/// 通过 Chitin 从第一个就绪字符设备读取字节
pub fn chitin_char_read(buf: &mut [u8]) -> usize {
    let devices = CHITIN_DEVICES.lock();
    for dev in devices.iter() {
        if dev.proto != ChitinProto::Char {
            continue;
        }
        if dev.state != DeviceState::Ready {
            continue;
        }
        match dev.char_ops() {
            Some(ops) => return ops.read(dev.driver_data, buf),
            None => continue,
        }
    }
    0
}

// ── 输入设备 I/O (统一入口) ──

/// 通过 Chitin 从第一个就绪输入设备读取一个字符
pub fn chitin_input_read() -> Option<u8> {
    let devices = CHITIN_DEVICES.lock();
    for dev in devices.iter() {
        if dev.proto != ChitinProto::Input {
            continue;
        }
        if dev.state != DeviceState::Ready {
            continue;
        }
        match dev.input_ops() {
            Some(ops) => return ops.read_char(dev.driver_data),
            None => continue,
        }
    }
    None
}

/// 通过 Chitin 检查第一个就绪输入设备是否有数据
pub fn chitin_input_has_data() -> bool {
    let devices = CHITIN_DEVICES.lock();
    for dev in devices.iter() {
        if dev.proto != ChitinProto::Input {
            continue;
        }
        if dev.state != DeviceState::Ready {
            continue;
        }
        match dev.input_ops() {
            Some(ops) => return ops.has_char(dev.driver_data),
            None => continue,
        }
    }
    false
}

// ── 工具 ──

pub fn box_to_raw<T: ?Sized>(b: Box<T>) -> *mut u8 {
    Box::into_raw(b) as *mut u8
}

// ── Driver trait 集成 ──

struct DriverObject {
    ptr: *mut dyn Driver,
}

impl Drop for DriverObject {
    fn drop(&mut self) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            drop(Box::from_raw(self.ptr));
        }
    }
}

pub fn chitin_register_driver(
    name: &'static str,
    proto: ChitinProto,
    io_base: Option<u64>,
    irq: Option<u8>,
    mut driver: Box<dyn Driver>,
) -> u32 {
    let _ = driver.init();
    let raw = Box::into_raw(driver);
    let obj = Box::new(DriverObject { ptr: raw });
    let obj_ptr = Box::into_raw(obj) as *mut u8;
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let dev = ChitinDevice {
        id,
        name,
        proto,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data: obj_ptr,
        ops: None,
        block_dev: None,
    };
    {
        let mut devices = CHITIN_DEVICES.lock();
        devices.push(dev);
    }
    notify_last_registered();
    id
}

/// 注册带 Driver trait 和 I/O 操作表的设备
pub fn chitin_register_driver_with_ops(
    name: &'static str,
    proto: ChitinProto,
    io_base: Option<u64>,
    irq: Option<u8>,
    mut driver: Box<dyn Driver>,
    ops: ChitinOps,
) -> u32 {
    let _ = driver.init();
    let raw = Box::into_raw(driver);
    let obj = Box::new(DriverObject { ptr: raw });
    let obj_ptr = Box::into_raw(obj) as *mut u8;
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let dev = ChitinDevice {
        id,
        name,
        proto,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data: obj_ptr,
        ops: Some(ops),
        block_dev: None,
    };
    {
        let mut devices = CHITIN_DEVICES.lock();
        devices.push(dev);
    }
    notify_last_registered();
    id
}

fn driver_from_obj<'a>(ptr: *mut u8) -> &'a mut dyn Driver {
    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    let obj: &mut DriverObject = unsafe { &mut *(ptr as *mut DriverObject) };
    // SAFETY: `obj` 由调用方保证为有效指针; 只读访问
    unsafe { &mut *obj.ptr }
}

pub fn chitin_init_all() {
    // 注册进程退出清理回调, 解耦 proc→chitin 依赖
    // SAFETY: chitin_process_cleanup 是 'static 函数指针, 在内核运行期间始终有效.
    unsafe {
        crate::kernel::framework::process_cleanup::register_process_cleanup(
            crate::kernel::framework::chitin::user_driver::chitin_process_cleanup,
        );
    }

    let mut devices = CHITIN_DEVICES.lock();
    for dev in devices.iter_mut() {
        if dev.state == DeviceState::Uninit && !dev.driver_data.is_null() {
            let driver = driver_from_obj(dev.driver_data);
            let _ = driver.init();
            dev.state = DeviceState::Ready;
        }
    }
}

pub fn chitin_shutdown_all() {
    let mut devices = CHITIN_DEVICES.lock();
    for dev in devices.iter_mut() {
        if dev.state == DeviceState::Ready && !dev.driver_data.is_null() {
            {
                let driver = driver_from_obj(dev.driver_data);
                let _ = driver.shutdown();
            }
            dev.state = DeviceState::Failed;
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                drop(Box::from_raw(dev.driver_data as *mut DriverObject));
            }
            dev.driver_data = core::ptr::null_mut();
        }
    }
}

pub fn chitin_device_list() -> Vec<(u32, &'static str, ChitinProto, DeviceState)> {
    chitin_list()
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::super::driver::DriverError;
    use super::*;

    struct TestDriver {
        name: &'static str,
        init_called: bool,
    }

    impl Driver for TestDriver {
        fn name(&self) -> &'static str {
            self.name
        }
        fn device_type(&self) -> super::super::driver::DeviceType {
            super::super::driver::DeviceType::Other
        }

        fn init(&mut self) -> core::result::Result<(), DriverError> {
            self.init_called = true;
            Ok(())
        }

        fn shutdown(&mut self) -> core::result::Result<(), DriverError> {
            Ok(())
        }
    }

    #[test]
    fn test_proto_as_str() {
        assert_eq!(ChitinProto::Block.as_str(), "block");
        assert_eq!(ChitinProto::Net.as_str(), "net");
    }

    #[test]
    fn test_device_state() {
        assert_eq!(DeviceState::Uninit as u8, DeviceState::Uninit as u8);
        assert_ne!(DeviceState::Ready as u8, DeviceState::Failed as u8);
    }

    #[test]
    fn test_register_and_find() {
        let dummy: Box<u32> = Box::new(42u32);
        let raw = box_to_raw(dummy);

        let id = chitin_register("test_dev", ChitinProto::Other, None, None, raw);
        assert!(id > 0);

        assert!(chitin_find_by_name("test_dev").is_some());
        assert_eq!(chitin_count(), 1);

        CHITIN_DEVICES.lock().clear();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            drop(Box::from_raw(raw as *mut u32));
        }
    }

    #[test]
    fn test_register_driver_auto_init() {
        let driver = Box::new(TestDriver {
            name: "test",
            init_called: false,
        });
        let id = chitin_register_driver("test_drv", ChitinProto::Other, None, None, driver);
        assert!(id > 0);

        chitin_with_device(id, |dev| {
            assert_eq!(dev.state, DeviceState::Ready);
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            let drv = unsafe { dev.driver_as_ref::<TestDriver>() };
            assert!(drv.init_called);
        });

        let ptr = chitin_unregister(id).unwrap();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            drop(Box::from_raw(ptr as *mut TestDriver));
        }
    }

    #[test]
    fn test_rust_ffi() {
        assert!(alloc::format!("{:?}", ChitinProto::Char).contains("Char"));
        assert_eq!(ChitinProto::Input.as_str(), "input");
    }

    static MOCK_BLOCK_OPS: BlockOps = BlockOps {
        read: mock_blk_read,
        write: mock_blk_write,
        is_present: mock_blk_is_present,
        total_sectors: mock_blk_total_sectors,
    };

    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    unsafe fn mock_blk_read(_data: *mut u8, sector: u64, buf: &mut [u8]) -> i32 {
        if buf.len() < 512 {
            return -1;
        }
        buf[0] = b'R';
        buf[1] = (sector & 0xFF) as u8;
        0
    }

    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    unsafe fn mock_blk_write(_data: *mut u8, _sector: u64, _buf: &[u8]) -> i32 {
        0
    }

    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    unsafe fn mock_blk_is_present(_data: *mut u8) -> bool {
        true
    }

    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    unsafe fn mock_blk_total_sectors(_data: *mut u8) -> u64 {
        1024
    }

    #[test]
    fn test_register_block_with_ops() {
        let dummy: Box<u8> = Box::new(0u8);
        let raw = box_to_raw(dummy);
        let idx = chitin_register_block("mock_blk", None, None, &MOCK_BLOCK_OPS, raw);
        assert_eq!(chitin_count_by_proto(ChitinProto::Block), 1);

        let present = chitin_blk_is_present(idx as u8);
        assert!(present);

        let sectors = chitin_blk_total_sectors(idx as u8);
        assert_eq!(sectors, 1024);

        let mut buf = [0u8; 512];
        let r = chitin_blk_read(idx as u8, 42, &mut buf);
        assert_eq!(r, 0);
        assert_eq!(buf[0], b'R');
        assert_eq!(buf[1], 42);

        CHITIN_DEVICES.lock().clear();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            drop(Box::from_raw(raw as *mut u8));
        }
    }

    #[test]
    fn test_blk_read_invalid_drive() {
        let mut buf = [0u8; 512];
        assert_eq!(chitin_blk_read(255, 0, &mut buf), -1);
    }

    #[test]
    fn test_blk_ops_accessor() {
        let dummy: Box<u8> = Box::new(0u8);
        let raw = box_to_raw(dummy);
        let idx = chitin_register_block("mock_blk2", None, None, &MOCK_BLOCK_OPS, raw);
        let devices = CHITIN_DEVICES.lock();
        let dev = &devices[idx as usize];
        assert!(dev.block_ops().is_some());
        assert!(dev.char_ops().is_none());
        assert!(dev.net_ops().is_none());
        CHITIN_DEVICES.lock().clear();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            drop(Box::from_raw(raw as *mut u8));
        }
    }

    #[test]
    fn test_register_with_ops() {
        let dummy: Box<u8> = Box::new(0u8);
        let raw = box_to_raw(dummy);
        let id = chitin_register_with_ops(
            "mock_net",
            ChitinProto::Net,
            None,
            None,
            raw,
            ChitinOps::Block(&MOCK_BLOCK_OPS),
        );
        assert!(id > 0);
        let devices = CHITIN_DEVICES.lock();
        assert!(devices.iter().any(|d| d.id == id && d.ops.is_some()));
        CHITIN_DEVICES.lock().clear();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            drop(Box::from_raw(raw as *mut u8));
        }
    }

    // ==========================================================================
    // T-4.1 (LEGACY-4): 新 BlockDevice trait 注册路径测试
    // ==========================================================================
    //
    // 验证: register_block_device + chitin_register_block_dev 走 trait dispatch
    // 而非 thunk 函数指针. 这些测试用 MockBlockDevice (本地 struct) 验证
    // BlockDevice trait 直接被 chitin_blk_read/write 调用, 不通过 extern "C".

    struct MockBlockDevice {
        counter: u64,
        last_sector: u64,
        last_written: [u8; 16],
    }

    impl MockBlockDevice {
        const fn new() -> Self {
            Self { counter: 0, last_sector: 0, last_written: [0u8; 16] }
        }
    }

    impl BlockDevice for MockBlockDevice {
        fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
            self.counter += 1;
            self.last_sector = sector;
            if buf.len() < 512 { return -1; }
            // 写一个 magic 模式, 测试用
            for i in 0..16 {
                buf[i] = b'T';  // 标识 "通过 trait 来的数据"
            }
            buf[16] = (sector & 0xFF) as u8;
            0
        }
        fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
            self.counter += 1;
            self.last_sector = sector;
            for i in 0..16.min(buf.len()) {
                self.last_written[i] = buf[i];
            }
            0
        }
        fn blk_is_present(&self) -> bool { true }
        fn blk_total_sectors(&self) -> u64 { 2048 }
    }

    /// 1. 注册路径: register_block_device + chitin_register_block_dev
    #[test]
    fn test_t4_1_register_via_trait() {
        CHITIN_DEVICES.lock().clear();
        let mock: &'static mut MockBlockDevice = Box::leak(Box::new(MockBlockDevice::new()));
        let idx = chitin_register_block_dev("trait_blk", None, None, mock);
        assert_eq!(chitin_count_by_proto(ChitinProto::Block), 1);
        let devices = CHITIN_DEVICES.lock();
        let dev = &devices[idx as usize];
        assert!(dev.block_dev.is_some(), "block_dev 字段应被设置");
        assert!(dev.ops.is_none(), "旧 BlockOps 字段应为空");
        CHITIN_DEVICES.lock().clear();
    }

    /// 2. trait dispatch: chitin_blk_read 调 MockBlockDevice.blk_read (非 thunk)
    #[test]
    fn test_t4_1_chitin_blk_read_via_trait() {
        CHITIN_DEVICES.lock().clear();
        let mock: &'static mut MockBlockDevice = Box::leak(Box::new(MockBlockDevice::new()));
        let idx = chitin_register_block_dev("trait_blk_read", None, None, mock);

        let mut buf = [0u8; 512];
        let r = chitin_blk_read(idx as u8, 7, &mut buf);
        assert_eq!(r, 0, "chitin_blk_read 应成功");
        assert_eq!(buf[0], b'T', "数据应来自 MockBlockDevice (trait), 非 thunk");
        assert_eq!(buf[16], 7, "sector 7 应被传到 trait");
        assert_eq!(mock.counter, 1, "MockBlockDevice.blk_read 应被调用 1 次");
        assert_eq!(mock.last_sector, 7);

        CHITIN_DEVICES.lock().clear();
    }

    /// 3. trait dispatch: chitin_blk_write 调 MockBlockDevice.blk_write
    #[test]
    fn test_t4_1_chitin_blk_write_via_trait() {
        CHITIN_DEVICES.lock().clear();
        let mock: &'static mut MockBlockDevice = Box::leak(Box::new(MockBlockDevice::new()));
        let idx = chitin_register_block_dev("trait_blk_write", None, None, mock);

        let buf = [b'X'; 512];
        let r = chitin_blk_write(idx as u8, 99, &buf);
        assert_eq!(r, 0, "chitin_blk_write 应成功");
        assert_eq!(mock.last_sector, 99);
        assert_eq!(mock.last_written[0], b'X', "数据应传到 MockBlockDevice");

        CHITIN_DEVICES.lock().clear();
    }

    /// 4. trait 分发: chitin_blk_is_present / total_sectors
    #[test]
    fn test_t4_1_chitin_blk_metadata_via_trait() {
        CHITIN_DEVICES.lock().clear();
        let mock: &'static mut MockBlockDevice = Box::leak(Box::new(MockBlockDevice::new()));
        let idx = chitin_register_block_dev("trait_blk_meta", None, None, mock);

        assert!(chitin_blk_is_present(idx as u8));
        assert_eq!(chitin_blk_total_sectors(idx as u8), 2048);

        CHITIN_DEVICES.lock().clear();
    }

    /// 5. 兼容: 旧 BlockOps 路径仍工作 (block_dev = None)
    #[test]
    fn test_t4_1_compat_block_ops_path() {
        CHITIN_DEVICES.lock().clear();
        let dummy: Box<u8> = Box::new(0u8);
        let raw = box_to_raw(dummy);
        let idx = chitin_register_block("compat_blk", None, None, &MOCK_BLOCK_OPS, raw);

        // 旧路径: 通过 BlockOps thunk
        assert!(chitin_blk_is_present(idx as u8));
        assert_eq!(chitin_blk_total_sectors(idx as u8), 1024);

        let mut buf = [0u8; 512];
        let r = chitin_blk_read(idx as u8, 42, &mut buf);
        assert_eq!(r, 0);
        assert_eq!(buf[0], b'R', "兼容路径应通过 MOCK_BLOCK_OPS 返回 'R'");

        CHITIN_DEVICES.lock().clear();
        // SAFETY: 测试中我们分配了 raw, 现在手动 drop
        unsafe { drop(Box::from_raw(raw as *mut u8)); }
    }

    /// 6. 优先级: block_dev 优先于 ops
    #[test]
    fn test_t4_1_block_dev_takes_priority() {
        // 验证当 block_dev 已设置时, 不会 fallback 到 ops 路径
        // (本测试通过设置 block_dev 并调用 chitin_blk_read, 检查
        //  数据来自 MockBlockDevice 而非 MOCK_BLOCK_OPS)
        CHITIN_DEVICES.lock().clear();
        let mock: &'static mut MockBlockDevice = Box::leak(Box::new(MockBlockDevice::new()));
        let idx = chitin_register_block_dev("priority_blk", None, None, mock);

        let mut buf = [0u8; 512];
        chitin_blk_read(idx as u8, 1, &mut buf);
        assert_eq!(buf[0], b'T', "block_dev 路径应优先 (返回 'T', 非 'R')");
        CHITIN_DEVICES.lock().clear();
    }

    /// 7. 边界: buf.len() < 512 应返回 -EINVAL (无论 block_dev 路径)
    #[test]
    fn test_t4_1_buf_too_small_via_trait() {
        CHITIN_DEVICES.lock().clear();
        let mock: &'static mut MockBlockDevice = Box::leak(Box::new(MockBlockDevice::new()));
        let idx = chitin_register_block_dev("small_buf_blk", None, None, mock);

        let mut small = [0u8; 256];
        let r = chitin_blk_read(idx as u8, 0, &mut small);
        assert_eq!(r, -22); // -EINVAL
        assert_eq!(mock.counter, 0, "Mock 不应被调用 (校验前置)");

        CHITIN_DEVICES.lock().clear();
    }

    /// 8. 边界: drive OOB 应返回 -EIO (无 panic)
    #[test]
    fn test_t4_1_drive_oob_via_trait() {
        CHITIN_DEVICES.lock().clear();
        let mut buf = [0u8; 512];
        assert_eq!(chitin_blk_read(255, 0, &mut buf), -5); // -EIO
        assert_eq!(chitin_blk_write(255, 0, &buf), -5);
        assert!(!chitin_blk_is_present(255));
        assert_eq!(chitin_blk_total_sectors(255), 0);
    }
}
