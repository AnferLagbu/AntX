#![deny(unsafe_code)]
//! `RamFS` 核心实现 — services 层 (E6-5 迁移)
//!
//! 从 framework/fs/ramfs/ramfs.rs 迁移而来。
//! 本文件包含 `RamFsData` 数据结构及其所有方法实现。
//! 0 unsafe, 100% safe Rust.
//!
//! ## 迁移变更
//! - `IrqSpinLock` 改用 `services::sync::irq_lock`
//! - `dcache` 改用 `services::fs::dcache` (直接引用)
//! - `credo::api` 保持 framework 引用 (公开 API)
//! - `crate::arch!(timestamp())` 改用 framework 公开的时钟 API

pub mod ramfs_node;
pub mod ramfs_data;

pub use ramfs_node::*;
pub use ramfs_data::*;

use crate::kernel::services::sync::irq_lock::IrqSpinLock as Mutex;
use crate::kernel::framework::fs::{FileSystem, KernelResult, VfsOpenFlags, VfsStat, VfsFileType, VfsDirEntry, VFS_MAX_NAME, VfsSeekWhence};
use crate::kernel::framework::fs::KernelError;

pub(crate) const RAMFS_MAX_NODES: usize = 256;
pub(crate) const RAMFS_MAX_BLOCKS: usize = 2048;
pub(crate) const RAMFS_BLOCK_SIZE: usize = crate::kernel::framework::mm::PAGE_SIZE as usize;
pub(crate) const RAMFS_MAX_ACES: usize = 128;
pub(crate) const INDIRECT_BLOCKS_PER_BLOCK: usize = RAMFS_BLOCK_SIZE / 4;
pub(crate) const SENSITIVITY_PUBLIC: u8 = 0;
pub(crate) const FS_CAP_READ: u64 = 1 << 0;
pub(crate) const FS_CAP_WRITE: u64 = 1 << 1;
pub(crate) const FS_CAP_CREATE: u64 = 1 << 3;

// ============================================================================
// 全局实例
// ============================================================================

pub static RAMFS_DATA: Mutex<RamFsData> = Mutex::new(RamFsData::new());

pub fn init() {
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.mount("/");
}

// ============================================================================
// E6-5: FileSystem trait 实现 (services 层)
// ============================================================================

