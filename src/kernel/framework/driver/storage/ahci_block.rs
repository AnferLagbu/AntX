//! AHCI 磁盘的 `BlockDevice` 适配器
//!
//! 将 AHCI/SATA 端口封装为标准的 `BlockDevice` trait 实现，
//! 使 `HvFS` 等多磁盘文件系统可以通过统一接口访问 AHCI 磁盘。

use crate::kernel::framework::driver::BlockDevice;

use super::AHCI_CONTROLLERS;

/// AHCI 端口的 `BlockDevice` 适配器。
///
/// 不直接持有端口引用，而是通过 (`controller_index`, `port_index`) 在
/// 全局 `AHCI_CONTROLLERS` 中查找对应的端口。
pub struct AhciBlockDevice {
    /// AHCI 控制器在 `AHCI_CONTROLLERS` 中的索引
    controller_index: usize,
    /// 端口在控制器 ports 数组中的索引
    port_index: usize,
    /// 缓存的磁盘容量 (512字节扇区数)
    total_sectors: u64,
}

impl AhciBlockDevice {
    /// 为指定的 AHCI 端口创建 `BlockDevice` 适配器。
    ///
    /// 返回 None 如果指定的端口上没有磁盘。
    pub fn new(controller_index: usize, port_index: usize) -> Option<Self> {
        // 确认端口存在且设备就绪
        let present = {
            let mut controllers = AHCI_CONTROLLERS.lock();
            if let Some(controller) = controllers.get_mut(controller_index) {
                controller
                    .get_port(port_index)
                    .map_or(false, |p| p.device_present)
            } else {
                false
            }
        };

        if !present {
            return None;
        }

        // 二分探测磁盘容量 (LBA48，需持有锁)
        let total_sectors = Self::probe_disk_size(controller_index, port_index);

        if total_sectors == 0 {
            return None;
        }

        Some(AhciBlockDevice {
            controller_index,
            port_index,
            total_sectors,
        })
    }

    /// 二分探测 AHCI 磁盘容量 (LBA48 格式，最大 2^48 扇区)
    fn probe_disk_size(ci: usize, pi: usize) -> u64 {
        let mut lo: u64 = 0;
        let mut hi: u64 = 1u64 << 24; // 2^24 sectors = 8GB

        // 先探测上界
        let mut controllers = AHCI_CONTROLLERS.lock();
        let mut buf = [0u8; 512];
        while hi < (1u64 << 32) {
            let ok = if let Some(controller) = controllers.get_mut(ci) {
                if let Some(port) = controller.get_port(pi) {
                    port.read(hi, 1, buf.as_mut_ptr()).is_ok()
                } else {
                    false
                }
            } else {
                false
            };

            if ok {
                lo = hi;
                hi = hi.saturating_mul(2);
            } else {
                break;
            }
        }

        // 二分搜索精确大小
        let mut buf = [0u8; 512];
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let ok = if let Some(controller) = controllers.get_mut(ci) {
                if let Some(port) = controller.get_port(pi) {
                    port.read(mid, 1, buf.as_mut_ptr()).is_ok()
                } else {
                    false
                }
            } else {
                false
            };

            if ok {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        if lo > 0 {
            lo
        } else {
            0
        }
    }
}

impl BlockDevice for AhciBlockDevice {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
        let mut controllers = AHCI_CONTROLLERS.lock();
        if let Some(controller) = controllers.get_mut(self.controller_index) {
            if let Some(port) = controller.get_port(self.port_index) {
                match port.read(sector, 1, buf.as_mut_ptr()) {
                    Ok(()) => 0,
                    Err(_) => -1,
                }
            } else {
                -1
            }
        } else {
            -1
        }
    }

    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
        let mut controllers = AHCI_CONTROLLERS.lock();
        if let Some(controller) = controllers.get_mut(self.controller_index) {
            if let Some(port) = controller.get_port(self.port_index) {
                match port.write(sector, 1, buf.as_ptr()) {
                    Ok(()) => 0,
                    Err(_) => -1,
                }
            } else {
                -1
            }
        } else {
            -1
        }
    }

    fn blk_is_present(&self) -> bool {
        let mut controllers = AHCI_CONTROLLERS.lock();
        if let Some(controller) = controllers.get_mut(self.controller_index) {
            controller
                .get_port(self.port_index)
                .map_or(false, |p| p.device_present)
        } else {
            false
        }
    }

    fn blk_total_sectors(&self) -> u64 {
        self.total_sectors
    }
}

// SAFETY: AhciBlockDevice 通过全局 AHCI_CONTROLLERS Mutex 访问控制器。
// 所有可变状态受锁获取保护。SMP 跨 CPU 访问安全。
unsafe impl Send for AhciBlockDevice {}
unsafe impl Sync for AhciBlockDevice {}
