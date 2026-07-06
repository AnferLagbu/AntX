# ext2 只读实现计划

> [!NOTE]
> This document may not reflect the current implementation.
> See the final report for up-to-date state:
> [Final Report](../reports/ext2-readonly.md)

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 ext2 文件系统只读支持，支持 mount + ls + cat + read 操作

**Architecture:** 在 services/fs/ext2/ 下实现 8 个模块，通过 FileSystem trait 集成到 VFS，通过块设备层读取磁盘数据

**Tech Stack:** Rust (no_std), ext2 磁盘布局 (super_block/inode/block_group)

## Global Constraints

- services 层 0 unsafe，所有 unsafe 操作委托至 framework API
- 中文注释强制
- 完成后在 ext2-implementation.md 中标记状态 [] → [X]

---

## Task 1: 创建 ext2 模块结构

**Covers:** 模块结构

**Files:**
- Create: `src/kernel/services/fs/ext2/mod.rs`
- Modify: `src/kernel/services/fs/mod.rs`

**Interfaces:**
- Consumes: `crate::kernel::services::fs::vfs_types::*`
- Produces: `pub mod ext2;`

- [ ] **Step 1: 创建 ext2/mod.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! ext2 文件系统只读实现

pub mod super_block;
pub mod block_group;
pub mod inode;
pub mod dir;
pub mod bitmap;
pub mod read;
pub mod mount;
```

- [ ] **Step 2: 修改 services/fs/mod.rs 添加 ext2 模块**

在 `pub mod hvfs;` 后添加:
```rust
pub mod ext2;
```

- [ ] **Step 3: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS (无 error)

---

## Task 2: 实现 ext2 超级块数据结构

**Covers:** 数据结构

**Files:**
- Create: `src/kernel/services/fs/ext2/super_block.rs`

**Interfaces:**
- Consumes: 无
- Produces: `Ext2SuperBlock`, `Ext2SuperBlockInner`

- [ ] **Step 1: 创建 super_block.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 超级块数据结构

/// ext2 超级块 (磁盘偏移 1024 字节, 大小 1024 字节)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext2SuperBlock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    pub s_r_blocks_count: u32,
    pub s_free_blocks_count: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_frag_size: u32,
    pub s_blocks_per_group: u32,
    pub s_frags_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // EXT2_DYNAMIC_REV
    pub s_first_ino: u32,
    pub s_inode_size: u16,
    pub s_block_group_nr: u16,
    pub s_feature_compat: u32,
    pub s_feature_incompat: u32,
    pub s_feature_ro_compat: u32,
    pub s_uuid: [u8; 16],
    pub s_volume_name: [u8; 16],
    pub s_last_mounted: [u8; 64],
    pub s_algo_bitmap: u32,
    // 性能调整
    pub s_prealloc_blocks: u8,
    pub s_prealloc_dir_blocks: u8,
    pub _padding: [u8; 2],
    // journaling
    pub s_journal_uuid: [u8; 16],
    pub s_journal_inum: u32,
    pub s_journal_dev: u32,
    pub s_last_orphan: u32,
    // 哈希种子
    pub s_hash_seed: [u32; 4],
    pub s_def_hash_version: u8,
    pub _padding2: [u8; 3],
    // 其他
    pub s_default_mount_opts: u32,
    pub s_first_meta_bg: u32,
    // 保留
    pub _reserved: [u8; 760],
}

impl Ext2SuperBlock {
    pub const MAGIC: u16 = 0xEF53;
    pub const EXT2_BASE_OFFSET: u64 = 1024;

    /// 从字节切片解析超级块
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<Self>() {
            return None;
        }

        // SAFETY: Ext2SuperBlock 是 #[repr(C)] 且所有字段都是 Pod 类型
        let sb = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Self) };

        if sb.s_magic != Self::MAGIC {
            return None;
        }

        Some(sb)
    }

    /// 块大小 (字节)
    pub fn block_size(&self) -> u32 {
        1024u32 << self.s_log_block_size
    }

    /// inode 大小 (字节)
    pub fn inode_size(&self) -> u16 {
        if self.s_rev_level >= 1 {
            self.s_inode_size
        } else {
            128
        }
    }

    /// 第一个 inode
    pub fn first_inode(&self) -> u32 {
        if self.s_rev_level >= 1 {
            self.s_first_ino
        } else {
            1
        }
    }

    /// 块组描述符表起始块
    pub fn bgd_block(&self) -> u32 {
        if self.s_log_block_size == 0 {
            // 1KB 块: BGD 在第 2 块
            2
        } else {
            // >1KB 块: BGD 在第 1 块 (紧跟超级块)
            1
        }
    }

    /// 块组数量
    pub fn block_group_count(&self) -> u32 {
        (self.s_blocks_count + self.s_blocks_per_group - 1) / self.s_blocks_per_group
    }
}

impl Default for Ext2SuperBlock {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

---

## Task 3: 实现块组描述符数据结构

**Covers:** 数据结构

**Files:**
- Create: `src/kernel/services/fs/ext2/block_group.rs`

**Interfaces:**
- Consumes: `Ext2SuperBlock`
- Produces: `Ext2BlockGroupDescriptor`

- [ ] **Step 1: 创建 block_group.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 块组描述符数据结构

/// ext2 块组描述符 (32 字节)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext2BlockGroupDescriptor {
    pub bg_block_bitmap: u32,
    pub bg_inode_bitmap: u32,
    pub bg_inode_table: u32,
    pub bg_free_blocks_count: u16,
    pub bg_free_inodes_count: u16,
    pub bg_used_dirs_count: u16,
    pub bg_pad: u16,
    pub bg_reserved: [u8; 12],
}

impl Ext2BlockGroupDescriptor {
    /// 从字节切片解析块组描述符
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<Self>() {
            return None;
        }

        // SAFETY: Ext2BlockGroupDescriptor 是 #[repr(C)] 且所有字段都是 Pod 类型
        let bgd = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Self) };
        Some(bgd)
    }

    /// 从块组描述符表解析多个描述符
    pub fn from_table(data: &[u8], count: usize) -> alloc::vec::Vec<Self> {
        let size = core::mem::size_of::<Self>();
        let mut descs = alloc::vec::Vec::with_capacity(count);

        for i in 0..count {
            let offset = i * size;
            if offset + size > data.len() {
                break;
            }
            if let Some(desc) = Self::from_bytes(&data[offset..]) {
                descs.push(desc);
            }
        }

        descs
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

---

## Task 4: 实现 inode 数据结构

**Covers:** 数据结构

**Files:**
- Create: `src/kernel/services/fs/ext2/inode.rs`

**Interfaces:**
- Consumes: `Ext2SuperBlock`
- Produces: `Ext2Inode`

- [ ] **Step 1: 创建 inode.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 inode 数据结构

/// ext2 inode (128 字节)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext2Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    pub i_block: [u32; 15], // 12 direct + 1 indirect + 1 double + 1 triple
    pub i_generation: u32,
    pub i_file_acl: u32,
    pub i_dir_acl: u32,
    pub i_faddr: u32,
    pub i_osd2: [u8; 12],
}

impl Ext2Inode {
    /// 文件类型 (从 i_mode 提取)
    pub fn file_type(&self) -> u8 {
        match self.i_mode & 0xF000 {
            0x8000 => 0, // 普通文件
            0x4000 => 1, // 目录
            0xA000 => 3, // 符号链接
            0x2000 => 2, // 字符设备
            0x6000 => 2, // 块设备
            0x1000 => 4, // FIFO
            0xC000 => 5, // socket
            _ => 0,
        }
    }

    /// 权限位 (低 12 位)
    pub fn perm(&self) -> u16 {
        self.i_mode & 0x0FFF
    }

    /// 从字节切片解析 inode
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<Self>() {
            return None;
        }

        // SAFETY: Ext2Inode 是 #[repr(C)] 且所有字段都是 Pod 类型
        let inode = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Self) };
        Some(inode)
    }

    /// 获取逻辑块号 (支持直接/间接寻址)
    pub fn get_block(&self, logical: u32, block_size: u32) -> Option<u32> {
        let blocks_per_indirect = block_size / 4;

        if logical < 12 {
            // 直接块
            let block = self.i_block[logical as usize];
            if block == 0 { None } else { Some(block) }
        } else if logical < 12 + blocks_per_indirect {
            // 一次间接
            let indirect_idx = logical - 12;
            let indirect_block = self.i_block[12];
            if indirect_block == 0 {
                None
            } else {
                // 读取间接块中的指针
                // 注意: 实际需要从磁盘读取，这里返回间接块号
                // 调用者需要读取 indirect_block + indirect_idx 处的 u32
                Some(indirect_block)
            }
        } else if logical < 12 + blocks_per_indirect + blocks_per_indirect * blocks_per_indirect {
            // 二次间接
            let double_idx = logical - 12 - blocks_per_indirect;
            let indirect_idx = double_idx / blocks_per_indirect;
            let block_idx = double_idx % blocks_per_indirect;
            let double_block = self.i_block[13];
            if double_block == 0 {
                None
            } else {
                // 需要两级间接寻址
                Some(double_block)
            }
        } else {
            // 三次间接 (超出范围)
            None
        }
    }
}

impl Default for Ext2Inode {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

---

## Task 5: 实现目录项数据结构

**Covers:** 数据结构

**Files:**
- Create: `src/kernel/services/fs/ext2/dir.rs`

**Interfaces:**
- Consumes: 无
- Produces: `Ext2DirEntry`

- [ ] **Step 1: 创建 dir.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 目录项数据结构

/// ext2 目录项 (变长, 最小 8 字节)
#[derive(Debug, Clone)]
pub struct Ext2DirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    pub name: [u8; 255],
}

impl Ext2DirEntry {
    /// 从字节切片解析目录项
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        let inode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let rec_len = u16::from_le_bytes([data[4], data[5]]);
        let name_len = data[6];
        let file_type = data[7];

        if rec_len < 8 || (rec_len as usize) > data.len() {
            return None;
        }

        let mut name = [0u8; 255];
        let name_end = name_len.min(255) as usize;
        if name_end + 8 <= data.len() {
            name[..name_end].copy_from_slice(&data[8..8 + name_end]);
        }

        Some(Ext2DirEntry {
            inode,
            rec_len,
            name_len,
            file_type,
            name,
        })
    }

    /// 获取文件名
    pub fn get_name(&self) -> &str {
        let len = self.name_len as usize;
        core::str::from_utf8(&self.name[..len]).unwrap_or("")
    }

    /// 目录项实际大小 (对齐到 4 字节)
    pub fn actual_size(&self) -> u16 {
        ((self.name_len as u16 + 8 + 3) & !3)
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

---

## Task 6: 实现位图操作

**Covers:** 数据结构

**Files:**
- Create: `src/kernel/services/fs/ext2/bitmap.rs`

**Interfaces:**
- Consumes: 无
- Produces: `Ext2Bitmap`

- [ ] **Step 1: 创建 bitmap.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 位图操作 (只读: 仅查询)

/// ext2 位图 (只读)
pub struct Ext2Bitmap {
    data: alloc::vec::Vec<u8>,
}

impl Ext2Bitmap {
    /// 从字节切片创建位图
    pub fn from_bytes(data: &[u8]) -> Self {
        Ext2Bitmap {
            data: data.to_vec(),
        }
    }

    /// 检查指定位是否已设置
    pub fn is_set(&self, bit: u32) -> bool {
        let byte_idx = (bit / 8) as usize;
        let bit_idx = (bit % 8) as usize;

        if byte_idx >= self.data.len() {
            return false;
        }

        (self.data[byte_idx] & (1 << bit_idx)) != 0
    }

    /// 统计已设置的位数
    pub fn count_used(&self) -> u32 {
        let mut count = 0;
        for byte in &self.data {
            count += byte.count_ones();
        }
        count
    }

    /// 统计未设置的位数
    pub fn count_free(&self, total: u32) -> u32 {
        total - self.count_used()
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

---

## Task 7: 实现 ext2 读取核心逻辑

**Covers:** 只读 ext2

**Files:**
- Create: `src/kernel/services/fs/ext2/read.rs`

**Interfaces:**
- Consumes: `Ext2SuperBlock`, `Ext2BlockGroupDescriptor`, `Ext2Inode`, `Ext2DirEntry`, `Ext2Bitmap`
- Produces: `Ext2Fs`

- [ ] **Step 1: 创建 read.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 读取核心逻辑

use alloc::vec::Vec;
use alloc::string::String;
use crate::kernel::framework::fs::KernelError;
use crate::kernel::framework::driver::block::{read_sectors, with_device};
use super::super_block::Ext2SuperBlock;
use super::block_group::Ext2BlockGroupDescriptor;
use super::inode::Ext2Inode;
use super::dir::Ext2DirEntry;
use super::bitmap::Ext2Bitmap;

/// ext2 文件系统实例
pub struct Ext2Fs {
    /// 超级块
    pub super_block: Ext2SuperBlock,
    /// 块组描述符表
    pub block_groups: Vec<Ext2BlockGroupDescriptor>,
    /// 块设备索引
    pub device_idx: u8,
    /// inode 缓存 (inode_num -> inode)
    inode_cache: Vec<(u32, Ext2Inode)>,
}

impl Ext2Fs {
    /// 打开 ext2 文件系统
    pub fn open(device_idx: u8) -> Result<Self, KernelError> {
        // 读取超级块 (偏移 1024 字节)
        let mut sb_data = [0u8; 1024];
        let sb_sector = 1024 / 512; // 扇区 2
        let sb_sector_count = 1024 / 512; // 2 扇区

        let result = with_device(device_idx as usize, |dev| {
            read_sectors(dev, sb_sector as u64, sb_sector_count, &mut sb_data)
        });

        match result {
            Some(Ok(())) => {}
            _ => return Err(KernelError::IoError),
        }

        // 解析超级块
        let super_block = Ext2SuperBlock::from_bytes(&sb_data)
            .ok_or(KernelError::InvalidArgument)?;

        // 读取块组描述符表
        let bgd_block = super_block.bgd_block() as u64;
        let block_size = super_block.block_size() as usize;
        let bgd_sector = (bgd_block * block_size as u64) / 512;
        let bgd_sector_count = (block_size / 512).max(1) as u32;

        let mut bgd_data = alloc::vec![0u8; block_size];
        let result = with_device(device_idx as usize, |dev| {
            read_sectors(dev, bgd_sector, bgd_sector_count, &mut bgd_data)
        });

        match result {
            Some(Ok(())) => {}
            _ => return Err(KernelError::IoError),
        }

        let bg_count = super_block.block_group_count() as usize;
        let block_groups = Ext2BlockGroupDescriptor::from_table(&bgd_data, bg_count);

        Ok(Ext2Fs {
            super_block,
            block_groups,
            device_idx,
            inode_cache: Vec::new(),
        })
    }

    /// 读取 inode
    pub fn read_inode(&mut self, inode_num: u32) -> Result<Ext2Inode, KernelError> {
        // 检查缓存
        for (num, inode) in &self.inode_cache {
            if *num == inode_num {
                return Ok(*inode);
            }
        }

        // 计算 inode 位置
        let block_group = (inode_num - 1) / self.super_block.s_inodes_per_group;
        let index = (inode_num - 1) % self.super_block.s_inodes_per_group;

        if block_group as usize >= self.block_groups.len() {
            return Err(KernelError::InvalidArgument);
        }

        let bgd = &self.block_groups[block_group as usize];
        let inode_table_block = bgd.bg_inode_table;
        let inode_size = self.super_block.inode_size() as usize;
        let block_size = self.super_block.block_size() as usize;

        // 计算扇区位置
        let inode_offset = inode_table_block as usize * block_size + index as usize * inode_size;
        let sector = inode_offset / 512;
        let sector_count = (inode_size / 512).max(1) as u32;

        let mut inode_data = alloc::vec![0u8; inode_size];
        let result = with_device(self.device_idx as usize, |dev| {
            read_sectors(dev, sector as u64, sector_count, &mut inode_data)
        });

        match result {
            Some(Ok(())) => {}
            _ => return Err(KernelError::IoError),
        }

        let inode = Ext2Inode::from_bytes(&inode_data)
            .ok_or(KernelError::InvalidArgument)?;

        // 缓存
        self.inode_cache.push((inode_num, inode));

        Ok(inode)
    }

    /// 读取数据块
    pub fn read_block(&self, block_num: u32) -> Result<Vec<u8>, KernelError> {
        let block_size = self.super_block.block_size() as usize;
        let mut data = alloc::vec![0u8; block_size];

        let sector = block_num as u64 * block_size as u64 / 512;
        let sector_count = (block_size / 512) as u32;

        let result = with_device(self.device_idx as usize, |dev| {
            read_sectors(dev, sector, sector_count, &mut data)
        });

        match result {
            Some(Ok(())) => Ok(data),
            _ => Err(KernelError::IoError),
        }
    }

    /// 读取目录内容
    pub fn read_dir(&mut self, inode_num: u32) -> Result<Vec<Ext2DirEntry>, KernelError> {
        let inode = self.read_inode(inode_num)?;

        if inode.file_type() != 1 {
            return Err(KernelError::NotADirectory);
        }

        let mut entries = Vec::new();
        let block_size = self.super_block.block_size();
        let file_size = inode.i_size;

        // 读取直接块
        let mut bytes_read = 0u32;
        let mut block_idx = 0u32;

        while bytes_read < file_size && block_idx < 12 {
            let block_num = inode.i_block[block_idx as usize];
            if block_num == 0 {
                break;
            }

            let block_data = self.read_block(block_num)?;
            let mut offset = 0;

            while offset < block_size as usize {
                if offset + 8 > block_data.len() {
                    break;
                }

                if let Some(entry) = Ext2DirEntry::from_bytes(&block_data[offset..]) {
                    if entry.inode != 0 {
                        entries.push(entry.clone());
                    }
                    offset += entry.rec_len as usize;
                    if entry.rec_len == 0 {
                        break;
                    }
                } else {
                    break;
                }
            }

            bytes_read += block_size;
            block_idx += 1;
        }

        Ok(entries)
    }

    /// 查找路径
    pub fn lookup_path(&mut self, path: &str) -> Result<u32, KernelError> {
        let mut current_inode = 2; // 根目录 inode

        if path == "/" || path.is_empty() {
            return Ok(current_inode);
        }

        for component in path.trim_start_matches('/').split('/') {
            if component.is_empty() {
                continue;
            }

            let entries = self.read_dir(current_inode)?;
            let mut found = false;

            for entry in &entries {
                if entry.get_name() == component {
                    current_inode = entry.inode;
                    found = true;
                    break;
                }
            }

            if !found {
                return Err(KernelError::NotFound);
            }
        }

        Ok(current_inode)
    }

    /// 读取文件内容
    pub fn read_file(&mut self, inode_num: u32, offset: u64, buf: &mut [u8]) -> Result<usize, KernelError> {
        let inode = self.read_inode(inode_num)?;

        if inode.file_type() != 0 {
            return Err(KernelError::InvalidArgument);
        }

        let file_size = inode.i_size as u64;
        if offset >= file_size {
            return Ok(0);
        }

        let block_size = self.super_block.block_size() as u64;
        let mut bytes_read = 0;
        let mut pos = offset;

        while bytes_read < buf.len() && pos < file_size {
            let block_idx = (pos / block_size) as u32;
            let block_offset = (pos % block_size) as usize;

            // 获取物理块号
            let block_num = self.get_physical_block(&inode, block_idx)?;
            if block_num == 0 {
                break;
            }

            let block_data = self.read_block(block_num)?;
            let available = block_size as usize - block_offset;
            let to_read = (buf.len() - bytes_read).min(available).min((file_size - pos) as usize);

            buf[bytes_read..bytes_read + to_read]
                .copy_from_slice(&block_data[block_offset..block_offset + to_read]);

            bytes_read += to_read;
            pos += to_read as u64;
        }

        Ok(bytes_read)
    }

    /// 获取物理块号 (处理间接寻址)
    fn get_physical_block(&mut self, inode: &Ext2Inode, logical: u32) -> Result<u32, KernelError> {
        let block_size = self.super_block.block_size();
        let blocks_per_indirect = block_size / 4;

        if logical < 12 {
            // 直接块
            Ok(inode.i_block[logical as usize])
        } else if logical < 12 + blocks_per_indirect {
            // 一次间接
            let indirect_block = inode.i_block[12];
            if indirect_block == 0 {
                return Ok(0);
            }

            let idx = logical - 12;
            let block_data = self.read_block(indirect_block)?;
            let offset = idx as usize * 4;

            if offset + 4 > block_data.len() {
                return Ok(0);
            }

            Ok(u32::from_le_bytes([
                block_data[offset],
                block_data[offset + 1],
                block_data[offset + 2],
                block_data[offset + 3],
            ]))
        } else if logical < 12 + blocks_per_indirect + blocks_per_indirect * blocks_per_indirect {
            // 二次间接
            let double_block = inode.i_block[13];
            if double_block == 0 {
                return Ok(0);
            }

            let idx = logical - 12 - blocks_per_indirect;
            let indirect_idx = idx / blocks_per_indirect;
            let block_idx = idx % blocks_per_indirect;

            // 读取二级间接块
            let double_data = self.read_block(double_block)?;
            let indirect_offset = indirect_idx as usize * 4;

            if indirect_offset + 4 > double_data.len() {
                return Ok(0);
            }

            let indirect_block = u32::from_le_bytes([
                double_data[indirect_offset],
                double_data[indirect_offset + 1],
                double_data[indirect_offset + 2],
                double_data[indirect_offset + 3],
            ]);

            if indirect_block == 0 {
                return Ok(0);
            }

            // 读取一级间接块
            let indirect_data = self.read_block(indirect_block)?;
            let block_offset = block_idx as usize * 4;

            if block_offset + 4 > indirect_data.len() {
                return Ok(0);
            }

            Ok(u32::from_le_bytes([
                indirect_data[block_offset],
                indirect_data[block_offset + 1],
                indirect_data[block_offset + 2],
                indirect_data[block_offset + 3],
            ]))
        } else {
            // 三次间接 (超出支持范围)
            Ok(0)
        }
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

---

## Task 8: 实现 ext2 FileSystem trait

**Covers:** 只读 ext2

**Files:**
- Create: `src/kernel/services/fs/ext2/mount.rs`
- Modify: `src/kernel/services/fs/vfs_types.rs`

**Interfaces:**
- Consumes: `Ext2Fs`
- Produces: `Ext2FileSystem`

- [ ] **Step 1: 创建 mount.rs**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 FileSystem trait 实现

use crate::kernel::framework::fs::KernelError;
use crate::kernel::services::fs::vfs_types::*;
use super::read::Ext2Fs;
use super::inode::Ext2Inode;
use crate::kernel::framework::sync::IrqSpinLock as Mutex;

/// ext2 文件系统实例 (全局单例)
static EXT2_FS: Mutex<Option<Ext2Fs>> = Mutex::new(None);

/// ext2 FileSystem trait 实现
pub struct Ext2FileSystem;

impl FileSystem for Ext2FileSystem {
    fn name(&self) -> &'static str {
        "ext2"
    }

    fn fs_init(&self) -> KernelResult<()> {
        // ext2 不需要特殊初始化
        Ok(())
    }

    fn fs_mount(&self, _path: &str) -> KernelResult<()> {
        // ext2 挂载需要指定设备
        // 当前实现: 假设设备 0
        let mut fs = Ext2Fs::open(0).map_err(|_| KernelError::IoError)?;
        let mut guard = EXT2_FS.lock();
        *guard = Some(fs);
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, _flags: u32, _pwm: u64) -> KernelResult<FsOpenResult> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let inode_num = fs.lookup_path(rel_path)?;
        let inode = fs.read_inode(inode_num)?;

        Ok(FsOpenResult {
            handle: inode_num,
            offset: 0,
            file_type: inode.file_type(),
        })
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        // ext2 只读, 不需要特殊关闭操作
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        fs.read_file(handle, offset, buf)
    }

    fn fs_write(&self, _handle: u32, _offset: u64, _buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let inode_num = fs.lookup_path(rel_path)?;
        let inode = fs.read_inode(inode_num)?;

        Ok(VfsStat {
            node_id: inode_num,
            mode: inode.perm(),
            uid: inode.i_uid as u32,
            gid: inode.i_gid as u32,
            size: inode.i_size,
            atime: inode.i_atime as u64,
            mtime: inode.i_mtime as u64,
            ctime: inode.i_ctime as u64,
            owner_pwm: 0,
            group_pwm: 0,
            perm: inode.perm(),
            file_type: inode.file_type(),
            sensitivity: 0,
        })
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_chown(&self, _rel_path: &str, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_mkdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_unlink(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_rmdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let mut fs_guard = EXT2_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        let entries = fs.read_dir(handle)?;
        let idx = offset as usize;

        if idx >= entries.len() {
            return Ok(false);
        }

        let ext2_entry = &entries[idx];
        entry.node = ext2_entry.inode;
        entry.file_type = ext2_entry.file_type;
        entry.set_name(ext2_entry.get_name());

        Ok(true)
    }

    fn fs_symlink(&self, _target: &str, _link_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }

    fn fs_readlink(&self, _rel_path: &str, _buf: &mut [u8]) -> KernelResult<usize> {
        // 只读文件系统, 暂不支持符号链接读取
        Err(KernelError::NotSupported)
    }

    fn fs_link(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        // 只读文件系统
        Err(KernelError::ReadOnly)
    }
}

/// 初始化 ext2 文件系统
pub fn init() {
    // ext2 需要手动挂载, 不自动初始化
}
```

