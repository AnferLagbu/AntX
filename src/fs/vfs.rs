use alloc::string::String;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

use super::types::*;

extern "C" {
    fn serial_putc(port: u16, c: i8);
}

fn log(s: &str) {
    unsafe {
        for c in s.bytes() {
            serial_putc(0x3F8, c as i8);
        }
    }
}

pub struct VfsMount {
    pub path: [u8; VFS_MAX_PATH],
    pub fs_name: [u8; 32],
    pub used: bool,
}

impl VfsMount {
    pub const fn new() -> Self {
        Self {
            path: [0; VFS_MAX_PATH],
            fs_name: [0; 32],
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
    
    pub fn set_fs_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(31);
        self.fs_name[..len].copy_from_slice(&bytes[..len]);
        self.fs_name[len] = 0;
    }
    
    pub fn get_fs_name(&self) -> &str {
        let end = self.fs_name.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.fs_name[..end]).unwrap_or("")
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

pub struct VfsManager {
    pub mounts: Mutex<[VfsMount; VFS_MAX_MOUNTS]>,
    pub fd_table: Mutex<[VfsFile; VFS_MAX_FDS]>,
    next_fd: AtomicU32,
    cwd: Mutex<[u8; VFS_MAX_PATH]>,
    initialized: Mutex<bool>,
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
            ]),
            next_fd: AtomicU32::new(3),
            cwd: Mutex::new([0; VFS_MAX_PATH]),
            initialized: Mutex::new(false),
        }
    }
    
    pub fn init(&self) {
        let mut mounts = self.mounts.lock();
        for mount in mounts.iter_mut() {
            mount.used = false;
        }
        
        let mut fd_table = self.fd_table.lock();
        for fd in fd_table.iter_mut() {
            fd.used = false;
        }
        
        let mut cwd = self.cwd.lock();
        cwd[0] = b'/';
        cwd[1] = 0;
        
        self.next_fd.store(3, Ordering::SeqCst);
        
        log("[VFS] Rust VFS initialized\n");
        *self.initialized.lock() = true;
    }
    
    pub fn find_mount(&self, path: &str) -> Option<usize> {
        let mounts = self.mounts.lock();
        let mut best_idx: Option<usize> = None;
        let mut best_depth = -1i32;
        
        for (i, mount) in mounts.iter().enumerate() {
            if !mount.used {
                continue;
            }
            
            let mount_path = mount.get_path();
            if path.starts_with(mount_path) {
                let depth = mount_path.matches('/').count() as i32;
                if depth > best_depth {
                    best_depth = depth;
                    best_idx = Some(i);
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
    
    pub fn mount(&self, path: &str, fs_name: &str) -> i32 {
        let mut mounts = self.mounts.lock();
        
        for mount in mounts.iter_mut() {
            if !mount.used {
                mount.set_path(path);
                mount.set_fs_name(fs_name);
                mount.used = true;
                
                log("[VFS] Mounted '");
                log(fs_name);
                log("' at '");
                log(path);
                log("'\n");
                
                return 0;
            }
        }
        
        -1
    }
    
    pub fn unmount(&self, path: &str) -> i32 {
        let mut mounts = self.mounts.lock();
        
        for mount in mounts.iter_mut() {
            if mount.used && mount.get_path() == path {
                mount.used = false;
                log("[VFS] Unmounted '");
                log(path);
                log("'\n");
                return 0;
            }
        }
        
        -1
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
}

pub static VFS_MANAGER: VfsManager = VfsManager::new();

pub fn init() {
    VFS_MANAGER.init();
}
