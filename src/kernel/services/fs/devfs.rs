#![deny(unsafe_code)]
//! 设备文件系统 (DevFS) — services 层完整实现
//!
//! 从 framework/fs/devfs/devfs.rs 迁移而来. 0 unsafe, 纯策略.
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 所有硬件交互通过 framework safe API
//! - **类型安全**: 设备类型用 `DevKind` 枚举, 而非裸 `u8`
//! - **完整实现**: 包含设备注册/IO/目录读取, 不再依赖 framework 实现
//!
//! 评估日期: 2026-06-10
//! E6-7: DevFS 策略提取

use core::sync::atomic::{AtomicU32, Ordering};

use crate::kernel::services::sync::irq_lock::IrqSpinLock as Mutex;

// ============================================================================
// 常量
// ============================================================================

/// 最大设备数
pub const DEVFS_MAX_DEVICES: usize = 16;
/// 设备名最大长度
pub const DEVFS_MAX_NAME: usize = 32;

// ============================================================================
// 设备类型
// ============================================================================

extern crate alloc;

/// 设备类型 (强类型枚举, 替代裸 `u8`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DevKind {
    /// 空设备 /dev/null
    Null = 0,
    /// 零字节源 /dev/zero
    Zero = 1,
    /// 控制台 /dev/console
    Console = 2,
    /// TTY /dev/tty
    Tty = 3,
    /// Credo 能力 (PWM) 接口 /dev/credo
    Credo = 4,
    /// 块设备 (如 /dev/sda)
    Block = 5,
    /// 字符设备 (如 /dev/serial0)
    Char = 6,
    /// 网络设备 (如 /dev/eth0)
    Net = 7,
    /// 输入设备 (如 /dev/input/kbd)
    Input = 8,
}

impl DevKind {
    /// 从 `u8` 解析 (容忍未知值, 返回 `None`)
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Null),
            1 => Some(Self::Zero),
            2 => Some(Self::Console),
            3 => Some(Self::Tty),
            4 => Some(Self::Credo),
            5 => Some(Self::Block),
            6 => Some(Self::Char),
            7 => Some(Self::Net),
            8 => Some(Self::Input),
            _ => None,
        }
    }

    /// 是否为虚拟设备 (内核内部实现)
    pub fn is_virtual(&self) -> bool {
        matches!(self, Self::Null | Self::Zero | Self::Console | Self::Tty | Self::Credo)
    }

    /// 是否为物理设备 (由 Chitin 驱动提供)
    pub fn is_physical(&self) -> bool {
        matches!(self, Self::Block | Self::Char | Self::Net | Self::Input)
    }
}

// ============================================================================
// 内部辅助
// ============================================================================

