#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 FileSystem trait 实现

use crate::kernel::framework::fs::KernelError;
use crate::kernel::services::fs::vfs_types::*;
use super::read::Ext2Fs;
use crate::kernel::framework::sync::IrqSpinLock as Mutex;

/// ext2 文件系统实例 (全局单例)
static EXT2_FS: Mutex<Option<Ext2Fs>> = Mutex::new(None);

/// ext2 FileSystem trait 实现
pub struct Ext2FileSystem;

impl FileSystem for Ext2FileSystem {
    fn name(&self) -> &'static str {
        "ext2"
    }

    fn fs_init(&self) -> KernelResult<()> {
        // ext2 不需要特殊初始化
        Ok(())
    }

    fn fs_mount(&self, _path: &str) -> KernelResult<()> {
        // ext2 挂载需要指定设备
        // 当前实现: 假设设备 0
        let fs = Ext2Fs::open(0).map_err(|_| KernelError::IoError)?;
        let mut guard = EXT2_FS.lock();
        *guard = Some(fs);
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, _flags: u32, _pwm: u64) -> KernelResult<FsOpenResult> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let inode_num = fs.lookup_path(rel_path)?;
        let inode = fs.read_inode(inode_num)?;

        Ok(FsOpenResult {
            handle: inode_num,
            offset: 0,
            file_type: inode.file_type(),
        })
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        // ext2 只读, 不需要特殊关闭操作
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        fs.read_file(handle, offset, buf)
    }

    fn fs_write(&self, _handle: u32, _offset: u64, _buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let inode_num = fs.lookup_path(rel_path)?;
        let inode = fs.read_inode(inode_num)?;

        Ok(VfsStat {
            node_id: inode_num,
            mode: inode.perm(),
            uid: inode.i_uid as u32,
            gid: inode.i_gid as u32,
            size: inode.i_size,
            atime: inode.i_atime as u64,
            mtime: inode.i_mtime as u64,
            ctime: inode.i_ctime as u64,
            owner_pwm: 0,
            group_pwm: 0,
            perm: inode.perm(),
            file_type: inode.file_type(),
            sensitivity: 0,
        })
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_chown(&self, _rel_path: &str, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_mkdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_unlink(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_rmdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let entries = fs.read_dir(handle)?;
        let idx = offset as usize;

        if idx >= entries.len() {
            return Ok(false);
        }

        let ext2_entry = &entries[idx];
        entry.node = ext2_entry.inode;
        entry.file_type = ext2_entry.file_type;
        entry.set_name(ext2_entry.get_name());

        Ok(true)
    }

    fn fs_symlink(&self, _target: &str, _link_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_readlink(&self, _rel_path: &str, _buf: &mut [u8]) -> KernelResult<usize> {
        // 只读文件系统, 暂不支持符号链接读取
        Err(KernelError::NotSupported)
    }

    fn fs_link(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }
}

/// 初始化 ext2 文件系统
pub fn init() {
    // ext2 需要手动挂载, 不自动初始化
}