//! Inode trait — 文件级操作抽象 (Plan B 核心) — framework 层完整定义
//!
//! ## B09-12/DECISION-H13 P1-B3 迁移记录 (2026-08-31)
//!
//! Inode trait 是 VFS 句柄级操作机制, 按"机制归 framework"原则从
//! `services::fs::inode` 迁回本文件. 0 语义变更.
//! 具象实现 (AnonymousInode/RamFsInode/LegacyInode) 与各文件系统的
//! `impl Inode for ...` 保留在 services, 实现 framework 侧 trait
//! (services→framework 合法方向).
//!
//! ## 与 FileSystem trait 的关系
//!
//! - `FileSystem` trait: 路径级操作 (mount/open/stat by path)
//! - `Inode` trait: 句柄级操作 (read/write/stat by open file)
//! - `FileSystem::fs_open` 返回 `Arc<dyn Inode>`, VFS 层将其封装为 `OpenFile`
//!
//! ## offset 管理
//!
//! read/write 接收 offset 参数, 由 `OpenFile` 层管理共享偏移.
//! Inode 实现者只需按给定 offset 执行 I/O, 不需要管理偏移状态.
//! 这保证 POSIX dup 共享 offset 语义: 多个 fd 通过同一个 OpenFile
//! 共享 offset, Inode 本身是无状态的 I/O 执行器.

use super::types::{KernelError, KernelResult, VfsFileType, VfsSeekWhence, VfsStat};

pub trait Inode: Send + Sync {
    /// 读取文件数据
    ///
    /// - `offset`: 文件内字节偏移
    /// - `buf`: 目标缓冲区
    /// - `pwm`: 权限凭证
    /// 返回: 实际读取的字节数
    ///
    /// # Errors
    /// 当偏移超出文件末尾、底层 I/O 失败或权限不足时返回 `KernelError` (具体由实现者决定).
    fn read(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize>;

    /// 写入文件数据
    ///
    /// - `offset`: 文件内字节偏移
    /// - `buf`: 源数据
    /// - `pwm`: 权限凭证
    /// 返回: 实际写入的字节数
    ///
    /// # Errors
    /// 当文件只读、超出文件系统容量或底层 I/O 失败时返回 `KernelError` (具体由实现者决定).
    fn write(&self, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize>;

    /// 获取文件属性
    ///
    /// # Errors
    /// 当 inode 已失效或底层元数据读取失败时返回 `KernelError` (具体由实现者决定).
    fn stat(&self, pwm: u64) -> KernelResult<VfsStat>;

    /// 截断文件到指定大小
    ///
    /// # Errors
    /// 当文件只读、无权限或底层元数据更新失败时返回 `KernelError` (具体由实现者决定).
    fn truncate(&self, size: u64, pwm: u64) -> KernelResult<()>;

    /// 计算 seek 后的新偏移
    ///
    /// - `offset`: seek 偏移量 (字节)
    /// - `whence`: 定位基准 (`SEEK_SET/SEEK_CUR/SEEK_END`)
    /// - `current_offset`: 当前文件偏移
    /// 返回: 新的文件偏移
    ///
    /// # Errors
    /// 当 seek 结果超出可表示范围或 `whence` 非法时返回 `KernelError` (具体由实现者决定).
    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64>;

    /// 是否为目录
    fn is_dir(&self) -> bool;

    /// 读取目录项 (仅目录 Inode 实现)
    ///
    /// - `offset`: 目录偏移 (字节)
    /// 返回: (`entry_name`, `file_type`, `has_more`), 失败返回 `KernelError`
    ///
    /// # Errors
    /// 默认实现返回 `NotADirectory`; 当 inode 不是目录或底层目录读取失败时返回 `KernelError`.
    fn readdir(&self, _offset: u64) -> KernelResult<(alloc::string::String, VfsFileType, bool)> {
        Err(KernelError::NotADirectory)
    }

    /// 创建子目录 (仅目录 Inode 实现)
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported`; 当无权限、名称已存在或底层创建失败时返回 `KernelError`.
    fn mkdir(&self, _name: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 删除文件 (仅目录 Inode 实现)
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported`; 当无权限、文件不存在或底层删除失败时返回 `KernelError`.
    fn unlink(&self, _name: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 重命名 (仅目录 Inode 实现)
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported`; 当无权限、源/目标无效或底层重命名失败时返回 `KernelError`.
    fn rename(&self, _old_name: &str, _new_name: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 创建硬链接 (仅目录 Inode 实现)
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported`; 当无权限、名称已存在或底层链接创建失败时返回 `KernelError`.
    fn link(&self, _name: &str, _target: &dyn Inode, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 创建符号链接 (仅目录 Inode 实现)
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported`; 当无权限、名称已存在或底层符号链接创建失败时返回 `KernelError`.
    fn symlink(&self, _name: &str, _target: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 读取符号链接目标 (仅符号链接 Inode 实现)
    ///
    /// # Errors
    /// 默认实现返回 `InvalidArgument`; 当 inode 不是符号链接或目标读取失败时返回 `KernelError`.
    fn readlink(&self, _buf: &mut [u8]) -> KernelResult<usize> {
        Err(KernelError::InvalidArgument)
    }

    /// 修改权限
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported` (不支持按 inode 修改权限的 FS 显式报错, 不再静默成功); 当无权限或底层元数据更新失败时返回 `KernelError` (具体由实现者决定).
    fn chmod(&self, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 修改所有者
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported` (不支持按 inode 修改所有者的 FS 显式报错, 不再静默成功); 当无权限或底层元数据更新失败时返回 `KernelError` (具体由实现者决定).
    fn chown(&self, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 设置文件时间戳 (atime, mtime)
    ///
    /// - `atime`: 访问时间 (纳秒), `u64::MAX` 表示不修改
    /// - `mtime`: 修改时间 (纳秒), `u64::MAX` 表示不修改
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported`; 当无权限或底层元数据更新失败时返回 `KernelError` (具体由实现者决定).
    fn set_times(&self, _atime: u64, _mtime: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 获取底层 `inode_id` (供 pcache 等需要 inode 标识的场景使用)
    fn node_id(&self) -> u32;

    /// 获取挂载点索引 (供 mmap 等需要挂载点信息的场景使用)
    fn mount_idx(&self) -> u32;

    /// 按 `inode_id` 直接读取 (mmap prewarm 用, 不经过 `OpenFile` offset)
    ///
    /// # Errors
    /// 默认实现返回 `NotSupported`; 当底层读取失败时返回 `KernelError` (具体由实现者决定).
    fn pread_inode(&self, _offset: u64, _buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }
}
