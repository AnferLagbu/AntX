use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};
use alloc::vec::Vec;

use crate::fs::vfs::types::*;

extern "C" {
    fn serial_putc(port: u16, c: u8);
    fn pwid_get_level(pwid: u64) -> u8;
}

fn log(s: &str) {
    unsafe {
        for c in s.bytes() {
            serial_putc(0x3F8, c);
        }
    }
}

const RAMFS_MAX_INODES: usize = 64;
const RAMFS_MAX_BLOCKS: usize = 2048;
const RAMFS_BLOCK_SIZE: usize = 4096;

const DIRECT_BLOCKS: usize = 12;
const INDIRECT_BLOCKS_PER_BLOCK: usize = RAMFS_BLOCK_SIZE / 4; // 1024 blocks per indirect block

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
    pub direct_blocks: [u32; DIRECT_BLOCKS],
    pub indirect_block: u32,
    pub double_indirect_block: u32,
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
            direct_blocks: [0; DIRECT_BLOCKS],
            indirect_block: 0,
            double_indirect_block: 0,
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

    fn block_set_free(&mut self, block_num: u32) {
        if block_num as usize >= RAMFS_MAX_BLOCKS {
            return;
        }
        let byte_idx = (block_num / 8) as usize;
        let bit_idx = (block_num % 8) as usize;
        self.block_bitmap[byte_idx] &= !(1 << bit_idx);
        self.free_blocks.fetch_add(1, Ordering::SeqCst);
    }

    fn get_or_alloc_block(
        inode: &mut RamFsInode,
        data_area: &mut [u8],
        block_bitmap: &mut [u8],
        free_blocks: &AtomicU32,
        block_idx: usize
    ) -> Option<u32> {
        let direct_limit = DIRECT_BLOCKS;
        let indirect_limit = direct_limit + INDIRECT_BLOCKS_PER_BLOCK;
        let double_indirect_limit = indirect_limit + INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK;

        if block_idx < direct_limit {
            if inode.direct_blocks[block_idx] == 0 {
                let new_block = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_block == u32::MAX {
                    return None;
                }
                inode.direct_blocks[block_idx] = new_block;
            }
            Some(inode.direct_blocks[block_idx])
        } else if block_idx < indirect_limit {
            if inode.indirect_block == 0 {
                let new_indirect = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_indirect == u32::MAX {
                    return None;
                }
                inode.indirect_block = new_indirect;
            }

            let indirect_offset = block_idx - direct_limit;
            let data_ptr = data_area.as_mut_ptr();
            let indirect_ptr_addr = inode.indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_offset * 4;

            let existing_block: u32 = unsafe {
                core::ptr::read_volatile(data_ptr.add(indirect_ptr_addr) as *const u32)
            };

            if existing_block == 0 {
                let new_data_block = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_data_block == u32::MAX {
                    return None;
                }

                unsafe {
                    let ptr = data_ptr.add(indirect_ptr_addr) as *mut u32;
                    core::ptr::write_volatile(ptr, new_data_block);
                }

                Some(new_data_block)
            } else {
                Some(existing_block)
            }
        } else if block_idx < double_indirect_limit {
            if inode.double_indirect_block == 0 {
                let new_double_indirect = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_double_indirect == u32::MAX {
                    return None;
                }
                inode.double_indirect_block = new_double_indirect;
            }

            let double_indirect_offset = block_idx - indirect_limit;
            let indirect_index = double_indirect_offset / INDIRECT_BLOCKS_PER_BLOCK;
            let block_index_in_indirect = double_indirect_offset % INDIRECT_BLOCKS_PER_BLOCK;

            let data_ptr = data_area.as_mut_ptr();
            let indirect_ptr_addr = inode.double_indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_index * 4;

            let existing_indirect: u32 = unsafe {
                core::ptr::read_volatile(data_ptr.add(indirect_ptr_addr) as *const u32)
            };

            let indirect_block_num = if existing_indirect == 0 {
                let new_indirect = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_indirect == u32::MAX {
                    return None;
                }

                unsafe {
                    let ptr = data_ptr.add(indirect_ptr_addr) as *mut u32;
                    core::ptr::write_volatile(ptr, new_indirect);
                }

                new_indirect
            } else {
                existing_indirect
            };

            let data_ptr_addr = indirect_block_num as usize * RAMFS_BLOCK_SIZE + block_index_in_indirect * 4;

            let existing_data: u32 = unsafe {
                core::ptr::read_volatile(data_ptr.add(data_ptr_addr) as *const u32)
            };

            if existing_data == 0 {
                let new_data_block = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_data_block == u32::MAX {
                    return None;
                }

                unsafe {
                    let ptr = data_ptr.add(data_ptr_addr) as *mut u32;
                    core::ptr::write_volatile(ptr, new_data_block);
                }

                Some(new_data_block)
            } else {
                Some(existing_data)
            }
        } else {
            None
        }
    }

    fn alloc_block_internal(
        data_area: &mut [u8],
        block_bitmap: &mut [u8],
        free_blocks: &AtomicU32
    ) -> u32 {
        for i in 0..RAMFS_MAX_BLOCKS {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            if (block_bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                block_bitmap[byte_idx] |= 1 << bit_idx;
                free_blocks.fetch_sub(1, Ordering::SeqCst);

                let start = i * RAMFS_BLOCK_SIZE;
                for b in &mut data_area[start..start + RAMFS_BLOCK_SIZE] {
                    *b = 0;
                }
                return i as u32;
            }
        }
        u32::MAX
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
        u32::MAX
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

    fn get_data_block(&mut self, inode: &mut RamFsInode, block_idx: usize) -> Option<u32> {
        let direct_limit = DIRECT_BLOCKS;
        let indirect_limit = direct_limit + INDIRECT_BLOCKS_PER_BLOCK;
        let double_indirect_limit = indirect_limit + INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK;

        if block_idx < direct_limit {
            if inode.direct_blocks[block_idx] == 0 {
                let new_block = self.block_alloc();
                if new_block == u32::MAX {
                    return None;
                }
                inode.direct_blocks[block_idx] = new_block;
            }
            Some(inode.direct_blocks[block_idx])
        } else if block_idx < indirect_limit {
            if inode.indirect_block == 0 {
                let new_indirect = self.block_alloc();
                if new_indirect == u32::MAX {
                    return None;
                }
                inode.indirect_block = new_indirect;
            }

            let indirect_offset = block_idx - direct_limit;
            let data_ptr = self.data_area.as_mut_ptr();
            let indirect_ptr_addr = inode.indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_offset * 4;

            let existing_block: u32 = unsafe {
                core::ptr::read_volatile(data_ptr.add(indirect_ptr_addr) as *const u32)
            };

            if existing_block == 0 {
                let new_data_block = self.block_alloc();
                if new_data_block == u32::MAX {
                    return None;
                }

                unsafe {
                    let ptr = data_ptr.add(indirect_ptr_addr) as *mut u32;
                    core::ptr::write_volatile(ptr, new_data_block);
                }

                Some(new_data_block)
            } else {
                Some(existing_block)
            }
        } else if block_idx < double_indirect_limit {
            if inode.double_indirect_block == 0 {
                let new_double_indirect = self.block_alloc();
                if new_double_indirect == u32::MAX {
                    return None;
                }
                inode.double_indirect_block = new_double_indirect;
            }

            let double_indirect_offset = block_idx - indirect_limit;
            let indirect_index = double_indirect_offset / INDIRECT_BLOCKS_PER_BLOCK;
            let block_index_in_indirect = double_indirect_offset % INDIRECT_BLOCKS_PER_BLOCK;

            let data_ptr = self.data_area.as_mut_ptr();
            let indirect_ptr_addr = inode.double_indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_index * 4;

            let existing_indirect: u32 = unsafe {
                core::ptr::read_volatile(data_ptr.add(indirect_ptr_addr) as *const u32)
            };

            let indirect_block_num = if existing_indirect == 0 {
                let new_indirect = self.block_alloc();
                if new_indirect == u32::MAX {
                    return None;
                }

                unsafe {
                    let ptr = data_ptr.add(indirect_ptr_addr) as *mut u32;
                    core::ptr::write_volatile(ptr, new_indirect);
                }

                new_indirect
            } else {
                existing_indirect
            };

            let data_ptr_addr = indirect_block_num as usize * RAMFS_BLOCK_SIZE + block_index_in_indirect * 4;

            let existing_data: u32 = unsafe {
                core::ptr::read_volatile(data_ptr.add(data_ptr_addr) as *const u32)
            };

            if existing_data == 0 {
                let new_data_block = self.block_alloc();
                if new_data_block == u32::MAX {
                    return None;
                }

                unsafe {
                    let ptr = data_ptr.add(data_ptr_addr) as *mut u32;
                    core::ptr::write_volatile(ptr, new_data_block);
                }

                Some(new_data_block)
            } else {
                Some(existing_data)
            }
        } else {
            None
        }
    }

    fn free_indirect_chain(&mut self, indirect_block: u32, start_idx: usize, end_idx: usize) {
        if indirect_block == 0 {
            return;
        }

        for i in start_idx..end_idx.min(INDIRECT_BLOCKS_PER_BLOCK) {
            let ptr_addr = indirect_block as usize * RAMFS_BLOCK_SIZE + i * 4;

            let block_num: u32 = unsafe {
                core::ptr::read_volatile(self.data_area.as_ptr().add(ptr_addr) as *const u32)
            };

            if block_num != 0 {
                self.block_set_free(block_num);
            }
        }

        self.block_set_free(indirect_block);
    }

    fn free_double_indirect_chain(&mut self, double_indirect_block: u32,
                                   start_global_idx: usize, end_global_idx: usize) {
        if double_indirect_block == 0 {
            return;
        }

        let start_indirect_idx = start_global_idx / INDIRECT_BLOCKS_PER_BLOCK;
        let end_indirect_idx = (end_global_idx + INDIRECT_BLOCKS_PER_BLOCK - 1) / INDIRECT_BLOCKS_PER_BLOCK;

        for indirect_idx in start_indirect_idx..end_indirect_idx.min(INDIRECT_BLOCKS_PER_BLOCK) {
            let indirect_ptr_addr = double_indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_idx * 4;

            let indirect_block_num: u32 = unsafe {
                core::ptr::read_volatile(self.data_area.as_ptr().add(indirect_ptr_addr) as *const u32)
            };

            if indirect_block_num != 0 {
                let local_start = if indirect_idx == start_indirect_idx {
                    start_global_idx % INDIRECT_BLOCKS_PER_BLOCK
                } else {
                    0
                };

                let local_end = if indirect_idx == end_indirect_idx - 1 {
                    end_global_idx % INDIRECT_BLOCKS_PER_BLOCK
                } else {
                    INDIRECT_BLOCKS_PER_BLOCK
                };

                if local_end > local_start {
                    self.free_indirect_chain(indirect_block_num, local_start, local_end);
                }
            }
        }

        self.block_set_free(double_indirect_block);
    }

    fn check_permission(&self, inode: &RamFsInode, pwid: u64, access_type: u16) -> bool {
        if pwid == 0 {
            return true;
        }

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
            if block_num == u32::MAX {
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
            direct_blocks: [block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            double_indirect_block: 0,
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
        
        0
    }
    
    pub fn open(&mut self, path: &str, flags: u32, pwid: u64) -> Option<(u32, u64, u8)> {
        let inode_num = self.resolve_path(path);

        unsafe {
            for c in b"[RAMFS] " { serial_putc(0x3F8, *c); }
            for c in path.bytes() { serial_putc(0x3F8, c); }
            serial_putc(0x3F8, ' ' as u8);
        }

        let inode_num = if let Some(num) = inode_num {
            unsafe { serial_putc(0x3F8, 'E' as u8); serial_putc(0x3F8, '\n' as u8); }
            num
        } else if (flags & VfsOpenFlags::CREAT.bits()) != 0 {
            unsafe { serial_putc(0x3F8, 'C' as u8); }
            let path = if path.starts_with('/') { &path[1..] } else { path };

            let filename = if let Some(pos) = path.rfind('/') {
                &path[pos + 1..]
            } else {
                path
            };

            let dir_path = if let Some(pos) = path.rfind('/') {
                if pos == 0 { "/" } else { &path[..pos] }
            } else {
                "/"
            };

            if filename.is_empty() {
                unsafe { serial_putc(0x3F8, 'F' as u8); serial_putc(0x3F8, '\n' as u8); }
                return None;
            }

            let parent_num = self.resolve_path(dir_path)?;

            unsafe {
                serial_putc(0x3F8, 'P' as u8);
                serial_putc(0x3F8, ('0' as u8) + (parent_num / 10) as u8);
                serial_putc(0x3F8, ('0' as u8) + (parent_num % 10) as u8);
                serial_putc(0x3F8, '\n' as u8);
            }
            
            if parent_num as usize >= RAMFS_MAX_INODES {
                return None;
            }
            
            if !self.inodes[parent_num as usize].used {
                return None;
            }
            
            if !self.check_permission(&self.inodes[parent_num as usize], pwid, VFS_PERM_W) {
                return None;
            }

            let dirent_size = core::mem::size_of::<RamFsDirent>();
            let parent_block_num = self.inodes[parent_num as usize].direct_blocks[0];

            if parent_block_num == u32::MAX {
                return None;
            }

            let num_entries = self.inodes[parent_num as usize].size as usize / dirent_size;

            for i in 0..num_entries {
                let check_offset = (parent_block_num as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
                let entry: &RamFsDirent = unsafe {
                    &*(&self.data_area[check_offset] as *const u8 as *const RamFsDirent)
                };

                if entry.inode != 0 {
                    let end = entry.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
                    let existing_name = core::str::from_utf8(&entry.name[..end]).unwrap_or("");
                    if existing_name == filename {
                        return None;
                    }
                }
            }

            let new_inode_num = self.alloc_inode(VfsFileType::File as u8, pwid)?;

            let offset = (parent_block_num as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size;
            
            if offset + dirent_size > self.data_area.len() {
                return None;
            }
            
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
                    direct_blocks: [block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    indirect_block: 0,
                    double_indirect_block: 0,
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

            let block_num = Self::get_or_alloc_block(
                &mut self.inodes[inode_num as usize],
                &mut self.data_area,
                &mut self.block_bitmap,
                &self.free_blocks,
                block_idx
            );

            if let Some(block_num) = block_num {
                let start = (block_num as usize) * RAMFS_BLOCK_SIZE + block_offset;
                if start + bytes_to_read <= self.data_area.len() {
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

            let block_num = Self::get_or_alloc_block(
                &mut self.inodes[inode_num as usize],
                &mut self.data_area,
                &mut self.block_bitmap,
                &self.free_blocks,
                block_idx
            );

            match block_num {
                Some(block_num) => {
                    let start = (block_num as usize) * RAMFS_BLOCK_SIZE + block_offset;
                    if start + bytes_to_write <= self.data_area.len() {
                        self.data_area[start..start + bytes_to_write]
                            .copy_from_slice(&buf[bytes_written..bytes_written + bytes_to_write]);
                    }
                }
                None => break,
            }

            bytes_written += bytes_to_write;
            *offset += bytes_to_write as u64;

            if *offset > self.inodes[inode_num as usize].size as u64 {
                self.inodes[inode_num as usize].size = *offset as u32;
            }
        }
        
        self.inodes[inode_num as usize].mtime = Self::get_time();
        
        bytes_written as i32
    }

    pub fn truncate(&mut self, inode_num: u32, new_size: u64, pwid: u64) -> i32 {
        if inode_num as usize >= RAMFS_MAX_INODES {
            return -1;
        }

        {
            let inode = &self.inodes[inode_num as usize];
            if !inode.used {
                return -1;
            }
            if !self.check_permission(inode, pwid, VFS_PERM_W) {
                return -1;
            }
        }

        unsafe {
            for c in b"[RAMFS-TRUNC] " { serial_putc(0x3F8, *c); }
            serial_putc(0x3F8, ('0' as u8) + (inode_num / 10) as u8);
            serial_putc(0x3F8, ('0' as u8) + (inode_num % 10) as u8);
            serial_putc(0x3F8, ' ' as u8);
            serial_putc(0x3F8, ('0' as u8) + ((new_size / 1000) % 10) as u8);
            serial_putc(0x3F8, ('0' as u8) + ((new_size / 100) % 10) as u8);
            serial_putc(0x3F8, ('0' as u8) + ((new_size / 10) % 10) as u8);
            serial_putc(0x3F8, ('0' as u8) + (new_size % 10) as u8);
            serial_putc(0x3F8, '\n' as u8);
        }

        let old_size = {
            let inode = &self.inodes[inode_num as usize];
            inode.size as u64
        };

        if new_size == old_size {
            return 0;
        }

        if new_size < old_size && new_size > 0 {
            let last_block_idx = ((new_size - 1) as usize) / RAMFS_BLOCK_SIZE;
            let offset_in_block = ((new_size - 1) as usize) % RAMFS_BLOCK_SIZE;

            {
                let block_num = Self::get_or_alloc_block(
                    &mut self.inodes[inode_num as usize],
                    &mut self.data_area,
                    &mut self.block_bitmap,
                    &self.free_blocks,
                    last_block_idx
                );

                if let Some(block_num) = block_num {
                    if block_num != 0 {
                        let start = block_num as usize * RAMFS_BLOCK_SIZE + offset_in_block + 1;
                        let end = (block_num as usize + 1) * RAMFS_BLOCK_SIZE;
                        let data_len = self.data_area.len();
                        for byte in &mut self.data_area[start..end.min(data_len)] {
                            *byte = 0;
                        }
                    }
                }
            }

            let first_block_to_free = (new_size + RAMFS_BLOCK_SIZE as u64 - 1) as usize / RAMFS_BLOCK_SIZE + 1;
            let last_block = (old_size + RAMFS_BLOCK_SIZE as u64 - 1) as usize / RAMFS_BLOCK_SIZE;

            {
                let mut blocks_to_free: Vec<u32> = Vec::new();
                let inode_ref = &self.inodes[inode_num as usize];

                for idx in first_block_to_free..last_block.min(DIRECT_BLOCKS) {
                    if inode_ref.direct_blocks[idx] != 0 {
                        blocks_to_free.push(inode_ref.direct_blocks[idx]);
                    }
                }

                for block_num in blocks_to_free {
                    self.block_set_free(block_num);
                }

                let inode_mut = &mut self.inodes[inode_num as usize];
                for idx in first_block_to_free..last_block.min(DIRECT_BLOCKS) {
                    if inode_mut.direct_blocks[idx] != 0 {
                        inode_mut.direct_blocks[idx] = 0;
                    }
                }

                let indirect_block = self.inodes[inode_num as usize].indirect_block;
                if first_block_to_free < DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK && indirect_block != 0 {
                    let indirect_start = first_block_to_free.saturating_sub(DIRECT_BLOCKS).max(0);
                    let indirect_end = last_block.saturating_sub(DIRECT_BLOCKS).min(INDIRECT_BLOCKS_PER_BLOCK);
                    self.free_indirect_chain(indirect_block, indirect_start, indirect_end);

                    if indirect_start == 0 {
                        self.inodes[inode_num as usize].indirect_block = 0;
                    }
                }

                let double_indirect_block = self.inodes[inode_num as usize].double_indirect_block;
                if first_block_to_free >= DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK && double_indirect_block != 0 {
                    let double_indirect_start = first_block_to_free.saturating_sub(DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK).max(0);
                    let double_indirect_end = last_block.saturating_sub(DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK);
                    self.free_double_indirect_chain(double_indirect_block, double_indirect_start, double_indirect_end);

                    if double_indirect_start == 0 {
                        self.inodes[inode_num as usize].double_indirect_block = 0;
                    }
                }
            }
        } else if new_size == 0 {
            let mut blocks_to_free: Vec<u32> = Vec::new();
            let indirect_blk = self.inodes[inode_num as usize].indirect_block;
            let double_indirect_blk = self.inodes[inode_num as usize].double_indirect_block;

            {
                let inode = &self.inodes[inode_num as usize];
                for i in 0..DIRECT_BLOCKS {
                    if inode.direct_blocks[i] != 0 {
                        blocks_to_free.push(inode.direct_blocks[i]);
                    }
                }
            }

            for block_num in blocks_to_free {
                self.block_set_free(block_num);
            }

            let inode = &mut self.inodes[inode_num as usize];
            for i in 0..DIRECT_BLOCKS {
                if inode.direct_blocks[i] != 0 {
                    inode.direct_blocks[i] = 0;
                }
            }

            if indirect_blk != 0 {
                self.free_indirect_chain(indirect_blk, 0, INDIRECT_BLOCKS_PER_BLOCK);
                self.inodes[inode_num as usize].indirect_block = 0;
            }

            if double_indirect_blk != 0 {
                self.free_double_indirect_chain(double_indirect_blk, 0,
                                                  INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK);
                self.inodes[inode_num as usize].double_indirect_block = 0;
            }
        }

        let inode = &mut self.inodes[inode_num as usize];
        inode.size = new_size as u32;
        inode.mtime = Self::get_time();

        0
    }

    pub fn mkdir(&mut self, parent_path: &str, name: &str, pwid: u64) -> i32 {
        unsafe {
            for c in b"[RAMFS-MKDIR] " { serial_putc(0x3F8, *c); }
            for c in parent_path.bytes() { serial_putc(0x3F8, c); }
            serial_putc(0x3F8, ' ' as u8);
            for c in name.bytes() { serial_putc(0x3F8, c); }
            serial_putc(0x3F8, '\n' as u8);
        }

        if name.is_empty() || name.contains('/') {
            return -1;
        }

        let parent_num = match self.resolve_path(parent_path) {
            Some(n) => {
                unsafe { serial_putc(0x3F8, 'P' as u8); }
                n
            },
            None => {
                unsafe { serial_putc(0x3F8, 'X' as u8); serial_putc(0x3F8, '\n' as u8); }
                return -1;
            },
        };

        if parent_num as usize >= RAMFS_MAX_INODES {
            return -1;
        }

        if !self.inodes[parent_num as usize].used {
            return -1;
        }

        if self.inodes[parent_num as usize].file_type != VfsFileType::Dir as u8 {
            return -1;
        }

        if !self.check_permission(&self.inodes[parent_num as usize], pwid, VFS_PERM_W) {
            return -1;
        }

        let parent_block = self.inodes[parent_num as usize].direct_blocks[0];
        if parent_block == u32::MAX {
            return -1;
        }

        let dirent_size = core::mem::size_of::<RamFsDirent>();
        let num_entries = self.inodes[parent_num as usize].size as usize / dirent_size;

        for i in 0..num_entries {
            let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
            let entry: &RamFsDirent = unsafe {
                &*(&self.data_area[offset] as *const u8 as *const RamFsDirent)
            };

            if entry.inode != 0 {
                let end = entry.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
                let existing_name = core::str::from_utf8(&entry.name[..end]).unwrap_or("");
                if existing_name == name {
                    unsafe { serial_putc(0x3F8, 'E' as u8); serial_putc(0x3F8, '\n' as u8); }
                    return -1;
                }
            }
        }

        let new_inode_num = match self.alloc_inode(VfsFileType::Dir as u8, pwid) {
            Some(n) => n,
            None => return -1,
        };
        
        let block = self.inodes[new_inode_num as usize].direct_blocks[0];
        if block == u32::MAX {
            return -1;
        }

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
        
        let parent_block = self.inodes[parent_num as usize].direct_blocks[0];
        if parent_block == u32::MAX {
            return -1;
        }
        
        let num_entries = self.inodes[parent_num as usize].size as usize / dirent_size;
        let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size;
        
        if offset + dirent_size > self.data_area.len() {
            return -1;
        }
        
        let entry: &mut RamFsDirent = unsafe {
            &mut *(&mut self.data_area[offset] as *mut u8 as *mut RamFsDirent)
        };
        entry.inode = new_inode_num;
        entry.file_type = VfsFileType::Dir as u8;
        entry.set_name(name);
        
        self.inodes[parent_num as usize].size += dirent_size as u32;
        self.inodes[parent_num as usize].link_count += 1;
        self.inodes[parent_num as usize].mtime = Self::get_time();
        
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

    pub fn seek(&self, inode_num: u32, offset: i64, whence: VfsSeekWhence) -> Option<u64> {
        if inode_num as usize >= RAMFS_MAX_INODES {
            return None;
        }

        let inode = &self.inodes[inode_num as usize];
        if !inode.used {
            return None;
        }

        let file_size = inode.size as i64;

        let new_offset = match whence {
            VfsSeekWhence::Set => offset,
            VfsSeekWhence::Cur => {
                let current = 0i64;
                current + offset
            }
            VfsSeekWhence::End => file_size + offset,
        };

        if new_offset < 0 {
            return None;
        }

        Some(new_offset as u64)
    }

    pub fn get_file_size(&self, inode_num: u32) -> Option<u32> {
        if inode_num as usize >= RAMFS_MAX_INODES {
            return None;
        }

        let inode = &self.inodes[inode_num as usize];
        if !inode.used {
            return None;
        }

        Some(inode.size)
    }
}

pub static RAMFS_DATA: Mutex<RamFsData> = Mutex::new(RamFsData::new());

pub fn init() {
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.mount("/");
}
