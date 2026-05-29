//! 几丁质设备驱动框架 (Chitin Driver Framework)
//!
//! QueenX 的统一设备驱动模型 — 唯一的注册/发现/初始化/I/O 入口。
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
    pub read:         unsafe fn(driver_data: *mut core::ffi::c_void, sector: u64, buf: &mut [u8]) -> i32,
    pub write:        unsafe fn(driver_data: *mut core::ffi::c_void, sector: u64, buf: &[u8]) -> i32,
    pub is_present:   unsafe fn(driver_data: *mut core::ffi::c_void) -> bool,
    pub total_sectors: unsafe fn(driver_data: *mut core::ffi::c_void) -> u64,
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
    pub driver_data: *mut core::ffi::c_void,
    pub ops: Option<ChitinOps>,
}

// SAFETY: ChitinDevice contains a raw pointer (driver_data) that does not
// own memory — it is managed externally by the driver. All other fields are
// Copy types or Option wrappers. Mutation is protected by the device tree
// lock. The pointer is only dereferenced under the driver's own lock.
unsafe impl Send for ChitinDevice {}
unsafe impl Sync for ChitinDevice {}

impl ChitinDevice {
    #[inline]
    pub unsafe fn driver_as_mut<T>(&self) -> &mut T {
        &mut *(self.driver_data as *mut T)
    }

    #[inline]
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

pub static CHITIN_DEVICES: Mutex<Vec<ChitinDevice>> = Mutex::new(Vec::new());

// ── 注册函数 ──

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
        ops: None,
    });
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
    driver_data: *mut core::ffi::c_void,
    ops: ChitinOps,
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
        ops: Some(ops),
    });
    id
}

/// 注册块设备 (便捷函数)
///
/// 同时提供 Driver trait 生命周期管理和 BlockOps I/O 能力。
/// 返回设备在 CHITIN_DEVICES 中的索引 (用作 drive_id)。
pub fn chitin_register_block(
    name: &'static str,
    io_base: Option<u64>,
    irq: Option<u8>,
    blk_ops: &'static BlockOps,
    driver_data: *mut core::ffi::c_void,
) -> u32 {
    let id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
    let mut devices = CHITIN_DEVICES.lock();
    let idx = devices.len() as u32;
    devices.push(ChitinDevice {
        id,
        name,
        proto: ChitinProto::Block,
        state: DeviceState::Ready,
        io_base,
        irq,
        driver_data,
        ops: Some(ChitinOps::Block(blk_ops)),
    });
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
    CHITIN_DEVICES.lock().iter()
        .map(|d| (d.id, d.name, d.proto, d.state))
        .collect()
}

pub fn chitin_count() -> usize {
    CHITIN_DEVICES.lock().len()
}

pub fn chitin_count_by_proto(proto: ChitinProto) -> usize {
    CHITIN_DEVICES.lock().iter().filter(|d| d.proto == proto).count()
}

pub fn chitin_with_device<F>(id: u32, f: F)
where F: FnOnce(&mut ChitinDevice)
{
    let mut devices = CHITIN_DEVICES.lock();
    if let Some(dev) = devices.iter_mut().find(|d| d.id == id) {
        f(dev);
    }
}

pub fn chitin_with_device_map<T, F>(id: u32, f: F) -> Option<T>
where F: FnOnce(&mut ChitinDevice) -> T
{
    let mut devices = CHITIN_DEVICES.lock();
    devices.iter_mut().find(|d| d.id == id).map(f)
}