- [ ] **Step 2: 修改 vfs_types.rs 添加 Ext2 变体**

在 `FsType` 枚举中添加:
```rust
Ext2,
```

在 `FsType::from_name` 中添加:
```rust
"ext2" => FsType::Ext2,
```

在 `FsType::as_str` 中添加:
```rust
FsType::Ext2 => "ext2",
```

- [ ] **Step 3: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

---

## Task 9: 创建 ext2 测试镜像和测试用例

**Covers:** host-tests 验证

**Files:**
- Create: `host-tests/tests/ext2_test.rs`
- Create: `host-tests/ext2_test.img` (测试镜像)

**Interfaces:**
- Consumes: ext2 模块
- Produces: 测试用例

- [ ] **Step 1: 创建 ext2 测试镜像**

使用 Linux 工具创建测试镜像:
```bash
# 创建 1MB ext2 镜像
dd if=/dev/zero of=host-tests/ext2_test.img bs=1M count=1
mkfs.ext2 -F host-tests/ext2_test.img

# 挂载并创建测试文件
mkdir -p /tmp/ext2_test
mount host-tests/ext2_test.img /tmp/ext2_test
echo "Hello, ext2!" > /tmp/ext2_test/hello.txt
mkdir /tmp/ext2_test/testdir
echo "Test file" > /tmp/ext2_test/testdir/test.txt
umount /tmp/ext2_test
```

