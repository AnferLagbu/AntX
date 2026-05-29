//! NVMe 磁盘的 BlockDevice 适配器
//!
//! 将 NVMe 命名空间封装为标准的 BlockDevice trait 实现，
//! 使 HvFS 等多磁盘文件系统可以通过统一接口访问 NVMe 磁盘。

use crate::kernel::driver::block::BlockDevice;

use super::NVME_CONTROLLERS;

/// NVMe 命名空间的 BlockDevice 适配器。
///
/// 不直接持有控制器引用，而是通过 (controller_index, namespace_id) 在
/// 全局 NVME_CONTROLLERS 中查找对应的控制器和命名空间。
pub struct NvmeBlockDevice {
    /// NVMe 控制器在 NVME_CONTROLLERS 中的索引
    controller_index: usize,
    /// 命名空间 ID (1-based)
    namespace_id: u32,
    /// 缓存的磁盘容量 (512字节扇区数)
    total_sectors: u64,
}

impl NvmeBlockDevice {
    /// 为指定的 NVMe 命名空间创建 BlockDevice 适配器。
    ///
    /// 返回 None 如果命名空间大小为 0（非块设备命名空间）。
    pub fn new(controller_index: usize, namespace_id: u32) -> Option<Self> {
        let controllers = NVME_CONTROLLERS.lock();
        let controller = controllers.get(controller_index)?;

        let ns_count = controller.namespace_count();
        if namespace_id < 1 || namespace_id > ns_count {
            return None;
        }

        let total_sectors = controller.namespace_size();

        if total_sectors == 0 {
            return None;
        }

        Some(NvmeBlockDevice {
            controller_index,
            namespace_id,
            total_sectors,
        })
    }
}

impl BlockDevice for NvmeBlockDevice {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
        let mut controllers = NVME_CONTROLLERS.lock();
        if let Some(controller) = controllers.get_mut(self.controller_index) {
            match controller.read(self.namespace_id, sector, 1, buf.as_mut_ptr()) {
                Ok(()) => 0,
                Err(_) => -1,
            }
        } else {
            -1
        }
    }

    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
        let mut controllers = NVME_CONTROLLERS.lock();
        if let Some(controller) = controllers.get_mut(self.controller_index) {
            match controller.write(self.namespace_id, sector, 1, buf.as_ptr()) {
                Ok(()) => 0,
                Err(_) => -1,
            }
        } else {
            -1
        }
    }

    fn blk_is_present(&self) -> bool {
        let controllers = NVME_CONTROLLERS.lock();
        if let Some(controller) = controllers.get(self.controller_index) {
            let ns_count = controller.namespace_count();
            if self.namespace_id >= 1 && self.namespace_id <= ns_count {
                return controller.namespace_size() > 0;
            }
        }
        false
    }

    fn blk_total_sectors(&self) -> u64 {
        self.total_sectors
    }
}

// SAFETY: NvmeBlockDevice accesses controllers through the global
// NVME_CONTROLLERS Mutex. All mutable state is protected by lock
// acquisition. Safe for SMP cross-CPU access.
unsafe impl Send for NvmeBlockDevice {}
unsafe impl Sync for NvmeBlockDevice {}