pub fn chitin_unregister(id: u32) -> Option<*mut core::ffi::c_void> {
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
pub fn chitin_blk_read(drive: u8, sector: u64, buf: &mut [u8]) -> i32 {
    if buf.len() < 512 { return -1; }
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() { return -1; }
    let dev = &devices[idx];
    if dev.proto != ChitinProto::Block { return -1; }
    if dev.state != DeviceState::Ready { return -1; }
    match dev.block_ops() {
        Some(ops) => unsafe { (ops.read)(dev.driver_data, sector, buf) },
        None => -1,
    }
}

/// 通过 Chitin 写入块设备扇区
pub fn chitin_blk_write(drive: u8, sector: u64, buf: &[u8]) -> i32 {
    if buf.len() < 512 { return -1; }
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() { return -1; }
    let dev = &devices[idx];
    if dev.proto != ChitinProto::Block { return -1; }
    if dev.state != DeviceState::Ready { return -1; }
    match dev.block_ops() {
        Some(ops) => unsafe { (ops.write)(dev.driver_data, sector, buf) },
        None => -1,
    }
}

/// 通过 Chitin 检查块设备是否存在
pub fn chitin_blk_is_present(drive: u8) -> bool {
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() { return false; }
    let dev = &devices[idx];
    if dev.proto != ChitinProto::Block { return false; }
    if dev.state != DeviceState::Ready { return false; }
    match dev.block_ops() {
        Some(ops) => unsafe { (ops.is_present)(dev.driver_data) },
        None => false,
    }
}

/// 通过 Chitin 获取块设备总扇区数
pub fn chitin_blk_total_sectors(drive: u8) -> u64 {
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() { return 0; }
    let dev = &devices[idx];
    if dev.proto != ChitinProto::Block { return 0; }
    match dev.block_ops() {
        Some(ops) => unsafe { (ops.total_sectors)(dev.driver_data) },
        None => 0,
    }
}

/// 获取块设备名称
pub fn chitin_blk_name(drive: u8) -> Option<&'static str> {
    let devices = CHITIN_DEVICES.lock();
    let idx = drive as usize;
    if idx >= devices.len() { return None; }
    if devices[idx].proto != ChitinProto::Block { return None; }
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

// ── 工具 ──

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
        ops: None,
    });
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
        ops: Some(ops),
    });
    id
}

fn driver_from_obj<'a>(ptr: *mut core::ffi::c_void) -> &'a mut dyn Driver {
    let obj: &mut DriverObject = unsafe { &mut *(ptr as *mut DriverObject) };
    unsafe { &mut *obj.ptr }
}

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

    static MOCK_BLOCK_OPS: BlockOps = BlockOps {
        read: mock_blk_read,
        write: mock_blk_write,
        is_present: mock_blk_is_present,
        total_sectors: mock_blk_total_sectors,
    };

    unsafe fn mock_blk_read(_data: *mut core::ffi::c_void, sector: u64, buf: &mut [u8]) -> i32 {
        if buf.len() < 512 { return -1; }
        buf[0] = b'R';
        buf[1] = (sector & 0xFF) as u8;
        0
    }

    unsafe fn mock_blk_write(_data: *mut core::ffi::c_void, _sector: u64, _buf: &[u8]) -> i32 {
        0
    }

    unsafe fn mock_blk_is_present(_data: *mut core::ffi::c_void) -> bool {
        true
    }

    unsafe fn mock_blk_total_sectors(_data: *mut core::ffi::c_void) -> u64 {
        1024
    }

    #[test]
    fn test_register_block_with_ops() {
        let dummy: Box<u8> = Box::new(0u8);
        let raw = box_to_raw(dummy);
        let idx = chitin_register_block(
            "mock_blk", None, None, &MOCK_BLOCK_OPS, raw,
        );
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
        unsafe { drop(Box::from_raw(raw as *mut u8)); }
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
        let idx = chitin_register_block(
            "mock_blk2", None, None, &MOCK_BLOCK_OPS, raw,
        );
        let devices = CHITIN_DEVICES.lock();
        let dev = &devices[idx as usize];
        assert!(dev.block_ops().is_some());
        assert!(dev.char_ops().is_none());
        assert!(dev.net_ops().is_none());
        CHITIN_DEVICES.lock().clear();
        unsafe { drop(Box::from_raw(raw as *mut u8)); }
    }

    #[test]
    fn test_register_with_ops() {
        let dummy: Box<u8> = Box::new(0u8);
        let raw = box_to_raw(dummy);
        let id = chitin_register_with_ops(
            "mock_net", ChitinProto::Net, None, None, raw,
            ChitinOps::Block(&MOCK_BLOCK_OPS),
        );
        assert!(id > 0);
        let devices = CHITIN_DEVICES.lock();
        assert!(devices.iter().any(|d| d.id == id && d.ops.is_some()));
        CHITIN_DEVICES.lock().clear();
        unsafe { drop(Box::from_raw(raw as *mut u8)); }
    }
}