fn write_u32_dec(buf: &mut [u8], mut off: usize, mut val: u32) -> usize {
    if val == 0 {
        if off < buf.len() {
            buf[off] = b'0';
            off += 1;
        }
        return off;
    }
    let mut digits = [0u8; 10];
    let mut i = 0;
    while val > 0 {
        digits[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    for j in (0..i).rev() {
        if off < buf.len() {
            buf[off] = digits[j];
            off += 1;
        }
    }
    off
}

// ============================================================================
// 设备条目
// ============================================================================

/// 设备条目
#[derive(Debug, Clone, Copy)]
pub struct DevfsDevice {
    pub name: [u8; DEVFS_MAX_NAME],
    pub dev_type: u8,
    pub used: bool,
}

impl DevfsDevice {
    pub const fn new() -> Self {
        Self {
            name: [0; DEVFS_MAX_NAME],
            dev_type: 0,
            used: false,
        }
    }
}

// ============================================================================
// DevfsData
// ============================================================================

/// DevFS 核心数据
pub struct DevfsData {
    devices: Mutex<[DevfsDevice; DEVFS_MAX_DEVICES]>,
    device_count: AtomicU32,
}

// DevfsData 自动 Send + Sync: IrqSpinLock<T> (framework) 已实现 unsafe impl Send/Sync,
// AtomicU32 也是 Send + Sync, 无需手动实现.

impl DevfsData {
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new([const { DevfsDevice::new() }; DEVFS_MAX_DEVICES]),
            device_count: AtomicU32::new(0),
        }
    }

    fn set_name(device: &mut DevfsDevice, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(DEVFS_MAX_NAME - 1);
        device.name[..len].copy_from_slice(&bytes[..len]);
        device.name[len] = 0;
    }

    pub fn register_device(&self, name: &str, dev_type: u8) -> KernelResult<()> {
        let mut devices = self.devices.lock();
        for device in devices.iter() {
            if device.used {
                let end = device
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(DEVFS_MAX_NAME);
                let existing = core::str::from_utf8(&device.name[..end]).unwrap_or("");
                if existing == name {
                    return Err(KernelError::AlreadyExists);
                }
            }
        }
        for device in devices.iter_mut() {
            if !device.used {
                Self::set_name(device, name);
                device.dev_type = dev_type;
                device.used = true;
                self.device_count.fetch_add(1, Ordering::SeqCst);
                return Ok(());
            }
        }
        Err(KernelError::NoSpace)
    }

    pub fn unregister_device(&self, name: &str) -> KernelResult<()> {
        let mut devices = self.devices.lock();
        for device in devices.iter_mut() {
            if device.used {
                let end = device
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(DEVFS_MAX_NAME);
                let existing = core::str::from_utf8(&device.name[..end]).unwrap_or("");
                if existing == name {
                    device.used = false;
                    device.dev_type = 0;
                    self.device_count.fetch_sub(1, Ordering::SeqCst);
                    return Ok(());
                }
            }
        }
        Err(KernelError::NotFound)
    }

    pub fn mount(&self, _path: &str) -> i32 {
        // E6-9a: 不再硬编码设备, 改为通过 register_device 注册
        // 标准虚拟设备由启动流程显式调用 register_standard() 注册
        self.device_count.store(0, Ordering::SeqCst);
        0
    }

    pub fn open(&self, path: &str) -> Option<(u32, u8)> {
        let dev_name = path.trim_start_matches('/');

        let devices = self.devices.lock();
        for device in devices.iter() {
            if device.used {
                let end = device
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(DEVFS_MAX_NAME);
                let name = core::str::from_utf8(&device.name[..end]).unwrap_or("");
                if name == dev_name {
                    return Some((device.dev_type as u32, device.dev_type));
                }
            }
        }
        None
    }

    pub fn read(&self, dev_type: u8, buf: &mut [u8]) -> i32 {
        match DevKind::from_u8(dev_type) {
            Some(DevKind::Null) => 0,
            Some(DevKind::Zero) => {
                buf.fill(0);
                buf.len() as i32
            }
            Some(DevKind::Console) | Some(DevKind::Tty) => 0,
            Some(DevKind::Credo) => {
                let pwm = crate::kernel::framework::credo::session::get_current_pwm();
                let euid = crate::kernel::framework::credo::session::get_euid();
                let uid = crate::kernel::framework::credo::session::get_current_uid();
                if pwm != 0 {
                    let mut off = 0;
                    let blen = buf.len();
                    if off < blen { buf[off] = b'O'; off += 1; }
                    if off < blen { buf[off] = b'K'; off += 1; }
                    if off < blen { buf[off] = b' '; off += 1; }
                    for &b in b"pwm=0x" {
                        if off < blen { buf[off] = b; off += 1; }
                    }
                    let hex = b"0123456789ABCDEF";
                    for shift in (0..64).rev().step_by(4) {
                        let nibble = ((pwm >> shift) & 0xF) as usize;
                        if off < blen { buf[off] = hex[nibble]; off += 1; }
                    }
                    for &b in b" uid=" {
                        if off < blen { buf[off] = b; off += 1; }
                    }
                    off = write_u32_dec(buf, off, uid);
                    for &b in b" euid=" {
                        if off < blen { buf[off] = b; off += 1; }
                    }
                    off = write_u32_dec(buf, off, euid);
                    if off < blen { buf[off] = b'\n'; off += 1; }
                    off as i32
                } else {
                    let msg = b"ERR not_authenticated\n";
                    let len = msg.len().min(buf.len());
                    buf[..len].copy_from_slice(&msg[..len]);
                    len as i32
                }
            }
            Some(DevKind::Block) | Some(DevKind::Char) | Some(DevKind::Net) | Some(DevKind::Input) => {
                // E6-9a: 物理设备 I/O 路由待 E6-9b (Chitin 桥接) 实现
                -1
            }
            None => -1,
        }
    }

    pub fn write(&self, dev_type: u8, buf: &[u8]) -> i32 {
        match DevKind::from_u8(dev_type) {
            Some(DevKind::Null) | Some(DevKind::Zero) => buf.len() as i32,
            Some(DevKind::Console) | Some(DevKind::Tty) => {
                // services 层不能使用 klog_info! (含 unsafe), 改用 safe 串口输出
                crate::kernel::framework::klog::serial_write_bytes(buf);
                buf.len() as i32
            }
            Some(DevKind::Credo) => {
                let input = core::str::from_utf8(buf).unwrap_or("");
                let input = input.trim_end_matches(['\n', '\r', '\0']);
                let mut parts = input.splitn(2, '\n');
                let note = parts.next().unwrap_or("").trim();
                let password = parts.next().unwrap_or("").trim();
                if note.is_empty() || password.is_empty() {
                    return KernelError::InvalidArgument.as_i32();
                }
                match crate::kernel::framework::credo::session::login(note, password) {
                    Ok(_pwm) => buf.len() as i32,
                    Err(_) => KernelError::PermissionDenied.as_i32(),
                }
            }
            Some(DevKind::Block) | Some(DevKind::Char) | Some(DevKind::Net) | Some(DevKind::Input) => {
                // E6-9a: 物理设备 I/O 路由待 E6-9b (Chitin 桥接) 实现
                -1
            }
            None => -1,
        }
    }

    pub fn readdir(&self, index: usize) -> Option<([u8; 32], u8)> {
        let devices = self.devices.lock();
        let mut count = 0;

        for device in devices.iter() {
            if device.used {
                if count == index {
                    let mut name = [0u8; 32];
                    let end = device.name.iter().position(|&b| b == 0).unwrap_or(32);
                    name[..end].copy_from_slice(&device.name[..end]);
                    return Some((name, device.dev_type));
                }
                count += 1;
            }
        }
        None
    }

    pub fn device_count(&self) -> u32 {
        self.device_count.load(Ordering::SeqCst)
    }
}

