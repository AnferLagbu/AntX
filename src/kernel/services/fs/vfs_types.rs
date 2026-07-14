#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯类型定义与 trait。
//! VFS 公共类型 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/fs/vfs/types.rs, 2026-06-16 提取到 services.
//! 纯类型定义 (常量/枚举/结构体/FileSystem trait), 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export.

pub const VFS_MAX_PATH: usize = 128;
pub const VFS_MAX_NAME: usize = 64;
pub const VFS_MAX_FDS: usize = 32;
pub const VFS_MAX_MOUNTS: usize = 8;

extern crate alloc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    NotFound,
    AlreadyExists,
    NoSpace,
    PermissionDenied,
    InvalidArgument,
    NotInitialized,
    IoError,
    OutOfMemory,
    Busy,
    NotSupported,
    NotADirectory,
    IsDirectory,
    ReadOnly,
    Overflow,
    /// 文件名过长 (ENAMETOOLONG=36)
    NameTooLong,
}

impl KernelError {
    pub fn as_i32(self) -> i32 {
        match self {
            Self::NotFound => -2,
            Self::AlreadyExists => -17,
            Self::NoSpace => -28,
            Self::PermissionDenied => -13,
            Self::InvalidArgument => -22,
            Self::NotInitialized => -5,
            Self::IoError => -5,
            Self::OutOfMemory => -12,
            Self::Busy => -16,
            Self::NotSupported => -95,
            Self::NotADirectory => -20,
            Self::IsDirectory => -21,
            Self::ReadOnly => -30,
            Self::Overflow => -75,
            Self::NameTooLong => -36,
        }
    }
}

pub type KernelResult<T> = Result<T, KernelError>;

pub trait IntoI32 {
    fn as_i32(self) -> i32;
}

