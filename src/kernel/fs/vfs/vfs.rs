use alloc::string::String;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

use super::types::*;
use super::types::KernelError;

pub struct VfsMount {
    pub path: [u8; VFS_MAX_PATH],
    fs_type: FsType,
    pub used: bool,
}

impl Clone for VfsMount {
    fn clone(&self) -> Self {
        Self {
            path: self.path,
            fs_type: self.fs_type,
            used: self.used,
        }
    }
}

impl VfsMount {
    pub const fn new() -> Self {
        Self {
            path: [0; VFS_MAX_PATH],
            fs_type: FsType::Unknown,
            used: false,
        }
    }

    pub fn set_path(&mut self, path: &str) {
        let bytes = path.as_bytes();
        let len = bytes.len().min(VFS_MAX_PATH - 1);
        self.path[..len].copy_from_slice(&bytes[..len]);
        self.path[len] = 0;
    }

    pub fn get_path(&self) -> &str {
        let end = self.path.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_PATH);
        core::str::from_utf8(&self.path[..end]).unwrap_or("")
    }

    pub fn set_fs_type(&mut self, name: &str) {
        self.fs_type = FsType::from_name(name);
    }

    pub fn get_fs_type(&self) -> FsType {
        self.fs_type
    }

    pub fn get_fs_name(&self) -> &str {
        self.fs_type.as_str()
    }
}

pub struct VfsFile {
    pub fd: u32,
    pub inode_num: u32,
    pub offset: u64,
    pub flags: u32,
    pub pwid: u64,
    pub used: bool,
    pub file_type: u8,
    pub path: [u8; VFS_MAX_PATH],
}

impl Clone for VfsFile {
    fn clone(&self) -> Self {
        Self {
            fd: self.fd, inode_num: self.inode_num, offset: self.offset,
            flags: self.flags, pwid: self.pwid, used: self.used,
            file_type: self.file_type, path: self.path,
        }
    }
}

impl VfsFile {
    pub const fn new() -> Self {
        Self {
            fd: 0,
            inode_num: 0,
            offset: 0,
            flags: 0,
            pwid: 0,
            used: false,
            file_type: 0,
            path: [0; VFS_MAX_PATH],
        }
    }

    pub fn set_path(&mut self, path: &str) {
        let bytes = path.as_bytes();
        let len = bytes.len().min(VFS_MAX_PATH - 1);
        self.path[..len].copy_from_slice(&bytes[..len]);
        self.path[len] = 0;
    }

    pub fn get_path(&self) -> &str {
        let end = self.path.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_PATH);
        core::str::from_utf8(&self.path[..end]).unwrap_or("")
    }
}

pub struct ResolvedMount {
    pub mount_idx: usize,
    pub rel_path: &'static str,
    pub fs_type: FsType,
}

pub struct VfsManager {
    pub mounts: Mutex<[VfsMount; VFS_MAX_MOUNTS]>,
    pub fd_table: Mutex<[VfsFile; VFS_MAX_FDS]>,
    next_fd: AtomicU32,
    cwd: Mutex<[u8; VFS_MAX_PATH]>,
    initialized: Mutex<bool>,
    snapshot: Mutex<Option<VfsSnapshot>>,
}

#[derive(Clone)]
struct VfsSnapshot {
    mounts: [VfsMount; VFS_MAX_MOUNTS],
    fd_table: [VfsFile; VFS_MAX_FDS],
    cwd: [u8; VFS_MAX_PATH],
    next_fd: u32,
}

unsafe impl Send for VfsManager {}
unsafe impl Sync for VfsManager {}

impl VfsManager {
    pub const fn new() -> Self {
        Self {
            mounts: Mutex::new([
                VfsMount::new(), VfsMount::new(), VfsMount::new(), VfsMount::new(),
                VfsMount::new(), VfsMount::new(), VfsMount::new(), VfsMount::new(),
            ]),
            fd_table: Mutex::new([
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
                VfsFile::new(), VfsFile::new(), VfsFile::new(), VfsFile::new(),
            ]),
            next_fd: AtomicU32::new(3),
            cwd: Mutex::new([0; VFS_MAX_PATH]),
            initialized: Mutex::new(false),
            snapshot: Mutex::new(None),
        }
    }

    pub fn init(&self) {
        let mut mounts = self.mounts.lock();
        for mount in mounts.iter_mut() {
            mount.used = false;
            mount.set_path("");
            mount.fs_type = FsType::Unknown;
        }

        let mut fd_table = self.fd_table.lock();
        for fd in fd_table.iter_mut() {
            fd.used = false;
            fd.fd = 0;
            fd.inode_num = 0;
            fd.offset = 0;
            fd.flags = 0;
            fd.pwid = 0;
            fd.file_type = 0;
            fd.set_path("");
        }

        let mut cwd = self.cwd.lock();
        cwd[0] = b'/';
        cwd[1] = 0;

        self.next_fd.store(3, Ordering::SeqCst);

        *self.initialized.lock() = true;

        if let Some(dom) = crate::kernel::barrier::RECOVERY_MANAGER.lock().find(2) {
            *dom.capture_cb.lock() = Some(vfs_barrier_capture_cb);
            *dom.rollback_cb.lock() = Some(vfs_barrier_rollback_cb);
        }
    }

    pub fn find_mount(&self, path: &str) -> Option<usize> {
        let mounts = self.mounts.lock();
        let mut best_idx: Option<usize> = None;
        let mut best_len = 0usize;

        for (i, mount) in mounts.iter().enumerate() {
            if !mount.used {
                continue;
            }

            let mount_path = mount.get_path();

            if path == mount_path {
                if mount_path.len() > best_len {
                    best_len = mount_path.len();
                    best_idx = Some(i);
                }
            } else if path.starts_with(mount_path) {
                let next_char = path.as_bytes().get(mount_path.len());
                if mount_path == "/" || next_char == Some(&b'/') {
                    if mount_path.len() > best_len {
                        best_len = mount_path.len();
                        best_idx = Some(i);
                    }
                }
            }
        }

        best_idx
    }