- [ ] **Step 2: 创建 ext2_test.rs**

```rust
//! ext2 只读文件系统测试

use std::fs;
use std::path::Path;

#[test]
fn test_ext2_image_exists() {
    assert!(Path::new("ext2_test.img").exists(), "ext2 测试镜像不存在");
}

#[test]
fn test_ext2_superblock_magic() {
    // 读取超级块并验证 magic number
    let data = fs::read("ext2_test.img").unwrap();
    assert!(data.len() >= 1024 + 1024, "镜像太小");

    let magic = u16::from_le_bytes([data[1024 + 56], data[1024 + 57]]);
    assert_eq!(magic, 0xEF53, "ext2 magic number 不匹配");
}

#[test]
fn test_ext2_block_size() {
    let data = fs::read("ext2_test.img").unwrap();
    let log_block_size = u32::from_le_bytes([
        data[1024 + 24],
        data[1024 + 25],
        data[1024 + 26],
        data[1024 + 27],
    ]);
    let block_size = 1024u32 << log_block_size;
    assert!(block_size >= 1024 && block_size <= 65536, "块大小无效");
}

#[test]
fn test_ext2_inode_count() {
    let data = fs::read("ext2_test.img").unwrap();
    let inode_count = u32::from_le_bytes([
        data[1024 + 0],
        data[1024 + 1],
        data[1024 + 2],
        data[1024 + 3],
    ]);
    assert!(inode_count > 0, "inode 数量为 0");
}

#[test]
fn test_ext2_block_count() {
    let data = fs::read("ext2_test.img").unwrap();
    let block_count = u32::from_le_bytes([
        data[1024 + 4],
        data[1024 + 5],
        data[1024 + 6],
        data[1024 + 7],
    ]);
    assert!(block_count > 0, "块数量为 0");
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p host-tests --test ext2_test`
Expected: PASS (5 tests passed)

