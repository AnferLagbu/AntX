//! 几丁质设备驱动框架 (Chitin Driver Framework)
//!
//! QueenX 的统一设备驱动模型 — 唯一的注册/发现/初始化入口。
//!
//! - **ChitinProto**: 设备协议分类 (Block/Char/Net/Input/Bus/Other)
//! - **ChitinDevice**: 统一设备描述符 (值类型, 无 trait object)
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
//! chitin_init_all()
//!   └── 遍历 CHITIN_DEVICES, 对 state=Uninit 的设备调用 Driver::init()
//!
//! chitin_shutdown_all()
//!   └── 遍历 CHITIN_DEVICES, 对 state=Ready 的设备调用 Driver::shutdown()
//! ```

use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

use super::driver::framework::Driver;
pub use super::driver::framework::DriverError as ChitinError;

// ── 协议模块 ──
pub mod proto_block;
pub mod proto_char;
pub mod proto_net;
pub mod proto_input;
pub mod devtree;

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

impl From<super::driver::framework::DeviceType> for ChitinProto {
    fn from(dt: super::driver::framework::DeviceType) -> Self {
        match dt {
            super::driver::framework::DeviceType::Block   => ChitinProto::Block,
            super::driver::framework::DeviceType::Char    => ChitinProto::Char,
            super::driver::framework::DeviceType::Network => ChitinProto::Net,
            super::driver::framework::DeviceType::Input   => ChitinProto::Input,
            super::driver::framework::DeviceType::Bus     => ChitinProto::Bus,
            super::driver::framework::DeviceType::Other   => ChitinProto::Other,
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

// ── Driver trait 集成 ──

struct DriverObject {
    ptr: *mut dyn Driver,
}

impl Drop for DriverObject {
    fn drop(&mut self) {
        unsafe { drop(Box::from_raw(self.ptr)); }
    }
}

/// 注册一个实现了 Driver trait 的设备。
///
/// 这是推荐的统一注册入口。传入一个已分配的 `Box<dyn Driver>`，
/// Chitin 会自动调用 `driver.init()` 完成初始化。
///
/// # 参数
/// - `name`: 设备名称 (静态字符串)
/// - `proto`: 协议类型
/// - `io_base`: I/O 基地址 (可选)
/// - `irq`: 中断号 (可选)
/// - `driver`: `Box<dyn Driver>` — 所有权转移至 Chitin
///
/// # 返回值
/// - 成功: 分配的全局设备 ID
///
/// # 生命周期
/// 设备在 `chitin_unregister(id)` 或 `chitin_shutdown_all()` 时释放。
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
    let obj_ptr = Box::into_raw(obj) as *mut core::ffi::c_void;
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let mut devices = CHITIN_DEVICES.lock();
    devices.push(ChitinDevice {
        id,
        name,
        proto,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data: obj_ptr,
    });
    id
}

fn driver_from_obj<'a>(ptr: *mut core::ffi::c_void) -> &'a mut dyn Driver {
    let obj: &mut DriverObject = unsafe { &mut *(ptr as *mut DriverObject) };
    unsafe { &mut *obj.ptr }
}

/// 遍历所有设备并调用 Driver::init()。
///
/// 仅初始化状态为 `DeviceState::Uninit` 的设备。
/// 通常在内核启动时由 `driver::init_all()` 调用。
pub fn chitin_init_all() {
    let mut devices = CHITIN_DEVICES.lock();
    for dev in devices.iter_mut() {
        if dev.state == DeviceState::Uninit && !dev.driver_data.is_null() {
            let driver = driver_from_obj(dev.driver_data);
            let _ = driver.init();
            dev.state = DeviceState::Ready;
        }
    }
}

/// 遍历所有设备并调用 Driver::shutdown()，然后释放 driver box。
///
/// 仅关闭状态为 `DeviceState::Ready` 的设备。
/// 通常在系统关机时调用。
pub fn chitin_shutdown_all() {
    let mut devices = CHITIN_DEVICES.lock();
    for dev in devices.iter_mut() {
        if dev.state == DeviceState::Ready && !dev.driver_data.is_null() {
            {
                let driver = driver_from_obj(dev.driver_data);
                let _ = driver.shutdown();
            }
            dev.state = DeviceState::Failed;
            unsafe { drop(Box::from_raw(dev.driver_data as *mut DriverObject)); }
            dev.driver_data = core::ptr::null_mut();
        }
    }
}

/// 获取所有设备的基本信息列表 (与旧 DeviceInfo 兼容)
pub fn chitin_device_list() -> Vec<(u32, &'static str, ChitinProto, DeviceState)> {
    chitin_list()
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::driver::framework::DriverError;

    struct TestDriver {
        name: &'static str,
        init_called: bool,
    }

    impl Driver for TestDriver {
        fn name(&self) -> &'static str { self.name }
        fn device_type(&self) -> super::super::driver::framework::DeviceType {
            super::super::driver::framework::DeviceType::Other
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
        unsafe { drop(Box::from_raw(raw as *mut u32)); }
    }

    #[test]
    fn test_register_driver_auto_init() {
        let driver = Box::new(TestDriver { name: "test", init_called: false });
        let id = chitin_register_driver("test_drv", ChitinProto::Other, None, None, driver);
        assert!(id > 0);

        chitin_with_device(id, |dev| {
            assert_eq!(dev.state, DeviceState::Ready);
            let drv = unsafe { dev.driver_as_ref::<TestDriver>() };
            assert!(drv.init_called);
        });

        let ptr = chitin_unregister(id).unwrap();
        unsafe { drop(Box::from_raw(ptr as *mut TestDriver)); }
    }

    #[test]
    fn test_rust_ffi() {
        assert!(alloc::format!("{:?}", ChitinProto::Char).contains("Char"));
        assert_eq!(ChitinProto::Input.as_str(), "input");
    }
}