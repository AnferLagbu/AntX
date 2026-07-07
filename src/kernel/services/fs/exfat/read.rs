#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! exFAT 读取核心逻辑

use alloc::string::String;
use alloc::vec::Vec;
use crate::kernel::framework::fs::KernelError;
use crate::kernel::framework::driver::block::{read_sectors, with_device};
use super::super_block::ExfatSuperBlock;
use super::dir::ExfatDirEntry;

/// exFAT 文件系统实例
pub struct ExfatFs {
    pub super_block: ExfatSuperBlock,
    pub device_idx: u8,
}

impl ExfatFs {
    /// 打开 exFAT 文件系统
    pub fn open(device_idx: u8) -> Result<Self, KernelError> {
        // 读取引导扇区 (偏移 0)
        let mut boot_data = [0u8; 512];
        let result = with_device(device_idx as usize, |dev| {
            read_sectors(dev, 0, 1, &mut boot_data)
        });

        match result {
            Some(Ok(())) => {}
            _ => return Err(KernelError::IoError),
        }

        // 解析超级块
        let super_block = ExfatSuperBlock::from_bytes(&boot_data)
            .ok_or(KernelError::InvalidArgument)?;

        Ok(ExfatFs {
            super_block,
            device_idx,
        })
    }

    /// 读取目录条目
    pub fn read_dir_entries(
        &self,
        start_cluster: u32,
    ) -> Result<Vec<ExfatDirEntry>, KernelError> {
        ExfatDirEntry::from_cluster(self.device_idx, &self.super_block, start_cluster)
    }

    /// 查找路径
    pub fn lookup_path(&self, path: &str) -> Result<u32, KernelError> {
        let mut current_cluster = self.super_block.root_dir_first_cluster;

        if path == "/" || path.is_empty() {
            return Ok(current_cluster);
        }

        for component in path.trim_start_matches('/').split('/') {
            if component.is_empty() {
                continue;
            }

            let entries = self.read_dir_entries(current_cluster)?;
            let mut found = false;

            for entry in &entries {
                if entry.is_file() || (entry.file_attributes() & 0x10 != 0) {
                    // 目录属性位
                    let name = self.extract_filename(entry)?;
                    if name == component {
                        current_cluster = entry.first_cluster();
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                return Err(KernelError::NotFound);
            }
        }

        Ok(current_cluster)
    }

    /// 从流扩展条目提取文件名
    fn extract_filename(&self, file_entry: &ExfatDirEntry) -> Result<String, KernelError> {
        // 查找对应的流扩展条目
        let entries = self.read_dir_entries(file_entry.first_cluster())?;

        for entry in &entries {
            if entry.is_stream() {
                // 流扩展条目包含 UTF-16 文件名
                let name_length = entry.data[3] as usize;
                let mut name = String::new();

                for i in 0..name_length {
                    let offset = 2 + i * 2;
                    if offset + 2 <= entry.data.len() {
                        let code_unit = u16::from_le_bytes([
                            entry.data[offset],
                            entry.data[offset + 1],
                        ]);
                        if let Some(ch) = char::from_u32(code_unit as u32) {
                            name.push(ch);
                        }
                    }
                }

                return Ok(name);
            }
        }

        Err(KernelError::NotFound)
    }

    /// 读取文件内容
    pub fn read_file(
        &self,
        start_cluster: u32,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, KernelError> {
        let chain = super::fat::read_fat_chain(self.device_idx, &self.super_block, start_cluster)?;
        let cluster_size = self.super_block.bytes_per_cluster() as u64;
        let mut bytes_read = 0;
        let mut pos = offset;

        for cluster in &chain {
            let mut cluster_data = alloc::vec![0u8; cluster_size as usize];
            super::alloc::read_cluster(
                self.device_idx,
                &self.super_block,
                *cluster,
                &mut cluster_data,
            )?;

            let offset_in_cluster = (pos % cluster_size) as usize;
            let available = cluster_size as usize - offset_in_cluster;
            let to_read = (buf.len() - bytes_read).min(available);

            if to_read > 0 {
                buf[bytes_read..bytes_read + to_read]
                    .copy_from_slice(&cluster_data[offset_in_cluster..offset_in_cluster + to_read]);
                bytes_read += to_read;
                pos += to_read as u64;
            }

            if bytes_read >= buf.len() {
                break;
            }
        }

        Ok(bytes_read)
    }

    /// 写入文件内容
    pub fn write_file(
        &self,
        start_cluster: u32,
        offset: u64,
        buf: &[u8],
    ) -> Result<usize, KernelError> {
        let chain = super::fat::read_fat_chain(self.device_idx, &self.super_block, start_cluster)?;
        let cluster_size = self.super_block.bytes_per_cluster() as u64;
        let mut bytes_written = 0;
        let mut pos = offset;

        for cluster in &chain {
            let mut cluster_data = alloc::vec![0u8; cluster_size as usize];
            super::alloc::read_cluster(
                self.device_idx,
                &self.super_block,
                *cluster,
                &mut cluster_data,
            )?;

            let offset_in_cluster = (pos % cluster_size) as usize;
            let available = cluster_size as usize - offset_in_cluster;
            let to_write = (buf.len() - bytes_written).min(available);

            if to_write > 0 {
                cluster_data[offset_in_cluster..offset_in_cluster + to_write]
                    .copy_from_slice(&buf[bytes_written..bytes_written + to_write]);

                super::alloc::write_cluster(
                    self.device_idx,
                    &self.super_block,
                    *cluster,
                    &cluster_data,
                )?;

                bytes_written += to_write;
                pos += to_write as u64;
            }

            if bytes_written >= buf.len() {
                break;
            }
        }

        Ok(bytes_written)
    }
}