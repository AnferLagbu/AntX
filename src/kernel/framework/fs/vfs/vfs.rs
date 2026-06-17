use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use super::types::{KernelError, FileSystem};
use super::types::*;

pub struct VfsMount {
    pub path: [u8; VFS_MAX_PATH],
    fs_type: FsType,
    /// E6-4: trait object 分发 (优先于 fs_type match)
    fs: Option<&'static dyn FileSystem>,
    pub used: bool,
}

impl Clone for VfsMount {
    fn clone(&self) -> Self {
        Self {
            path: self.path,
            fs_type: self.fs_type,
            fs: self.fs,
            used: self.used,
        }
    }
}

impl VfsMount {
    pub const fn new() -> Self {
        Self {
            path: [0; VFS_MAX_PATH],
            fs_type: FsType::Unknown,
            fs: None,
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
        let end = self
            .path
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(VFS_MAX_PATH);
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

    /// E6-4: 获取 trait object (优先于 fs_type match)
    pub fn get_fs(&self) -> Option<&'static dyn FileSystem> {
        self.fs
    }

    /// E6-4: 设置 trait object (注册文件系统时调用)
    pub fn set_fs(&mut self, fs: &'static dyn FileSystem) {
        self.fs = Some(fs);
    }
}

pub struct VfsFile {
    pub fd: u32,
    pub node_id: u32,
    pub offset: u64,
    pub flags: u32,
    pub pwm: u64,
    pub used: bool,
    pub file_type: u8,
    pub path: [u8; VFS_MAX_PATH],
}

impl Clone for VfsFile {
    fn clone(&self) -> Self {
        Self {
            fd: self.fd,
            node_id: self.node_id,
            offset: self.offset,
            flags: self.flags,
            pwm: self.pwm,
            used: self.used,
            file_type: self.file_type,
            path: self.path,
        }
    }
}

impl VfsFile {
    pub const fn new() -> Self {
        Self {
            fd: 0,
            node_id: 0,
            offset: 0,
            flags: 0,
            pwm: 0,
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
        let end = self
            .path
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(VFS_MAX_PATH);
        core::str::from_utf8(&self.path[..end]).unwrap_or("")
    }
}

pub struct ResolvedMount {
    pub mount_idx: usize,
    pub rel_path: &'static str,
    pub fs_type: FsType,
    /// E6-4: trait object (优先于 fs_type)
    pub fs: Option<&'static dyn FileSystem>,
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

// 所有字段 (Mutex<T>, AtomicU32) 自动实现 Send + Sync
// 因为 T: Send 对所有内部类型成立 ([VfsMount; N], [VfsFile; N], [u8; N], bool, Option<VfsSnapshot>)。

impl VfsManager {
    pub const fn new() -> Self {
        Self {
            mounts: Mutex::new([
                VfsMount::new(),
                VfsMount::new(),
                VfsMount::new(),
                VfsMount::new(),
                VfsMount::new(),
                VfsMount::new(),
                VfsMount::new(),
                VfsMount::new(),
            ]),
            fd_table: Mutex::new([
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
                VfsFile::new(),
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
            fd.node_id = 0;
            fd.offset = 0;
            fd.flags = 0;
            fd.pwm = 0;
            fd.file_type = 0;
            fd.set_path("");
        }

        let mut cwd = self.cwd.lock();
        cwd[0] = b'/';
        cwd[1] = 0;

        self.next_fd.store(3, Ordering::SeqCst);

        *self.initialized.lock() = true;

        if let Some(dom) = crate::kernel::framework::barrier::RECOVERY_MANAGER.lock().find(2) {
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

    /// E6-4: 解析挂载点, 同时返回 trait object (优先于 fs_type)
    pub fn resolve_mount_fs(&self, path: &str) -> Option<(usize, FsType, Option<&'static dyn FileSystem>)> {
        let mount_idx = self.find_mount(path)?;
        let (fs_type, fs) = {
            let mounts = self.mounts.lock();
            if mount_idx < VFS_MAX_MOUNTS && mounts[mount_idx].used {
                (mounts[mount_idx].get_fs_type(), mounts[mount_idx].get_fs())
            } else {
                (FsType::Unknown, None)
            }
        };
        if fs_type == FsType::Unknown && fs.is_none() {
            return None;
        }
        Some((mount_idx, fs_type, fs))
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
            fd_table[idx].node_id = 0;
            fd_table[idx].offset = 0;
        }
    }

    pub fn set_fd(
        &self,
        idx: usize,
        node_id: u32,
        offset: u64,
        flags: u32,
        pwm: u64,
        file_type: u8,
        path: &str,
    ) {
        let mut fd_table = self.fd_table.lock();
        if idx < VFS_MAX_FDS {
            fd_table[idx].node_id = node_id;
            fd_table[idx].offset = offset;
            fd_table[idx].flags = flags;
            fd_table[idx].pwm = pwm;
            fd_table[idx].file_type = file_type;
            fd_table[idx].set_path(path);
        }
    }

    pub fn get_fd_info(&self, idx: usize) -> Option<(u32, u64, u64)> {
        let fd_table = self.fd_table.lock();
        if idx < VFS_MAX_FDS && fd_table[idx].used {
            Some((
                fd_table[idx].node_id,
                fd_table[idx].offset,
                fd_table[idx].pwm,
            ))
        } else {
            None
        }
    }

    /// 通过 fd 查找其所属挂载点索引 (供 mmap 反查用)
    ///
    /// 与 `get_fd_info` 不同: 不返回 inode/offset/pwm, 而是把 fd 打开时
    /// 的路径解析为挂载点. mmap 后续 #PF miss 需要该信息以决定
    /// 调哪个 FileSystem trait 的 fs_pread_inode.
    pub fn get_fd_mount_idx(&self, idx: usize) -> Option<usize> {
        // 1. 在 fd_table 锁内拷贝路径 (避免借用锁的临时生命期)
        let path_buf: [u8; VFS_MAX_PATH] = {
            let fd_table = self.fd_table.lock();
            if idx >= VFS_MAX_FDS || !fd_table[idx].used {
                return None;
            }
            let mut buf = [0u8; VFS_MAX_PATH];
            buf.copy_from_slice(&fd_table[idx].path);
            buf
        };
        // 2. 锁释放后再解析挂载点 (find_mount 会自己加 mounts 锁)
        let path_end = path_buf.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_PATH);
        let path_str = core::str::from_utf8(&path_buf[..path_end]).unwrap_or("");
        self.find_mount(path_str)
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

    /// E6-4: 带 trait object 的挂载 (优先于 fs_name match)
    pub fn mount_with_fs(&self, path: &str, fs_name: &str, fs: &'static dyn FileSystem) -> Result<(), KernelError> {
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
                mount.set_fs(fs);
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

fn vfs_barrier_capture_cb() {
    VFS_MANAGER.capture_snapshot();
}

fn vfs_barrier_rollback_cb() -> bool {
    VFS_MANAGER.restore_from_snapshot();
    true
}