// ============================================================================
// 全局实例
// ============================================================================

/// 全局 DevFS 数据
pub static DEVFS_DATA: DevfsData = DevfsData::new();

/// 初始化全局 DevFS 并注册标准虚拟设备
pub fn init() {
    register_standard();
}

/// 初始化 DevFS 并订阅 Chitin 设备注册回调 (E6-9b)
///
/// Chitin 驱动注册新设备时, DevFS 自动创建对应设备节点。
pub fn init_with_chitin_bridge() {
    register_standard();
    crate::kernel::framework::chitin::chitin_set_register_callback(on_chitin_device_registered);
}

/// Chitin 设备注册回调 — 自动创建 DevFS 设备节点 (E6-9b)
fn on_chitin_device_registered(dev: &crate::kernel::framework::chitin::ChitinDevice) {
    let kind = match dev.proto {
        crate::kernel::framework::chitin::ChitinProto::Block => DevKind::Block,
        crate::kernel::framework::chitin::ChitinProto::Char => DevKind::Char,
        crate::kernel::framework::chitin::ChitinProto::Net => DevKind::Net,
        crate::kernel::framework::chitin::ChitinProto::Input => DevKind::Input,
        // Bus/Other 不创建 DevFS 节点
        _ => return,
    };
    // I-20: register_device 现在返回 KernelResult<()>, 失败仅记录
    // (设备可能已存在, Chitin 重启场景), 不阻断 chitin 回调链.
    let _ = DEVFS_DATA.register_device(dev.name, kind as u8);
}

// ============================================================================
// 安全设备文件描述符
// ============================================================================

/// DevFS 文件描述符
#[derive(Debug, Clone, Copy)]
pub struct DevFile {
    /// 内部节点索引
    pub index: u32,
    /// 设备类型
    pub kind: DevKind,
}

impl DevFile {
    /// 设备名 (如 "null", "zero", "console")
    /// 物理设备无固定名称, 返回 "device"
    pub fn name(&self) -> &'static str {
        match self.kind {
            DevKind::Null => "null",
            DevKind::Zero => "zero",
            DevKind::Console => "console",
            DevKind::Tty => "tty",
            DevKind::Credo => "credo",
            DevKind::Block | DevKind::Char | DevKind::Net | DevKind::Input => "device",
        }
    }
}

