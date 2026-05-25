//! 几丁质设备驱动框架 (Chitin Driver Framework)
//!
//! QueenX 的轻量级设备模型。提供:
//! - **ChitinProto**: 设备协议分类 (Block/Char/Net/Input/Bus/Other)
//! - **ChitinDevice**: 统一设备描述符 (值类型, 无 trait object)
//! - **全局注册表**: `CHITIN_DEVICES` 统一管理所有设备
//!
//! ## 设计理念
//!
//! ```text
//! ChitinDevice (值类型, 不含虚表)
//!   ├── proto: ChitinProto  ── 协议类型分类
//!   ├── state: DeviceState  ── 生命周期状态
//!   ├── driver_data: *mut c_void ── 指向内核堆上的实际驱动结构
//!   └── io_base/irq       ── 硬件资源信息
//!
//! 注册表 (Mutex<Vec<ChitinDevice>>)
//!   ├── chitin_find_by_name("e1000")
//!   ├── chitin_find_by_proto(Net)
//!   └── chitin_list()
//! ```
//!
//! ## 与 Driver trait 的关系
//!
//! - `Driver` trait (framework.rs): 驱动初始化/关闭的**接口契约**
//! - Chitin: 设备**注册/发现/分类**的**管理框架**
//! - 两者互补: Driver 负责行为, Chitin 负责组织

use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

// ── 协议模块 ──
pub mod proto_block;
pub mod proto_char;
pub mod proto_net;
pub mod proto_input;

// ── 协议类型 ──

/// 设备协议类型 (不含虚表, 纯分类)
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
            ChitinProto::Char  => "char",
            ChitinProto::Net   => "net",
            ChitinProto::Input => "input",
            ChitinProto::Bus   => "bus",
            ChitinProto::Other => "other",
        }
    }
}

// ── 设备状态 ──

/// 设备生命周期状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// 未初始化
    Uninit,
    /// 正在探测
    Probing,
    /// 就绪
    Ready,
    /// 初始化/运行失败
    Failed,
    /// (预留) 已移除 — 热插拔
    Removed,
}

// ── ChitinDevice ──

/// 几丁质设备描述符 (值类型, 无虚表, 无 trait object)
///
/// 每个设备在注册表中占用一个条目, `driver_data` 指向内核堆上
/// 的实际驱动结构体 (通过 Box::into_raw 分配)。
pub struct ChitinDevice {
    /// 全局唯一设备 ID
    pub id: u32,
    /// 设备名称
    pub name: &'static str,
    /// 协议类型
    pub proto: ChitinProto,
    /// 当前状态
    pub state: DeviceState,
    /// I/O 基地址 (MMIO 物理地址 或 IO port)
    pub io_base: Option<u64>,
    /// IRQ 号
    pub irq: Option<u8>,
    /// 指向实际驱动结构体的不透明指针
    pub driver_data: *mut core::ffi::c_void,
}

// SAFETY: 单核内核, 无抢占, 驱动数据在 PMM/内核堆上
unsafe impl Send for ChitinDevice {}
unsafe impl Sync for ChitinDevice {}

impl ChitinDevice {
    /// 从 driver_data 获取指定类型的可变引用
    ///
    /// # Safety
    /// 调用者必须确保 T 与实际存储的类型匹配
    #[inline]
    pub unsafe fn driver_as_mut<T>(&self) -> &mut T {
        &mut *(self.driver_data as *mut T)
    }

    /// 从 driver_data 获取指定类型的共享引用
    #[inline]
    pub unsafe fn driver_as_ref<T>(&self) -> &T {
        &*(self.driver_data as *const T)
    }
}

// ── 全局注册表 ──

static NEXT_DEVICE_ID: AtomicU32 = AtomicU32::new(1);

/// 全局几丁质设备注册表
pub static CHITIN_DEVICES: Mutex<Vec<ChitinDevice>> = Mutex::new(Vec::new());

