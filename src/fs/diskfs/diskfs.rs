use alloc::string::String;
use spin::Mutex;

use crate::fs::hvfs::hvfs::{get_hvfs, HVFS_O_CREAT, HVFS_O_WRONLY, HVFS_PERM_R, HVFS_DISK_OK, HVFS_DISK_NO_DISK, 
                    HVFS_DISK_UNFORMATTED};
use crate::fs::vfs::types::*;

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

pub const DISKFS_MAX_FDS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct DiskFsFd {
    pub fd: u32,
    pub inode_num: u32,
    pub offset: u64,
    pub flags: u32,
    pub pwid: u64,
    pub used: bool,
}

fn new_diskfs_fd() -> DiskFsFd {
    DiskFsFd { fd: 0, inode_num: 0, offset: 0, flags: 0, pwid: 0, used: false }
}

pub struct DiskFsData {
    fds: [DiskFsFd; DISKFS_MAX_FDS],
    next_fd: u32,
    mounted: bool,
}

unsafe impl Send for DiskFsData {}
unsafe impl Sync for DiskFsData {}

impl DiskFsData {
    pub fn new() -> Self {
        Self {
            fds: [new_diskfs_fd(); DISKFS_MAX_FDS],
            next_fd: 3,
            mounted: false,
        }
    }

    fn alloc_fd(&mut self) -> Option<usize> {
        for (i, fd) in self.fds.iter_mut().enumerate() {
            if !fd.used {
                fd.used = true;
                fd.fd = self.next_fd;
                self.next_fd += 1;
                return Some(i);
            }
        }
        None
    }

    fn free_fd(&mut self, idx: usize) {
        if idx < DISKFS_MAX_FDS {
            self.fds[idx].used = false;
            self.fds[idx].fd = 0;
            self.fds[idx].inode_num = 0;
            self.fds[idx].offset = 0;
        }
    }

    fn find_fd_by_system_fd(&self, fd: u32) -> Option<usize> {
        for (i, f) in self.fds.iter().enumerate() {
            if f.used && f.fd == fd {
                return Some(i);
            }
        }
        None
    }

    pub fn init(&mut self) {
        for i in 0..DISKFS_MAX_FDS {
            self.fds[i] = new_diskfs_fd();
        }
        self.next_fd = 3;
        self.mounted = false;
        
        log("[DiskFS] Initialized\n");
    }

    pub fn mount(&mut self, path: &str) -> i32 {
        if self.mounted {
            log("[DiskFS] Already mounted\n");
            return 0;
        }
        
        let mut hvfs = get_hvfs().lock();
        let status = hvfs.check_disk();
        
        match status {
            HVFS_DISK_OK => {
                log("[DiskFS] Found valid disk filesystem\n");
                if hvfs.mount() != 0 {
                    return -1;
                }
            }
            HVFS_DISK_NO_DISK => {
                log("[DiskFS] No disk detected\n");
                return -1;
            }
            HVFS_DISK_UNFORMATTED => {
                log("[DiskFS] Disk unformatted, formatting...\n");
                if hvfs.format() != 0 {
                    return -1;
                }
                hvfs.sync();
            }
            _ => {
                log("[DiskFS] Unknown disk status\n");
                return -1;
            }
        }
        
        drop(hvfs);
        
        self.mounted = true;
        
        log("[DiskFS] Mounted at '");
        log(path);
        log("'\n");
        
        0
    }

    pub fn unmount(&mut self) -> i32 {
        if !self.mounted {
            return 0;
        }
        
        {
            let mut hvfs = get_hvfs().lock();
            hvfs.sync();
        }
        
        self.mounted = false;
        
        log("[DiskFS] Unmounted\n");
        0
    }

    pub fn open(&mut self, path: &str, flags: u32, pwid: u64) -> Option<(u32, u64, u8)> {
        let mut hvfs = get_hvfs().lock();
        
        let inode_num = hvfs.resolve_path(path);
        
        let inode_num = if let Some(num) = inode_num {
            if (flags & VfsOpenFlags::TRUNC.bits()) != 0 {
                if let Some(inode) = hvfs.get_inode_mut(num) {
                    inode.size = 0;
                    inode.dirty = true;
                }
            }
            num
        } else if (flags & VfsOpenFlags::CREAT.bits()) != 0 {
            let hvfs_flags = HVFS_O_CREAT | HVFS_O_WRONLY;
            let result = hvfs.open(path, hvfs_flags, pwid);
            
            if result < 0 { return None; }
            
            hvfs.close(result as u32);
            
            hvfs.resolve_path(path)?
        } else {
            return None;
        };
        
        let inode = match hvfs.get_inode(inode_num) {
            Some(i) => i,
            None => return None,
        };
        
        if !hvfs.check_permission(inode, pwid, HVFS_PERM_R) {
            return None;
        }
        
        let fd_idx = self.alloc_fd()?;
        
        let offset = if (flags & VfsOpenFlags::APPEND.bits()) != 0 {
            inode.size as u64
        } else {
            0
        };
        
        let file_type = ((inode.mode >> 12) & 0xF) as u8;
        
        self.fds[fd_idx].inode_num = inode_num;
        self.fds[fd_idx].offset = offset;
        self.fds[fd_idx].flags = flags;
        self.fds[fd_idx].pwid = pwid;
        
        drop(hvfs);
        
        Some((inode_num, offset, file_type))
    }