    pub fn get_relative_path<'a>(&self, path: &'a str, mount_idx: usize) -> &'a str {
        let mounts = self.mounts.lock();
        if mount_idx >= VFS_MAX_MOUNTS {
            return path;
        }

        let mount_path = mounts[mount_idx].get_path();
        let rel_path = &path[mount_path.len()..];

        let rel_path = rel_path.trim_start_matches('/');

        if rel_path.is_empty() {
            "/"
        } else {
            rel_path
        }
    }

    pub fn resolve_mount(&self, path: &str) -> Option<(usize, FsType)> {
        let mount_idx = self.find_mount(path)?;
        let fs_type = {
            let mounts = self.mounts.lock();
            if mount_idx < VFS_MAX_MOUNTS && mounts[mount_idx].used {
                mounts[mount_idx].get_fs_type()
            } else {
                FsType::Unknown
            }
        };
        if fs_type == FsType::Unknown {
            return None;
        }
        Some((mount_idx, fs_type))
    }

    pub fn alloc_fd(&self) -> Option<usize> {
        let mut fd_table = self.fd_table.lock();
        for (i, fd) in fd_table.iter_mut().enumerate() {
            if !fd.used {
                fd.used = true;
                fd.fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
                return Some(i);
            }
        }
        None
    }

    pub fn free_fd(&self, idx: usize) {
        let mut fd_table = self.fd_table.lock();
        if idx < VFS_MAX_FDS {
            fd_table[idx].used = false;
            fd_table[idx].fd = 0;
            fd_table[idx].inode_num = 0;
            fd_table[idx].offset = 0;
        }
    }

    pub fn set_fd(&self, idx: usize, inode_num: u32, offset: u64, flags: u32, pwid: u64, file_type: u8, path: &str) {
        let mut fd_table = self.fd_table.lock();
        if idx < VFS_MAX_FDS {
            fd_table[idx].inode_num = inode_num;
            fd_table[idx].offset = offset;
            fd_table[idx].flags = flags;
            fd_table[idx].pwid = pwid;
            fd_table[idx].file_type = file_type;
            fd_table[idx].set_path(path);
        }
    }

    pub fn get_fd_info(&self, idx: usize) -> Option<(u32, u64, u64)> {
        let fd_table = self.fd_table.lock();
        if idx < VFS_MAX_FDS && fd_table[idx].used {
            Some((fd_table[idx].inode_num, fd_table[idx].offset, fd_table[idx].pwid))
        } else {
            None
        }
    }

    pub fn set_fd_offset(&self, idx: usize, offset: u64) {
        let mut fd_table = self.fd_table.lock();
        if idx < VFS_MAX_FDS {
            fd_table[idx].offset = offset;
        }
    }

    pub fn mount(&self, path: &str, fs_name: &str) -> Result<(), KernelError> {
        let mut mounts = self.mounts.lock();

        for mount in mounts.iter() {
            if mount.used && mount.get_path() == path {
                return Err(KernelError::AlreadyExists);
            }
        }

        for mount in mounts.iter_mut() {
            if !mount.used {
                mount.set_path(path);
                mount.set_fs_type(fs_name);
                mount.used = true;

                return Ok(());
            }
        }

        Err(KernelError::NoSpace)
    }

    pub fn unmount(&self, path: &str) -> Result<(), KernelError> {
        let mut mounts = self.mounts.lock();

        for mount in mounts.iter_mut() {
            if mount.used && mount.get_path() == path {
                mount.used = false;
                return Ok(());
            }
        }

        Err(KernelError::NotFound)
    }

    pub fn set_cwd(&self, path: &str) {
        let mut cwd = self.cwd.lock();
        let bytes = path.as_bytes();
        let len = bytes.len().min(VFS_MAX_PATH - 1);
        cwd[..len].copy_from_slice(&bytes[..len]);
        cwd[len] = 0;
    }

    pub fn get_cwd(&self) -> String {
        let cwd = self.cwd.lock();
        let end = cwd.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_PATH);
        String::from(core::str::from_utf8(&cwd[..end]).unwrap_or("/"))
    }

    pub fn capture_snapshot(&self) {
        let mounts_data = {
            let m = self.mounts.lock();
            m.clone()
        };
        let fd_data = {
            let f = self.fd_table.lock();
            f.clone()
        };
        let cwd_data = *self.cwd.lock();
        let nf = self.next_fd.load(Ordering::SeqCst);
        *self.snapshot.lock() = Some(VfsSnapshot {
            mounts: mounts_data,
            fd_table: fd_data,
            cwd: cwd_data,
            next_fd: nf,
        });
    }

    pub fn restore_from_snapshot(&self) {
        if let Some(ref snap) = *self.snapshot.lock() {
            *self.mounts.lock() = snap.mounts.clone();
            *self.fd_table.lock() = snap.fd_table.clone();
            *self.cwd.lock() = snap.cwd;
            self.next_fd.store(snap.next_fd, Ordering::SeqCst);
        }
    }
}

pub static VFS_MANAGER: VfsManager = VfsManager::new();

pub fn init() {
    VFS_MANAGER.init();
}

extern "C" fn vfs_barrier_capture_cb() {
    VFS_MANAGER.capture_snapshot();
}

extern "C" fn vfs_barrier_rollback_cb() -> bool {
    VFS_MANAGER.restore_from_snapshot();
    true
}
