#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯 trait 定义。
//! Inode trait — 文件级操作抽象 (Plan B 核心)
//!
//! 每个文件系统通过实现本 trait 提供文件级 I/O 能力.
//! `OpenFile` 持有 `Arc<dyn Inode>` 替代原来的 `inode_id: u32`,
//! 实现 POSIX "打开文件描述" 语义 (多个 fd 共享同一 Inode).
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

extern crate alloc;

use alloc::sync::Arc;

use super::vfs_types::{KernelResult, VfsStat, VfsFileType, VfsSeekWhence, KernelError};

// ============================================================================
// Inode trait — 文件级操作
// ============================================================================

/// 文件级操作 trait — 每个文件系统实现此 trait
///
/// 所有方法接收 `&self` (非 `&mut self`), 内部可变性由实现者自行管理
/// (例如通过内部 Mutex/IrqSpinLock).
///
/// read/write 接收 offset 参数, 偏移由 `OpenFile` 层管理.
pub trait Inode: Send + Sync {
    /// 读取文件数据
    ///
    /// - `offset`: 文件内字节偏移
    /// - `buf`: 目标缓冲区
    /// - `pwm`: 权限凭证
    /// 返回: 实际读取的字节数
    fn read(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize>;

    /// 写入文件数据
    ///
    /// - `offset`: 文件内字节偏移
    /// - `buf`: 源数据
    /// - `pwm`: 权限凭证
    /// 返回: 实际写入的字节数
    fn write(&self, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize>;

    /// 获取文件属性
    fn stat(&self, pwm: u64) -> KernelResult<VfsStat>;

    /// 截断文件到指定大小
    fn truncate(&self, size: u64, pwm: u64) -> KernelResult<()>;

    /// 计算 seek 后的新偏移
    ///
    /// - `offset`: seek 偏移量 (字节)
    /// - `whence`: SEEK_SET/SEEK_CUR/SEEK_END
    /// - `current_offset`: 当前文件偏移
    /// 返回: 新的文件偏移
    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64>;

    /// 是否为目录
    fn is_dir(&self) -> bool;

    /// 读取目录项 (仅目录 Inode 实现)
    ///
    /// - `offset`: 目录偏移 (字节)
    /// 返回: (entry_name, file_type, has_more), 失败返回 KernelError
    fn readdir(&self, _offset: u64) -> KernelResult<(alloc::string::String, VfsFileType, bool)> {
        Err(KernelError::NotADirectory)
    }

    /// 创建子目录 (仅目录 Inode 实现)
    fn mkdir(&self, _name: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 删除文件 (仅目录 Inode 实现)
    fn unlink(&self, _name: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 重命名 (仅目录 Inode 实现)
    fn rename(&self, _old_name: &str, _new_name: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 创建硬链接 (仅目录 Inode 实现)
    fn link(&self, _name: &str, _target: &dyn Inode, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 创建符号链接 (仅目录 Inode 实现)
    fn symlink(&self, _name: &str, _target: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    /// 读取符号链接目标 (仅符号链接 Inode 实现)
    fn readlink(&self, _buf: &mut [u8]) -> KernelResult<usize> {
        Err(KernelError::InvalidArgument)
    }

    /// 修改权限
    fn chmod(&self, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Ok(())
    }

    /// 修改所有者
    fn chown(&self, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        Ok(())
    }

    /// 获取底层 inode_id (供 pcache 等需要 inode 标识的场景使用)
    fn node_id(&self) -> u32;

    /// 获取挂载点索引 (供 mmap 等需要挂载点信息的场景使用)
    fn mount_idx(&self) -> u32;

    /// 按 inode_id 直接读取 (mmap prewarm 用, 不经过 OpenFile offset)
    fn pread_inode(&self, _offset: u64, _buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }
}

// ============================================================================
// 通用实现: 匿名 Inode (memfd / 无路径文件)
// ============================================================================

use super::anonymous::ANONYMOUS_FS;

/// 匿名文件 Inode — memfd / 无路径文件的 Inode 实现
pub struct AnonymousInode {
    inode_id: u32,
    mount_idx: u32,
}

impl AnonymousInode {
    /// 创建新的匿名 Inode
    pub fn new(inode_id: u32) -> Self {
        Self {
            inode_id,
            mount_idx: u32::MAX, // 匿名文件无挂载点
        }
    }
}

impl Inode for AnonymousInode {
    fn read(&self, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        ANONYMOUS_FS
            .read_at(self.inode_id, offset, buf)
            .ok_or(KernelError::IoError)
    }

    fn write(&self, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        ANONYMOUS_FS
            .write_at(self.inode_id, offset, buf)
            .ok_or(KernelError::IoError)
    }

    fn stat(&self, _pwm: u64) -> KernelResult<VfsStat> {
        let size = ANONYMOUS_FS
            .get_size(self.inode_id)
            .unwrap_or(0);
        Ok(VfsStat {
            node_id: self.inode_id,
            size,
            file_type: VfsFileType::File.as_u8(),
            ..VfsStat::default()
        })
    }

    fn truncate(&self, size: u64, _pwm: u64) -> KernelResult<()> {
        if ANONYMOUS_FS.truncate(self.inode_id, size) {
            Ok(())
        } else {
            Err(KernelError::IoError)
        }
    }

    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64> {
        let file_size = ANONYMOUS_FS
            .get_size(self.inode_id)
            .unwrap_or(0) as u64;
        let new_offset = match whence {
            VfsSeekWhence::Set => offset as u64,
            VfsSeekWhence::Cur => current_offset.saturating_add(offset as u64),
            VfsSeekWhence::End => file_size.saturating_add(offset as u64),
        };
        Ok(new_offset)
    }

    fn is_dir(&self) -> bool {
        false
    }

    fn node_id(&self) -> u32 {
        self.inode_id
    }

    fn mount_idx(&self) -> u32 {
        self.mount_idx
    }
}

// ============================================================================
// RamFs Inode adapter — 将 RamFsData 包装为 Inode trait
// ============================================================================

/// RamFS 文件 Inode — 将全局 RamFsData 包装为 Inode trait
///
/// 每个实例对应一个 RamFS 中的文件节点.
/// 通过全局 `RAMFS_DATA` 锁实现内部可变性.
pub struct RamFsInode {
    inode_id: u32,
    mount_idx: u32,
}

impl RamFsInode {
    /// 创建新的 RamFS Inode
    pub fn new(inode_id: u32, mount_idx: u32) -> Self {
        Self { inode_id, mount_idx }
    }
}

impl Inode for RamFsInode {
    fn read(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
        let mut ramfs = RAMFS_DATA.lock();
        let (bytes_read, _new_offset) = ramfs.read_at_offset(self.inode_id, offset, buf, pwm);
        if bytes_read == 0 && offset >= ramfs.get_file_size(self.inode_id).unwrap_or(0) as u64 {
            // EOF
            Ok(0)
        } else if bytes_read > 0 {
            Ok(bytes_read)
        } else {
            Err(KernelError::IoError)
        }
    }

    fn write(&self, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize> {
        use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
        let mut ramfs = RAMFS_DATA.lock();
        let (bytes_written, _new_offset) = ramfs.write_at_offset(self.inode_id, offset, buf, pwm);
        if bytes_written > 0 {
            Ok(bytes_written)
        } else {
            Err(KernelError::IoError)
        }
    }

    fn stat(&self, pwm: u64) -> KernelResult<VfsStat> {
        use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
        let ramfs = RAMFS_DATA.lock();
        ramfs.get_stat(self.inode_id, pwm)
    }

    fn truncate(&self, size: u64, pwm: u64) -> KernelResult<()> {
        use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
        let mut ramfs = RAMFS_DATA.lock();
        let rc = ramfs.truncate(self.inode_id, size, pwm);
        if rc == 0 {
            Ok(())
        } else {
            Err(KernelError::IoError)
        }
    }

    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64> {
        use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
        let ramfs = RAMFS_DATA.lock();
        let file_size = ramfs.get_file_size(self.inode_id).unwrap_or(0) as u64;
        let new_offset = match whence {
            VfsSeekWhence::Set => offset as u64,
            VfsSeekWhence::Cur => current_offset.saturating_add(offset as u64),
            VfsSeekWhence::End => file_size.saturating_add(offset as u64),
        };
        Ok(new_offset)
    }

    fn is_dir(&self) -> bool {
        use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
        let ramfs = RAMFS_DATA.lock();
        if (self.inode_id as usize) < 256 {
            ramfs.nodes[self.inode_id as usize].file_type == 1 // DIR
        } else {
            false
        }
    }

    fn node_id(&self) -> u32 {
        self.inode_id
    }

    fn mount_idx(&self) -> u32 {
        self.mount_idx
    }

    fn pread_inode(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
        let mut ramfs = RAMFS_DATA.lock();
        let (bytes_read, _) = ramfs.read_at_offset(self.inode_id, offset, buf, pwm);
        Ok(bytes_read)
    }
}

// ============================================================================
// LegacyInode — 过渡期适配器: 将 FileSystem opaque handle 包装为 Inode trait
// ============================================================================
//
// Plan B 过渡期间, FileSystem::fs_open 仍返回 FsOpenResult { handle: u32 }.
// LegacyInode 将 handle + mount_idx 包装为 Inode trait object,
// 委托给 VFS_MANAGER 中注册的 FileSystem trait 方法.

/// 过渡期 Inode 适配器 — 将 FileSystem 的 opaque handle 包装为 Inode trait
///
/// 每次调用 Inode 方法时, 通过 mount_idx 查找 FileSystem trait object,
/// 委托给对应的 `fs_*` 方法. 性能不是最优, 但保证正确性.
pub struct LegacyInode {
    handle: u32,
    mount_idx: u32,
    file_type: u8,
    /// 文件相对路径 (供 stat/chmod/chown 等需要路径的操作使用)
    rel_path: alloc::string::String,
}

impl LegacyInode {
    /// 从 FsOpenResult 创建 LegacyInode
    pub fn from_fs_result(handle: u32, mount_idx: u32, file_type: u8, rel_path: &str) -> Self {
        Self {
            handle,
            mount_idx,
            file_type,
            rel_path: alloc::string::String::from(rel_path),
        }
    }
}

impl Inode for LegacyInode {
    fn read(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_read(self.handle, offset, buf, pwm),
            None => Err(KernelError::NotInitialized),
        }
    }

    fn write(&self, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_write(self.handle, offset, buf, pwm),
            None => Err(KernelError::NotInitialized),
        }
    }

    fn stat(&self, pwm: u64) -> KernelResult<VfsStat> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_stat(&self.rel_path, pwm),
            None => Err(KernelError::NotInitialized),
        }
    }

    fn truncate(&self, size: u64, pwm: u64) -> KernelResult<()> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_truncate(self.handle, size, pwm),
            None => Err(KernelError::NotInitialized),
        }
    }

    fn seek(&self, offset: i64, whence: VfsSeekWhence, current_offset: u64) -> KernelResult<u64> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_seek(self.handle, offset, whence, current_offset),
            None => Err(KernelError::NotInitialized),
        }
    }