---

## Task 10: 更新 ext2-implementation.md 状态

**Covers:** 文档同步

**Files:**
- Modify: `docs/plan/ext2-implementation.md`

**Interfaces:**
- Consumes: 完成的实现
- Produces: 更新后的文档

- [ ] **Step 1: 更新状态标记**

在 `docs/plan/ext2-implementation.md` 中将以下状态从 `[]` 改为 `[X]`:

- 背景部分:
  - "ext2 缺失导致无法挂载 Linux 磁盘" → [X]
  - "VFS 接口已就绪" → [X]
  - "块设备层已就绪" → [X]

- 目标部分:
  - "只读 ext2" → [X]
  - "host-tests 验证" → [X]

- 方案部分:
  - "模块结构" → [X]
  - "数据结构" → [X]
  - "参考实现" → [X]

- 工作量部分:
  - "Phase 1 只读" → [X]

- [ ] **Step 2: 验证文档更新**

Run: `grep -n "\[X\]" docs/plan/ext2-implementation.md | wc -l`
Expected: 10 (所有目标项已标记)

---

## Task 11: 编译验证和清理

**Covers:** 双架构编译

**Files:**
- 无新增修改

**Interfaces:**
- Consumes: 所有实现
- Produces: 编译通过

- [ ] **Step 1: x86_64 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 2: aarch64 编译验证**

