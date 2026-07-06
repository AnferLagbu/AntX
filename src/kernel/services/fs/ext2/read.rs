#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! ext2 读取核心逻辑

use alloc::vec::Vec;
use crate::kernel::framework::fs::KernelError;
use crate::kernel::framework::driver::block::{read_sectors, with_device};
use super::super_block::Ext2SuperBlock;
use super::block_group::Ext2BlockGroupDescriptor;
use super::inode::Ext2Inode;
use super::dir::Ext2DirEntry;

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