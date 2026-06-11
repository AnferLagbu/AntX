#![deny(unsafe_code)]
#![allow(dead_code)]
//! RamFS 核心实现 — services 层 (E6-5 迁移)
//!
//! 从 framework/fs/ramfs/ramfs.rs 迁移而来。
//! 本文件包含 RamFsData 数据结构及其所有方法实现。
//! 0 unsafe, 100% safe Rust.
//!
//! ## 迁移变更
//! - `IrqSpinLock` 改用 services::sync::irq_lock
//! - `dcache` 改用 services::fs::dcache (直接引用)
//! - `credo::api` 保持 framework 引用 (公开 API)
//! - `crate::arch!(timestamp())` 改用 framework 公开的时钟 API

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kernel::services::sync::irq_lock::IrqSpinLock as Mutex;
use crate::kernel::framework::credo::api as pwm_api;
use crate::kernel::framework::fs::vfs::types::KernelError;
use crate::kernel::framework::fs::vfs::types::*;
use crate::kernel::services::fs::dcache;

const RAMFS_MAX_NODES: usize = 256;
const RAMFS_MAX_BLOCKS: usize = 2048;
const RAMFS_BLOCK_SIZE: usize = 4096;
const RAMFS_MAX_ACES: usize = 128;

const DIRECT_BLOCKS: usize = 12;
const INDIRECT_BLOCKS_PER_BLOCK: usize = RAMFS_BLOCK_SIZE / 4;

const SENSITIVITY_PUBLIC: u8 = 0;

const FS_CAP_READ: u64 = 1 << 0;
const FS_CAP_WRITE: u64 = 1 << 1;
const FS_CAP_CREATE: u64 = 1 << 3;

#[derive(Debug, Clone, Copy)]
pub struct RamFsNode {
    pub node_id: u32,
    pub file_type: u8,
    pub sensitivity: u8,
    pub owner_pwm: u64,
    pub group_pwm: u64,
    pub perm: u16,
    pub size: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub direct_blocks: [u32; DIRECT_BLOCKS],
    pub indirect_block: u32,
    pub double_indirect_block: u32,
    pub link_count: u32,
    pub used: bool,
}