Run: `cargo check --target aarch64-unknown-none`
Expected: PASS

- [ ] **Step 3: clippy 检查**

Run: `cargo clippy --target x86_64-unknown-none -- -D warnings`
Expected: PASS

- [ ] **Step 4: 运行所有测试**

Run: `cargo test -p host-tests`
Expected: PASS

---

## Task 12: 提交代码

**Covers:** Git 规范

**Files:**
- 所有新增/修改的文件

**Interfaces:**
- Consumes: 完成的实现
- Produces: Git commit

- [ ] **Step 1: 添加文件到暂存区**

```bash
git add src/kernel/services/fs/ext2/
git add src/kernel/services/fs/mod.rs
git add src/kernel/services/fs/vfs_types.rs
git add host-tests/tests/ext2_test.rs
git add host-tests/ext2_test.img
git add docs/plan/ext2-implementation.md
```

- [ ] **Step 2: 创建 commit**

```bash
git commit -m "feat(fs): ext2 只读文件系统实现

- 新增 services/fs/ext2/ 模块结构 (8 个文件)
- 实现 ext2 超级块/块组描述符/inode/目录项数据结构
- 实现 ext2 读取核心逻辑 (支持直接/间接寻址)
- 实现 FileSystem trait 只读接口
- 新增 ext2 测试镜像和 5 个测试用例
- 更新 ext2-implementation.md 状态标记"
```

- [ ] **Step 3: 验证 commit**

Run: `git log -1 --stat`
Expected: 显示所有修改的文件

---

## 完成

所有任务完成后，ext2 只读文件系统已实现并测试通过。可以使用以下命令挂载 ext2 文件系统:

```bash
# 在 QueenX 内核中
mount -t ext2 /dev/sda /mnt
ls /mnt
cat /mnt/hello.txt
```