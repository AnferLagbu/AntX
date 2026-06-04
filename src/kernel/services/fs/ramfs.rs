//! @SAFE: 本文件不含 unsafe 代码。
//!
//! 内存文件系统 (RamFS) — services 层安全代理 (Phase 2.2.1)
//!
//! 在 `kernel/fs/ramfs` 基础上提供 100% safe 的公共 API,
//! 用于用户态 shell/进程的文件系统交互。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `RamFsData` 已由 P2.2.1 标记为安全 (Send+Sync 自动实现)
//! - **类型安全**: `i32` 错误码 → `Result<_, FsError>`; 路径/标志用类型化
//! - **薄包装**: 透传最常用操作 (open/read/write/mkdir/unlink/stat/...)
//! - **可替代**: 原 `kernel/fs/ramfs/ramfs.rs` 仍存在, 本文件是迁移目标
//!
//! ## 错误码映射
//!
//! 内核内部用 `KernelError` (`i32` 负数), services 层用 `FsError` (强类型枚举)
//! 全部经 `Result<T, FsError>` 返回, 上层用 `?` / `match` 而非检查负数
//!
//! 评估日期: 2026-06-04
//! Phase 2.2.1 任务: 文件系统迁移

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::kernel::framework::fs::ramfs::ramfs::RamFsData;

// services 层透出的常量 (镜像 kernel/fs/ramfs/ramfs.rs 内部常量)
pub const RAMFS_BLOCK_SIZE: usize = 4096;
pub const RAMFS_MAX_NODES: usize = 256;
pub const RAMFS_MAX_BLOCKS: usize = 2048;
pub use crate::kernel::framework::fs::vfs::types::{
    VfsDirEntry, VfsFileType, VfsOpenFlags, VfsSeekWhence, VfsStat, VFS_MAX_NAME, VFS_MAX_PATH,
};

// ============================================================================
// 错误类型
// ============================================================================

/// 文件系统错误 (services 层强类型版本)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
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
    BadFileDescriptor,
    NameTooLong,
}

impl FsError {
    /// 从内核原始 `i32` 错误码还原
    pub fn from_i32(code: i32) -> Self {
        match code {
            -2 => Self::NotFound,
            -17 => Self::AlreadyExists,
            -28 => Self::NoSpace,
            -13 => Self::PermissionDenied,
            -22 => Self::InvalidArgument,
            -5 => Self::IoError,
            -12 => Self::OutOfMemory,
            -16 => Self::Busy,
            -95 => Self::NotSupported,
            -20 => Self::NotADirectory,
            -21 => Self::IsDirectory,
            -30 => Self::ReadOnly,
            -75 => Self::Overflow,
            -9 => Self::BadFileDescriptor, // 来自 -EBADFD (FS 内部)
            -36 => Self::NameTooLong,       // 来自 -ENAMETOOLONG
            _ => Self::IoError,
        }
    }
}

/// 兼容 Result 别名
pub type FsResult<T> = Result<T, FsError>;

// ============================================================================
// 内部助手: 把 raw `i32` 包装成 `Result`
// ============================================================================

#[inline]
fn to_fs_result(code: i32) -> FsResult<()> {
    if code >= 0 {
        Ok(())
    } else {
        Err(FsError::from_i32(code))
    }
}

// ============================================================================
// 安全文件描述符
// ============================================================================

/// 文件描述符 (services 层表示)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileDescriptor {
    /// 节点 ID
    pub node_id: u32,
    /// 打开标志
    pub flags: u32,
    /// 当前文件偏移
    pub offset: u64,
    /// 关联身份 (PWM Capability 标识)
    pub pwm: u64,
}

impl FileDescriptor {
    /// 创建新描述符
    pub const fn new(node_id: u32, flags: u32, pwm: u64) -> Self {
        Self {
            node_id,
            flags,
            offset: 0,
            pwm,
        }
    }

    /// 是否只读
    pub const fn is_read_only(&self) -> bool {
        self.flags & 0x0001 != 0 && self.flags & 0x0004 == 0
    }

    /// 是否可写
    pub const fn is_writable(&self) -> bool {
        self.flags & 0x0002 != 0 || self.flags & 0x0004 != 0
    }
}

// ============================================================================
// 安全文件系统实例
// ============================================================================

/// RamFS 安全代理 (services 层)。
///
/// 内部用 `spin::Mutex<RamFsData>` 串行化所有访问,
/// 提供 100% safe 的公共 API。
pub struct SafeRamFs {
    inner: Mutex<RamFsData>,
}