impl FileSystem for RamFsData {
    fn name(&self) -> &'static str {
        "ramfs"
    }

    fn fs_init(&self) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mount(&self, path: &str) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        if ramfs.mount(path) != 0 {
            return Err(KernelError::Io);
        }
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, flags: u32, pwm: u64) -> KernelResult<alloc::sync::Arc<dyn crate::kernel::services::fs::inode::Inode>> {
        let mount_idx = 0; // RamFs 默认挂载索引
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.open(rel_path, flags, pwm) {
            Some((node_id, _offset, _file_type)) => {
                if (flags & VfsOpenFlags::TRUNC.bits()) != 0 {
                    ramfs.truncate(node_id, 0, pwm);
                }
                drop(ramfs);
                Ok(alloc::sync::Arc::new(crate::kernel::services::fs::inode::RamFsInode::new(node_id, mount_idx)))
            }
            None => Err(KernelError::FileNotFound),
        }
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut new_offset = offset;
        let result = ramfs.read(handle, &mut new_offset, buf, pwm);
        if result < 0 {
            Err(KernelError::Io)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut new_offset = offset;
        let result = ramfs.write(handle, &mut new_offset, buf, pwm);
        if result < 0 {
            Err(KernelError::Io)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let ramfs = RAMFS_DATA.lock();
        match ramfs.resolve_path(rel_path) {
            Some(node_id) => {
                drop(ramfs); // 释放锁, 尝试 icache
                if let Some(cached) = crate::kernel::services::fs::dcache::icache_lookup(node_id) {
                    return Ok(VfsStat {
                        node_id: cached.ino,
                        file_type: cached.file_type,
                        perm: cached.perm,
                        size: cached.size,
                        mtime: cached.mtime,
                        ctime: cached.ctime,
                        owner_pwm: cached.owner_pwm,
                        group_pwm: cached.group_pwm,
                        ..VfsStat::default()
                    });
                }
                let ramfs = RAMFS_DATA.lock();
                ramfs.stat(node_id).inspect(|st| {
                    crate::kernel::services::fs::dcache::icache_insert(
                        node_id, st.file_type, st.perm, st.size as u32, st.mtime,
                        st.ctime, st.owner_pwm, st.group_pwm,
                    );
                }).ok_or(KernelError::FileNotFound)
            }
            None => Err(KernelError::FileNotFound),
        }
    }

    fn fs_chmod(&self, rel_path: &str, mode: u16, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.chmod(rel_path, mode, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::PermissionDenied) }
    }

    fn fs_chown(&self, rel_path: &str, owner_pwm: u64, group_pwm: u64, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.chown_ext(rel_path, owner_pwm, group_pwm, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::PermissionDenied) }
    }

    fn fs_mkdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let (parent_path, name) = if let Some(pos) = rel_path.rfind('/') {
            if pos == 0 { ("/", &rel_path[1..]) } else { (&rel_path[..pos], &rel_path[pos + 1..]) }
        } else {
            ("/", rel_path)
        };
        if name.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        let result = ramfs.mkdir(parent_path, name, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::Io) }
    }

    fn fs_unlink(&self, rel_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.unlink(rel_path, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::FileNotFound) }
    }

    fn fs_rmdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.resolve_path(rel_path) {
            Some(node_id) => {
                let stat = ramfs.stat(node_id);
                match stat {
                    Some(s) if s.file_type == VfsFileType::Dir.as_u8() => {
                        let result = ramfs.truncate(node_id, 0, pwm);
                        if result == 0 { Ok(()) } else { Err(KernelError::Io) }
                    }
                    _ => Err(KernelError::NotADirectory),
                }
            }
            None => Err(KernelError::FileNotFound),
        }
    }

    fn fs_rename(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        ramfs.unlink(old_path, pwm);
        ramfs.link(0, 0, new_path, pwm);
        Ok(())
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut dir_offset = offset;
        let dirent_size = core::mem::size_of::<RamFsDirEntry>();
        let mut raw_buf = alloc::vec![0u8; dirent_size];
        let result = ramfs.read(handle, &mut dir_offset, &mut raw_buf, 0);
        let raw_entry = RamFsDirEntry::read_at(&raw_buf, 0);
        if result <= 0 || raw_entry.node == 0 {
            return Ok(false);
        }
        entry.node = raw_entry.node;
        entry.file_type = raw_entry.file_type;
        let name_len = raw_entry.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
        let copy_len = name_len.min(VFS_MAX_NAME);
        entry.name[..copy_len].copy_from_slice(&raw_entry.name[..copy_len]);
        if name_len < VFS_MAX_NAME {
            entry.name[name_len] = 0;
        }
        Ok(raw_entry.node != 0)
    }

    // L4 重构: 扩展方法实现 (override trait 默认实现)
    // P3-I-19: vfs_pread_inode trait 分发. 直接按 inode 寻址 (mmap prewarm).
    fn fs_pread_inode(&self, node_id: u32, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut new_offset = offset;
        let result = ramfs.read(node_id, &mut new_offset, buf, pwm);
        if result < 0 {
            Err(KernelError::Io)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_symlink(&self, target: &str, link_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let (parent_path, name) = if let Some(pos) = link_path.rfind('/') {
            if pos == 0 { ("/", &link_path[1..]) } else { (&link_path[..pos], &link_path[pos + 1..]) }
        } else {
            ("/", link_path)
        };
        if name.is_empty() || name.contains('/') {
            return Err(KernelError::InvalidArgument);
        }
        let result = ramfs.symlink(target, parent_path, name, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::Io) }
    }

    fn fs_readlink(&self, rel_path: &str, buf: &mut [u8]) -> KernelResult<usize> {
        let ramfs = RAMFS_DATA.lock();
        match ramfs.resolve_path(rel_path) {
            Some(node_id) => {
                let result = ramfs.readlink(node_id, buf);
                if result < 0 { Err(KernelError::Io) } else { Ok(result as usize) }
            }
            None => Err(KernelError::FileNotFound),
        }
    }

#[expect(clippy::manual_let_else, reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底")]
    fn fs_link(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let target_node = match ramfs.resolve_path(old_path) {
            Some(n) => n,
            None => return Err(KernelError::FileNotFound),
        };
        if (target_node as usize) >= ramfs.nodes.len() || !ramfs.nodes[target_node as usize].used {
            return Err(KernelError::FileNotFound);
        }
        if ramfs.nodes[target_node as usize].file_type == VfsFileType::Dir as u8 {
            return Err(KernelError::PermissionDenied);
        }
        let (parent_path, name) = if let Some(pos) = new_path.rfind('/') {
            if pos == 0 { ("/", &new_path[1..]) } else { (&new_path[..pos], &new_path[pos + 1..]) }
        } else {
            ("/", new_path)
        };
        if name.is_empty() || name.contains('/') {
            return Err(KernelError::InvalidArgument);
        }
        let parent_num = match ramfs.resolve_path(parent_path) {
            Some(n) => n,
            None => return Err(KernelError::FileNotFound),
        };
        let result = ramfs.link(parent_num, target_node, name, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::Io) }
    }

    fn fs_truncate(&self, handle: u32, size: u64, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.truncate(handle, size, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::Io) }
    }

    fn fs_seek(&self, handle: u32, offset: i64, whence: VfsSeekWhence, current: u64) -> KernelResult<u64> {
        let ramfs = RAMFS_DATA.lock();
        match ramfs.seek(handle, current, offset, whence) {
            Some(new_offset) => Ok(new_offset),
            None => Err(KernelError::InvalidArgument),
        }
    }

    fn fs_resolve_path(&self, rel_path: &str) -> Option<u32> {
        let ramfs = RAMFS_DATA.lock();
        ramfs.resolve_path(rel_path)
    }

    fn fs_create(&self, parent_path: &str, name: &str, pwm: u64) -> KernelResult<alloc::sync::Arc<dyn crate::kernel::services::fs::inode::Inode>> {
        let mount_idx = 0;
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.create_file(parent_path, name, pwm) {
            Some(new_inode) => {
                drop(ramfs);
                Ok(alloc::sync::Arc::new(crate::kernel::services::fs::inode::RamFsInode::new(new_inode, mount_idx)))
            }
            None => Err(KernelError::NoSpace),
        }
    }

    fn fs_resolve_inode(&self, inode_id: u32, mount_idx: u32) -> Option<alloc::sync::Arc<dyn crate::kernel::services::fs::inode::Inode>> {
        Some(alloc::sync::Arc::new(crate::kernel::services::fs::inode::RamFsInode::new(inode_id, mount_idx)))
    }
}
