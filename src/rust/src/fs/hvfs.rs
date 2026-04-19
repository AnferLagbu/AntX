use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

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

fn log_num(n: u32) {
    let mut buf = [0u8; 12];
    let mut num = n;
    let mut i = 11;
    
    if num == 0 {
        log("0");
        return;
    }
    
    while num > 0 {
        buf[i] = (num % 10) as u8 + b'0';
        num /= 10;
        i -= 1;
    }
    
    let s = core::str::from_utf8(&buf[i + 1..]).unwrap_or("?");
    log(s);
}

pub const HVFS_MAGIC: u32 = 0x48564653;
pub const HVFS_VERSION: u32 = 1;
pub const HVFS_BLOCK_SIZE: usize = 512;
pub const HVFS_MAX_INODES: usize = 128;
pub const HVFS_MAX_BLOCKS: usize = 1024;
pub const HVFS_MAX_FDS: usize = 16;
pub const HVFS_MAX_PATH: usize = 128;
pub const HVFS_MAX_NAME: usize = 64;

pub const HVFS_TYPE_FILE: u16 = 0;
pub const HVFS_TYPE_DIR: u16 = 1;
pub const HVFS_TYPE_SYMLINK: u16 = 2;

pub const HVFS_PERM_R: u16 = 0x04;
pub const HVFS_PERM_W: u16 = 0x02;
pub const HVFS_PERM_X: u16 = 0x01;

pub const HVFS_O_RDONLY: u32 = 0x0001;
pub const HVFS_O_WRONLY: u32 = 0x0002;
pub const HVFS_O_RDWR: u32 = 0x0004;
pub const HVFS_O_CREAT: u32 = 0x0100;
pub const HVFS_O_TRUNC: u32 = 0x0200;
pub const HVFS_O_APPEND: u32 = 0x0400;

pub const HVFS_DISK_OK: i32 = 0;
pub const HVFS_DISK_NO_DISK: i32 = -1;
pub const HVFS_DISK_UNFORMATTED: i32 = -2;
pub const HVFS_DISK_VERSION_ERROR: i32 = -3;
pub const HVFS_DISK_CORRUPT: i32 = -4;