// ============================================================================
// 安全 DevFS 代理
// ============================================================================

/// DevFS 安全代理 (services 层)。
pub struct SafeDevFs {
    inner: &'static DevfsData,
}

impl SafeDevFs {
    /// 创建全局 DevFS 代理
    pub fn new() -> Self {
        Self {
            inner: &DEVFS_DATA,
        }
    }

    /// 注册设备
    /// I-20: 内部 `register_device` 改 KernelResult 后, 错误码通过映射传播到
    /// `&'static str` (保持 services 层公开 API 表面不变).
    pub fn register(&self, name: &str, kind: DevKind) -> Result<(), &'static str> {
        match self.inner.register_device(name, kind as u8) {
            Ok(()) => Ok(()),
            Err(KernelError::AlreadyExists) => Err("device already exists"),
            Err(KernelError::NoSpace) => Err("devfs table full"),
            Err(_) => Err("register failed"),
        }
    }

    /// 注销设备
    /// I-20: 同上, NotFound 直接映射为 `device not found`.
    pub fn unregister(&self, name: &str) -> Result<(), &'static str> {
        match self.inner.unregister_device(name) {
            Ok(()) => Ok(()),
            Err(KernelError::NotFound) => Err("device not found"),
            Err(_) => Err("unregister failed"),
        }
    }

    /// 打开设备
    pub fn open(&self, path: &str) -> Result<DevFile, &'static str> {
        match self.inner.open(path) {
            Some((index, dev_type)) => {
                let kind = DevKind::from_u8(dev_type)
                    .ok_or("unknown device type")?;
                Ok(DevFile { index, kind })
            }
            None => Err("device not found"),
        }
    }

    /// 从设备读
    pub fn read(&self, dev: &DevFile, buf: &mut [u8]) -> Result<usize, &'static str> {
        let rc = self.inner.read(dev.kind as u8, buf);
        if rc < 0 {
            Err("read failed")
        } else {
            Ok(rc as usize)
        }
    }

    /// 向设备写
    pub fn write(&self, dev: &DevFile, buf: &[u8]) -> Result<usize, &'static str> {
        let rc = self.inner.write(dev.kind as u8, buf);
        if rc < 0 {
            Err("write failed")
        } else {
            Ok(rc as usize)
        }
    }

    /// 读目录项
    pub fn readdir(&self, index: usize) -> Option<(alloc::string::String, DevKind)> {
        let (raw_name, dev_type) = self.inner.readdir(index)?;
        let kind = DevKind::from_u8(dev_type)?;
        let end = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
        let name = alloc::string::String::from_utf8_lossy(&raw_name[..end]).into_owned();
        Some((name, kind))
    }

    /// 设备总数
    pub fn device_count(&self) -> u32 {
        self.inner.device_count()
    }
}

impl Default for SafeDevFs {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 全局实例 (Once)
// ============================================================================

// I-16: 替换 spin::Once → 项目自研 services::sync::once::OnceCell
use crate::kernel::services::sync::once::OnceCell;
static GLOBAL_DEVFS: OnceCell<SafeDevFs> = OnceCell::new();

/// 初始化全局 DevFS
pub fn init_global() {
    let _ = GLOBAL_DEVFS.get_or_init(|slot| { slot.write(SafeDevFs::new()); });
}

/// 获取全局 DevFS 引用
pub fn global() -> &'static SafeDevFs {
    GLOBAL_DEVFS
        .get()
        .expect("devfs::global() called before init_global()")
}

// ============================================================================
// 便利函数
// ============================================================================

/// 注册设备到全局 DevFS
pub fn register(name: &str, kind: DevKind) -> Result<(), &'static str> {
    global().register(name, kind)
}

/// 打开全局 DevFS 设备
pub fn open(path: &str) -> Result<DevFile, &'static str> {
    global().open(path)
}

/// 设备名最大长度
pub const fn max_name_len() -> usize {
    DEVFS_MAX_NAME
}

// ============================================================================
// 标准设备预注册
// ============================================================================

/// 注册所有标准设备 (null/zero/console/tty/credo)
pub fn register_standard() {
    let _ = register("null", DevKind::Null);
    let _ = register("zero", DevKind::Zero);
    let _ = register("console", DevKind::Console);
    let _ = register("tty", DevKind::Tty);
    let _ = register("credo", DevKind::Credo);
}