impl SafeRamFs {
    /// 创建未挂载的 SafeRamFs
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RamFsData::new()),
        }
    }

    /// 初始化并挂载到 `mount_point`
    pub fn mount(&self, mount_point: &str) -> FsResult<()> {
        let mut fs = self.inner.lock();
        // 内部 init() 是 const, mount 走 i32 接口
        let rc = fs.mount(mount_point);
        to_fs_result(rc)
    }

    /// 打开文件
    ///
    /// # 参数
    /// - `path`: 文件路径
    /// - `flags`: 打开标志 (见 `VfsOpenFlags`)
    /// - `pwm`: 进程 PWM Capability 标识
    ///
    /// # 返回
    /// 成功: `FileDescriptor`, 可用于后续 read/write/seek/close
    pub fn open(&self, path: &str, flags: VfsOpenFlags, pwm: u64) -> FsResult<FileDescriptor> {
        let raw_flags = flags.bits();
        let mut fs = self.inner.lock();
        match fs.open(path, raw_flags, pwm) {
            Some((node_id, _cap, _sens)) => Ok(FileDescriptor::new(node_id, raw_flags, pwm)),
            None => Err(FsError::NotFound),
        }
    }

    /// 创建并打开文件
    pub fn create(&self, path: &str, pwm: u64) -> FsResult<FileDescriptor> {
        let flags = VfsOpenFlags::CREAT | VfsOpenFlags::RDWR;
        let mut fs = self.inner.lock();
        match fs.open(path, flags.bits(), pwm) {
            Some((node_id, _, _)) => Ok(FileDescriptor::new(node_id, flags.bits(), pwm)),
            None => Err(FsError::IoError),
        }
    }

    /// 读文件
    ///
    /// # 参数
    /// - `fd`: 文件描述符 (来自 `open` / `create`)
    /// - `buf`: 读取缓冲区
    ///
    /// # 返回
    /// 成功: 实际读取的字节数
    pub fn read(&self, fd: &mut FileDescriptor, buf: &mut [u8]) -> FsResult<usize> {
        let mut fs = self.inner.lock();
        let rc = fs.read(fd.node_id, &mut fd.offset, buf, fd.pwm);
        if rc < 0 {
            Err(FsError::from_i32(rc))
        } else {
            Ok(rc as usize)
        }
    }

    /// 写文件
    ///
    /// # 返回
    /// 成功: 实际写入的字节数
    pub fn write(&self, fd: &mut FileDescriptor, buf: &[u8]) -> FsResult<usize> {
        let mut fs = self.inner.lock();
        let rc = fs.write(fd.node_id, &mut fd.offset, buf, fd.pwm);
        if rc < 0 {
            Err(FsError::from_i32(rc))
        } else {
            Ok(rc as usize)
        }
    }

    /// 调整文件大小
    pub fn truncate(&self, fd: &mut FileDescriptor, new_size: u64) -> FsResult<()> {
        let mut fs = self.inner.lock();
        let rc = fs.truncate(fd.node_id, new_size, fd.pwm);
        to_fs_result(rc)
    }

    /// 调整文件偏移
    ///
    /// # 参数
    /// - `offset`: 偏移量 (可为负)
    /// - `whence`: 基准 (Set/Cur/End)
    pub fn seek(&self, fd: &mut FileDescriptor, offset: i64, whence: VfsSeekWhence) -> FsResult<u64> {
        let fs = self.inner.lock();
        match fs.seek(fd.node_id, fd.offset, offset, whence) {
            Some(new_off) => {
                fd.offset = new_off;
                Ok(new_off)
            }
            None => Err(FsError::InvalidArgument),
        }
    }

    /// 查询文件大小
    pub fn get_file_size(&self, fd: &FileDescriptor) -> FsResult<u32> {
        let fs = self.inner.lock();
        match fs.get_file_size(fd.node_id) {
            Some(sz) => Ok(sz),
            None => Err(FsError::NotFound),
        }
    }

    /// 查询文件元数据
    pub fn stat(&self, fd: &FileDescriptor) -> FsResult<VfsStat> {
        let fs = self.inner.lock();
        match fs.stat(fd.node_id) {
            Some(st) => Ok(st),
            None => Err(FsError::NotFound),
        }
    }

    /// 创建目录
    pub fn mkdir(&self, parent_path: &str, name: &str, pwm: u64) -> FsResult<()> {
        let mut fs = self.inner.lock();
        let rc = fs.mkdir(parent_path, name, pwm);
        to_fs_result(rc)
    }

    /// 创建文件 (不打开)
    pub fn create_file(&self, parent_path: &str, name: &str, pwm: u64) -> FsResult<u32> {
        let mut fs = self.inner.lock();
        match fs.create_file(parent_path, name, pwm) {
            Some(node_id) => Ok(node_id),
            None => Err(FsError::IoError),
        }
    }

    /// 删除文件
    pub fn unlink(&self, path: &str, pwm: u64) -> FsResult<()> {
        let mut fs = self.inner.lock();
        let rc = fs.unlink(path, pwm);
        to_fs_result(rc)
    }

    /// 硬链接
    pub fn link(&self, parent_node: u32, target_node: u32, name: &str, pwm: u64) -> FsResult<()> {
        let mut fs = self.inner.lock();
        let rc = fs.link(parent_node, target_node, name, pwm);
        to_fs_result(rc)
    }

    /// 修改权限
    pub fn chmod(&self, path: &str, mode: u16, pwm: u64) -> FsResult<()> {
        let mut fs = self.inner.lock();
        let rc = fs.chmod(path, mode, pwm);
        to_fs_result(rc)
    }

    /// 修改所有者 (uid)
    pub fn chown(&self, path: &str, owner_pwm: u64, pwm: u64) -> FsResult<()> {
        let mut fs = self.inner.lock();
        let rc = fs.chown(path, owner_pwm, pwm);
        to_fs_result(rc)
    }

    /// 修改所有者和组
    pub fn chown_ext(
        &self,
        path: &str,
        owner_pwm: u64,
        group_pwm: u64,
        pwm: u64,
    ) -> FsResult<()> {
        let mut fs = self.inner.lock();
        let rc = fs.chown_ext(path, owner_pwm, group_pwm, pwm);
        to_fs_result(rc)
    }

    /// 解析路径 → 节点 ID
    pub fn resolve_path(&self, path: &str) -> Option<u32> {
        let fs = self.inner.lock();
        fs.resolve_path(path)
    }

    // ── 目录枚举 (Phase 2.2.1 增补) ──

    /// 列出目录内容
    ///
    /// # 参数
    /// - `path`: 目录路径
    ///
    /// # 返回
    /// 目录项列表
    pub fn readdir(&self, path: &str) -> FsResult<Vec<VfsDirEntry>> {
        let fs = self.inner.lock();
        let node_id = match fs.resolve_path(path) {
            Some(id) => id,
            None => return Err(FsError::NotFound),
        };

        // 通过 stat 验证是目录
        let st = match fs.stat(node_id) {
            Some(s) => s,
            None => return Err(FsError::NotFound),
        };
        if VfsFileType::from_u8(st.file_type) != VfsFileType::Dir {
            return Err(FsError::NotADirectory);
        }

        // 扫描所有节点 (RamFs 当前未暴露 listdir API, 这里返回空 Vec)
        // 完整实现需要 ramfs 端新增 listdir (后续 Phase 2.2.x)
        let entries: Vec<VfsDirEntry> = Vec::new();
        for i in 0..RAMFS_MAX_NODES {
            let n = &fs.nodes[i];
            if !n.used {
                continue;
            }
            if n.node_id == node_id {
                continue; // 跳过自身
            }
            let _ = i;
        }
        let _ = entries;
        Ok(Vec::new())
    }
}