impl IntoI32 for Result<(), KernelError> {
    fn as_i32(self) -> i32 {
        match self {
            Ok(()) => 0,
            Err(e) => e.as_i32(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsFileType {
    File,
    Dir,
    Dev,
    Symlink,
}

impl VfsFileType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => VfsFileType::File,
            1 => VfsFileType::Dir,
            2 => VfsFileType::Dev,
            3 => VfsFileType::Symlink,
            _ => VfsFileType::File,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            VfsFileType::File => 0,
            VfsFileType::Dir => 1,
            VfsFileType::Dev => 2,
            VfsFileType::Symlink => 3,
        }
    }
}

pub const VFS_PERM_R: u16 = 0x04;
pub const VFS_PERM_W: u16 = 0x02;
pub const VFS_PERM_X: u16 = 0x01;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VfsOpenFlags: u32 {
        const RDONLY = 0x0001;
        const WRONLY = 0x0002;
        const RDWR   = 0x0004;
        const CREAT  = 0x0100;
        const TRUNC  = 0x0200;
        const APPEND = 0x0400;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsSeekWhence {
    Set,
    Cur,
    End,
}

impl VfsSeekWhence {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => VfsSeekWhence::Set,
            1 => VfsSeekWhence::Cur,
            2 => VfsSeekWhence::End,
            _ => VfsSeekWhence::Set,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    RamFs,
    HvFs,
    DevFs,
    Ext2,
    ExFat,
    TmpFs,
    OverlayFs,
    Unknown,
}

impl FsType {
    pub fn from_name(name: &str) -> Self {
        match name {
            "ramfs" => FsType::RamFs,
            "hvfs" => FsType::HvFs,
            "devfs" => FsType::DevFs,
            "ext2" => FsType::Ext2,
            "exfat" => FsType::ExFat,
            "tmpfs" => FsType::TmpFs,
            "overlay" => FsType::OverlayFs,
            _ => FsType::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FsType::RamFs => "ramfs",
            FsType::HvFs => "hvfs",
            FsType::DevFs => "devfs",
            FsType::Ext2 => "ext2",
            FsType::ExFat => "exfat",
            FsType::TmpFs => "tmpfs",
            FsType::OverlayFs => "overlay",
            FsType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VfsStat {
    pub node_id: u32,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub owner_pwm: u64,
    pub group_pwm: u64,
    pub perm: u16,
    pub file_type: u8,
    pub sensitivity: u8,
}

impl Default for VfsStat {
    fn default() -> Self {
        Self {
            node_id: 0,
            mode: 0,
            uid: 0xFFFF_FFFF,
            gid: 0xFFFF_FFFF,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwm: 0,
            group_pwm: 0,
            perm: 0,
            file_type: 0,
            sensitivity: 0,
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VfsDirEntry {
    pub node: u32,
    pub file_type: u8,
    pub name: [u8; VFS_MAX_NAME],
}

impl Default for VfsDirEntry {
    fn default() -> Self {
        Self {
            node: 0,
            file_type: 0,
            name: [0u8; VFS_MAX_NAME],
        }
    }
}

impl VfsDirEntry {
    pub fn new() -> Self {
        Self {
            node: 0,
            file_type: 0,
            name: [0; VFS_MAX_NAME],
        }
    }

    pub fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(VFS_MAX_NAME - 1);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len] = 0;
    }

    pub fn get_name(&self) -> &str {
        let end = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(VFS_MAX_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

// ============================================================================
// FileSystem trait — VFS 分发策略接口
// ============================================================================
//
// E6-4: 将 api.rs 中 14+ 个 `match fs_type` 分发替换为 trait object 分发.
// 新增文件系统只需实现本 trait, 无需修改 framework.
//
// 设计原则:
// - 统一 RamFS (node_id) / HvFS (fd) 的差异: open 返回 FsOpenResult,
//   内部不透明 handle 由各 FS 自行解释
// - 所有方法接收 `&self` (非 `&mut self`), 内部可变性由各 FS 自行管理
//   (RamFS 用内部 Mutex, HvFS 用内部原子操作)
// - pwm 参数由 VFS 层传入, FS 实现负责权限检查

/// `fs_open` 返回结果
#[derive(Debug, Clone, Copy)]
pub struct FsOpenResult {
    /// FS 内部不透明 handle (RamFS 填 node_id, HvFS 填 fd)
    pub handle: u32,
    /// 文件初始偏移
    pub offset: u64,
    /// 文件类型 (VfsFileType::as_u8)
    pub file_type: u8,
}

/// 文件系统策略接口 — services 层实现, framework 层调用
///
/// 所有方法返回 `KernelResult<T>`, 由 VFS api.rs 统一转为 i32 错误码.
/// 新增文件系统只需实现本 trait 并注册到 VFS_MANAGER, 无需修改 framework.
pub trait FileSystem: Send + Sync {
    /// 文件系统名称 (如 "ramfs", "hvfs")
    fn name(&self) -> &'static str;

    // ---- 生命周期 ----

    /// 初始化文件系统 (mount 前调用)
    fn fs_init(&self) -> KernelResult<()>;
    /// 挂载到指定路径
    fn fs_mount(&self, path: &str) -> KernelResult<()>;

    // ---- 文件操作 ----

    /// 打开文件, 返回 Inode trait object
    ///
    /// Plan B: 返回 `Arc<dyn Inode>` 替代原来的 `FsOpenResult`.
    /// 调用者 (VFS API) 直接将返回的 Inode 封装为 `OpenFile`.
    fn fs_open(&self, rel_path: &str, flags: u32, pwm: u64) -> KernelResult<Arc<dyn Inode>>;
    /// 关闭文件
    fn fs_close(&self, handle: u32) -> KernelResult<()>;
    /// 读文件, 返回实际读取字节数
    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize>;
    /// 写文件, 返回实际写入字节数
    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize>;

    // ---- 元数据 ----

    /// 获取文件属性
    fn fs_stat(&self, rel_path: &str, pwm: u64) -> KernelResult<VfsStat>;
    /// 修改文件权限
    fn fs_chmod(&self, rel_path: &str, mode: u16, pwm: u64) -> KernelResult<()>;
    /// 修改文件所有者
    fn fs_chown(&self, rel_path: &str, owner_pwm: u64, group_pwm: u64, pwm: u64) -> KernelResult<()>;

    // ---- 目录操作 ----

    /// 创建目录
    fn fs_mkdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()>;
    /// 删除文件
    fn fs_unlink(&self, rel_path: &str, pwm: u64) -> KernelResult<()>;
    /// 删除目录
    fn fs_rmdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()>;
    /// 重命名
    fn fs_rename(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()>;
    /// 读取目录项, 返回 true 表示还有更多项
    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool>;

    // ---- 符号链接 ----

    /// 创建符号链接
    fn fs_symlink(&self, target: &str, link_path: &str, pwm: u64) -> KernelResult<()>;
    /// 读取符号链接目标
    fn fs_readlink(&self, rel_path: &str, buf: &mut [u8]) -> KernelResult<usize>;
    /// 创建硬链接
    fn fs_link(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()>;

    // ---- 扩展 (可选, 默认返回 NotSupported) ----

    /// 截断文件
    fn fs_truncate(&self, handle: u32, size: u64, pwm: u64) -> KernelResult<()> {
        let _ = (handle, size, pwm);
        Err(KernelError::NotSupported)
    }

    /// seek
    fn fs_seek(&self, handle: u32, offset: i64, whence: VfsSeekWhence, current: u64) -> KernelResult<u64> {
        let _ = (handle, offset, whence, current);
        Err(KernelError::NotSupported)
    }

    /// 解析路径到内部 handle (用于 inotify/flock 等需要 inode 的场景)
    fn fs_resolve_path(&self, rel_path: &str) -> Option<u32> {
        let _ = rel_path;
        None
    }

    /// 从 inode_id + mount_idx 构造 Inode trait object (用于 open_by_handle_at)
    ///
    /// 各 FS 应 override 此方法, 返回原生 Inode.
    /// 默认实现返回 None, 调用方应回退到 LegacyInode.
    fn fs_resolve_inode(&self, _inode_id: u32, _mount_idx: u32) -> Option<Arc<dyn Inode>> {
        None
    }

    /// 创建文件 (CREAT 语义), 返回 Inode trait object
    fn fs_create(&self, parent_path: &str, name: &str, pwm: u64) -> KernelResult<Arc<dyn Inode>> {
        let _ = (parent_path, name, pwm);
        Err(KernelError::NotSupported)
    }

    /// 把文件系统内存态同步到底层 (例如 HvFS 的 txg commit).
    /// 大多数 FS (RamFS/DevFS) 没有持久化, 默认实现为 Ok(()). 需要实际
    /// 刷盘/事务提交的 FS (HvFS) 应当 override 本方法.
    fn fs_sync(&self) -> KernelResult<()> {
        Ok(())
    }

    /// 按 inode_id 直接读取 (mmap prewarm 用)
    ///
    /// 区别于 `fs_read`: 不依赖 fd handle, 直接按 inode 寻址.
    /// 默认实现 NotSupported, 因为 mmap prewarm 不是所有 FS 都必须支持.
    /// 真实实现 (RamFS) 应当 override 本方法, 在 mmap #PF miss 路径上被
    /// `vfs_pread_inode` 调用以同步填 Page Cache.
    fn fs_pread_inode(&self, _node_id: u32, _offset: u64, _buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }

    // ---- 扩展属性 (可选, 默认返回 NotSupported) ----

    /// 设置扩展属性
    fn fs_setxattr(&self, _rel_path: &str, _name: &str, _value: &[u8], _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 获取扩展属性
    fn fs_getxattr(&self, _rel_path: &str, _name: &str, _buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }

    /// 列出扩展属性
    fn fs_listxattr(&self, _rel_path: &str, _buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }

    /// 删除扩展属性
    fn fs_removexattr(&self, _rel_path: &str, _name: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }
}

// ============================================================================
// OpenFile — 打开文件描述 (POSIX open file description)
// ============================================================================
//
// 对应 POSIX "打开文件描述", 多个 fd 可共享 (dup 语义).
// offset 和 flags 在所有共享者之间共享.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use alloc::sync::Arc;
use super::inode::Inode;

/// 打开文件描述 — POSIX open file description 的 QueenX 实现
///
/// 多个 fd 可以指向同一个 OpenFile (通过 dup).
/// offset 和 flags 在所有共享者之间共享.
///
/// Plan B: 持有 `Arc<dyn Inode>` 替代原来的 `inode_id: u32`,
/// 实现完全的 POSIX 打开文件描述语义.
pub struct OpenFile {
    /// Inode trait object — 文件级 I/O 操作
    inode: Arc<dyn Inode>,
    /// 共享文件偏移 (原子操作, dup 共享)
    pub offset: AtomicU64,
    /// 共享状态标志 (O_RDONLY, O_APPEND 等, dup 共享)
    pub flags: u32,
    /// 权限凭证
    pub pwm: u64,
    /// 引用计数 (dup 增加, close 减少)
    pub refcount: AtomicU32,
    /// 文件类型 (VfsFileType::as_u8)
    pub file_type: u8,
    /// 是否匿名文件 (memfd)
    pub is_anonymous: bool,
}

impl core::fmt::Debug for OpenFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OpenFile")
            .field("inode_id", &self.inode.node_id())
            .field("offset", &self.offset)
            .field("flags", &self.flags)
            .field("refcount", &self.refcount)
            .field("file_type", &self.file_type)
            .field("is_anonymous", &self.is_anonymous)
            .finish()
    }
}

impl OpenFile {
    /// 创建新的 OpenFile (Plan B: 使用 Inode trait object)
    pub fn new(inode: Arc<dyn Inode>, flags: u32, pwm: u64, file_type: u8) -> Self {
        Self {
            inode,
            offset: AtomicU64::new(0),
            flags,
            pwm,
            refcount: AtomicU32::new(1),
            file_type,
            is_anonymous: false,
        }
    }

    /// 创建匿名文件 OpenFile (memfd 用)
    pub fn new_anonymous(inode: Arc<dyn Inode>, flags: u32, pwm: u64, file_type: u8) -> Self {
        let mut of = Self::new(inode, flags, pwm, file_type);
        of.is_anonymous = true;
        of
    }

    // ---- 兼容旧 API (折中实现过渡期) ----

    /// 获取底层 inode_id (兼容旧代码, 供 pcache 等使用)
    ///
    /// 注意: 新代码应优先使用 `inode()` 方法.
    pub fn inode_id(&self) -> u32 {
        self.inode.node_id()
    }

    /// 获取挂载点索引 (兼容旧代码)
    pub fn mount_idx(&self) -> u32 {
        self.inode.mount_idx()
    }

    /// 获取 Inode trait object 引用
    pub fn inode(&self) -> &dyn Inode {
        &*self.inode
    }

    /// 获取 Inode Arc 引用 (用于 dup 时克隆)
    pub fn inode_arc(&self) -> &Arc<dyn Inode> {
        &self.inode
    }

    /// 增加引用计数 (dup 时调用)
    pub fn inc_ref(&self) {
        self.refcount.fetch_add(1, Ordering::Relaxed);
    }

    /// 减少引用计数 (close 时调用)
    /// 返回减少前的值, 如果返回 1 说明这是最后一个引用
    pub fn dec_ref(&self) -> u32 {
        self.refcount.fetch_sub(1, Ordering::Release)
    }

    /// 获取当前引用计数
    pub fn ref_count(&self) -> u32 {
        self.refcount.load(Ordering::Acquire)
    }

    /// 获取当前文件偏移
    pub fn get_offset(&self) -> u64 {
        self.offset.load(Ordering::Acquire)
    }

    /// 设置文件偏移
    pub fn set_offset(&self, offset: u64) {
        self.offset.store(offset, Ordering::Release);
    }

    /// 获取状态标志
    pub fn get_flags(&self) -> u32 {
        self.flags
    }

    /// 设置状态标志
    pub fn set_flags(&mut self, flags: u32) {
        self.flags = flags;
    }
}