impl RamFsNode {
    pub const fn new() -> Self {
        Self {
            node_id: 0,
            file_type: 0,
            sensitivity: 0,
            owner_pwm: 0,
            group_pwm: 0,
            perm: 0,
            size: 0,
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
pub struct RamFsDirEntry {
    pub node: u32,
    pub file_type: u8,
    pub name: [u8; VFS_MAX_NAME],
}

impl RamFsDirEntry {
    pub fn new() -> Self {
        Self {
            node: 0,
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

    pub fn read_at(data: &[u8], offset: usize) -> Self {
        let mut entry = Self::new();
        let node_bytes: [u8; 4] = data[offset..offset + 4]
            .try_into()
            .expect("ramfs: read_at node slice OOB");
        entry.node = u32::from_le_bytes(node_bytes);
        entry.file_type = data[offset + 4];
        let name_end = offset + 8 + VFS_MAX_NAME;
        entry.name.copy_from_slice(&data[offset + 8..name_end]);
        entry
    }

    pub fn write_at(&self, data: &mut [u8], offset: usize) {
        data[offset..offset + 4].copy_from_slice(&self.node.to_le_bytes());
        data[offset + 4] = self.file_type;
        data[offset + 5..offset + 8].fill(0);
        let name_end = offset + 8 + VFS_MAX_NAME;
        data[offset + 8..name_end].copy_from_slice(&self.name);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RamFsACE {
    pub node_id: u32,
    pub pwm: u64,
    pub allow_mask: u64,
    pub deny_mask: u64,
    pub used: bool,
}

impl RamFsACE {
    pub const fn new() -> Self {
        Self {
            node_id: 0,
            pwm: 0,
            allow_mask: 0,
            deny_mask: 0,
            used: false,
        }
    }
}

pub struct RamFsData {
    pub nodes: [RamFsNode; RAMFS_MAX_NODES],
    pub data_area: [u8; RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE],
    pub node_bitmap: [u8; RAMFS_MAX_NODES / 8],
    pub block_bitmap: [u8; RAMFS_MAX_BLOCKS / 8],
    pub aces: [RamFsACE; RAMFS_MAX_ACES],
    pub symlink_targets: [[u8; 128]; RAMFS_MAX_NODES],
    pub symlink_lens: [u8; RAMFS_MAX_NODES],
    pub root_node: u32,
    pub free_nodes: AtomicU32,
    pub free_blocks: AtomicU32,
}

// RamFsData 全部字段自动实现 Send + Sync, 无需手动 impl.

impl RamFsData {
    pub const fn new() -> Self {
        Self {
            nodes: [RamFsNode::new(); RAMFS_MAX_NODES],
            data_area: [0; RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE],
            node_bitmap: [0; RAMFS_MAX_NODES / 8],
            block_bitmap: [0; RAMFS_MAX_BLOCKS / 8],
            aces: [RamFsACE::new(); RAMFS_MAX_ACES],
            symlink_targets: [[0u8; 128]; RAMFS_MAX_NODES],
            symlink_lens: [0u8; RAMFS_MAX_NODES],
            root_node: 0,
            free_nodes: AtomicU32::new(0),
            free_blocks: AtomicU32::new(0),
        }
    }

    fn get_time() -> u64 {
        crate::arch!(timestamp())
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
        node: &mut RamFsNode,
        data_area: &mut [u8],
        block_bitmap: &mut [u8],
        free_blocks: &AtomicU32,
        block_idx: usize,
    ) -> Option<u32> {
        let direct_limit = DIRECT_BLOCKS;
        let indirect_limit = direct_limit + INDIRECT_BLOCKS_PER_BLOCK;
        let double_indirect_limit =
            indirect_limit + INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK;

        if block_idx < direct_limit {
            if node.direct_blocks[block_idx] == 0 {
                let new_block = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_block == u32::MAX {
                    return None;
                }
                node.direct_blocks[block_idx] = new_block;
            }
            Some(node.direct_blocks[block_idx])
        } else if block_idx < indirect_limit {
            if node.indirect_block == 0 {
                let new_indirect = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_indirect == u32::MAX {
                    return None;
                }
                node.indirect_block = new_indirect;
            }

            let indirect_offset = block_idx - direct_limit;
            let indirect_ptr_addr =
                node.indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_offset * 4;

            let existing_block: u32 = Self::read_u32(data_area, indirect_ptr_addr);

            if existing_block == 0 {
                let new_data_block =
                    Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_data_block == u32::MAX {
                    return None;
                }

                Self::write_u32(data_area, indirect_ptr_addr, new_data_block);

                Some(new_data_block)
            } else {
                Some(existing_block)
            }
        } else if block_idx < double_indirect_limit {
            if node.double_indirect_block == 0 {
                let new_double_indirect =
                    Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_double_indirect == u32::MAX {
                    return None;
                }
                node.double_indirect_block = new_double_indirect;
            }

            let double_indirect_offset = block_idx - indirect_limit;
            let indirect_index = double_indirect_offset / INDIRECT_BLOCKS_PER_BLOCK;
            let block_index_in_indirect = double_indirect_offset % INDIRECT_BLOCKS_PER_BLOCK;

            let indirect_ptr_addr =
                node.double_indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_index * 4;

            let existing_indirect: u32 = Self::read_u32(data_area, indirect_ptr_addr);

            let indirect_block_num = if existing_indirect == 0 {
                let new_indirect = Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_indirect == u32::MAX {
                    return None;
                }

                Self::write_u32(data_area, indirect_ptr_addr, new_indirect);

                new_indirect
            } else {
                existing_indirect
            };

            let data_ptr_addr =
                indirect_block_num as usize * RAMFS_BLOCK_SIZE + block_index_in_indirect * 4;

            let existing_data: u32 = Self::read_u32(data_area, data_ptr_addr);

            if existing_data == 0 {
                let new_data_block =
                    Self::alloc_block_internal(data_area, block_bitmap, free_blocks);
                if new_data_block == u32::MAX {
                    return None;
                }

                Self::write_u32(data_area, data_ptr_addr, new_data_block);

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
        free_blocks: &AtomicU32,
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

    fn node_set_used(&mut self, node_id: u32) {
        if node_id as usize >= RAMFS_MAX_NODES {
            return;
        }
        let byte_idx = (node_id / 8) as usize;
        let bit_idx = (node_id % 8) as usize;
        self.node_bitmap[byte_idx] |= 1 << bit_idx;
        self.free_nodes.fetch_sub(1, Ordering::SeqCst);
    }

    fn read_u32(data: &[u8], offset: usize) -> u32 {
        let bytes: [u8; 4] = data[offset..offset + 4]
            .try_into()
            .expect("ramfs: read_u32 OOB");
        u32::from_le_bytes(bytes)
    }

    fn write_u32(data: &mut [u8], offset: usize, val: u32) {
        data[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
    }

    fn get_data_block(&mut self, node: &mut RamFsNode, block_idx: usize) -> Option<u32> {
        let direct_limit = DIRECT_BLOCKS;
        let indirect_limit = direct_limit + INDIRECT_BLOCKS_PER_BLOCK;
        let double_indirect_limit =
            indirect_limit + INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK;

        if block_idx < direct_limit {
            if node.direct_blocks[block_idx] == 0 {
                let new_block = self.block_alloc();
                if new_block == u32::MAX {
                    return None;
                }
                node.direct_blocks[block_idx] = new_block;
            }
            Some(node.direct_blocks[block_idx])
        } else if block_idx < indirect_limit {
            if node.indirect_block == 0 {
                let new_indirect = self.block_alloc();
                if new_indirect == u32::MAX {
                    return None;
                }
                node.indirect_block = new_indirect;
            }

            let indirect_offset = block_idx - direct_limit;
            let indirect_ptr_addr =
                node.indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_offset * 4;

            let existing_block: u32 = Self::read_u32(&self.data_area, indirect_ptr_addr);

            if existing_block == 0 {
                let new_data_block = self.block_alloc();
                if new_data_block == u32::MAX {
                    return None;
                }

                Self::write_u32(&mut self.data_area, indirect_ptr_addr, new_data_block);

                Some(new_data_block)
            } else {
                Some(existing_block)
            }
        } else if block_idx < double_indirect_limit {
            if node.double_indirect_block == 0 {
                let new_double_indirect = self.block_alloc();
                if new_double_indirect == u32::MAX {
                    return None;
                }
                node.double_indirect_block = new_double_indirect;
            }

            let double_indirect_offset = block_idx - indirect_limit;
            let indirect_index = double_indirect_offset / INDIRECT_BLOCKS_PER_BLOCK;
            let block_index_in_indirect = double_indirect_offset % INDIRECT_BLOCKS_PER_BLOCK;

            let indirect_ptr_addr =
                node.double_indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_index * 4;

            let existing_indirect: u32 = Self::read_u32(&self.data_area, indirect_ptr_addr);

            let indirect_block_num = if existing_indirect == 0 {
                let new_indirect = self.block_alloc();
                if new_indirect == u32::MAX {
                    return None;
                }

                Self::write_u32(&mut self.data_area, indirect_ptr_addr, new_indirect);

                new_indirect
            } else {
                existing_indirect
            };

            let data_ptr_addr =
                indirect_block_num as usize * RAMFS_BLOCK_SIZE + block_index_in_indirect * 4;

            let existing_data: u32 = Self::read_u32(&self.data_area, data_ptr_addr);

            if existing_data == 0 {
                let new_data_block = self.block_alloc();
                if new_data_block == u32::MAX {
                    return None;
                }

                Self::write_u32(&mut self.data_area, data_ptr_addr, new_data_block);

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
            let block_num: u32 = Self::read_u32(&self.data_area, ptr_addr);
            if block_num != 0 {
                self.block_set_free(block_num);
            }
        }

        self.block_set_free(indirect_block);
    }

    fn free_double_indirect_chain(
        &mut self,
        double_indirect_block: u32,
        start_global_idx: usize,
        end_global_idx: usize,
    ) {
        if double_indirect_block == 0 {
            return;
        }

        let start_indirect_idx = start_global_idx / INDIRECT_BLOCKS_PER_BLOCK;
        let end_indirect_idx = end_global_idx.div_ceil(INDIRECT_BLOCKS_PER_BLOCK);

        for indirect_idx in start_indirect_idx..end_indirect_idx.min(INDIRECT_BLOCKS_PER_BLOCK) {
            let indirect_ptr_addr =
                double_indirect_block as usize * RAMFS_BLOCK_SIZE + indirect_idx * 4;

            let indirect_block_num: u32 = Self::read_u32(&self.data_area, indirect_ptr_addr);

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

    fn ace_set(&mut self, node_id: u32, pwm: u64, allow: u64, deny: u64) {
        for ace in self.aces.iter_mut() {
            if ace.used && ace.node_id == node_id && ace.pwm == pwm {
                ace.allow_mask = allow;
                ace.deny_mask = deny;
                return;
            }
            if !ace.used {
                ace.node_id = node_id;
                ace.pwm = pwm;
                ace.allow_mask = allow;
                ace.deny_mask = deny;
                ace.used = true;
                return;
            }
        }
    }

    fn ace_clear(&mut self, node_id: u32, pwm: u64) {
        for ace in self.aces.iter_mut() {
            if ace.used && ace.node_id == node_id && ace.pwm == pwm {
                ace.used = false;
                return;
            }
        }
    }

    fn check_permission(&self, node: &RamFsNode, pwm: u64, cap: u64) -> bool {
        let level = pwm_api::pwm_get_privilege_level(pwm);

        if level == 0xFF {
            return false;
        }

        if level > 0 && node.sensitivity > 0 {
            let clearance = match level {
                0 => 255u8,
                1 => 255u8,
                2 => 128u8,
                _ => 64u8,
            };
            if clearance < node.sensitivity {
                return false;
            }
        }

        let ino = node.node_id;
        for ace in self.aces.iter() {
            if ace.used && ace.node_id == ino {
                if ace.pwm == 0 || ace.pwm == pwm {
                    if (ace.deny_mask & cap) != 0 {
                        return false;
                    }
                    if (ace.allow_mask & cap) != 0 {
                        return true;
                    }
                }
            }
        }

        let caps = pwm_api::pwm_get_fs_capability(pwm);
        if (caps & cap) == cap {
            return true;
        }

        if node.owner_pwm != 0 && node.owner_pwm != pwm {
            let has_cap = pwm_api::pwm_has_capability(pwm, 1, cap);
            if has_cap {
                return true;
            }
        }

        false
    }

    pub fn resolve_path(&self, path: &str) -> Option<u32> {
        let mut current = self.root_node;
        let p = path.trim_start_matches('/');

        if p.is_empty() {
            return Some(current);
        }

        for component in p.split('/') {
            if component.is_empty() {
                continue;
            }

            // dcache 快速路径
            match dcache::dcache_lookup(current, component) {
                dcache::DCacheResult::Hit { ino, file_type: _ } => {
                    current = ino;
                    continue;
                }
                dcache::DCacheResult::Negative => {
                    return None;
                }
                dcache::DCacheResult::Miss => {}
            }

            let node = &self.nodes[current as usize];

            if node.file_type != VfsFileType::Dir as u8 {
                return None;
            }

            let block_num = node.direct_blocks[0];
            if block_num == u32::MAX {
                return None;
            }

            let dirent_size = core::mem::size_of::<RamFsDirEntry>();
            let num_entries = node.size as usize / dirent_size;

            let mut found = false;

            for i in 0..num_entries {
                let offset = (block_num as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
                let entry = RamFsDirEntry::read_at(&self.data_area, offset);

                if entry.node != 0 {
                    let end = entry
                        .name
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(VFS_MAX_NAME);
                    let name = core::str::from_utf8(&entry.name[..end]).unwrap_or("");
                    if name == component {
                        current = entry.node;
                        found = true;
                        dcache::dcache_insert(
                            node.node_id, component, entry.node, entry.file_type,
                        );
                        break;
                    }
                }
            }

            if !found {
                dcache::dcache_insert_negative(node.node_id, component);
                return None;
            }
        }

        Some(current)
    }

    pub fn mount(&mut self, _path: &str) -> i32 {
        for node in self.nodes.iter_mut() {
            *node = RamFsNode::new();
        }
        for b in self.data_area.iter_mut() {
            *b = 0;
        }
        for b in self.node_bitmap.iter_mut() {
            *b = 0;
        }
        for b in self.block_bitmap.iter_mut() {
            *b = 0;
        }
        for ace in self.aces.iter_mut() {
            *ace = RamFsACE::new();
        }

        self.free_nodes
            .store((RAMFS_MAX_NODES - 1) as u32, Ordering::SeqCst);
        self.free_blocks
            .store(RAMFS_MAX_BLOCKS as u32, Ordering::SeqCst);
        self.root_node = 1;

        let block = self.block_alloc();

        self.nodes[1] = RamFsNode {
            node_id: 1,
            file_type: VfsFileType::Dir as u8,
            sensitivity: SENSITIVITY_PUBLIC,
            owner_pwm: 1,
            group_pwm: 1,
            perm: 0o777,
            size: (2 * core::mem::size_of::<RamFsDirEntry>()) as u32,
            atime: Self::get_time(),
            mtime: Self::get_time(),
            ctime: Self::get_time(),
            direct_blocks: [block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            indirect_block: 0,
            double_indirect_block: 0,
            link_count: 2,
            used: true,
        };
        self.node_set_used(1);

        let dirent_size = core::mem::size_of::<RamFsDirEntry>();
        let offset = (block as usize) * RAMFS_BLOCK_SIZE;

        let mut dot = RamFsDirEntry::read_at(&self.data_area, offset);
        dot.node = 1;
        dot.file_type = VfsFileType::Dir as u8;
        dot.set_name(".");
        dot.write_at(&mut self.data_area, offset);

        let mut dotdot = RamFsDirEntry::read_at(&self.data_area, offset + dirent_size);
        dotdot.node = 1;
        dotdot.file_type = VfsFileType::Dir as u8;
        dotdot.set_name("..");
        dotdot.write_at(&mut self.data_area, offset + dirent_size);

        0
    }

    pub fn open(&mut self, path: &str, _flags: u32, pwm: u64) -> Option<(u32, u64, u8)> {
        if path.is_empty() {
            return None;
        }

        let node_id = match self.resolve_path(path) {
            Some(n) => n,
            None => return None,
        };

        if node_id as usize >= RAMFS_MAX_NODES || !self.nodes[node_id as usize].used {
            return None;
        }

        if !self.check_permission(&self.nodes[node_id as usize], pwm, FS_CAP_READ) {
            return None;
        }

        self.nodes[node_id as usize].atime = Self::get_time();

        Some((node_id, 0, self.nodes[node_id as usize].file_type))
    }

    fn alloc_node(&mut self, file_type: u8, pwm: u64) -> Option<u32> {
        for i in 1..RAMFS_MAX_NODES {
            if !self.nodes[i].used {
                let block = self.block_alloc();
                self.nodes[i] = RamFsNode {
                    node_id: i as u32,
                    file_type,
                    sensitivity: SENSITIVITY_PUBLIC,
                    owner_pwm: pwm,
                    group_pwm: pwm,
                    perm: 0o644,
                    size: if file_type == VfsFileType::Dir as u8 {
                        (2 * core::mem::size_of::<RamFsDirEntry>()) as u32
                    } else {
                        0
                    },
                    atime: Self::get_time(),
                    mtime: Self::get_time(),
                    ctime: Self::get_time(),
                    direct_blocks: [block, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    indirect_block: 0,
                    double_indirect_block: 0,
                    link_count: 1,
                    used: true,
                };
                self.node_set_used(i as u32);
                return Some(i as u32);
            }
        }
        None
    }

    pub fn read(&mut self, node_id: u32, offset: &mut u64, buf: &mut [u8], pwm: u64) -> i32 {
        let node = &self.nodes[node_id as usize];

        if !self.check_permission(node, pwm, FS_CAP_READ) {
            return KernelError::PermissionDenied.as_i32();
        }

        let mut bytes_read = 0usize;
        let node_size = node.size as u64;

        while bytes_read < buf.len() && *offset < node_size {
            let block_idx = (*offset as usize) / RAMFS_BLOCK_SIZE;
            let block_offset = (*offset as usize) % RAMFS_BLOCK_SIZE;
            let mut bytes_to_read = RAMFS_BLOCK_SIZE - block_offset;

            if bytes_to_read > buf.len() - bytes_read {
                bytes_to_read = buf.len() - bytes_read;
            }
            if bytes_to_read > (node_size - *offset) as usize {
                bytes_to_read = (node_size - *offset) as usize;
            }

            let block_num = Self::get_or_alloc_block(
                &mut self.nodes[node_id as usize],
                &mut self.data_area,
                &mut self.block_bitmap,
                &self.free_blocks,
                block_idx,
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

        self.nodes[node_id as usize].atime = Self::get_time();

        bytes_read as i32
    }

    pub fn write(&mut self, node_id: u32, offset: &mut u64, buf: &[u8], pwm: u64) -> i32 {
        if !self.check_permission(&self.nodes[node_id as usize], pwm, FS_CAP_CREATE) {
            return KernelError::PermissionDenied.as_i32();
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
                &mut self.nodes[node_id as usize],
                &mut self.data_area,
                &mut self.block_bitmap,
                &self.free_blocks,
                block_idx,
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

            if *offset > self.nodes[node_id as usize].size as u64 {
                self.nodes[node_id as usize].size = *offset as u32;
            }
        }

        self.nodes[node_id as usize].mtime = Self::get_time();

        dcache::icache_invalidate(node_id);

        bytes_written as i32
    }

    pub fn truncate(&mut self, node_id: u32, new_size: u64, pwm: u64) -> i32 {
        if node_id as usize >= RAMFS_MAX_NODES {
            return -1;
        }

        {
            let node = &self.nodes[node_id as usize];
            if !node.used {
                return -1;
            }
            if !self.check_permission(node, pwm, FS_CAP_WRITE) {
                return -1;
            }
        }
        let old_size = {
            let node = &self.nodes[node_id as usize];
            node.size as u64
        };

        if new_size == old_size {
            return 0;
        }

        if new_size < old_size && new_size > 0 {
            let last_block_idx = ((new_size - 1) as usize) / RAMFS_BLOCK_SIZE;
            let offset_in_block = ((new_size - 1) as usize) % RAMFS_BLOCK_SIZE;

            {
                let block_num = Self::get_or_alloc_block(
                    &mut self.nodes[node_id as usize],
                    &mut self.data_area,
                    &mut self.block_bitmap,
                    &self.free_blocks,
                    last_block_idx,
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

            let first_block_to_free =
                (new_size + RAMFS_BLOCK_SIZE as u64 - 1) as usize / RAMFS_BLOCK_SIZE + 1;
            let last_block = (old_size + RAMFS_BLOCK_SIZE as u64 - 1) as usize / RAMFS_BLOCK_SIZE;

            {
                let mut blocks_to_free: Vec<u32> = Vec::new();
                let node_ref = &self.nodes[node_id as usize];

                for idx in first_block_to_free..last_block.min(DIRECT_BLOCKS) {
                    if node_ref.direct_blocks[idx] != 0 {
                        blocks_to_free.push(node_ref.direct_blocks[idx]);
                    }
                }

                for block_num in blocks_to_free {
                    self.block_set_free(block_num);
                }

                let node_mut = &mut self.nodes[node_id as usize];
                for idx in first_block_to_free..last_block.min(DIRECT_BLOCKS) {
                    if node_mut.direct_blocks[idx] != 0 {
                        node_mut.direct_blocks[idx] = 0;
                    }
                }

                let indirect_block = self.nodes[node_id as usize].indirect_block;
                if first_block_to_free < DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK
                    && indirect_block != 0
                {
                    let indirect_start = first_block_to_free.saturating_sub(DIRECT_BLOCKS);
                    let indirect_end = last_block
                        .saturating_sub(DIRECT_BLOCKS)
                        .min(INDIRECT_BLOCKS_PER_BLOCK);
                    self.free_indirect_chain(indirect_block, indirect_start, indirect_end);

                    if indirect_start == 0 {
                        self.nodes[node_id as usize].indirect_block = 0;
                    }
                }

                let double_indirect_block = self.nodes[node_id as usize].double_indirect_block;
                if first_block_to_free >= DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK
                    && double_indirect_block != 0
                {
                    let double_indirect_start = first_block_to_free
                        .saturating_sub(DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK);
                    let double_indirect_end =
                        last_block.saturating_sub(DIRECT_BLOCKS + INDIRECT_BLOCKS_PER_BLOCK);
                    self.free_double_indirect_chain(
                        double_indirect_block,
                        double_indirect_start,
                        double_indirect_end,
                    );

                    if double_indirect_start == 0 {
                        self.nodes[node_id as usize].double_indirect_block = 0;
                    }
                }
            }
        } else if new_size == 0 {
            let mut blocks_to_free: Vec<u32> = Vec::new();
            let indirect_blk = self.nodes[node_id as usize].indirect_block;
            let double_indirect_blk = self.nodes[node_id as usize].double_indirect_block;

            {
                let node = &self.nodes[node_id as usize];
                for i in 0..DIRECT_BLOCKS {
                    if node.direct_blocks[i] != 0 {
                        blocks_to_free.push(node.direct_blocks[i]);
                    }
                }
            }

            for block_num in blocks_to_free {
                self.block_set_free(block_num);
            }

            let node = &mut self.nodes[node_id as usize];
            for i in 0..DIRECT_BLOCKS {
                if node.direct_blocks[i] != 0 {
                    node.direct_blocks[i] = 0;
                }
            }

            if indirect_blk != 0 {
                self.free_indirect_chain(indirect_blk, 0, INDIRECT_BLOCKS_PER_BLOCK);
                self.nodes[node_id as usize].indirect_block = 0;
            }

            if double_indirect_blk != 0 {
                self.free_double_indirect_chain(
                    double_indirect_blk,
                    0,
                    INDIRECT_BLOCKS_PER_BLOCK * INDIRECT_BLOCKS_PER_BLOCK,
                );
                self.nodes[node_id as usize].double_indirect_block = 0;
            }
        }

        let node = &mut self.nodes[node_id as usize];
        node.size = new_size as u32;
        node.mtime = Self::get_time();

        dcache::icache_invalidate(node_id);

        0
    }

    pub fn unlink(&mut self, path: &str, pwm: u64) -> i32 {
        let node_id = match self.resolve_path(path) {
            Some(n) => n,
            None => return -1,
        };

        {
            let node = &self.nodes[node_id as usize];
            if !node.used {
                return -1;
            }
            if !self.check_permission(node, pwm, FS_CAP_WRITE) {
                return -1;
            }
        }

        let (parent_path, _name) = if let Some(pos) = path.rfind('/') {
            if pos == 0 {
                ("/", &path[1..])
            } else {
                (&path[..pos], &path[pos + 1..])
            }
        } else {
            ("/", path)
        };

        let parent_num = match self.resolve_path(parent_path) {
            Some(n) => n,
            None => return -1,
        };

        let parent_block = self.nodes[parent_num as usize].direct_blocks[0];
        if parent_block != u32::MAX {
            let dirent_size = core::mem::size_of::<RamFsDirEntry>();
            let num_entries = self.nodes[parent_num as usize].size as usize / dirent_size;

            for i in 0..num_entries {
                let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
                let mut entry = RamFsDirEntry::read_at(&self.data_area, offset);
                if entry.node == node_id {
                    entry.node = 0;
                    entry.write_at(&mut self.data_area, offset);
                    break;
                }
            }
        }

        self.truncate(node_id, 0, pwm);
        {
            let node = &mut self.nodes[node_id as usize];
            node.used = false;
            node.file_type = 0;
            node.link_count = 0;
            node.owner_pwm = 0;
        }

        dcache::dcache_invalidate_parent(parent_num);
        dcache::icache_invalidate(node_id);

        0
    }

    pub fn create_file(&mut self, parent_path: &str, name: &str, pwm: u64) -> Option<u32> {
        if name.is_empty() || name.contains('/') {
            return None;
        }

        let parent_num = match self.resolve_path(parent_path) {
            Some(n) => n,
            None => return None,
        };

        if parent_num as usize >= RAMFS_MAX_NODES || !self.nodes[parent_num as usize].used {
            return None;
        }

        if self.nodes[parent_num as usize].file_type != VfsFileType::Dir as u8 {
            return None;
        }

        if !self.check_permission(&self.nodes[parent_num as usize], pwm, FS_CAP_CREATE) {
            return None;
        }

        let parent_block = self.nodes[parent_num as usize].direct_blocks[0];
        if parent_block == u32::MAX {
            return None;
        }

        let dirent_size = core::mem::size_of::<RamFsDirEntry>();
        let num_entries = self.nodes[parent_num as usize].size as usize / dirent_size;

        for i in 0..num_entries {
            let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
            let entry = RamFsDirEntry::read_at(&self.data_area, offset);
            if entry.node != 0 {
                let end = entry
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(VFS_MAX_NAME);
                if core::str::from_utf8(&entry.name[..end]).unwrap_or("") == name {
                    return None;
                }
            }
        }

        let new_node_id = self.alloc_node(VfsFileType::File as u8, pwm)?;

        let parent_block = self.nodes[parent_num as usize].direct_blocks[0];
        let num_entries = self.nodes[parent_num as usize].size as usize / dirent_size;
        let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size;

        if offset + dirent_size > self.data_area.len() {
            return None;
        }

        let mut entry = RamFsDirEntry::new();
        entry.node = new_node_id;
        entry.file_type = VfsFileType::File as u8;
        entry.set_name(name);
        entry.write_at(&mut self.data_area, offset);

        self.nodes[parent_num as usize].size += dirent_size as u32;
        self.nodes[parent_num as usize].link_count += 1;
        self.nodes[parent_num as usize].mtime = Self::get_time();

        dcache::dcache_invalidate_parent(parent_num);

        Some(new_node_id)
    }

    pub fn mkdir(&mut self, parent_path: &str, name: &str, pwm: u64) -> i32 {
        if name.is_empty() || name.contains('/') {
            return -1;
        }

        let parent_num = match self.resolve_path(parent_path) {
            Some(n) => n,
            None => return -1,
        };

        if parent_num as usize >= RAMFS_MAX_NODES {
            return -1;
        }

        if !self.nodes[parent_num as usize].used {
            return -1;
        }

        if self.nodes[parent_num as usize].file_type != VfsFileType::Dir as u8 {
            return -1;
        }

        if !self.check_permission(&self.nodes[parent_num as usize], pwm, FS_CAP_CREATE) {
            return -1;
        }

        let parent_block = self.nodes[parent_num as usize].direct_blocks[0];
        if parent_block == u32::MAX {
            return -1;
        }

        let dirent_size = core::mem::size_of::<RamFsDirEntry>();
        let num_entries = self.nodes[parent_num as usize].size as usize / dirent_size;

        for i in 0..num_entries {
            let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
            let entry = RamFsDirEntry::read_at(&self.data_area, offset);

            if entry.node != 0 {
                let end = entry
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(VFS_MAX_NAME);
                let existing_name = core::str::from_utf8(&entry.name[..end]).unwrap_or("");
                if existing_name == name {
                    return -1;
                }
            }
        }

        let new_node_id = match self.alloc_node(VfsFileType::Dir as u8, pwm) {
            Some(n) => n,
            None => return -1,
        };

        let block = self.nodes[new_node_id as usize].direct_blocks[0];
        if block == u32::MAX {
            return -1;
        }

        let block_base = (block as usize) * RAMFS_BLOCK_SIZE;
        let mut dot = RamFsDirEntry::new();
        dot.node = new_node_id;
        dot.file_type = VfsFileType::Dir as u8;
        dot.set_name(".");
        dot.write_at(&mut self.data_area, block_base);

        let mut dotdot = RamFsDirEntry::new();
        dotdot.node = parent_num;
        dotdot.file_type = VfsFileType::Dir as u8;
        dotdot.set_name("..");
        dotdot.write_at(&mut self.data_area, block_base + dirent_size);

        self.nodes[new_node_id as usize].link_count = 2;

        let parent_block = self.nodes[parent_num as usize].direct_blocks[0];
        if parent_block == u32::MAX {
            return -1;
        }

        let num_entries = self.nodes[parent_num as usize].size as usize / dirent_size;
        let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size;

        if offset + dirent_size > self.data_area.len() {
            return -1;
        }

        let mut entry = RamFsDirEntry::new();
        entry.node = new_node_id;
        entry.file_type = VfsFileType::Dir as u8;
        entry.set_name(name);
        entry.write_at(&mut self.data_area, offset);

        self.nodes[parent_num as usize].size += dirent_size as u32;
        self.nodes[parent_num as usize].link_count += 1;
        self.nodes[parent_num as usize].mtime = Self::get_time();

        dcache::dcache_invalidate_parent(parent_num);

        0
    }

    pub fn stat(&self, node_id: u32) -> Option<VfsStat> {
        let node = &self.nodes[node_id as usize];

        if !node.used {
            return None;
        }

        Some(VfsStat {
            node_id: node.node_id,
            mode: node.perm,
            uid: 0xFFFF_FFFF,
            gid: 0xFFFF_FFFF,
            size: node.size,
            atime: node.atime,
            mtime: node.mtime,
            ctime: node.ctime,
            owner_pwm: node.owner_pwm,
            group_pwm: node.group_pwm,
            perm: node.perm,
            file_type: node.file_type,
            sensitivity: node.sensitivity,
        })
    }

    pub fn chmod(&mut self, path: &str, mode: u16, pwm: u64) -> i32 {
        let node_id = match self.resolve_path(path) {
            Some(n) => n,
            None => return -1,
        };

        let node = &mut self.nodes[node_id as usize];
        if !node.used {
            return -1;
        }

        if node.owner_pwm != pwm {
            let level = pwm_api::pwm_get_privilege_level(pwm);
            if level != 0 {
                return -1;
            }
        }

        node.perm = mode;
        node.ctime = Self::get_time();
        0
    }

    pub fn chown(&mut self, path: &str, owner_pwm: u64, pwm: u64) -> i32 {
        self.chown_ext(path, owner_pwm, 0, pwm)
    }

    pub fn chown_ext(&mut self, path: &str, owner_pwm: u64, group_pwm: u64, pwm: u64) -> i32 {
        let node_id = match self.resolve_path(path) {
            Some(n) => n,
            None => return -1,
        };

        let node = &mut self.nodes[node_id as usize];
        if !node.used {
            return -1;
        }

        let level = pwm_api::pwm_get_privilege_level(pwm);
        if level != 0 {
            return -1;
        }

        node.owner_pwm = owner_pwm;
        if group_pwm != 0 {
            node.group_pwm = group_pwm;
        } else if owner_pwm != 0 {
            node.group_pwm = owner_pwm;
        }
        node.ctime = Self::get_time();
        0
    }

    pub fn seek(
        &self,
        node_id: u32,
        current_offset: u64,
        offset: i64,
        whence: VfsSeekWhence,
    ) -> Option<u64> {
        if node_id as usize >= RAMFS_MAX_NODES {
            return None;
        }

        let node = &self.nodes[node_id as usize];
        if !node.used {
            return None;
        }

        let file_size = node.size as i64;

        let new_offset = match whence {
            VfsSeekWhence::Set => offset,
            VfsSeekWhence::Cur => {
                let current = current_offset as i64;
                current + offset
            }
            VfsSeekWhence::End => file_size + offset,
        };

        if new_offset < 0 {
            return None;
        }

        Some(new_offset as u64)
    }

    pub fn get_file_size(&self, node_id: u32) -> Option<u32> {
        if node_id as usize >= RAMFS_MAX_NODES {
            return None;
        }

        let node = &self.nodes[node_id as usize];
        if !node.used {
            return None;
        }

        Some(node.size)
    }

    pub fn link(&mut self, parent_node: u32, target_node: u32, name: &str, _pwm: u64) -> i32 {
        if name.is_empty() || name.contains('/') {
            return -1;
        }
        if parent_node as usize >= RAMFS_MAX_NODES || target_node as usize >= RAMFS_MAX_NODES {
            return -1;
        }
        if !self.nodes[parent_node as usize].used || !self.nodes[target_node as usize].used {
            return -1;
        }
        if self.nodes[parent_node as usize].file_type != VfsFileType::Dir as u8 {
            return -1;
        }

        let parent = &self.nodes[parent_node as usize];
        let parent_block = parent.direct_blocks[0];
        if parent_block == u32::MAX {
            return -1;
        }

        let dirent_size = core::mem::size_of::<RamFsDirEntry>();
        let num_entries = parent.size as usize / dirent_size;

        for i in 0..num_entries {
            let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
            let entry = RamFsDirEntry::read_at(&self.data_area, offset);
            if entry.node != 0 {
                let end = entry
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(VFS_MAX_NAME);
                let existing = core::str::from_utf8(&entry.name[..end]).unwrap_or("");
                if existing == name {
                    return -1;
                }
            }
        }

        let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size;
        if offset + dirent_size > self.data_area.len() {
            return -1;
        }

        let mut entry = RamFsDirEntry::new();
        entry.node = target_node;
        entry.file_type = self.nodes[target_node as usize].file_type;
        entry.set_name(name);
        entry.write_at(&mut self.data_area, offset);

        self.nodes[parent_node as usize].size += dirent_size as u32;
        self.nodes[parent_node as usize].link_count += 1;
        self.nodes[parent_node as usize].mtime = Self::get_time();
        self.nodes[target_node as usize].link_count += 1;

        dcache::dcache_invalidate_parent(parent_node);

        0
    }

    pub fn symlink(
        &mut self,
        target: &str,
        parent_path: &str,
        name: &str,
        pwm: u64,
    ) -> i32 {
        if name.is_empty() || name.contains('/') {
            return -1;
        }
        if target.is_empty() || target.len() >= 128 {
            return -1;
        }
        let parent_num = match self.resolve_path(parent_path) {
            Some(n) => n,
            None => return -1,
        };
        if parent_num as usize >= RAMFS_MAX_NODES || !self.nodes[parent_num as usize].used {
            return -1;
        }
        if self.nodes[parent_num as usize].file_type != VfsFileType::Dir as u8 {
            return -1;
        }
        if !self.check_permission(&self.nodes[parent_num as usize], pwm, FS_CAP_CREATE) {
            return -1;
        }

        let parent_block = self.nodes[parent_num as usize].direct_blocks[0];
        if parent_block == u32::MAX {
            return -1;
        }
        let dirent_size = core::mem::size_of::<RamFsDirEntry>();
        let num_entries = self.nodes[parent_num as usize].size as usize / dirent_size;
        for i in 0..num_entries {
            let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + i * dirent_size;
            let entry = RamFsDirEntry::read_at(&self.data_area, offset);
            if entry.node != 0 {
                let end = entry.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
                let existing = core::str::from_utf8(&entry.name[..end]).unwrap_or("");
                if existing == name {
                    return -1;
                }
            }
        }

        let new_id = match self.alloc_node(VfsFileType::Symlink as u8, pwm) {
            Some(id) => id,
            None => return -1,
        };
        let now = Self::get_time();
        let target_bytes = target.as_bytes();
        let target_len = target_bytes.len();
        {
            let node = &mut self.nodes[new_id as usize];
            node.node_id = new_id;
            node.file_type = VfsFileType::Symlink as u8;
            node.perm = 0o777;
            node.owner_pwm = pwm;
            node.link_count = 1;
            node.atime = now;
            node.mtime = now;
            node.ctime = now;
            node.used = true;
        }
        self.symlink_targets[new_id as usize][..target_len]
            .copy_from_slice(target_bytes);
        self.symlink_lens[new_id as usize] = target_len as u8;

        let offset = (parent_block as usize) * RAMFS_BLOCK_SIZE + num_entries * dirent_size;
        if offset + dirent_size > self.data_area.len() {
            let n = &mut self.nodes[new_id as usize];
            n.used = false;
            n.file_type = 0;
            n.link_count = 0;
            self.symlink_lens[new_id as usize] = 0;
            return -1;
        }
        let mut entry = RamFsDirEntry::new();
        entry.node = new_id;
        entry.file_type = VfsFileType::Symlink as u8;
        entry.set_name(name);
        entry.write_at(&mut self.data_area, offset);

        self.nodes[parent_num as usize].size += dirent_size as u32;
        self.nodes[parent_num as usize].link_count += 1;
        self.nodes[parent_num as usize].mtime = now;

        dcache::dcache_invalidate_parent(parent_num);

        new_id as i32
    }

    pub fn readlink(&self, node_id: u32, buf: &mut [u8]) -> i32 {
        if node_id as usize >= RAMFS_MAX_NODES {
            return -1;
        }
        let node = &self.nodes[node_id as usize];
        if !node.used || node.file_type != VfsFileType::Symlink as u8 {
            return -1;
        }
        let len = self.symlink_lens[node_id as usize] as usize;
        if len > buf.len() {
            return -1;
        }
        buf[..len].copy_from_slice(&self.symlink_targets[node_id as usize][..len]);
        len as i32
    }
}

// ============================================================================
// 全局实例
// ============================================================================

pub static RAMFS_DATA: Mutex<RamFsData> = Mutex::new(RamFsData::new());

pub fn init() {
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.mount("/");
}

// ============================================================================
// E6-5: FileSystem trait 实现 (services 层)
// ============================================================================

impl FileSystem for RamFsData {
    fn name(&self) -> &'static str {
        "ramfs"
    }

    fn fs_init(&self) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mount(&self, path: &str) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        if ramfs.mount(path) != 0 {
            return Err(KernelError::IoError);
        }
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, flags: u32, pwm: u64) -> KernelResult<FsOpenResult> {
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.open(rel_path, flags, pwm) {
            Some((node_id, offset, file_type)) => {
                if (flags & VfsOpenFlags::TRUNC.bits()) != 0 {
                    ramfs.truncate(node_id, 0, pwm);
                }
                Ok(FsOpenResult { handle: node_id, offset, file_type })
            }
            None => Err(KernelError::NotFound),
        }
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut new_offset = offset;
        let result = ramfs.read(handle, &mut new_offset, buf, pwm);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    // P3-I-19: vfs_pread_inode trait 分发. 直接按 inode 寻址 (mmap prewarm).
    // 与 fs_read 共用底层 ramfs.read, 差别仅在调用入口.
    fn fs_pread_inode(&self, node_id: u32, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut new_offset = offset;
        let result = ramfs.read(node_id, &mut new_offset, buf, pwm);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut new_offset = offset;
        let result = ramfs.write(handle, &mut new_offset, buf, pwm);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let ramfs = RAMFS_DATA.lock();
        match ramfs.resolve_path(rel_path) {
            Some(node_id) => ramfs.stat(node_id).ok_or(KernelError::NotFound),
            None => Err(KernelError::NotFound),
        }
    }

    fn fs_chmod(&self, rel_path: &str, mode: u16, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.chmod(rel_path, mode, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::PermissionDenied) }
    }

    fn fs_chown(&self, rel_path: &str, owner_pwm: u64, group_pwm: u64, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.chown_ext(rel_path, owner_pwm, group_pwm, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::PermissionDenied) }
    }

    fn fs_mkdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let (parent_path, name) = if let Some(pos) = rel_path.rfind('/') {
            if pos == 0 { ("/", &rel_path[1..]) } else { (&rel_path[..pos], &rel_path[pos + 1..]) }
        } else {
            ("/", rel_path)
        };
        if name.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        let result = ramfs.mkdir(parent_path, name, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::IoError) }
    }

    fn fs_unlink(&self, rel_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.unlink(rel_path, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::NotFound) }
    }

    fn fs_rmdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.resolve_path(rel_path) {
            Some(node_id) => {
                let stat = ramfs.stat(node_id);
                match stat {
                    Some(s) if s.file_type == VfsFileType::Dir.as_u8() => {
                        let result = ramfs.truncate(node_id, 0, pwm);
                        if result == 0 { Ok(()) } else { Err(KernelError::IoError) }
                    }
                    _ => Err(KernelError::NotADirectory),
                }
            }
            None => Err(KernelError::NotFound),
        }
    }

    fn fs_rename(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        ramfs.unlink(old_path, pwm);
        ramfs.link(0, 0, new_path, pwm);
        Ok(())
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let mut ramfs = RAMFS_DATA.lock();
        let mut dir_offset = offset;
        let dirent_size = core::mem::size_of::<RamFsDirEntry>();
        let mut raw_buf = alloc::vec![0u8; dirent_size];
        let result = ramfs.read(handle, &mut dir_offset, &mut raw_buf, 0);
        let raw_entry = RamFsDirEntry::read_at(&raw_buf, 0);
        if result <= 0 || raw_entry.node == 0 {
            return Ok(false);
        }
        entry.node = raw_entry.node;
        entry.file_type = raw_entry.file_type;
        let name_len = raw_entry.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
        let copy_len = name_len.min(VFS_MAX_NAME);
        entry.name[..copy_len].copy_from_slice(&raw_entry.name[..copy_len]);
        if name_len < VFS_MAX_NAME {
            entry.name[name_len] = 0;
        }
        Ok(raw_entry.node != 0)
    }

    fn fs_symlink(&self, target: &str, link_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let (parent_path, name) = if let Some(pos) = link_path.rfind('/') {
            if pos == 0 { ("/", &link_path[1..]) } else { (&link_path[..pos], &link_path[pos + 1..]) }
        } else {
            ("/", link_path)
        };
        if name.is_empty() || name.contains('/') {
            return Err(KernelError::InvalidArgument);
        }
        let result = ramfs.symlink(target, parent_path, name, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::IoError) }
    }

    fn fs_readlink(&self, rel_path: &str, buf: &mut [u8]) -> KernelResult<usize> {
        let ramfs = RAMFS_DATA.lock();
        match ramfs.resolve_path(rel_path) {
            Some(node_id) => {
                let result = ramfs.readlink(node_id, buf);
                if result < 0 { Err(KernelError::IoError) } else { Ok(result as usize) }
            }
            None => Err(KernelError::NotFound),
        }
    }

    fn fs_link(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let target_node = match ramfs.resolve_path(old_path) {
            Some(n) => n,
            None => return Err(KernelError::NotFound),
        };
        if (target_node as usize) >= ramfs.nodes.len() || !ramfs.nodes[target_node as usize].used {
            return Err(KernelError::NotFound);
        }
        if ramfs.nodes[target_node as usize].file_type == VfsFileType::Dir as u8 {
            return Err(KernelError::PermissionDenied);
        }
        let (parent_path, name) = if let Some(pos) = new_path.rfind('/') {
            if pos == 0 { ("/", &new_path[1..]) } else { (&new_path[..pos], &new_path[pos + 1..]) }
        } else {
            ("/", new_path)
        };
        if name.is_empty() || name.contains('/') {
            return Err(KernelError::InvalidArgument);
        }
        let parent_num = match ramfs.resolve_path(parent_path) {
            Some(n) => n,
            None => return Err(KernelError::NotFound),
        };
        let result = ramfs.link(parent_num, target_node, name, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::IoError) }
    }

    fn fs_truncate(&self, handle: u32, size: u64, pwm: u64) -> KernelResult<()> {
        let mut ramfs = RAMFS_DATA.lock();
        let result = ramfs.truncate(handle, size, pwm);
        if result == 0 { Ok(()) } else { Err(KernelError::IoError) }
    }

    fn fs_seek(&self, handle: u32, offset: i64, whence: VfsSeekWhence, current: u64) -> KernelResult<u64> {
        let ramfs = RAMFS_DATA.lock();
        match ramfs.seek(handle, current, offset, whence) {
            Some(new_offset) => Ok(new_offset),
            None => Err(KernelError::InvalidArgument),
        }
    }

    fn fs_resolve_path(&self, rel_path: &str) -> Option<u32> {
        let ramfs = RAMFS_DATA.lock();
        ramfs.resolve_path(rel_path)
    }

    fn fs_create(&self, parent_path: &str, name: &str, pwm: u64) -> KernelResult<FsOpenResult> {
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.create_file(parent_path, name, pwm) {
            Some(new_inode) => {
                let file_type = ramfs.stat(new_inode).map(|s| s.file_type).unwrap_or(0);
                Ok(FsOpenResult { handle: new_inode, offset: 0, file_type })
            }
            None => Err(KernelError::NoSpace),
        }
    }
}