// ============================================================================
// DevFs Inode — 设备文件 Inode 实现
// ============================================================================

use alloc::sync::Arc;
use crate::kernel::services::fs::inode::Inode;

/// 设备文件 Inode — DevFS 的 Inode 实现
pub struct DevFsInode {
    dev_type: u8,
    mount_idx: u32,
}

impl DevFsInode {
    pub fn new(dev_type: u8, mount_idx: u32) -> Self {
        Self { dev_type, mount_idx }
    }
}

impl Inode for DevFsInode {
    fn read(&self, _offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let result = DEVFS_DATA.read(self.dev_type, buf);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn write(&self, _offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let result = DEVFS_DATA.write(self.dev_type, buf);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn stat(&self, _pwm: u64) -> KernelResult<VfsStat> {
        Ok(VfsStat {
            node_id: self.dev_type as u32,
            mode: 0o20666,
            file_type: crate::kernel::framework::fs::VfsFileType::Dev.as_u8(),
            perm: 0o666,
            ..VfsStat::default()
        })
    }

    fn truncate(&self, _size: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn seek(&self, _offset: i64, _whence: crate::kernel::framework::fs::VfsSeekWhence, _current_offset: u64) -> KernelResult<u64> {
        Err(KernelError::InvalidArgument)
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn set_times(&self, _atime: u64, _mtime: u64, _pwm: u64) -> KernelResult<()> {
        // DevFS: 设备文件, 无时间戳
        Ok(())
    }

    fn node_id(&self) -> u32 {
        self.dev_type as u32
    }

    fn mount_idx(&self) -> u32 {
        self.mount_idx
    }
}

// ============================================================================
// FileSystem trait 实现 (E6-9c: VFS 分发接入)
// ============================================================================

use crate::kernel::framework::fs::{
    FileSystem, KernelError, KernelResult, VfsDirEntry, VfsStat,
};

impl FileSystem for DevfsData {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn fs_init(&self) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mount(&self, path: &str) -> KernelResult<()> {
        if self.mount(path) != 0 {
            return Err(KernelError::IoError);
        }
        // E6-9a: mount 不再硬编码设备, 由 init() 或 init_with_chitin_bridge() 注册
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, _flags: u32, _pwm: u64) -> KernelResult<Arc<dyn Inode>> {
        match self.open(rel_path) {
            Some((_index, dev_type)) => Ok(Arc::new(DevFsInode::new(dev_type, 0))),
            None => Err(KernelError::NotFound),
        }
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, _handle: u32, _offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        // DevFS read 需要 dev_type, handle 即为 dev_type
        let result = self.read(_handle as u8, buf);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_write(&self, _handle: u32, _offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let result = self.write(_handle as u8, buf);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        match self.open(rel_path) {
            Some((index, dev_type)) => Ok(VfsStat {
                node_id: index,
                mode: 0o20666, // 字符设备, rw-rw-rw-
                size: 0,
                uid: 0,
                gid: 0,
                atime: 0,
                mtime: 0,
                ctime: 0,
                owner_pwm: 0,
                group_pwm: 0,
                perm: 0o666,
                file_type: dev_type,
                sensitivity: 0,
            }),
            None => Err(KernelError::NotFound),
        }
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::PermissionDenied)
    }

    fn fs_chown(&self, _rel_path: &str, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::PermissionDenied)
    }

    fn fs_mkdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_unlink(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::PermissionDenied)
    }

    fn fs_rmdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readdir(&self, _handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        match self.readdir(offset as usize) {
            Some((raw_name, dev_type)) => {
                let end = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
                let len = end.min(entry.name.len());
                entry.name[..len].copy_from_slice(&raw_name[..len]);
                // 剩余部分填零
                entry.name[len..].fill(0);
                entry.file_type = dev_type;
                entry.node = offset as u32;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn fs_symlink(&self, _target: &str, _link_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readlink(&self, _rel_path: &str, _buf: &mut [u8]) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }

    fn fs_link(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_resolve_inode(&self, inode_id: u32, mount_idx: u32) -> Option<alloc::sync::Arc<dyn crate::kernel::services::fs::inode::Inode>> {
        Some(alloc::sync::Arc::new(DevFsInode::new(inode_id as u8, mount_idx)))
    }
}
