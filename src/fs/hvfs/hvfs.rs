use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

extern "C" {
    fn serial_putc(port: u16, c: u8);
    fn pwid_get_level(pwid: u64) -> u8;
    fn pwid_get_fs_capability(pwid: u64) -> u64;
    fn pwid_check_trust(subject: u64, target: u64, domain: u16, caps: u64, max_depth: u8) -> i32;
    fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32;
    fn ata_write_sector(disk: u8, sector: u32, buf: *const u8) -> i32;
    fn ata_read_sectors(disk: u8, start: u32, count: u32, buf: *mut u8) -> i32;
    fn ata_write_sectors(disk: u8, start: u32, count: u32, buf: *const u8) -> i32;
    fn ata_disk_present(disk: u8) -> i32;
}

fn log(s: &str) {
    unsafe {
        for c in s.bytes() {
            serial_putc(0x3F8, c);
        }
    }
}

fn log_num(n: u64) {
    if n == 0 {
        log("0");
        return;
    }
    let mut buf = [0u8; 24];
    let mut num = n;
    let mut i = 23;
    while num > 0 {
        buf[i] = (num % 10) as u8 + b'0';
        num /= 10;
        i -= 1;
    }
    let s = core::str::from_utf8(&buf[i + 1..]).unwrap_or("?");
    log(s);
}

fn log_hex(n: u64) {
    log("0x");
    let hex_chars = b"0123456789ABCDEF";
    let mut started = false;
    for i in (0..16).rev() {
        let nibble = (n >> (i * 4)) & 0xF;
        if nibble != 0 || started || i == 0 {
            log(unsafe { core::str::from_utf8_unchecked(&[hex_chars[nibble as usize]]) });
            started = true;
        }
    }
}

pub const HVFS_MAGIC: u32 = 0x48564653;

/// Format version history:
/// 1 - Initial format (before permission model v3)
/// 2 - Pre-sensitivity field
/// 3 - Permission Model v3: HvfsInodeDisk.sensitivity added, reserved2 shrunk to 18
pub const HVFS_VERSION: u32 = 3;
pub const HVFS_BLOCK_SIZE: usize = 4096;
pub const HVFS_DISK_SECTOR_SIZE: usize = 512;
pub const HVFS_SECTORS_PER_BLOCK: usize = HVFS_BLOCK_SIZE / HVFS_DISK_SECTOR_SIZE;

pub const HVFS_DEFAULT_INODES: u32 = 4096;
pub const HVFS_DEFAULT_BLOCKS: u32 = 65536;
pub const HVFS_MAX_INODES_LIMIT: u32 = 1048576;
pub const HVFS_MAX_BLOCKS_LIMIT: u32 = 16777216;

pub const HVFS_MAX_FDS: usize = 64;
pub const HVFS_MAX_PATH: usize = 256;
pub const HVFS_MAX_NAME: usize = 128;

pub const HVFS_TYPE_FILE: u16 = 0;
pub const HVFS_TYPE_DIR: u16 = 1;
pub const HVFS_TYPE_SYMLINK: u16 = 2;

pub const HVFS_CAP_READ: u64 = 1 << 0;
pub const HVFS_CAP_WRITE: u64 = 1 << 1;
pub const HVFS_CAP_EXECUTE: u64 = 1 << 2;
pub const HVFS_CAP_CREATE: u64 = 1 << 3;

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

pub const HVFS_CACHE_SIZE: usize = 256;

#[derive(Debug, Clone)]
pub enum FsckError {
    NotInitialized,
    InvalidMagic,
    InvalidVersion,
    InvalidRootInode,
    RootInodeNotUsed,
    OrphanInode(u32),
    CorruptedDirectory(u32),
}

#[derive(Debug, Clone)]
pub enum FsckWarning {
    InodeCountMismatch { expected: u32, actual: u32 },
    BlockCountMismatch { expected: u32, actual: u32 },
    OrphanBlock(u32),
    UnreferencedInode(u32),
}

#[derive(Debug, Clone)]
pub struct FsckResult {
    pub passed: bool,
    pub fixed: bool,
    pub errors: Vec<FsckError>,
    pub warnings: Vec<FsckWarning>,
}

