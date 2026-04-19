use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

use super::types::*;

extern "C" {
    fn serial_putc(port: u16, c: i8);
    fn pwid_get_level(pwid: u64) -> u8;
}

fn log(s: &str) {
    unsafe {
        for c in s.bytes() {
            serial_putc(0x3F8, c as i8);
        }
    }
}

const RAMFS_MAX_INODES: usize = 64;
const RAMFS_MAX_BLOCKS: usize = 256;
const RAMFS_BLOCK_SIZE: usize = 512;

const PWID_LEVEL_ROOT: u8 = 0;

#[derive(Debug, Clone, Copy)]
pub struct RamFsInode {
    pub inode_num: u32,
    pub file_type: u8,
    pub perm: u16,
    pub size: u32,
    pub owner_pwid: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub direct_blocks: [u32; 8],
    pub link_count: u32,
    pub used: bool,
}

impl RamFsInode {
    pub const fn new() -> Self {
        Self {
            inode_num: 0,
            file_type: 0,
            perm: 0,
            size: 0,
            owner_pwid: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            direct_blocks: [0; 8],
            link_count: 0,
            used: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RamFsDirent {
    pub inode: u32,
    pub file_type: u8,
    pub name: [u8; VFS_MAX_NAME],
}

impl RamFsDirent {
    pub fn new() -> Self {
        Self {
            inode: 0,
            file_type: 0,
            name: [0; VFS_MAX_NAME],
        }
    }
    
    pub fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(VFS_MAX_NAME - 1);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len] = 0;
    }
}

pub struct RamFsData {
    pub inodes: [RamFsInode; RAMFS_MAX_INODES],
    pub data_area: [u8; RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE],
    pub inode_bitmap: [u8; RAMFS_MAX_INODES / 8],
    pub block_bitmap: [u8; RAMFS_MAX_BLOCKS / 8],
    pub root_inode: u32,
    pub free_inodes: AtomicU32,
    pub free_blocks: AtomicU32,
}

unsafe impl Send for RamFsData {}
unsafe impl Sync for RamFsData {}

impl RamFsData {
    pub const fn new() -> Self {
        Self {
            inodes: [RamFsInode::new(); RAMFS_MAX_INODES],
            data_area: [0; RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE],
            inode_bitmap: [0; RAMFS_MAX_INODES / 8],
            block_bitmap: [0; RAMFS_MAX_BLOCKS / 8],
            root_inode: 0,
            free_inodes: AtomicU32::new(0),
            free_blocks: AtomicU32::new(0),
        }
    }
    