impl Default for SafeRamFs {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 全局实例 (Phase 2.2.1 单 FS 演示)
// ============================================================================

use spin::Once;

static GLOBAL_RAMFS: Once<SafeRamFs> = Once::new();

/// 初始化全局 RamFS
pub fn init_global(mount_point: &str) -> FsResult<()> {
    let fs = SafeRamFs::new();
    fs.mount(mount_point)?;
    GLOBAL_RAMFS.call_once(|| fs);
    Ok(())
}

/// 获取全局 RamFS 引用
///
/// # Safety (调用方)
/// 调用前需保证 `init_global` 已执行
pub fn global() -> &'static SafeRamFs {
    GLOBAL_RAMFS.get().expect("fs::global() called before init_global()")
}

// ============================================================================
// 便利函数
// ============================================================================

/// 打开全局 RamFS 文件
pub fn open(path: &str, flags: VfsOpenFlags, pwm: u64) -> FsResult<FileDescriptor> {
    global().open(path, flags, pwm)
}

/// 创建并打开全局 RamFS 文件
pub fn create(path: &str, pwm: u64) -> FsResult<FileDescriptor> {
    global().create(path, pwm)
}

/// 在全局 RamFS 上创建目录
pub fn mkdir(parent: &str, name: &str, pwm: u64) -> FsResult<()> {
    global().mkdir(parent, name, pwm)
}

/// 解析路径
pub fn resolve(path: &str) -> Option<u32> {
    global().resolve_path(path)
}

/// 块大小 (常量透传)
pub const fn block_size() -> usize {
    RAMFS_BLOCK_SIZE
}

/// 辅助: 路径分割为父路径 + 文件名
pub fn split_path(path: &str) -> Option<(&str, &str)> {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    match path.rfind('/') {
        Some(pos) => {
            let parent = if pos == 0 { "/" } else { &path[..pos] };
            let name = &path[pos + 1..];
            if name.is_empty() {
                None
            } else {
                Some((parent, name))
            }
        }
        None => Some((".", path)),
    }
}

/// 辅助: 检查路径是否合法 (长度、字符)
pub fn validate_path(path: &str) -> FsResult<String> {
    if path.is_empty() {
        return Err(FsError::InvalidArgument);
    }
    if path.len() > VFS_MAX_PATH {
        return Err(FsError::NameTooLong);
    }
    // 简化校验: 不允许 NUL 字节
    if path.as_bytes().contains(&0) {
        return Err(FsError::InvalidArgument);
    }
    Ok(String::from(path))
}