impl FsckResult {
    pub fn new() -> Self {
        Self {
            passed: false,
            fixed: false,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

const INDIRECT_BLOCKS_PER_BLOCK: usize = HVFS_BLOCK_SIZE / core::mem::size_of::<u32>();

const HVFS_BOOT_SECTOR_START: u32 = 0;
const HVFS_SUPER_SECTOR_START: u32 = 200;
const HVFS_SUPER_SECTOR_COUNT: u32 = 8;
const HVFS_INODE_SECTOR_START: u32 = 208;
const HVFS_BLOCK_BITMAP_START: u32 = 8400;
const HVFS_INODE_BITMAP_START: u32 = 10448;
const HVFS_DATA_SECTOR_START: u32 = 10720;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvfsSuperBlock {
    pub magic: u32,
    pub version: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub free_blocks: u32,
    pub inode_count: u32,
    pub free_inodes: u32,
    pub first_data_block: u32,
    pub root_inode: u32,
    pub max_path_depth: u32,
    pub max_entries: u32,
    pub created_time: u64,
    pub modified_time: u64,
    pub mount_time: u64,
    pub mount_count: u32,
    pub state: u32,
    pub dynamic_inodes: u32,
    pub dynamic_blocks: u32,
}

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
    pub dynamic_inodes: u32,
    pub dynamic_blocks: u32,
    pub checksum: u32,
    pub reserved: [u8; 452],
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvfsInode {
    pub inode_num: u32,
    pub mode: u16,
    pub sensitivity: u8,
    pub reserved_uid: u16,
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
    pub triple_indirect: u32,
    pub link_count: u32,
    pub ref_count: u32,
    pub used: bool,
    pub dirty: bool,
    pub in_cache: bool,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvfsInodeDisk {
    pub inode_num: u32,
    pub mode: u16,
    pub sensitivity: u8,
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
    pub triple_indirect: u32,
    pub flags: u8,
    pub reserved2: [u8; 18],
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvfsDirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    pub name: [u8; HVFS_MAX_NAME],
}

impl HvfsDirEntry {
    pub fn new() -> Self {
        Self {
            inode: 0,
            rec_len: core::mem::size_of::<HvfsDirEntry>() as u16,
            name_len: 0,
            file_type: 0,
            name: [0; HVFS_MAX_NAME],
        }
    }

    pub fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(HVFS_MAX_NAME);
        self.name[..len].copy_from_slice(&bytes[..len]);
        for i in len..HVFS_MAX_NAME {
            self.name[i] = 0;
        }
        self.name_len = len as u8;
    }

    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(HVFS_MAX_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
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
pub struct BlockCacheEntry {
    pub block_num: u32,
    pub data: Box<[u8; HVFS_BLOCK_SIZE]>,
    pub dirty: bool,
    pub valid: bool,
    pub access_time: u32,
}

impl BlockCacheEntry {
    pub fn new() -> Self {
        Self {
            block_num: 0,
            data: Box::new([0u8; HVFS_BLOCK_SIZE]),
            dirty: false,
            valid: false,
            access_time: 0,
        }
    }
    
    pub fn new_uninit() -> Self {
        Self {
            block_num: 0,
            data: Box::new([0u8; HVFS_BLOCK_SIZE]),
            dirty: false,
            valid: false,
            access_time: 0,
        }
    }
}

pub struct HvFsData {
    pub super_block: HvfsSuperBlock,
    pub inode_table: Vec<HvfsInode>,
    pub block_bitmap: Vec<u8>,
    pub inode_bitmap: Vec<u8>,
    pub fds: [HvfsFd; HVFS_MAX_FDS],
    pub next_fd: AtomicU32,
    pub current_pwid: AtomicU64,
    pub current_dir: AtomicU32,
    pub block_cache: Vec<BlockCacheEntry>,
    pub cache_access_counter: AtomicU32,
    pub mounted: bool,
    pub disk_present: bool,
    pub initialized: bool,
}

unsafe impl Send for HvFsData {}
unsafe impl Sync for HvFsData {}

impl HvFsData {
    pub fn new() -> Self {
        let mut fds = [HvfsFd { 
            fd: 0, inode_num: 0, offset: 0, flags: 0, pwid: 0, used: false 
        }; HVFS_MAX_FDS];
        
        let mut block_cache = Vec::new();
        // Temporarily skip cache pre-allocation; allocate lazily in get_block()

        
        Self {
            super_block: Self::new_super_block(),
            inode_table: Vec::new(),
            block_bitmap: Vec::new(),
            inode_bitmap: Vec::new(),
            fds,
            next_fd: AtomicU32::new(3),
            current_pwid: AtomicU64::new(0),
            current_dir: AtomicU32::new(1),
            block_cache,
            cache_access_counter: AtomicU32::new(0),
            mounted: false,
            disk_present: false,
            initialized: false,
        }
    }

    fn new_super_block() -> HvfsSuperBlock {
        HvfsSuperBlock {
            magic: 0,
            version: 0,
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            inode_count: 0,
            free_inodes: 0,
            first_data_block: 0,
            root_inode: 0,
            max_path_depth: 0,
            max_entries: 0,
            created_time: 0,
            modified_time: 0,
            mount_time: 0,
            mount_count: 0,
            state: 0,
            dynamic_inodes: 0,
            dynamic_blocks: 0,
        }
    }

    fn new_inode() -> HvfsInode {
        HvfsInode {
            inode_num: 0,
            mode: 0,
            sensitivity: 0,
            reserved_uid: 0,
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
            triple_indirect: 0,
            link_count: 0,
            ref_count: 0,
            used: false,
            dirty: false,
            in_cache: false,
        }
    }

    pub fn get_time() -> u64 {
        let tsc: u64;
        unsafe {
            core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack));
        }
        tsc
    }

    pub fn init(&mut self) {
        self.super_block = Self::new_super_block();
        self.inode_table.clear();
        self.block_bitmap.clear();
        self.inode_bitmap.clear();
        
        for fd in self.fds.iter_mut() {
            *fd = HvfsFd { fd: 0, inode_num: 0, offset: 0, flags: 0, pwid: 0, used: false };
        }
        
        self.next_fd.store(3, Ordering::SeqCst);
        self.current_pwid.store(0, Ordering::SeqCst);
        self.current_dir.store(1, Ordering::SeqCst);
        self.mounted = false;
        self.initialized = false;
        
        for entry in self.block_cache.iter_mut() {
            entry.valid = false;
            entry.dirty = false;
        }
        
        unsafe {
            if ata_disk_present(0) != 0 {
                self.disk_present = true;
            } else if ata_disk_present(1) != 0 {
                self.disk_present = true;
            } else if ata_disk_present(2) != 0 {
                self.disk_present = true;
            } else if ata_disk_present(3) != 0 {
                self.disk_present = true;
            } else {
                self.disk_present = false;
            }
        }
    }

    fn block_is_free(&self, block_num: u32) -> bool {
        if block_num as usize >= self.super_block.total_blocks as usize {
            return false;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        if byte_idx >= self.block_bitmap.len() {
            return false;
        }
        (self.block_bitmap[byte_idx] & (1 << bit_idx)) == 0
    }

    fn block_set_used(&mut self, block_num: u32) {
        if block_num as usize >= self.super_block.total_blocks as usize {
            return;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        if byte_idx >= self.block_bitmap.len() {
            return;
        }
        self.block_bitmap[byte_idx] |= 1 << bit_idx;
        self.super_block.free_blocks -= 1;
    }

    fn block_set_free(&mut self, block_num: u32) {
        if block_num as usize >= self.super_block.total_blocks as usize {
            return;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        if byte_idx >= self.block_bitmap.len() {
            return;
        }
        self.block_bitmap[byte_idx] &= !(1 << bit_idx);
        self.super_block.free_blocks += 1;
    }

    fn inode_is_used(&self, inode_num: u32) -> bool {
        if inode_num as usize >= self.super_block.inode_count as usize {
            return false;
        }
        let byte_idx = (inode_num / 8) as usize;
        let bit_idx = (inode_num % 8) as usize;
        if byte_idx >= self.inode_bitmap.len() {
            return false;
        }
        (self.inode_bitmap[byte_idx] & (1 << bit_idx)) != 0
    }

    fn inode_set_used(&mut self, inode_num: u32) {
        if inode_num as usize >= self.super_block.inode_count as usize {
            return;
        }
        let byte_idx = (inode_num / 8) as usize;
        let bit_idx = (inode_num % 8) as usize;
        if byte_idx >= self.inode_bitmap.len() {
            return;
        }
        self.inode_bitmap[byte_idx] |= 1 << bit_idx;
        self.super_block.free_inodes -= 1;
    }

    fn inode_set_free(&mut self, inode_num: u32) {
        if inode_num as usize >= self.super_block.inode_count as usize {
            return;
        }
        let byte_idx = (inode_num / 8) as usize;
        let bit_idx = (inode_num % 8) as usize;
        if byte_idx >= self.inode_bitmap.len() {
            return;
        }
        self.inode_bitmap[byte_idx] &= !(1 << bit_idx);
        self.super_block.free_inodes += 1;
    }

    fn get_block(&mut self, block_num: u32) -> Option<&mut [u8; HVFS_BLOCK_SIZE]> {
        if block_num >= self.super_block.total_blocks {
            return None;
        }
        
        let mut found_idx: Option<usize> = None;
        for (i, entry) in self.block_cache.iter().enumerate() {
            if entry.valid && entry.block_num == block_num {
                found_idx = Some(i);
                break;
            }
        }
        
        if let Some(idx) = found_idx {
            self.block_cache[idx].access_time = self.cache_access_counter.fetch_add(1, Ordering::SeqCst);
            return Some(&mut self.block_cache[idx].data);
        }
        
        // If cache not full, allocate a new entry
        if self.block_cache.len() < HVFS_CACHE_SIZE {
            let idx = self.block_cache.len();
            self.block_cache.push(BlockCacheEntry::new());
            self.block_cache[idx].block_num = block_num;
            self.block_cache[idx].valid = true;
            self.block_cache[idx].access_time = self.cache_access_counter.fetch_add(1, Ordering::SeqCst);
            if self.disk_present {
                let sector_start = HVFS_DATA_SECTOR_START + block_num * HVFS_SECTORS_PER_BLOCK as u32;
                for s in 0..HVFS_SECTORS_PER_BLOCK {
                    let offset = s * HVFS_DISK_SECTOR_SIZE;
                    unsafe {
                        ata_read_sector(0, sector_start + s as u32, self.block_cache[idx].data.as_mut_ptr().add(offset));
                    }
                }
            } else {
                self.block_cache[idx].data.fill(0);
            }
            return Some(&mut self.block_cache[idx].data);
        }
        
        // Cache full: LRU eviction
        let mut lru_idx = 0;
        let mut lru_time = u32::MAX;
        
        for (i, entry) in self.block_cache.iter().enumerate() {
            if !entry.valid {
                lru_idx = i;
                break;
            }
            if entry.access_time < lru_time {
                lru_time = entry.access_time;
                lru_idx = i;
            }
        }
        
        let (old_block_num, old_dirty, old_valid) = {
            let entry = &self.block_cache[lru_idx];
            (entry.block_num, entry.dirty, entry.valid)
        };
        
        if old_valid && old_dirty && self.disk_present {
            let data_copy = self.block_cache[lru_idx].data.clone();
            self.write_block_to_disk(old_block_num, &data_copy);
        }
        
        let disk_present = self.disk_present;
        
        {
            let entry = &mut self.block_cache[lru_idx];
            entry.block_num = block_num;
            entry.valid = true;
            entry.dirty = false;
            entry.access_time = self.cache_access_counter.fetch_add(1, Ordering::SeqCst);
            
            if disk_present {
                let sector_start = HVFS_DATA_SECTOR_START + block_num * HVFS_SECTORS_PER_BLOCK as u32;
                for s in 0..HVFS_SECTORS_PER_BLOCK {
                    let offset = s * HVFS_DISK_SECTOR_SIZE;
                    unsafe {
                        ata_read_sector(0, sector_start + s as u32, entry.data.as_mut_ptr().add(offset));
                    }
                }
            } else {
                entry.data.fill(0);
            }
        }
        
        Some(&mut self.block_cache[lru_idx].data)
    }

    fn mark_block_dirty(&mut self, block_num: u32) {
        for entry in self.block_cache.iter_mut() {
            if entry.valid && entry.block_num == block_num {
                entry.dirty = true;
                return;
            }
        }
    }

    fn read_block_from_disk(&self, block_num: u32, data: &mut [u8; HVFS_BLOCK_SIZE]) {
        if !self.disk_present {
            return;
        }
        
        let sector_start = HVFS_DATA_SECTOR_START + block_num * HVFS_SECTORS_PER_BLOCK as u32;
        
        for s in 0..HVFS_SECTORS_PER_BLOCK {
            let offset = s * HVFS_DISK_SECTOR_SIZE;
            unsafe {
                ata_read_sector(0, sector_start + s as u32, data.as_mut_ptr().add(offset));
            }
        }
    }

    fn write_block_to_disk(&self, block_num: u32, data: &[u8; HVFS_BLOCK_SIZE]) {
        if !self.disk_present {
            return;
        }
        
        let sector_start = HVFS_DATA_SECTOR_START + block_num * HVFS_SECTORS_PER_BLOCK as u32;
        
        for s in 0..HVFS_SECTORS_PER_BLOCK {
            let offset = s * HVFS_DISK_SECTOR_SIZE;
            unsafe {
                ata_write_sector(0, sector_start + s as u32, data.as_ptr().add(offset));
            }
        }
    }

    fn alloc_block(&mut self) -> Option<u32> {
        for i in self.super_block.first_data_block..self.super_block.total_blocks {
            if self.block_is_free(i) {
                self.block_set_used(i);
                if let Some(block) = self.get_block(i) {
                    block.fill(0);
                }
                self.mark_block_dirty(i);
                return Some(i);
            }
        }
        
        if self.super_block.total_blocks < HVFS_MAX_BLOCKS_LIMIT {
            let new_total = (self.super_block.total_blocks * 2).min(HVFS_MAX_BLOCKS_LIMIT);
            if self.expand_blocks(new_total).is_ok() {
                return self.alloc_block();
            }
        }
        
        None
    }

    fn alloc_inode(&mut self) -> Option<u32> {
        for i in 1..self.super_block.inode_count {
            if !self.inode_table[i as usize].used {
                self.inode_table[i as usize].used = true;
                self.inode_table[i as usize].dirty = true;
                self.inode_table[i as usize].inode_num = i;
                self.inode_table[i as usize].ref_count = 1;
                self.inode_table[i as usize].link_count = 1;
                self.inode_set_used(i);
                return Some(i);
            }
        }
        
        if self.super_block.inode_count < HVFS_MAX_INODES_LIMIT {
            let new_count = (self.super_block.inode_count * 2).min(HVFS_MAX_INODES_LIMIT);
            if self.expand_inodes(new_count).is_ok() {
                return self.alloc_inode();
            }
        }
        
        None
    }

    fn expand_inodes(&mut self, new_count: u32) -> Result<(), ()> {
        if new_count <= self.super_block.inode_count || new_count > HVFS_MAX_INODES_LIMIT {
            return Err(());
        }
        
        let new_bitmap_size = ((new_count + 7) / 8) as usize;
        
        self.inode_table.reserve(new_count as usize - self.inode_table.len());
        while self.inode_table.len() < new_count as usize {
            self.inode_table.push(Self::new_inode());
        }
        
        self.inode_bitmap.resize(new_bitmap_size, 0);
        
        self.super_block.free_inodes += new_count - self.super_block.inode_count;
        self.super_block.inode_count = new_count;
        self.super_block.dynamic_inodes = 1;
        
        Ok(())
    }

    fn expand_blocks(&mut self, new_count: u32) -> Result<(), ()> {
        if new_count <= self.super_block.total_blocks || new_count > HVFS_MAX_BLOCKS_LIMIT {
            return Err(());
        }
        
        let new_bitmap_size = ((new_count + 7) / 8) as usize;
        self.block_bitmap.resize(new_bitmap_size, 0);
        
        self.super_block.free_blocks += new_count - self.super_block.total_blocks;
        self.super_block.total_blocks = new_count;
        self.super_block.dynamic_blocks = 1;
        
        Ok(())
    }

    fn get_block_for_index(&mut self, inode: &HvfsInode, block_idx: u32, alloc: bool) -> Option<u32> {
        let max_blocks = 12 + INDIRECT_BLOCKS_PER_BLOCK as u32 + 
                        (INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK) as u32 +
                        (INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK) as u32;
        
        if block_idx >= max_blocks {
            return None;
        }
        
        if block_idx < 12 {
            return Some(inode.direct_blocks[block_idx as usize]);
        }
        
        let indirect_start = 12;
        let indirect_end = 12 + INDIRECT_BLOCKS_PER_BLOCK as u32;
        
        if block_idx >= indirect_start && block_idx < indirect_end {
            let indirect_block = inode.indirect_block;
            if indirect_block == 0 {
                if !alloc {
                    return None;
                }
                let new_block = self.alloc_block()?;
                return Some(new_block);
            }
            
            let idx = (block_idx - indirect_start) as usize;
            
            let block_num_at_idx = {
                let indirect = self.get_block(indirect_block)?;
                let block_nums = unsafe { 
                    core::slice::from_raw_parts(indirect.as_ptr() as *const u32, INDIRECT_BLOCKS_PER_BLOCK) 
                };
                block_nums[idx]
            };
            
            if block_num_at_idx == 0 && alloc {
                let new_block = self.alloc_block()?;
                if let Some(indirect) = self.get_block(indirect_block) {
                    let block_nums_mut = unsafe { 
                        core::slice::from_raw_parts_mut(indirect.as_ptr() as *mut u32, INDIRECT_BLOCKS_PER_BLOCK) 
                    };
                    block_nums_mut[idx] = new_block;
                }
                self.mark_block_dirty(indirect_block);
                return Some(new_block);
            }
            
            return Some(block_num_at_idx);
        }
        
        None
    }

    pub fn check_permission(&self, inode: &HvfsInode, pwid: u64, cap: u64) -> bool {
        let level = unsafe { pwid_get_level(pwid) };
        if level == 0 {
            return true;
        }

        if level > 0 && inode.sensitivity > 0 {
            let clearance = match level {
                1 => 255u8,
                2 => 128u8,
                _ => 64u8,
            };
            if clearance < inode.sensitivity {
                return false;
            }
        }

        let caps = unsafe { pwid_get_fs_capability(pwid) };
        if caps == 0 {
            return true;
        }
        if (caps & cap) == cap {
            return true;
        }

        if inode.owner_pwid != 0 && inode.owner_pwid != pwid {
            let has_trust = unsafe {
                pwid_check_trust(pwid, inode.owner_pwid, 1, cap, 8)
            };
            if has_trust != 0 {
                return true;
            }
        }

        false
    }

    pub fn get_inode(&self, inode_num: u32) -> Option<&HvfsInode> {
        if inode_num == 0 || inode_num as usize >= self.inode_table.len() {
            return None;
        }
        let inode = &self.inode_table[inode_num as usize];
        if !inode.used {
            return None;
        }
        Some(inode)
    }

    pub fn get_inode_mut(&mut self, inode_num: u32) -> Option<&mut HvfsInode> {
        if inode_num == 0 || inode_num as usize >= self.inode_table.len() {
            return None;
        }
        let inode = &mut self.inode_table[inode_num as usize];
        if !inode.used {
            return None;
        }
        inode.dirty = true;
        Some(inode)
    }

    pub fn read_file_data(&mut self, inode_num: u32, offset: u64, count: u32) -> Option<Vec<u8>> {
        let (inode_size, direct_blocks) = {
            let inode = self.get_inode(inode_num)?;
            (inode.size, inode.direct_blocks)
        };

        if offset >= inode_size as u64 {
            return Some(Vec::new());
        }

        let actual_count = (count as u64).min(inode_size as u64 - offset) as usize;
        let mut data = Vec::with_capacity(actual_count);
        let mut bytes_read = 0usize;

        while bytes_read < actual_count {
            let current_offset = offset + bytes_read as u64;
            let block_idx = (current_offset / HVFS_BLOCK_SIZE as u64) as usize;
            let block_offset = (current_offset % HVFS_BLOCK_SIZE as u64) as usize;

            if block_idx >= 12 {
                break;
            }

            let block_num = direct_blocks[block_idx];
            if block_num == 0 {
                break;
            }

            match self.get_block(block_num) {
                Some(block_data) => {
                    let bytes_to_copy = (HVFS_BLOCK_SIZE - block_offset).min(actual_count - bytes_read);
                    data.extend_from_slice(&block_data[block_offset..block_offset + bytes_to_copy]);
                    bytes_read += bytes_to_copy;
                }
                None => break,
            }
        }

        Some(data)
    }

    pub fn write_file_data(&mut self, inode_num: u32, offset: u64, data: &[u8]) -> bool {
        let mut new_size: Option<u32> = None;
        let mut success = false;

        let mut bytes_written = 0usize;
        let mut blocks_to_mark_dirty: Vec<u32> = Vec::new();

        while bytes_written < data.len() {
            let current_offset = offset + bytes_written as u64;
            let block_idx = (current_offset / HVFS_BLOCK_SIZE as u64) as usize;
            let block_offset = (current_offset % HVFS_BLOCK_SIZE as u64) as usize;

            if block_idx >= 12 {
                break;
            }

            let (need_alloc, current_block_num) = {
                let inode = match self.get_inode(inode_num) {
                    Some(i) => i,
                    None => return false,
                };
                let is_zero = inode.direct_blocks[block_idx] == 0;
                (is_zero, inode.direct_blocks[block_idx])
            };

            if need_alloc {
                let new_block_num = match self.alloc_block() {
                    Some(nb) => nb,
                    None => break,
                };
                if let Some(inode) = self.get_inode_mut(inode_num) {
                    inode.direct_blocks[block_idx] = new_block_num;
                }
                continue;
            }

            let block_num = current_block_num;

            match self.get_block(block_num) {
                Some(block_data_ref) => {
                    let bytes_to_copy = (HVFS_BLOCK_SIZE - block_offset).min(data.len() - bytes_written);
                    for i in 0..bytes_to_copy {
                        block_data_ref[block_offset + i] = data[bytes_written + i];
                    }
                    blocks_to_mark_dirty.push(block_num);
                    bytes_written += bytes_to_copy;
                }
                None => break,
            }
        }

        for block_num in blocks_to_mark_dirty {
            self.mark_block_dirty(block_num);
        }

        if bytes_written > 0 {
            let calculated_size = (offset + bytes_written as u64) as u32;
            if let Some(inode) = self.get_inode(inode_num) {
                if calculated_size > inode.size {
                    new_size = Some(calculated_size);
                }
            }
            success = true;
        }

        if let Some(size) = new_size {
            if let Some(inode) = self.get_inode_mut(inode_num) {
                inode.size = size;
            }
        }

        success
    }

    pub fn resolve_path(&mut self, path: &str) -> Option<u32> {
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
            
            let block = self.get_block(inode.direct_blocks[0])?;
            let entries = unsafe { 
                core::slice::from_raw_parts(block.as_ptr() as *const HvfsDirEntry, 
                    HVFS_BLOCK_SIZE / core::mem::size_of::<HvfsDirEntry>())
            };
            
            let mut found = false;
            for entry in entries {
                if entry.inode != 0 {
                    let name = entry.get_name();
                    if name == component {
                        current = entry.inode;
                        found = true;
                        break;
                    }
                }
            }
            
            if !found { return None; }
        }
        
        Some(current)
    }

    pub fn format(&mut self) -> i32 {
        log("[FORMAT] Starting disk format...\n");

        self.super_block.magic = HVFS_MAGIC;
        self.super_block.version = HVFS_VERSION;
        self.super_block.block_size = HVFS_BLOCK_SIZE as u32;
        self.super_block.total_blocks = HVFS_DEFAULT_BLOCKS;
        self.super_block.free_blocks = HVFS_DEFAULT_BLOCKS - 100;
        self.super_block.inode_count = HVFS_DEFAULT_INODES;
        self.super_block.free_inodes = HVFS_DEFAULT_INODES - 2;
        self.super_block.first_data_block = 10;
        self.super_block.root_inode = 1;
        self.super_block.max_path_depth = 256;
        self.super_block.max_entries = 1048576;
        self.super_block.created_time = Self::get_time();
        self.super_block.modified_time = Self::get_time();
        self.super_block.dynamic_inodes = 1;
        self.super_block.dynamic_blocks = 1;

        self.inode_table.clear();
        self.inode_table.reserve(HVFS_DEFAULT_INODES as usize);
        for _ in 0..HVFS_DEFAULT_INODES {
            self.inode_table.push(Self::new_inode());
        }

        let block_bitmap_size = ((HVFS_DEFAULT_BLOCKS + 7) / 8) as usize;
        self.block_bitmap.clear();
        self.block_bitmap.resize(block_bitmap_size, 0);

        let inode_bitmap_size = ((HVFS_DEFAULT_INODES + 7) / 8) as usize;
        self.inode_bitmap.clear();
        self.inode_bitmap.resize(inode_bitmap_size, 0);

        for i in 0..self.super_block.first_data_block {
            self.block_set_used(i);
        }

        self.create_root_inode();
        self.create_lost_found();

        if self.disk_present {
            log("[FORMAT] Writing to disk...\n");
            self.write_superblock_to_disk();
            self.write_inode_table_to_disk();
            self.write_bitmaps_to_disk();

            let dirty_blocks: Vec<u32> = self.block_cache.iter()
                .filter(|e| e.valid && e.dirty)
                .map(|e| e.block_num)
                .collect();

            let mut dirty_data: Vec<(u32, [u8; HVFS_BLOCK_SIZE])> = Vec::new();

            for block_num in dirty_blocks {
                if let Some(entry) = self.block_cache.iter().find(|e| e.block_num == block_num && e.valid) {
                    dirty_data.push((block_num, *entry.data));
                }
            }

            for (block_num, data) in dirty_data {
                self.write_block_to_disk(block_num, &data);
                if let Some(entry) = self.block_cache.iter_mut().find(|e| e.block_num == block_num && e.valid) {
                    entry.dirty = false;
                }
            }
        }

        self.initialized = true;

        log("[FORMAT] Format completed\n");
        0
    }

    fn create_root_inode(&mut self) {
        let data_block = self.alloc_block().unwrap_or(10);
        
        self.inode_table[1] = HvfsInode {
            inode_num: 1,
            mode: (HVFS_TYPE_DIR << 12) | 0o755,
            sensitivity: 0,
            reserved_uid: 0,
            size: 0,
            atime: Self::get_time(),
            mtime: Self::get_time(),
            ctime: Self::get_time(),
            owner_pwid: 0,
            group_pwid: 0,
            pwid_perm: 0o755,
            direct_blocks: [data_block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            double_indirect: 0,
            triple_indirect: 0,
            link_count: 2,
            ref_count: 1,
            used: true,
            dirty: true,
            in_cache: true,
        };
        
        self.inode_set_used(1);
        
        if let Some(block) = self.get_block(data_block) {
            let entries = unsafe { 
                core::slice::from_raw_parts_mut(block.as_mut_ptr() as *mut HvfsDirEntry, 
                    HVFS_BLOCK_SIZE / core::mem::size_of::<HvfsDirEntry>())
            };
            
            entries[0].inode = 1;
            entries[0].rec_len = core::mem::size_of::<HvfsDirEntry>() as u16;
            entries[0].name_len = 1;
            entries[0].file_type = HVFS_TYPE_DIR as u8;
            entries[0].set_name(".");
            
            entries[1].inode = 1;
            entries[1].rec_len = core::mem::size_of::<HvfsDirEntry>() as u16;
            entries[1].name_len = 2;
            entries[1].file_type = HVFS_TYPE_DIR as u8;
            entries[1].set_name("..");
            
            self.inode_table[1].size = (2 * core::mem::size_of::<HvfsDirEntry>()) as u32;
        }
        
        self.mark_block_dirty(data_block);
    }

    fn create_lost_found(&mut self) {
        let inode_num = match self.alloc_inode() {
            Some(n) => n,
            None => return,
        };
        
        let data_block = self.alloc_block().unwrap_or(0);
        
        self.inode_table[inode_num as usize] = HvfsInode {
            inode_num,
            mode: (HVFS_TYPE_DIR << 12) | 0o755,
            sensitivity: 0,
            reserved_uid: 0,
            size: 0,
            atime: Self::get_time(),
            mtime: Self::get_time(),
            ctime: Self::get_time(),
            owner_pwid: 0,
            group_pwid: 0,
            pwid_perm: 0o755,
            direct_blocks: [data_block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            double_indirect: 0,
            triple_indirect: 0,
            link_count: 2,
            ref_count: 1,
            used: true,
            dirty: true,
            in_cache: true,
        };
        
        if let Some(block) = self.get_block(self.inode_table[1].direct_blocks[0]) {
            let entries = unsafe { 
                core::slice::from_raw_parts_mut(block.as_mut_ptr() as *mut HvfsDirEntry, 
                    HVFS_BLOCK_SIZE / core::mem::size_of::<HvfsDirEntry>())
            };
            
            let num_entries = self.inode_table[1].size as usize / core::mem::size_of::<HvfsDirEntry>();
            
            entries[num_entries].inode = inode_num;
            entries[num_entries].rec_len = core::mem::size_of::<HvfsDirEntry>() as u16;
            entries[num_entries].name_len = 10;
            entries[num_entries].file_type = HVFS_TYPE_DIR as u8;
            entries[num_entries].set_name("lost+found");
            
            self.inode_table[1].size += core::mem::size_of::<HvfsDirEntry>() as u32;
        }
        
        self.mark_block_dirty(self.inode_table[1].direct_blocks[0]);
    }

    pub fn check_disk(&mut self) -> i32 {
        if !self.disk_present {
            return HVFS_DISK_NO_DISK;
        }

        if self.initialized {
            return HVFS_DISK_OK;
        }

        let result = self.read_superblock_from_disk();

        match result {
            0 => {
                log("[CHECK] Disk is formatted, can mount\n");
                HVFS_DISK_OK
            }
            -2 => {
                log("[CHECK] Disk not formatted\n");
                HVFS_DISK_UNFORMATTED
            }
            -3 => {
                log("[CHECK] Unsupported version\n");
                HVFS_DISK_VERSION_ERROR
            }
            _ => {
                log("[CHECK] Error reading disk\n");
                HVFS_DISK_NO_DISK
            }
        }
    }

    pub fn set_disk_present(&mut self, present: bool) {
        self.disk_present = present;
    }

    pub fn mount(&mut self) -> i32 {
        if self.mounted {
            return 0;
        }

        if !self.disk_present {
            log("[MOUNT] No disk present\n");
            return -1;
        }

        if !self.initialized {
            log("[MOUNT] Loading filesystem from disk...\n");

            if self.read_superblock_from_disk() != 0 {
                log("[MOUNT] Failed to read superblock, formatting...\n");
                if self.format() != 0 {
                    return -1;
                }
            } else {
                log("[MOUNT] Reading inode table...\n");
                if self.read_inode_table_from_disk() != 0 {
                    log("[MOUNT] Failed to read inode table\n");
                    return -2;
                }

                log("[MOUNT] Reading bitmaps...\n");
                if self.read_bitmaps_from_disk() != 0 {
                    log("[MOUNT] Failed to read bitmaps\n");
                    return -3;
                }

                self.initialized = true;
                log("[MOUNT] Filesystem loaded from disk successfully\n");
            }
        }

        self.super_block.mount_time = Self::get_time();
        self.super_block.mount_count += 1;
        self.mounted = true;

        log("[MOUNT] Mount completed\n");
        0
    }

    pub fn stat(&mut self, path: &str, pwid: u64) -> Option<HvfsInode> {
        let inode_num = self.resolve_path(path)?;
        let inode = self.get_inode(inode_num)?;
        
        if !self.check_permission(inode, pwid, HVFS_CAP_READ) {
            return None;
        }
        
        Some(*inode)
    }

    pub fn truncate_inode(&mut self, inode_num: u32, new_size: u64) -> i32 {
        if inode_num as usize >= self.inode_table.len() {
            return -1;
        }

        let old_size = {
            let inode = match self.get_inode(inode_num) {
                Some(i) => i,
                None => return -1,
            };
            inode.size as u64
        };

        if new_size == old_size {
            return 0;
        }

        if new_size < old_size {
            let start_block = (new_size + HVFS_BLOCK_SIZE as u64 - 1) as usize / HVFS_BLOCK_SIZE;
            let end_block = (old_size + HVFS_BLOCK_SIZE as u64 - 1) as usize / HVFS_BLOCK_SIZE;

            let blocks_to_free: Vec<u32> = {
                let inode = match self.get_inode(inode_num) {
                    Some(i) => i,
                    None => return -1,
                };
                (start_block..end_block.min(12))
                    .filter_map(|idx| {
                        if idx < 12 && inode.direct_blocks[idx] != 0 {
                            Some(inode.direct_blocks[idx])
                        } else {
                            None
                        }
                    })
                    .collect()
            };

            for block_num in blocks_to_free {
                self.block_set_free(block_num);
            }

            if new_size > 0 && start_block < 12 {
                let last_block_idx = start_block.saturating_sub(1);
                let (block_to_clear, offset_in_block) = {
                    let inode = match self.get_inode(inode_num) {
                        Some(i) => i,
                        None => return -1,
                    };
                    if last_block_idx < 12 && inode.direct_blocks[last_block_idx] != 0 {
                        (Some(inode.direct_blocks[last_block_idx]), (new_size as usize) % HVFS_BLOCK_SIZE)
                    } else {
                        (None, 0)
                    }
                };

                if let Some(block_num) = block_to_clear {
                    if let Some(block_data) = self.get_block(block_num) {
                        for byte in &mut block_data[offset_in_block..] {
                            *byte = 0;
                        }
                        drop(block_data);
                        self.mark_block_dirty(block_num);
                    }
                }
            }

            if let Some(inode) = self.get_inode_mut(inode_num) {
                for idx in start_block..end_block.min(12) {
                    inode.direct_blocks[idx] = 0;
                }
                inode.size = new_size as u32;
                inode.mtime = Self::get_time();
                inode.dirty = true;
            }
        } else {
            if let Some(inode) = self.get_inode_mut(inode_num) {
                inode.size = new_size as u32;
                inode.mtime = Self::get_time();
                inode.dirty = true;
            }
        }

        0
    }

    pub fn open(&mut self, path: &str, flags: u32, pwid: u64) -> i32 {
        if !self.initialized {
            return -1;
        }
        
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
        
        if !self.check_permission(inode, pwid, HVFS_CAP_READ) {
            return -1;
        }
        
        let inode_size = inode.size;
        
        for i in 0..HVFS_MAX_FDS {
            if !self.fds[i].used {
                self.fds[i].used = true;
                self.fds[i].fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
                self.fds[i].inode_num = inode_num;
                self.fds[i].offset = if (flags & HVFS_O_APPEND) != 0 { inode_size as u64 } else { 0 };
                self.fds[i].flags = flags;
                self.fds[i].pwid = pwid;
                
                if let Some(inode) = self.get_inode_mut(inode_num) {
                    inode.ref_count += 1;
                }
                
                return self.fds[i].fd as i32;
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
            if !self.check_permission(parent, pwid, HVFS_CAP_CREATE) {
                return None;
            }
        }
        
        let new_inode_num = self.alloc_inode()?;
        let data_block = self.alloc_block().unwrap_or(0);
        
        {
            let inode = &mut self.inode_table[new_inode_num as usize];
            inode.mode = (HVFS_TYPE_FILE << 12) | 0o644;
            inode.size = 0;
            inode.atime = Self::get_time();
            inode.mtime = Self::get_time();
            inode.ctime = Self::get_time();
            inode.owner_pwid = pwid;
            inode.group_pwid = 0;
            inode.pwid_perm = 0o644;
            inode.direct_blocks[0] = data_block;
        }
        
        let parent_block_num = self.inode_table.get(parent_num as usize)
            .map(|p| p.direct_blocks[0])
            .unwrap_or(0);
        
        let parent_size = self.inode_table.get(parent_num as usize)
            .map(|p| p.size)
            .unwrap_or(0);
        
        if let Some(block) = self.get_block(parent_block_num) {
            let entries = unsafe { 
                core::slice::from_raw_parts_mut(block.as_mut_ptr() as *mut HvfsDirEntry, 
                    HVFS_BLOCK_SIZE / core::mem::size_of::<HvfsDirEntry>())
            };
            
            let num_entries = parent_size as usize / core::mem::size_of::<HvfsDirEntry>();
            
            entries[num_entries].inode = new_inode_num;
            entries[num_entries].rec_len = core::mem::size_of::<HvfsDirEntry>() as u16;
            entries[num_entries].name_len = filename.len() as u8;
            entries[num_entries].file_type = HVFS_TYPE_FILE as u8;
            entries[num_entries].set_name(filename);
        }
        self.mark_block_dirty(parent_block_num);
        
        if let Some(parent) = self.get_inode_mut(parent_num) {
            parent.size += core::mem::size_of::<HvfsDirEntry>() as u32;
            parent.mtime = Self::get_time();
        }
        
        Some(new_inode_num)
    }

    pub fn close(&mut self, fd: u32) -> i32 {
        for i in 0..HVFS_MAX_FDS {
            if self.fds[i].used && self.fds[i].fd == fd {
                let inode_num = self.fds[i].inode_num;
                
                if let Some(inode) = self.get_inode_mut(inode_num) {
                    inode.ref_count -= 1;
                }
                
                self.fds[i].used = false;
                self.fds[i].fd = 0;
                self.fds[i].inode_num = 0;
                self.fds[i].offset = 0;
                return 0;
            }
        }
        -1
    }

    pub fn read(&mut self, fd: u32, buf: &mut [u8]) -> i32 {
        let fd_idx = match self.find_fd(fd) {
            Some(idx) => idx,
            None => return -1,
        };
        
        let inode_num = self.fds[fd_idx].inode_num;
        let offset = self.fds[fd_idx].offset;
        let pwid = self.fds[fd_idx].pwid;
        
        let inode = match self.get_inode(inode_num) {
            Some(i) => i.clone(),
            None => return -1,
        };
        
        if !self.check_permission(&inode, pwid, HVFS_CAP_READ) {
            return -1;
        }
        
        let mut bytes_read = 0;
        
        while bytes_read < buf.len() && self.fds[fd_idx].offset < inode.size as u64 {
            let block_idx = (self.fds[fd_idx].offset / HVFS_BLOCK_SIZE as u64) as u32;
            let block_offset = (self.fds[fd_idx].offset % HVFS_BLOCK_SIZE as u64) as usize;
            
            let mut bytes_to_read = HVFS_BLOCK_SIZE - block_offset;
            if bytes_to_read > buf.len() - bytes_read {
                bytes_to_read = buf.len() - bytes_read;
            }
            if bytes_to_read > (inode.size as u64 - self.fds[fd_idx].offset) as usize {
                bytes_to_read = (inode.size as u64 - self.fds[fd_idx].offset) as usize;
            }
            
            if let Some(block_num) = self.get_block_for_index(&inode, block_idx, false) {
                if let Some(block) = self.get_block(block_num) {
                    buf[bytes_read..bytes_read + bytes_to_read]
                        .copy_from_slice(&block[block_offset..block_offset + bytes_to_read]);
                }
            }
            
            bytes_read += bytes_to_read;
            self.fds[fd_idx].offset += bytes_to_read as u64;
        }
        
        if let Some(inode) = self.get_inode_mut(inode_num) {
            inode.atime = Self::get_time();
        }
        
        bytes_read as i32
    }

    pub fn write(&mut self, fd: u32, buf: &[u8]) -> i32 {
        let fd_idx = match self.find_fd(fd) {
            Some(idx) => idx,
            None => return -1,
        };
        
        let inode_num = self.fds[fd_idx].inode_num;
        let pwid = self.fds[fd_idx].pwid;
        
        {
            let inode = match self.get_inode(inode_num) {
                Some(i) => i.clone(),
                None => return -1,
            };
            
            if !self.check_permission(&inode, pwid, HVFS_CAP_WRITE) {
                return -1;
            }
        }
        
        let mut bytes_written = 0;
        
        while bytes_written < buf.len() {
            let current_offset = self.fds[fd_idx].offset;
            let block_idx = (current_offset / HVFS_BLOCK_SIZE as u64) as u32;
            let block_offset = (current_offset % HVFS_BLOCK_SIZE as u64) as usize;
            
            let mut bytes_to_write = HVFS_BLOCK_SIZE - block_offset;
            if bytes_to_write > buf.len() - bytes_written {
                bytes_to_write = buf.len() - bytes_written;
            }
            
            let inode = self.get_inode(inode_num).cloned().unwrap();
            
            if let Some(block_num) = self.get_block_for_index(&inode, block_idx, true) {
                if let Some(block) = self.get_block(block_num) {
                    block[block_offset..block_offset + bytes_to_write]
                        .copy_from_slice(&buf[bytes_written..bytes_written + bytes_to_write]);
                    self.mark_block_dirty(block_num);
                }
            }
            
            bytes_written += bytes_to_write;
            self.fds[fd_idx].offset += bytes_to_write as u64;
            
            let new_offset = self.fds[fd_idx].offset;
            if let Some(inode) = self.get_inode_mut(inode_num) {
                if new_offset > inode.size as u64 {
                    inode.size = new_offset as u32;
                }
            }
        }
        
        if let Some(inode) = self.get_inode_mut(inode_num) {
            inode.mtime = Self::get_time();
        }
        
        self.super_block.modified_time = Self::get_time();
        
        bytes_written as i32
    }

    pub fn mkdir(&mut self, path: &str, pwid: u64) -> i32 {
        if !self.initialized {
            return -1;
        }
        
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
            if !self.check_permission(parent, pwid, HVFS_CAP_CREATE) {
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
            inode.mode = (HVFS_TYPE_DIR << 12) | 0o755;
            inode.size = (2 * core::mem::size_of::<HvfsDirEntry>()) as u32;
            inode.atime = Self::get_time();
            inode.mtime = Self::get_time();
            inode.ctime = Self::get_time();
            inode.owner_pwid = pwid;
            inode.group_pwid = 0;
            inode.pwid_perm = 0o755;
            inode.direct_blocks[0] = data_block;
            inode.link_count = 2;
        }
        
        if let Some(block) = self.get_block(data_block) {
            let entries = unsafe { 
                core::slice::from_raw_parts_mut(block.as_mut_ptr() as *mut HvfsDirEntry, 
                    HVFS_BLOCK_SIZE / core::mem::size_of::<HvfsDirEntry>())
            };
            
            entries[0].inode = new_inode_num;
            entries[0].rec_len = core::mem::size_of::<HvfsDirEntry>() as u16;
            entries[0].name_len = 1;
            entries[0].file_type = HVFS_TYPE_DIR as u8;
            entries[0].set_name(".");
            
            entries[1].inode = parent_num;
            entries[1].rec_len = core::mem::size_of::<HvfsDirEntry>() as u16;
            entries[1].name_len = 2;
            entries[1].file_type = HVFS_TYPE_DIR as u8;
            entries[1].set_name("..");
            
            self.mark_block_dirty(data_block);
        }
        
        let parent_block_num = self.inode_table.get(parent_num as usize)
            .map(|p| p.direct_blocks[0])
            .unwrap_or(0);
        
        let parent_size = self.inode_table.get(parent_num as usize)
            .map(|p| p.size)
            .unwrap_or(0);
        
        if let Some(block) = self.get_block(parent_block_num) {
            let entries = unsafe { 
                core::slice::from_raw_parts_mut(block.as_mut_ptr() as *mut HvfsDirEntry, 
                    HVFS_BLOCK_SIZE / core::mem::size_of::<HvfsDirEntry>())
            };
            
            let num_entries = parent_size as usize / core::mem::size_of::<HvfsDirEntry>();
            
            entries[num_entries].inode = new_inode_num;
            entries[num_entries].rec_len = core::mem::size_of::<HvfsDirEntry>() as u16;
            entries[num_entries].name_len = dirname.len() as u8;
            entries[num_entries].file_type = HVFS_TYPE_DIR as u8;
            entries[num_entries].set_name(dirname);
        }
        self.mark_block_dirty(parent_block_num);
        
        if let Some(parent) = self.get_inode_mut(parent_num) {
            parent.size += core::mem::size_of::<HvfsDirEntry>() as u32;
            parent.link_count += 1;
            parent.mtime = Self::get_time();
        }
        
        0
    }

    fn find_fd(&self, fd: u32) -> Option<usize> {
        for (i, f) in self.fds.iter().enumerate() {
            if f.used && f.fd == fd {
                return Some(i);
            }
        }
        None
    }

    fn write_superblock_to_disk(&self) -> i32 {
        if !self.disk_present {
            return -1;
        }

        let disk_sb = HvfsSuperBlockDisk {
            magic: self.super_block.magic,
            version: self.super_block.version,
            block_size: self.super_block.block_size,
            total_blocks: self.super_block.total_blocks,
            free_blocks: self.super_block.free_blocks,
            inode_count: self.super_block.inode_count,
            free_inodes: self.super_block.free_inodes,
            first_data_block: self.super_block.first_data_block,
            root_inode: self.super_block.root_inode,
            block_bitmap_block: HVFS_BLOCK_BITMAP_START / HVFS_SECTORS_PER_BLOCK as u32,
            inode_bitmap_block: HVFS_INODE_BITMAP_START / HVFS_SECTORS_PER_BLOCK as u32,
            inode_table_block: HVFS_INODE_SECTOR_START / HVFS_SECTORS_PER_BLOCK as u32,
            created_time: self.super_block.created_time,
            modified_time: self.super_block.modified_time,
            mount_time: self.super_block.mount_time,
            mount_count: self.super_block.mount_count,
            state: self.super_block.state,
            dynamic_inodes: self.super_block.dynamic_inodes,
            dynamic_blocks: self.super_block.dynamic_blocks,
            checksum: 0,
            reserved: [0; 452],
        };

        let sb_ptr = &disk_sb as *const HvfsSuperBlockDisk as *const u8;
        let sb_size = core::mem::size_of::<HvfsSuperBlockDisk>();

        let sectors_needed = (sb_size + HVFS_DISK_SECTOR_SIZE - 1) / HVFS_DISK_SECTOR_SIZE;
        for i in 0..sectors_needed {
            let offset = i * HVFS_DISK_SECTOR_SIZE;
            unsafe {
                ata_write_sector(0, HVFS_SUPER_SECTOR_START + i as u32, sb_ptr.add(offset));
            }
        }

        log("[PERSIST] SuperBlock written to disk\n");
        0
    }

    fn read_superblock_from_disk(&mut self) -> i32 {
        if !self.disk_present {
            return -1;
        }

        let mut disk_sb = HvfsSuperBlockDisk {
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
            dynamic_inodes: 0,
            dynamic_blocks: 0,
            checksum: 0,
            reserved: [0; 452],
        };

        let sb_ptr = &mut disk_sb as *mut HvfsSuperBlockDisk as *mut u8;
        let sb_size = core::mem::size_of::<HvfsSuperBlockDisk>();

        let sectors_needed = (sb_size + HVFS_DISK_SECTOR_SIZE - 1) / HVFS_DISK_SECTOR_SIZE;
        for i in 0..sectors_needed {
            let offset = i * HVFS_DISK_SECTOR_SIZE;
            unsafe {
                ata_read_sector(0, HVFS_SUPER_SECTOR_START + i as u32, sb_ptr.add(offset));
            }
        }

        if disk_sb.magic != HVFS_MAGIC {
            log("[PERSIST] Invalid magic, disk unformatted\n");
            return -2;
        }

        if disk_sb.version != HVFS_VERSION {
            log("[PERSIST] Unsupported format version\n");
            return -3;
        }

        self.super_block.magic = disk_sb.magic;
        self.super_block.version = disk_sb.version;
        self.super_block.block_size = disk_sb.block_size;
        self.super_block.total_blocks = disk_sb.total_blocks;
        self.super_block.free_blocks = disk_sb.free_blocks;
        self.super_block.inode_count = disk_sb.inode_count;
        self.super_block.free_inodes = disk_sb.free_inodes;
        self.super_block.first_data_block = disk_sb.first_data_block;
        self.super_block.root_inode = disk_sb.root_inode;
        self.super_block.created_time = disk_sb.created_time;
        self.super_block.modified_time = disk_sb.modified_time;
        self.super_block.mount_time = disk_sb.mount_time;
        self.super_block.mount_count = disk_sb.mount_count;
        self.super_block.state = disk_sb.state;
        self.super_block.dynamic_inodes = disk_sb.dynamic_inodes;
        self.super_block.dynamic_blocks = disk_sb.dynamic_blocks;

        log("[PERSIST] SuperBlock read from disk\n");
        0
    }

    fn write_inode_table_to_disk(&self) -> i32 {
        if !self.disk_present || self.inode_table.is_empty() {
            return -1;
        }

        let inode_disk_size = core::mem::size_of::<HvfsInodeDisk>();
        let inodes_per_sector = HVFS_DISK_SECTOR_SIZE / inode_disk_size;
        let mut sector_buf = [0u8; HVFS_DISK_SECTOR_SIZE];
        let mut inode_idx = 0usize;

        for sector_idx in 0.. {
            let base_sector = HVFS_INODE_SECTOR_START + sector_idx;

            sector_buf.fill(0);
            let mut count = 0u32;

            while count < inodes_per_sector as u32 && inode_idx < self.inode_table.len() {
                let inode = &self.inode_table[inode_idx];
                let disk_inode = HvfsInodeDisk {
                    inode_num: inode.inode_num,
                    mode: inode.mode,
                    sensitivity: inode.sensitivity,
                    reserved: 0,
                    size: inode.size,
                    blocks: 0,
                    atime: inode.atime,
                    mtime: inode.mtime,
                    ctime: inode.ctime,
                    owner_pwid: inode.owner_pwid,
                    group_pwid: inode.group_pwid,
                    pwid_perm: inode.pwid_perm,
                    link_count: inode.link_count,
                    direct_blocks: inode.direct_blocks,
                    indirect_block: inode.indirect_block,
                    double_indirect: inode.double_indirect,
                    triple_indirect: inode.triple_indirect,
                    flags: if inode.used { 1 } else { 0 },
                    reserved2: [0; 18],
                };

                let offset = count as usize * inode_disk_size;
                let src = &disk_inode as *const HvfsInodeDisk as *const u8;
                sector_buf[offset..offset + inode_disk_size].copy_from_slice(
                    unsafe { core::slice::from_raw_parts(src, inode_disk_size) }
                );

                count += 1;
                inode_idx += 1;
            }

            unsafe {
                ata_write_sector(0, base_sector, sector_buf.as_ptr());
            }

            if inode_idx >= self.inode_table.len() {
                break;
            }
        }

        log("[PERSIST] Inode table written to disk\n");
        0
    }

    fn read_inode_table_from_disk(&mut self) -> i32 {
        if !self.disk_present {
            return -1;
        }

        self.inode_table.clear();
        self.inode_table.reserve(self.super_block.inode_count as usize);

        let inode_disk_size = core::mem::size_of::<HvfsInodeDisk>();
        let inodes_per_sector = HVFS_DISK_SECTOR_SIZE / inode_disk_size;
        let mut sector_buf = [0u8; HVFS_DISK_SECTOR_SIZE];
        let mut total_read = 0usize;

        for sector_idx in 0.. {
            if total_read >= self.super_block.inode_count as usize {
                break;
            }

            let base_sector = HVFS_INODE_SECTOR_START + sector_idx;

            unsafe {
                ata_read_sector(0, base_sector, sector_buf.as_mut_ptr());
            }

            for i in 0..inodes_per_sector {
                if total_read >= self.super_block.inode_count as usize {
                    break;
                }

                let offset = i * inode_disk_size;
                let disk_inode: HvfsInodeDisk = unsafe {
                    core::ptr::read(sector_buf.as_ptr().add(offset) as *const HvfsInodeDisk)
                };

                let used = (disk_inode.flags & 1) != 0;

                self.inode_table.push(HvfsInode {
                    inode_num: disk_inode.inode_num,
                    mode: disk_inode.mode,
                    sensitivity: disk_inode.sensitivity,
                    reserved_uid: disk_inode.reserved,
                    size: disk_inode.size,
                    atime: disk_inode.atime,
                    mtime: disk_inode.mtime,
                    ctime: disk_inode.ctime,
                    owner_pwid: disk_inode.owner_pwid,
                    group_pwid: disk_inode.group_pwid,
                    pwid_perm: disk_inode.pwid_perm,
                    direct_blocks: disk_inode.direct_blocks,
                    indirect_block: disk_inode.indirect_block,
                    double_indirect: disk_inode.double_indirect,
                    triple_indirect: disk_inode.triple_indirect,
                    link_count: disk_inode.link_count,
                    ref_count: 1,
                    used: used,
                    dirty: false,
                    in_cache: false,
                });

                total_read += 1;
            }
        }

        log("[PERSIST] Inode table read from disk\n");
        0
    }

    fn write_bitmaps_to_disk(&self) -> i32 {
        if !self.disk_present {
            return -1;
        }

        let block_bitmap_bytes = self.block_bitmap.len();
        let inode_bitmap_bytes = self.inode_bitmap.len();

        let mut sector_buf = [0u8; HVFS_DISK_SECTOR_SIZE];
        let mut byte_idx = 0usize;

        for sector_idx in 0.. {
            let base_sector = HVFS_BLOCK_BITMAP_START + sector_idx;

            sector_buf.fill(0);
            let mut written = 0usize;

            while written < HVFS_DISK_SECTOR_SIZE && byte_idx < block_bitmap_bytes {
                sector_buf[written] = self.block_bitmap[byte_idx];
                written += 1;
                byte_idx += 1;
            }

            unsafe {
                ata_write_sector(0, base_sector, sector_buf.as_ptr());
            }

            if byte_idx >= block_bitmap_bytes {
                break;
            }
        }

        byte_idx = 0;
        for sector_idx in 0.. {
            let base_sector = HVFS_INODE_BITMAP_START + sector_idx;

            sector_buf.fill(0);
            let mut written = 0usize;

            while written < HVFS_DISK_SECTOR_SIZE && byte_idx < inode_bitmap_bytes {
                sector_buf[written] = self.inode_bitmap[byte_idx];
                written += 1;
                byte_idx += 1;
            }

            unsafe {
                ata_write_sector(0, base_sector, sector_buf.as_ptr());
            }

            if byte_idx >= inode_bitmap_bytes {
                break;
            }
        }

        log("[PERSIST] Bitmaps written to disk\n");
        0
    }

    fn read_bitmaps_from_disk(&mut self) -> i32 {
        if !self.disk_present {
            return -1;
        }

        let block_bitmap_size = ((self.super_block.total_blocks + 7) / 8) as usize;
        let inode_bitmap_size = ((self.super_block.inode_count + 7) / 8) as usize;

        self.block_bitmap.resize(block_bitmap_size, 0);
        self.inode_bitmap.resize(inode_bitmap_size, 0);

        let mut sector_buf = [0u8; HVFS_DISK_SECTOR_SIZE];
        let mut byte_idx = 0usize;

        for sector_idx in 0.. {
            if byte_idx >= block_bitmap_size {
                break;
            }

            let base_sector = HVFS_BLOCK_BITMAP_START + sector_idx;

            unsafe {
                ata_read_sector(0, base_sector, sector_buf.as_mut_ptr());
            }

            for i in 0..HVFS_DISK_SECTOR_SIZE {
                if byte_idx + i < block_bitmap_size {
                    self.block_bitmap[byte_idx + i] = sector_buf[i];
                }
            }

            byte_idx += HVFS_DISK_SECTOR_SIZE;
        }

        byte_idx = 0;
        for sector_idx in 0.. {
            if byte_idx >= inode_bitmap_size {
                break;
            }

            let base_sector = HVFS_INODE_BITMAP_START + sector_idx;

            unsafe {
                ata_read_sector(0, base_sector, sector_buf.as_mut_ptr());
            }

            for i in 0..HVFS_DISK_SECTOR_SIZE {
                if byte_idx + i < inode_bitmap_size {
                    self.inode_bitmap[byte_idx + i] = sector_buf[i];
                }
            }

            byte_idx += HVFS_DISK_SECTOR_SIZE;
        }

        log("[PERSIST] Bitmaps read from disk\n");
        0
    }

    pub fn sync(&mut self) -> i32 {
        if !self.initialized {
            return -1;
        }

        if !self.disk_present {
            log("[SYNC] Memory-only mode: clearing dirty flags\n");
            self.super_block.modified_time = Self::get_time();

            for inode in self.inode_table.iter_mut() {
                if inode.dirty {
                    inode.dirty = false;
                }
            }

            for entry in self.block_cache.iter_mut() {
                entry.dirty = false;
            }

            log("[SYNC] Memory sync completed\n");
            return 0;
        }

        log("[SYNC] Starting full persistence sync...\n");

        self.super_block.modified_time = Self::get_time();

        if self.write_superblock_to_disk() != 0 {
            log("[SYNC] Failed to write superblock\n");
            return -1;
        }

        if self.write_inode_table_to_disk() != 0 {
            log("[SYNC] Failed to write inode table\n");
            return -2;
        }

        if self.write_bitmaps_to_disk() != 0 {
            log("[SYNC] Failed to write bitmaps\n");
            return -3;
        }

        for entry in self.block_cache.iter() {
            if entry.valid && entry.dirty && self.disk_present {
                self.write_block_to_disk(entry.block_num, &entry.data);
            }
        }

        log("[SYNC] Full persistence sync completed\n");
        0
    }

    pub fn fsck(&mut self) -> FsckResult {
        let mut result = FsckResult::new();
        
        if !self.initialized {
            result.errors.push(FsckError::NotInitialized);
            return result;
        }
        
        if self.super_block.magic != HVFS_MAGIC {
            result.errors.push(FsckError::InvalidMagic);
            return result;
        }
        
        if self.super_block.version > HVFS_VERSION {
            result.errors.push(FsckError::InvalidVersion);
            return result;
        }
        
        let mut actual_free_inodes = 0u32;
        for i in 0..self.super_block.inode_count as usize {
            if i < self.inode_table.len() {
                if !self.inode_table[i].used {
                    actual_free_inodes += 1;
                }
            }
        }
        if actual_free_inodes != self.super_block.free_inodes {
            result.warnings.push(FsckWarning::InodeCountMismatch {
                expected: self.super_block.free_inodes,
                actual: actual_free_inodes,
            });
            self.super_block.free_inodes = actual_free_inodes;
            result.fixed = true;
        }
        
        let mut actual_free_blocks = 0u32;
        for i in 0..self.super_block.total_blocks {
            if self.block_is_free(i) {
                actual_free_blocks += 1;
            }
        }
        if actual_free_blocks != self.super_block.free_blocks {
            result.warnings.push(FsckWarning::BlockCountMismatch {
                expected: self.super_block.free_blocks,
                actual: actual_free_blocks,
            });
            self.super_block.free_blocks = actual_free_blocks;
            result.fixed = true;
        }
        
        if self.super_block.root_inode == 0 || self.super_block.root_inode >= self.super_block.inode_count {
            result.errors.push(FsckError::InvalidRootInode);
        } else if !self.inode_table.get(self.super_block.root_inode as usize)
            .map(|i| i.used).unwrap_or(false) {
            result.errors.push(FsckError::RootInodeNotUsed);
        }
        
        result.passed = result.errors.is_empty();
        result
    }

    pub fn get_stats(&self) -> (u32, u32, u32, u32) {
        (
            self.super_block.total_blocks,
            self.super_block.free_blocks,
            self.super_block.inode_count,
            self.super_block.free_inodes,
        )
    }

    pub fn set_current_dir(&mut self, inode_num: u32) {
        if inode_num > 0 && (inode_num as usize) < self.inode_table.len() && self.inode_table[inode_num as usize].used {
            self.current_dir.store(inode_num, Ordering::SeqCst);
        }
    }

    pub fn get_current_dir(&self) -> u32 {
        self.current_dir.load(Ordering::SeqCst)
    }

    pub fn set_current_pwid(&mut self, pwid: u64) {
        self.current_pwid.store(pwid, Ordering::SeqCst);
    }

    pub fn get_current_pwid(&self) -> u64 {
        self.current_pwid.load(Ordering::SeqCst)
    }
}

static HVFS_DONE: AtomicBool = AtomicBool::new(false);
static mut HVFS_DATA: Option<HvFsData> = None;

/// Get mutable reference to the global HvFsData singleton.
/// Initializes on first call via lazy init; subsequent calls return the same instance.
/// Single-threaded access is guaranteed during init; later usage is guarded by caller.
pub fn get_hvfs() -> &'static mut HvFsData {
    if !HVFS_DONE.load(Ordering::Acquire) {
        unsafe {
            HVFS_DATA = Some(HvFsData::new());
        }
        HVFS_DONE.store(true, Ordering::Release);
    }
    unsafe { HVFS_DATA.as_mut().unwrap() }
}

pub fn init() {
    let hvfs = get_hvfs();
    hvfs.init();
}