    fn get_time() -> u64 {
        let tsc: u64;
        unsafe {
            core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack));
        }
        tsc
    }
    
    fn block_is_free(&self, block_num: u32) -> bool {
        if block_num as usize >= RAMFS_MAX_BLOCKS {
            return false;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        (self.block_bitmap[byte_idx] & (1 << bit_idx)) == 0
    }
    
    fn block_set_used(&mut self, block_num: u32) {
        if block_num as usize >= RAMFS_MAX_BLOCKS {
            return;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        self.block_bitmap[byte_idx] |= 1 << bit_idx;
        self.free_blocks.fetch_sub(1, Ordering::SeqCst);
    }
    
    fn block_alloc(&mut self) -> u32 {
        for i in 0..RAMFS_MAX_BLOCKS {
            if self.block_is_free(i as u32) {
                self.block_set_used(i as u32);
                let start = i * RAMFS_BLOCK_SIZE;
                for b in &mut self.data_area[start..start + RAMFS_BLOCK_SIZE] {
                    *b = 0;
                }
                return i as u32;
            }
        }
        0
    }
    
    fn inode_set_used(&mut self, inode_num: u32) {
        if inode_num as usize >= RAMFS_MAX_INODES {
            return;
        }
        let byte_idx = (inode_num / 8) as usize;
        let bit_idx = (inode_num % 8) as usize;
        self.inode_bitmap[byte_idx] |= 1 << bit_idx;
        self.free_inodes.fetch_sub(1, Ordering::SeqCst);
    }
    
    fn check_permission(&self, inode: &RamFsInode, pwid: u64, access_type: u16) -> bool {
        let level = unsafe { pwid_get_level(pwid) };
        
        if level == PWID_LEVEL_ROOT {
            return true;
        }
        
        if pwid == inode.owner_pwid {
            let owner_perm = (inode.perm >> 6) & 0x07;
            return (owner_perm & access_type) == access_type;
        }
        
        let other_perm = inode.perm & 0x07;
        (other_perm & access_type) == access_type
    }
    
    pub fn resolve_path(&self, path: &str) -> Option<u32> {
        let mut current = self.root_inode;
        let p = path.trim_start_matches('/');
        
        if p.is_empty() {
            return Some(current);
        }
        
        for component in p.split('/') {
            if component.is_empty() {
                continue;
            }
            
            let inode = &self.inodes[current as usize];
            
            if inode.file_type != VfsFileType::Dir as u8 {
                return None;
            }
            
            let block_num = inode.direct_blocks[0];
            if block_num == 0 {
                return None;
            }
            
            let dirent_size = core::mem::size_of::<RamFsDirent>();
            let num_entries = inode.size as usize / dirent_size;
            
            let mut found = false;
            
            for i in 0..num_entries {
                let offset = (block_num as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
                let entry: &RamFsDirent = unsafe {
                    &*(&self.data_area[offset] as *const u8 as *const RamFsDirent)
                };
                
                if entry.inode != 0 {
                    let end = entry.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
                    let name = core::str::from_utf8(&entry.name[..end]).unwrap_or("");
                    if name == component {
                        current = entry.inode;
                        found = true;
                        break;
                    }
                }
            }
            
            if !found {
                return None;
            }
        }
        
        Some(current)
    }
    
    pub fn mount(&mut self, _path: &str) -> i32 {
        for inode in self.inodes.iter_mut() {
            *inode = RamFsInode::new();
        }
        for b in self.data_area.iter_mut() {
            *b = 0;
        }
        for b in self.inode_bitmap.iter_mut() {
            *b = 0;
        }
        for b in self.block_bitmap.iter_mut() {
            *b = 0;
        }
        
        self.free_inodes.store((RAMFS_MAX_INODES - 1) as u32, Ordering::SeqCst);
        self.free_blocks.store(RAMFS_MAX_BLOCKS as u32, Ordering::SeqCst);
        self.root_inode = 1;
        
        let block = self.block_alloc();
        
        self.inodes[1] = RamFsInode {
            inode_num: 1,
            file_type: VfsFileType::Dir as u8,
            perm: 0o755,
            size: (2 * core::mem::size_of::<RamFsDirent>()) as u32,
            owner_pwid: 0,
            atime: Self::get_time(),
            mtime: Self::get_time(),
            ctime: Self::get_time(),
            direct_blocks: [block, 0, 0, 0, 0, 0, 0, 0],
            link_count: 2,
            used: true,
        };
        self.inode_set_used(1);
        
        let dirent_size = core::mem::size_of::<RamFsDirent>();
        let offset = (block as usize) * RAMFS_BLOCK_SIZE;
        
        let dot: &mut RamFsDirent = unsafe {
            &mut *(&mut self.data_area[offset] as *mut u8 as *mut RamFsDirent)
        };
        dot.inode = 1;
        dot.file_type = VfsFileType::Dir as u8;
        dot.set_name(".");
        
        let dotdot: &mut RamFsDirent = unsafe {
            &mut *(&mut self.data_area[offset + dirent_size] as *mut u8 as *mut RamFsDirent)
        };
        dotdot.inode = 1;
        dotdot.file_type = VfsFileType::Dir as u8;
        dotdot.set_name("..");
        
        log("[RamFS] Mounted\n");
        0
    }
    
    pub fn open(&mut self, path: &str, flags: u32, pwid: u64) -> Option<(u32, u64, u8)> {
        let inode_num = self.resolve_path(path);
        
        let inode_num = if let Some(num) = inode_num {
            num
        } else if (flags & VfsOpenFlags::CREAT.bits()) != 0 {
            let filename = path.rsplit('/').next().unwrap_or(path);
            let dir_path = if let Some(pos) = path.rfind('/') {
                if pos == 0 { "/" } else { &path[..pos] }
            } else {
                "/"
            };
            
            let parent_num = self.resolve_path(dir_path)?;
            
            if !self.check_permission(&self.inodes[parent_num as usize], pwid, VFS_PERM_W) {
                return None;
            }
            
            let new_inode_num = self.alloc_inode(VfsFileType::File as u8, pwid)?;
            
            let dirent_size = core::mem::size_of::<RamFsDirent>();
            let parent = &self.inodes[parent_num as usize];
            let block_num = parent.direct_blocks[0];
            let num_entries = parent.size as usize / dirent_size;
            
            let offset = (block_num as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size;
            let entry: &mut RamFsDirent = unsafe {
                &mut *(&mut self.data_area[offset] as *mut u8 as *mut RamFsDirent)
            };
            entry.inode = new_inode_num;
            entry.file_type = VfsFileType::File as u8;
            entry.set_name(filename);
            
            self.inodes[parent_num as usize].size += dirent_size as u32;
            self.inodes[parent_num as usize].mtime = Self::get_time();
            
            new_inode_num
        } else {
            return None;
        };
        
        let inode = &self.inodes[inode_num as usize];
        
        if !self.check_permission(inode, pwid, VFS_PERM_R) {
            return None;
        }
        
        let offset = if (flags & VfsOpenFlags::APPEND.bits()) != 0 {
            inode.size as u64
        } else {
            0
        };
        
        Some((inode_num, offset, inode.file_type))
    }
    
    fn alloc_inode(&mut self, file_type: u8, pwid: u64) -> Option<u32> {
        for i in 1..RAMFS_MAX_INODES {
            if !self.inodes[i].used {
                let block = self.block_alloc();
                self.inodes[i] = RamFsInode {
                    inode_num: i as u32,
                    file_type,
                    perm: if file_type == VfsFileType::Dir as u8 { 0o755 } else { 0o644 },
                    size: if file_type == VfsFileType::Dir as u8 { 
                        (2 * core::mem::size_of::<RamFsDirent>()) as u32 
                    } else { 
                        0 
                    },
                    owner_pwid: pwid,
                    atime: Self::get_time(),
                    mtime: Self::get_time(),
                    ctime: Self::get_time(),
                    direct_blocks: [block, 0, 0, 0, 0, 0, 0, 0],
                    link_count: 1,
                    used: true,
                };
                self.inode_set_used(i as u32);
                return Some(i as u32);
            }
        }
        None
    }
    
    pub fn read(&mut self, inode_num: u32, offset: &mut u64, buf: &mut [u8], pwid: u64) -> i32 {
        let inode = &self.inodes[inode_num as usize];
        
        if !self.check_permission(inode, pwid, VFS_PERM_R) {
            return -1;
        }
        
        let mut bytes_read = 0usize;
        let inode_size = inode.size as u64;
        
        while bytes_read < buf.len() && *offset < inode_size {
            let block_idx = (*offset as usize) / RAMFS_BLOCK_SIZE;
            let block_offset = (*offset as usize) % RAMFS_BLOCK_SIZE;
            let mut bytes_to_read = RAMFS_BLOCK_SIZE - block_offset;
            
            if bytes_to_read > buf.len() - bytes_read {
                bytes_to_read = buf.len() - bytes_read;
            }
            if bytes_to_read > (inode_size - *offset) as usize {
                bytes_to_read = (inode_size - *offset) as usize;
            }
            
            if block_idx < 8 {
                let block_num = inode.direct_blocks[block_idx];
                if block_num != 0 {
                    let start = (block_num as usize) * RAMFS_BLOCK_SIZE + block_offset;
                    buf[bytes_read..bytes_read + bytes_to_read]
                        .copy_from_slice(&self.data_area[start..start + bytes_to_read]);
                }
            }
            
            bytes_read += bytes_to_read;
            *offset += bytes_to_read as u64;
        }
        
        self.inodes[inode_num as usize].atime = Self::get_time();
        
        bytes_read as i32
    }
    
    pub fn write(&mut self, inode_num: u32, offset: &mut u64, buf: &[u8], pwid: u64) -> i32 {
        if !self.check_permission(&self.inodes[inode_num as usize], pwid, VFS_PERM_W) {
            return -1;
        }
        
        let mut bytes_written = 0usize;
        
        while bytes_written < buf.len() {
            let block_idx = (*offset as usize) / RAMFS_BLOCK_SIZE;
            let block_offset = (*offset as usize) % RAMFS_BLOCK_SIZE;
            let mut bytes_to_write = RAMFS_BLOCK_SIZE - block_offset;
            
            if bytes_to_write > buf.len() - bytes_written {
                bytes_to_write = buf.len() - bytes_written;
            }
            
            if block_idx >= 8 {
                break;
            }
            
            let block_num = self.inodes[inode_num as usize].direct_blocks[block_idx];
            let block_num = if block_num == 0 {
                let new_block = self.block_alloc();
                self.inodes[inode_num as usize].direct_blocks[block_idx] = new_block;
                new_block
            } else {
                block_num
            };
            
            if block_num == 0 {
                break;
            }
            
            let start = (block_num as usize) * RAMFS_BLOCK_SIZE + block_offset;
            self.data_area[start..start + bytes_to_write]
                .copy_from_slice(&buf[bytes_written..bytes_written + bytes_to_write]);
            
            bytes_written += bytes_to_write;
            *offset += bytes_to_write as u64;
            
            if *offset > self.inodes[inode_num as usize].size as u64 {
                self.inodes[inode_num as usize].size = *offset as u32;
            }
        }
        
        self.inodes[inode_num as usize].mtime = Self::get_time();
        
        bytes_written as i32
    }
    
    pub fn mkdir(&mut self, parent_path: &str, name: &str, pwid: u64) -> i32 {
        let parent_num = match self.resolve_path(parent_path) {
            Some(n) => n,
            None => return -1,
        };
        
        if !self.check_permission(&self.inodes[parent_num as usize], pwid, VFS_PERM_W) {
            return -1;
        }
        
        let new_inode_num = match self.alloc_inode(VfsFileType::Dir as u8, pwid) {
            Some(n) => n,
            None => return -1,
        };
        
        let block = self.inodes[new_inode_num as usize].direct_blocks[0];
        let dirent_size = core::mem::size_of::<RamFsDirent>();
        
        let dot: &mut RamFsDirent = unsafe {
            &mut *(&mut self.data_area[(block as usize) * RAMFS_BLOCK_SIZE] as *mut u8 as *mut RamFsDirent)
        };
        dot.inode = new_inode_num;
        dot.file_type = VfsFileType::Dir as u8;
        dot.set_name(".");
        
        let dotdot: &mut RamFsDirent = unsafe {
            &mut *(&mut self.data_area[(block as usize) * RAMFS_BLOCK_SIZE + dirent_size] as *mut u8 as *mut RamFsDirent)
        };
        dotdot.inode = parent_num;
        dotdot.file_type = VfsFileType::Dir as u8;
        dotdot.set_name("..");
        
        self.inodes[new_inode_num as usize].link_count = 2;
        
        let parent = &self.inodes[parent_num as usize];
        let parent_block = parent.direct_blocks[0];
        let num_entries = parent.size as usize / dirent_size;
        
        let entry: &mut RamFsDirent = unsafe {
            &mut *(&mut self.data_area[(parent_block as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size] as *mut u8 as *mut RamFsDirent)
        };
        entry.inode = new_inode_num;
        entry.file_type = VfsFileType::Dir as u8;
        entry.set_name(name);
        
        self.inodes[parent_num as usize].size += dirent_size as u32;
        self.inodes[parent_num as usize].link_count += 1;
        self.inodes[parent_num as usize].mtime = Self::get_time();
        
        log("[RamFS] Created directory '");
        log(name);
        log("'\n");
        
        0
    }
    
    pub fn stat(&self, inode_num: u32) -> Option<VfsStat> {
        let inode = &self.inodes[inode_num as usize];
        
        if !inode.used {
            return None;
        }
        
        Some(VfsStat {
            inode_num: inode.inode_num,
            mode: inode.perm,
            size: inode.size,
            atime: inode.atime,
            mtime: inode.mtime,
            ctime: inode.ctime,
            owner_pwid: inode.owner_pwid,
            perm: inode.perm,
            file_type: inode.file_type,
            reserved: 0,
        })
    }
}

pub static RAMFS_DATA: Mutex<RamFsData> = Mutex::new(RamFsData::new());

pub fn init() {
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.mount("/");
}