    pub fn close(&mut self, system_fd: u32) -> i32 {
        if let Some(idx) = self.find_fd_by_system_fd(system_fd) {
            self.free_fd(idx);
            0
        } else {
            -1
        }
    }

    pub fn read(&mut self, system_fd: u32, buf: &mut [u8], count: u32) -> i32 {
        let idx = match self.find_fd_by_system_fd(system_fd) {
            Some(i) => i,
            None => return -1,
        };
        
        let pwid = self.fds[idx].pwid;
        
        let mut hvfs = get_hvfs().lock();
        let mut bytes_read = 0usize;
        let buf_slice = &mut buf[..count as usize];
        
        buf_slice.fill(0);
        bytes_read = count.min(buf_slice.len() as u32) as usize;
        self.fds[idx].offset += bytes_read as u64;
        
        drop(hvfs);
        
        bytes_read as i32
    }

    pub fn write(&mut self, system_fd: u32, buf: &[u8], count: u32) -> i32 {
        let idx = match self.find_fd_by_system_fd(system_fd) {
            Some(i) => i,
            None => return -1,
        };
        
        let inode_num = self.fds[idx].inode_num;
        let pwid = self.fds[idx].pwid;
        
        let mut hvfs = get_hvfs().lock();
        
        let bytes_written = (buf.len() as u32).min(count) as usize;
        let new_offset = self.fds[idx].offset + bytes_written as u64;
        
        if let Some(inode) = hvfs.get_inode_mut(inode_num) {
            if new_offset > inode.size as u64 {
                inode.size = new_offset as u32;
            }
            inode.mtime = crate::fs::hvfs::hvfs::HvFsData::get_time();
            inode.dirty = true;
        }
        
        self.fds[idx].offset = new_offset;
        
        bytes_written as i32
    }

    pub fn mkdir(&mut self, parent_path: &str, name: &str, pwid: u64) -> i32 {
        let mut full_path = [0u8; 128];
        let parent_bytes = parent_path.as_bytes();
        let name_bytes = name.as_bytes();
        
        let mut len = 0;
        for &b in parent_bytes {
            if len < 127 { full_path[len] = b; len += 1; }
        }
        if len > 0 && len < 127 && full_path[len-1] != b'/' {
            full_path[len] = b'/';
            len += 1;
        }
        for &b in name_bytes {
            if len < 127 { full_path[len] = b; len += 1; }
        }
        full_path[len] = 0;
        
        let path_str = core::str::from_utf8(&full_path[..len]).unwrap_or("/");
        
        let mut hvfs = get_hvfs().lock();
        hvfs.mkdir(path_str, pwid)
    }

    pub fn stat(&self, path: &str, pwid: u64) -> Option<VfsStat> {
        let mut hvfs = get_hvfs().lock();
        
        match hvfs.stat(path, pwid) {
            Some(inode) => Some(VfsStat {
                inode_num: inode.inode_num,
                mode: inode.pwid_perm,
                size: inode.size,
                atime: inode.atime,
                mtime: inode.mtime,
                ctime: inode.ctime,
                owner_pwid: inode.owner_pwid,
                perm: inode.pwid_perm,
                file_type: ((inode.mode >> 12) & 0xF) as u8,
                reserved: 0,
            }),
            None => None,
        }
    }

    pub fn sync(&self) -> i32 {
        let mut hvfs = get_hvfs().lock();
        hvfs.sync()
    }

    pub fn is_mounted(&self) -> bool {
        self.mounted
    }
}

pub static DISKFS_DATA: spin::Once<Mutex<DiskFsData>> = spin::Once::new();

pub fn get_diskfs() -> &'static Mutex<DiskFsData> {
    DISKFS_DATA.call_once(|| Mutex::new(DiskFsData::new()))
}

pub fn init() {
    let mut diskfs = get_diskfs().lock();
    diskfs.init();
}