const PWID_LEVEL_ROOT: u8 = 0;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvfsSuperBlockDisk {
    pub magic: u32,
    pub version: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub free_blocks: u32,
    pub inode_count: u32,
    pub free_inodes: u32,
    pub first_data_block: u32,
    pub root_inode: u32,
    pub block_bitmap_block: u32,
    pub inode_bitmap_block: u32,
    pub inode_table_block: u32,
    pub created_time: u64,
    pub modified_time: u64,
    pub mount_time: u64,
    pub mount_count: u32,
    pub state: u32,
    pub checksum: u32,
    pub reserved: [u8; 468],
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvfsInodeDisk {
    pub inode_num: u32,
    pub mode: u16,
    pub reserved: u16,
    pub size: u32,
    pub blocks: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub owner_pwid: u64,
    pub group_pwid: u64,
    pub pwid_perm: u16,
    pub link_count: u32,
    pub direct_blocks: [u32; 12],
    pub indirect_block: u32,
    pub double_indirect: u32,
    pub flags: u8,
    pub reserved2: [u8; 23],
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvfsDirEntryDisk {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    pub name: [i8; HVFS_MAX_NAME],
    pub reserved: [u8; 52],
}

#[derive(Debug, Clone, Copy)]
pub struct HvfsInode {
    pub inode_num: u32,
    pub mode: u16,
    pub size: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub owner_pwid: u64,
    pub group_pwid: u64,
    pub pwid_perm: u16,
    pub direct_blocks: [u32; 12],
    pub indirect_block: u32,
    pub double_indirect: u32,
    pub link_count: u32,
    pub ref_count: u32,
    pub used: bool,
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct HvfsDirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    pub name: [i8; HVFS_MAX_NAME],
}

impl HvfsDirEntry {
    pub fn new() -> Self {
        Self {
            inode: 0,
            rec_len: 128,
            name_len: 0,
            file_type: 0,
            name: [0; HVFS_MAX_NAME],
        }
    }

    pub fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(HVFS_MAX_NAME);
        for i in 0..len {
            self.name[i] = bytes[i] as i8;
        }
        for i in len..HVFS_MAX_NAME {
            self.name[i] = 0;
        }
        self.name_len = len as u8;
    }

    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(HVFS_MAX_NAME);
        let slice = &self.name[..end];
        
        unsafe {
            let ptr = slice.as_ptr() as *const u8;
            let u8_slice = core::slice::from_raw_parts(ptr, end);
            core::str::from_utf8(u8_slice).unwrap_or("")
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HvfsFd {
    pub fd: u32,
    pub inode_num: u32,
    pub offset: u64,
    pub flags: u32,
    pub pwid: u64,
    pub used: bool,
}

#[derive(Debug)]
pub struct HvfsContext {
    pub current_pwid: u64,
    pub current_dir: u32,
    pub fds: [HvfsFd; HVFS_MAX_FDS],
    pub next_fd: AtomicU32,
}

impl HvfsContext {
    pub fn new() -> Self {
        Self {
            current_pwid: 0,
            current_dir: 0,
            fds: [HvfsFd { fd: 0, inode_num: 0, offset: 0, flags: 0, pwid: 0, used: false }; HVFS_MAX_FDS],
            next_fd: AtomicU32::new(3),
        }
    }
}

pub struct HvFsData {
    pub super_block: HvfsSuperBlockDisk,
    pub inode_table: [HvfsInode; HVFS_MAX_INODES],
    pub block_bitmap: [u8; HVFS_MAX_BLOCKS / 8],
    pub inode_bitmap: [u8; HVFS_MAX_INODES / 8],
    pub context: HvfsContext,
    pub mounted: bool,
    pub disk_present: bool,
    pub formatted: bool,
}

unsafe impl Send for HvFsData {}
unsafe impl Sync for HvFsData {}

fn new_inode() -> HvfsInode {
    HvfsInode {
        inode_num: 0,
        mode: 0,
        size: 0,
        atime: 0,
        mtime: 0,
        ctime: 0,
        owner_pwid: 0,
        group_pwid: 0,
        pwid_perm: 0,
        direct_blocks: [0; 12],
        indirect_block: 0,
        double_indirect: 0,
        link_count: 0,
        ref_count: 0,
        used: false,
        dirty: false,
    }
}

fn new_super_block() -> HvfsSuperBlockDisk {
    HvfsSuperBlockDisk {
        magic: 0,
        version: 0,
        block_size: 0,
        total_blocks: 0,
        free_blocks: 0,
        inode_count: 0,
        free_inodes: 0,
        first_data_block: 0,
        root_inode: 0,
        block_bitmap_block: 0,
        inode_bitmap_block: 0,
        inode_table_block: 0,
        created_time: 0,
        modified_time: 0,
        mount_time: 0,
        mount_count: 0,
        state: 0,
        checksum: 0,
        reserved: [0; 468],
    }
}

impl HvFsData {
    pub fn new() -> Self {
        let mut inodes = [new_inode(); HVFS_MAX_INODES];
        inodes[0].used = true;
        
        Self {
            super_block: new_super_block(),
            inode_table: inodes,
            block_bitmap: [0; HVFS_MAX_BLOCKS / 8],
            inode_bitmap: [0; HVFS_MAX_INODES / 8],
            context: HvfsContext::new(),
            mounted: false,
            disk_present: false,
            formatted: false,
        }
    }

    pub fn get_time() -> u64 {
        let tsc: u64;
        unsafe {
            core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack));
        }
        tsc
    }

    pub fn block_is_free(&self, block_num: u32) -> bool {
        if block_num as usize >= HVFS_MAX_BLOCKS {
            return false;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        (self.block_bitmap[byte_idx] & (1 << bit_idx)) == 0
    }

    fn block_set_used(&mut self, block_num: u32) {
        if block_num as usize >= HVFS_MAX_BLOCKS {
            return;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        self.block_bitmap[byte_idx] |= 1 << bit_idx;
        self.super_block.free_blocks -= 1;
    }

    fn alloc_block(&mut self) -> Option<u32> {
        for i in self.super_block.first_data_block..HVFS_MAX_BLOCKS as u32 {
            if self.block_is_free(i) {
                self.block_set_used(i);
                log("[HvFS] Allocated block ");
                log_num(i);
                log("\n");
                return Some(i);
            }
        }
        None
    }

    fn inode_set_used(&mut self, inode_num: u32) {
        if inode_num as usize >= HVFS_MAX_INODES {
            return;
        }
        let byte_idx = (inode_num / 8) as usize;
        let bit_idx = (inode_num % 8) as usize;
        self.inode_bitmap[byte_idx] |= 1 << bit_idx;
        self.super_block.free_inodes -= 1;
    }

    pub fn check_permission(&self, inode: &HvfsInode, pwid: u64, access_type: u16) -> bool {
        let level = unsafe { pwid_get_level(pwid) };
        
        if level == PWID_LEVEL_ROOT {
            return true;
        }
        
        if pwid == inode.owner_pwid {
            let owner_perm = (inode.pwid_perm >> 6) & 0x07;
            return (owner_perm & access_type) == access_type;
        }
        
        let other_perm = inode.pwid_perm & 0x07;
        (other_perm & access_type) == access_type
    }

    pub fn get_inode(&self, inode_num: u32) -> Option<&HvfsInode> {
        if inode_num == 0 || inode_num as usize >= HVFS_MAX_INODES {
            return None;
        }
        if !self.inode_table[inode_num as usize].used {
            return None;
        }
        Some(&self.inode_table[inode_num as usize])
    }

    pub fn get_inode_mut(&mut self, inode_num: u32) -> Option<&mut HvfsInode> {
        if inode_num == 0 || inode_num as usize >= HVFS_MAX_INODES {
            return None;
        }
        if !self.inode_table[inode_num as usize].used {
            return None;
        }
        self.inode_table[inode_num as usize].dirty = true;
        Some(&mut self.inode_table[inode_num as usize])
    }

    pub fn resolve_path(&self, path: &str) -> Option<u32> {
        let mut current = self.super_block.root_inode;
        let p = path.trim_start_matches('/');
        
        if p.is_empty() {
            return Some(current);
        }
        
        for component in p.split('/') {
            if component.is_empty() { continue; }
            
            let inode = self.get_inode(current)?;
            
            let file_type = (inode.mode >> 12) & 0xF;
            if file_type != HVFS_TYPE_DIR {
                return None;
            }
            
            if inode.direct_blocks[0] == 0 {
                return None;
            }
            
            let mut found = false;
            
            for i in 0..HVFS_MAX_BLOCKS {
                let entry: HvfsDirEntryDisk = unsafe {
                    core::mem::zeroed()
                };
                
                if entry.inode != 0 && i < HVFS_MAX_NAME {
                    let name_bytes: &[u8] = unsafe {
                        core::slice::from_raw_parts(entry.name.as_ptr() as *const u8, entry.name_len as usize)
                    };
                    if let Ok(name) = core::str::from_utf8(name_bytes) {
                        if name == component {
                            current = entry.inode;
                            found = true;
                            break;
                        }
                    }
                }
                
                if found || i > 10 { break; }
            }
            
            if !found { return None; }
        }
        
        Some(current)
    }

    pub fn init(&mut self) {
        log("[HvFS] Initializing\n");
        
        for i in 0..HVFS_MAX_INODES {
            self.inode_table[i] = new_inode();
        }
        
        for i in 0..HVFS_MAX_FDS {
            self.context.fds[i] = HvfsFd { fd: 0, inode_num: 0, offset: 0, flags: 0, pwid: 0, used: false };
        }
        
        self.context.next_fd.store(3, Ordering::SeqCst);
        self.mounted = false;
        self.disk_present = false;
        self.formatted = false;
        
        log("[HvFS] Initialized\n");
    }

    pub fn format(&mut self) -> i32 {
        log("[HvFS] Formatting filesystem...\n");
        
        self.super_block.magic = HVFS_MAGIC;
        self.super_block.version = HVFS_VERSION;
        self.super_block.block_size = HVFS_BLOCK_SIZE as u32;
        self.super_block.total_blocks = HVFS_MAX_BLOCKS as u32;
        self.super_block.free_blocks = (HVFS_MAX_BLOCKS - 400) as u32;
        self.super_block.inode_count = HVFS_MAX_INODES as u32;
        self.super_block.free_inodes = (HVFS_MAX_INODES - 1) as u32;
        self.super_block.first_data_block = 400;
        self.super_block.root_inode = 1;
        self.super_block.block_bitmap_block = 336;
        self.super_block.inode_bitmap_block = 351;
        self.super_block.inode_table_block = 208;
        self.super_block.created_time = Self::get_time();
        self.super_block.modified_time = Self::get_time();
        self.super_block.mount_time = Self::get_time();
        self.super_block.mount_count = 0;
        self.super_block.state = 1;
        self.super_block.checksum = 0;
        
        for i in 0..HVFS_MAX_INODES {
            self.inode_table[i] = new_inode();
        }
        
        for b in self.block_bitmap.iter_mut() { *b = 0; }
        for b in self.inode_bitmap.iter_mut() { *b = 0; }
        
        for i in 0..400u32 {
            let byte_idx = (i / 8) as usize;
            let bit_idx = (i % 8) as usize;
            self.block_bitmap[byte_idx] |= 1 << bit_idx;
        }
        
        self.create_root_inode();
        
        self.formatted = true;
        log("[HvFS] Format complete\n");
        0
    }

    fn create_root_inode(&mut self) {
        let data_block = self.alloc_block().unwrap_or(400);
        
        self.inode_table[1] = HvfsInode {
            inode_num: 1,
            mode: (HVFS_TYPE_DIR << 12) | 0o755,
            size: (2 * core::mem::size_of::<HvfsDirEntryDisk>()) as u32,
            atime: Self::get_time(),
            mtime: Self::get_time(),
            ctime: Self::get_time(),
            owner_pwid: 0,
            group_pwid: 0,
            pwid_perm: 0o755,
            direct_blocks: [data_block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            double_indirect: 0,
            link_count: 2,
            ref_count: 1,
            used: true,
            dirty: true,
        };
        
        self.inode_set_used(1);
        
        log("[HvFS] Root inode created\n");
    }

    pub fn check_disk(&mut self) -> i32 {
        log("[HvFS] Checking disk...\n");
        
        if self.disk_present {
            if self.formatted {
                log("[HvFS] Found valid filesystem\n");
                return HVFS_DISK_OK;
            } else {
                log("[HvFS] Disk unformatted\n");
                return HVFS_DISK_UNFORMATTED;
            }
        } else {
            log("[HvFS] No disk detected\n");
            return HVFS_DISK_NO_DISK;
        }
    }

    pub fn mount(&mut self) -> i32 {
        if self.mounted {
            log("[HvFS] Already mounted\n");
            return 0;
        }
        
        if !self.disk_present {
            log("[HvFS] No disk to mount\n");
            return -1;
        }
        
        if !self.formatted {
            log("[HvFS] Formatting disk...\n");
            if self.format() != 0 {
                return -1;
            }
        }
        
        self.super_block.mount_time = Self::get_time();
        self.super_block.mount_count += 1;
        self.mounted = true;
        
        log("[HvFS] Mounted successfully\n");
        0
    }

    pub fn open(&mut self, path: &str, flags: u32, pwid: u64) -> i32 {
        let inode_num = match self.resolve_path(path) {
            Some(num) => {
                if (flags & HVFS_O_TRUNC) != 0 {
                    if let Some(inode) = self.get_inode_mut(num) {
                        inode.size = 0;
                        inode.dirty = true;
                    }
                }
                num
            }
            None => {
                if (flags & HVFS_O_CREAT) != 0 {
                    match self.create_file(path, pwid) {
                        Some(num) => num,
                        None => return -1,
                    }
                } else {
                    return -1;
                }
            }
        };
        
        let inode = match self.get_inode(inode_num) {
            Some(i) => i,
            None => return -1,
        };
        
        if !self.check_permission(inode, pwid, HVFS_PERM_R) {
            return -1;
        }
        
        let inode_size = inode.size;
        
        for i in 0..HVFS_MAX_FDS {
            if !self.context.fds[i].used {
                self.context.fds[i].used = true;
                self.context.fds[i].fd = self.context.next_fd.fetch_add(1, Ordering::SeqCst);
                self.context.fds[i].inode_num = inode_num;
                self.context.fds[i].offset = if (flags & HVFS_O_APPEND) != 0 { inode_size as u64 } else { 0 };
                self.context.fds[i].flags = flags;
                self.context.fds[i].pwid = pwid;
                
                log("[HvFS] Opened fd=");
                log_num(self.context.fds[i].fd);
                log("\n");
                
                return self.context.fds[i].fd as i32;
            }
        }
        
        -1
    }

    fn create_file(&mut self, path: &str, pwid: u64) -> Option<u32> {
        let filename = path.rsplit('/').next().unwrap_or(path);
        let dir_path = if let Some(pos) = path.rfind('/') {
            if pos == 0 { "/" } else { &path[..pos] }
        } else {
            "/"
        };
        
        let parent_num = self.resolve_path(dir_path)?;
        
        if let Some(parent) = self.get_inode(parent_num) {
            if !self.check_permission(parent, pwid, HVFS_PERM_W) {
                return None;
            }
        }
        
        let new_inode_num = self.alloc_inode()?;
        let data_block = self.alloc_block().unwrap_or(0);
        
        {
            let inode = &mut self.inode_table[new_inode_num as usize];
            *inode = HvfsInode {
                inode_num: new_inode_num,
                mode: (HVFS_TYPE_FILE << 12) | 0o644,
                size: 0,
                atime: Self::get_time(),
                mtime: Self::get_time(),
                ctime: Self::get_time(),
                owner_pwid: pwid,
                group_pwid: 0,
                pwid_perm: 0o644,
                direct_blocks: [data_block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                indirect_block: 0,
                double_indirect: 0,
                link_count: 1,
                ref_count: 1,
                used: true,
                dirty: true,
            };
        }
        
        if let Some(parent) = self.get_inode_mut(parent_num) {
            parent.size += core::mem::size_of::<HvfsDirEntryDisk>() as u32;
            parent.mtime = Self::get_time();
            parent.dirty = true;
        }
        
        Some(new_inode_num)
    }

    fn alloc_inode(&mut self) -> Option<u32> {
        for i in 1..HVFS_MAX_INODES as u32 {
            if !self.inode_table[i as usize].used {
                self.inode_table[i as usize].used = true;
                self.inode_table[i as usize].dirty = true;
                self.inode_table[i as usize].inode_num = i;
                self.inode_set_used(i);
                log("[HvFS] Allocated inode ");
                log_num(i);
                log("\n");
                return Some(i);
            }
        }
        None
    }

    pub fn close(&mut self, fd: u32) -> i32 {
        for i in 0..HVFS_MAX_FDS {
            if self.context.fds[i].used && self.context.fds[i].fd == fd {
                self.context.fds[i].used = false;
                self.context.fds[i].fd = 0;
                self.context.fds[i].inode_num = 0;
                self.context.fds[i].offset = 0;
                return 0;
            }
        }
        -1
    }

    pub fn read(&mut self, fd: u32, buf: &mut [u8], _count: u32) -> i32 {
        let hvfs_fd = match self.find_fd(fd) {
            Some(idx) => idx,
            None => return -1,
        };
        
        let inode_num = self.context.fds[hvfs_fd].inode_num;
        let offset = self.context.fds[hvfs_fd].offset;
        let pwid = self.context.fds[hvfs_fd].pwid;
        
        let inode = match self.get_inode(inode_num) {
            Some(i) => i,
            None => return -1,
        };
        
        if !self.check_permission(inode, pwid, HVFS_PERM_R) {
            return -1;
        }
        
        let inode_size = inode.size as u64;
        let bytes_to_read = (buf.len() as u64).min(inode_size - offset) as usize;
        
        buf[..bytes_to_read].fill(0);
        self.context.fds[hvfs_fd].offset += bytes_to_read as u64;
        
        bytes_to_read as i32
    }

    pub fn write(&mut self, fd: u32, buf: &[u8], count: u32) -> i32 {
        let hvfs_fd = match self.find_fd(fd) {
            Some(idx) => idx,
            None => return -1,
        };
        
        let inode_num = self.context.fds[hvfs_fd].inode_num;
        let pwid = self.context.fds[hvfs_fd].pwid;
        
        if let Some(inode) = self.get_inode(inode_num) {
            if !self.check_permission(inode, pwid, HVFS_PERM_W) {
                return -1;
            }
        }
        
        let bytes_written = (buf.len() as u32).min(count) as usize;
        let new_offset = self.context.fds[hvfs_fd].offset + bytes_written as u64;
        
        if let Some(inode) = self.get_inode_mut(inode_num) {
            if new_offset > inode.size as u64 {
                inode.size = new_offset as u32;
            }
            inode.mtime = Self::get_time();
            inode.dirty = true;
        }
        
        self.context.fds[hvfs_fd].offset = new_offset;
        
        bytes_written as i32
    }

    pub fn mkdir(&mut self, path: &str, pwid: u64) -> i32 {
        let dirname = path.rsplit('/').next().unwrap_or(path);
        let parent_path = if let Some(pos) = path.rfind('/') {
            if pos == 0 { "/" } else { &path[..pos] }
        } else {
            "/"
        };
        
        let parent_num = match self.resolve_path(parent_path) {
            Some(n) => n,
            None => return -1,
        };
        
        if let Some(parent) = self.get_inode(parent_num) {
            if !self.check_permission(parent, pwid, HVFS_PERM_W) {
                return -1;
            }
        }
        
        let new_inode_num = match self.alloc_inode() {
            Some(n) => n,
            None => return -1,
        };
        
        let data_block = self.alloc_block().unwrap_or(0);
        
        {
            let inode = &mut self.inode_table[new_inode_num as usize];
            *inode = HvfsInode {
                inode_num: new_inode_num,
                mode: (HVFS_TYPE_DIR << 12) | 0o755,
                size: (2 * core::mem::size_of::<HvfsDirEntryDisk>()) as u32,
                atime: Self::get_time(),
                mtime: Self::get_time(),
                ctime: Self::get_time(),
                owner_pwid: pwid,
                group_pwid: 0,
                pwid_perm: 0o755,
                direct_blocks: [data_block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                indirect_block: 0,
                double_indirect: 0,
                link_count: 2,
                ref_count: 1,
                used: true,
                dirty: true,
            };
        }
        
        if let Some(parent) = self.get_inode_mut(parent_num) {
            parent.size += core::mem::size_of::<HvfsDirEntryDisk>() as u32;
            parent.link_count += 1;
            parent.mtime = Self::get_time();
            parent.dirty = true;
        }
        
        log("[HvFS] Created directory '");
        log(dirname);
        log("'\n");
        
        0
    }

    pub fn stat(&self, path: &str, pwid: u64) -> Option<HvfsInode> {
        let inode_num = self.resolve_path(path)?;
        let inode = self.get_inode(inode_num)?;
        
        if !self.check_permission(inode, pwid, HVFS_PERM_R) {
            return None;
        }
        
        Some(*inode)
    }

    fn find_fd(&self, fd: u32) -> Option<usize> {
        for (i, f) in self.context.fds.iter().enumerate() {
            if f.used && f.fd == fd {
                return Some(i);
            }
        }
        None
    }

    pub fn sync(&mut self) -> i32 {
        log("[HvFS] Syncing filesystem\n");
        self.super_block.modified_time = Self::get_time();
        0
    }

    pub fn set_disk_present(&mut self, present: bool) {
        self.disk_present = present;
        if present {
            log("[HvFS] Disk detected\n");
        }
    }
}

pub static HVFS_DATA: spin::Once<Mutex<HvFsData>> = spin::Once::new();

pub fn get_hvfs() -> &'static Mutex<HvFsData> {
    HVFS_DATA.call_once(|| Mutex::new(HvFsData::new()))
}

pub fn init() {
    let mut hvfs = get_hvfs().lock();
    hvfs.init();
}