    fn is_dir(&self) -> bool {
        self.file_type == VfsFileType::Dir.as_u8()
    }

    fn node_id(&self) -> u32 {
        self.handle
    }

    fn mount_idx(&self) -> u32 {
        self.mount_idx
    }

    fn chmod(&self, mode: u16, pwm: u64) -> KernelResult<()> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_chmod(&self.rel_path, mode, pwm),
            None => Err(KernelError::NotInitialized),
        }
    }

    fn chown(&self, owner_pwm: u64, group_pwm: u64, pwm: u64) -> KernelResult<()> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_chown(&self.rel_path, owner_pwm, group_pwm, pwm),
            None => Err(KernelError::NotInitialized),
        }
    }

    fn pread_inode(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        use crate::kernel::services::fs::vfs_manager::VFS_MANAGER;
        let fs = {
            let mounts = VFS_MANAGER.mounts.lock();
            if (self.mount_idx as usize) < mounts.len() && mounts[self.mount_idx as usize].used {
                mounts[self.mount_idx as usize].get_fs()
            } else {
                None
            }
        };
        match fs {
            Some(f) => f.fs_pread_inode(self.handle, offset, buf, pwm),
            None => Err(KernelError::NotInitialized),
        }
    }
}

// ============================================================================
// Inode 构造工厂
// ============================================================================

/// 创建匿名 Inode 的 Arc 包装
pub fn new_anonymous_inode(inode_id: u32) -> Arc<dyn Inode> {
    Arc::new(AnonymousInode::new(inode_id))
}

/// 创建 RamFS Inode 的 Arc 包装
pub fn new_ramfs_inode(inode_id: u32, mount_idx: u32) -> Arc<dyn Inode> {
    Arc::new(RamFsInode::new(inode_id, mount_idx))
}

/// 创建 LegacyInode 的 Arc 包装 (过渡期: 从 FsOpenResult 创建)
pub fn new_legacy_inode(handle: u32, mount_idx: u32, file_type: u8, rel_path: &str) -> Arc<dyn Inode> {
    Arc::new(LegacyInode::from_fs_result(handle, mount_idx, file_type, rel_path))
}