/// 注册一个设备到几丁质框架
///
/// `driver_data` 应为 `Box::into_raw(Box::new(driver_struct))` 的返回值。
/// 当设备被移除时, 调用者负责从 raw pointer 重建 Box 以释放内存。
pub fn chitin_register(
    name: &'static str,
    proto: ChitinProto,
    io_base: Option<u64>,
    irq: Option<u8>,
    driver_data: *mut core::ffi::c_void,
) -> u32 {
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let mut devices = CHITIN_DEVICES.lock();
    devices.push(ChitinDevice {
        id,
        name,
        proto,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data,
    });
    id
}

/// 按 ID 查找设备
pub fn chitin_find_by_id(id: u32) -> Option<usize> {
    CHITIN_DEVICES.lock().iter().position(|d| d.id == id)
}

/// 按名称查找设备
pub fn chitin_find_by_name(name: &str) -> Option<usize> {
    CHITIN_DEVICES.lock().iter().position(|d| d.name == name)
}

/// 按协议类型查找第一个匹配的设备索引
pub fn chitin_find_by_proto(proto: ChitinProto) -> Option<usize> {
    CHITIN_DEVICES.lock().iter().position(|d| d.proto == proto)
}

/// 列出所有设备 (返回 Vec, 可用于遍历)
pub fn chitin_list() -> Vec<(u32, &'static str, ChitinProto, DeviceState)> {
    CHITIN_DEVICES.lock().iter()
        .map(|d| (d.id, d.name, d.proto, d.state))
        .collect()
}

/// 设备总数
pub fn chitin_count() -> usize {
    CHITIN_DEVICES.lock().len()
}

/// 按协议类型统计
pub fn chitin_count_by_proto(proto: ChitinProto) -> usize {
    CHITIN_DEVICES.lock().iter().filter(|d| d.proto == proto).count()
}

/// 安全地访问设备: 锁定注册表, 找到设备, 执行闭包
///
/// 闭包接收 `&mut ChitinDevice` 引用。如果设备不存在则不做任何事。
pub fn chitin_with_device<F>(id: u32, f: F)
where F: FnOnce(&mut ChitinDevice)
{
    let mut devices = CHITIN_DEVICES.lock();
    if let Some(dev) = devices.iter_mut().find(|d| d.id == id) {
        f(dev);
    }
}

/// 安全地访问设备并返回值
pub fn chitin_with_device_map<T, F>(id: u32, f: F) -> Option<T>
where F: FnOnce(&mut ChitinDevice) -> T
{
    let mut devices = CHITIN_DEVICES.lock();
    devices.iter_mut().find(|d| d.id == id).map(f)
}

/// 从注册表中移除设备并返回其 driver_data 指针。
///
/// 调用者负责 `Box::from_raw(ptr as *mut T)` 释放驱动内存。
/// 返回 `None` 如果设备不存在。
///
/// # Safety
/// 调用者必须确保返回的 raw pointer 对应正确的类型 T。
pub fn chitin_unregister(id: u32) -> Option<*mut core::ffi::c_void> {
    let mut devices = CHITIN_DEVICES.lock();
    let pos = devices.iter().position(|d| d.id == id)?;
    let dev = devices.remove(pos);
    Some(dev.driver_data)
}

/// 设置设备生命周期状态 (插入 → Ready, 拔出 → Removed)
pub fn chitin_set_state(id: u32, state: DeviceState) {
    let mut devices = CHITIN_DEVICES.lock();
    if let Some(dev) = devices.iter_mut().find(|d| d.id == id) {
        dev.state = state;
    }
}

// ── 工具: from raw Box ──

/// 将 `Box<T>` 转为 raw pointer 用于 chitin_register 的 driver_data
pub fn box_to_raw<T: ?Sized>(b: Box<T>) -> *mut core::ffi::c_void {
    Box::into_raw(b) as *mut core::ffi::c_void
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;

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
        // 测试注册和查找
        let dummy: Box<u32> = Box::new(42u32);
        let raw = box_to_raw(dummy);

        let id = chitin_register("test_dev", ChitinProto::Other, None, None, raw);
        assert!(id > 0);

        assert!(chitin_find_by_name("test_dev").is_some());
        assert_eq!(chitin_count(), 1);

        // 清理
        CHITIN_DEVICES.lock().clear();
        unsafe { drop(Box::from_raw(raw as *mut u32)); }
    }
}