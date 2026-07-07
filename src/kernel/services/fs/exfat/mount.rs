#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! exFAT FileSystem trait 实现

use crate::kernel::framework::fs::KernelError;
use crate::kernel::services::fs::vfs_types::*;
use super::read::ExfatFs;
use crate::kernel::framework::sync::IrqSpinLock as Mutex;

/// exFAT 文件系统实例 (全局单例)
static EXFAT_FS: Mutex<Option<ExfatFs>> = Mutex::new(None);

/// exFAT FileSystem trait 实现
pub struct ExfatFileSystem;

impl FileSystem for ExfatFileSystem {
    fn name(&self) -> &'static str {
        "exfat"
    }

    fn fs_init(&self) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mount(&self, _path: &str) -> KernelResult<()> {
        let fs = ExfatFs::open(0).map_err(|_| KernelError::IoError)?;
        let mut guard = EXFAT_FS.lock();
        *guard = Some(fs);
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, _flags: u32, _pwm: u64) -> KernelResult<FsOpenResult> {
        let fs_guard = EXFAT_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        let cluster = fs.lookup_path(rel_path)?;

        Ok(FsOpenResult {
            handle: cluster,
            offset: 0,
            file_type: 0,
        })
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let fs_guard = EXFAT_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        fs.read_file(handle, offset, buf)
    }

    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let fs_guard = EXFAT_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        fs.write_file(handle, offset, buf)
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let fs_guard = EXFAT_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        let cluster = fs.lookup_path(rel_path)?;

        Ok(VfsStat {
            node_id: cluster,
            mode: 0o777,
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwm: 0,
            group_pwm: 0,
            perm: 0o777,
            file_type: 0,
            sensitivity: 0,
        })
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Ok(())
    }

    fn fs_chown(&self, _rel_path: &str, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mkdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_unlink(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_rmdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let fs_guard = EXFAT_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        let entries = fs.read_dir_entries(handle)?;
        let idx = offset as usize;

        if idx >= entries.len() {
            return Ok(false);
        }

        let ext2_entry = &entries[idx];
        entry.node = idx as u32;
        entry.file_type = if ext2_entry.file_attributes() & 0x10 != 0 { 1 } else { 0 };
        entry.set_name(&alloc::format!("entry_{}", idx));

        Ok(true)
    }

    fn fs_symlink(&self, _target: &str, _link_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnly)
    }

    fn fs_readlink(&self, _rel_path: &str, _buf: &mut [u8]) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }

    fn fs_link(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnly)
    }
}

/// 初始化 exFAT 文件系统
pub fn init() {
    // exFAT 需要手动挂载
}