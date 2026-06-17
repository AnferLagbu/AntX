use crate::kernel::framework::chitin::{devtree_children, devtree_get_node, devtree_walk, register_block_device};
use crate::kernel::framework::chitin::{
    chitin_blk_is_present, chitin_blk_read, chitin_blk_total_sectors, chitin_blk_write,
    chitin_find_by_id, chitin_find_by_name, ChitinProto, BlockDevice,
};
use crate::kernel::framework::fs::KernelError;
use crate::klog_info;
use crate::klog_warn;
use core::sync::atomic::{AtomicU32, Ordering};

const MAX_COMPOSITE_CHILDREN: usize = 8;
const MIN_STRIPE_SIZE: u64 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeType {
    Raid0,
    Raid1,
}

impl CompositeType {
    pub fn compatible_str(&self) -> &'static str {
        match self {
            CompositeType::Raid0 => "qx,raid0",
            CompositeType::Raid1 => "qx,raid1",
        }
    }

    pub fn from_compatible(compat: &str) -> Option<Self> {
        match compat {
            "qx,raid0" => Some(CompositeType::Raid0),
            "qx,raid1" | "qx,mirror" => Some(CompositeType::Raid1),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            CompositeType::Raid0 => "RAID0",
            CompositeType::Raid1 => "RAID1",
        }
    }
}

pub struct CompositeBlockDevice {
    device_type: CompositeType,
    child_drives: [u8; MAX_COMPOSITE_CHILDREN],
    child_count: u8,
    stripe_sectors: u64,
    total_sectors: u64,
    read_round_robin: AtomicU32,
}

// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe impl Send for CompositeBlockDevice {}
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe impl Sync for CompositeBlockDevice {}

impl Clone for CompositeBlockDevice {
    fn clone(&self) -> Self {
        Self {
            device_type: self.device_type,
            child_drives: self.child_drives,
            child_count: self.child_count,
            stripe_sectors: self.stripe_sectors,
            total_sectors: self.total_sectors,
            read_round_robin: AtomicU32::new(self.read_round_robin.load(Ordering::Relaxed)),
        }
    }
}

impl CompositeBlockDevice {
    pub fn new(device_type: CompositeType, child_drives: &[u8], stripe_size: u64) -> Option<Self> {
        if child_drives.is_empty() || child_drives.len() > MAX_COMPOSITE_CHILDREN {
            return None;
        }

        let mut drives: [u8; MAX_COMPOSITE_CHILDREN] = [0; MAX_COMPOSITE_CHILDREN];
        drives[..child_drives.len()].copy_from_slice(child_drives);

        let stripe_size = stripe_size.max(MIN_STRIPE_SIZE);
        let stripe_sectors = stripe_size.div_ceil(512);

        let total: u64;
        let count = child_drives.len() as u64;

        match device_type {
            CompositeType::Raid0 => {
                let mut min_sectors = u64::MAX;
                for &drive in child_drives {
                    let sectors = chitin_blk_total_sectors(drive);
                    if sectors < stripe_sectors {
                        return None;
                    }
                    let aligned = (sectors / stripe_sectors) * stripe_sectors;
                    if aligned < min_sectors {
                        min_sectors = aligned;
                    }
                }
                total = min_sectors.checked_mul(count)?;
            }
            CompositeType::Raid1 => {
                let mut min_sectors = u64::MAX;
                for &drive in child_drives {
                    let sectors = chitin_blk_total_sectors(drive);
                    if sectors == 0 {
                        return None;
                    }
                    if sectors < min_sectors {
                        min_sectors = sectors;
                    }
                }
                total = min_sectors;
            }
        }

        Some(Self {
            device_type,
            child_drives: drives,
            child_count: child_drives.len() as u8,
            stripe_sectors,
            total_sectors: total,
            read_round_robin: AtomicU32::new(0),
        })
    }

    #[inline]
    fn map_raid0_sector(&self, logical: u64) -> Option<(u8, u64)> {
        let stripe = logical / self.stripe_sectors;
        let child_idx = (stripe % self.child_count as u64) as u8;
        let stripe_row = stripe / self.child_count as u64;
        let offset = logical % self.stripe_sectors;
        let physical = stripe_row * self.stripe_sectors + offset;
        Some((child_idx, physical))
    }
}

impl BlockDevice for CompositeBlockDevice {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
        let num_sectors = (buf.len() / 512) as u64;
        if num_sectors == 0 {
            return KernelError::InvalidArgument.as_i32();
        }
        if sector + num_sectors > self.total_sectors {
            return KernelError::InvalidArgument.as_i32();
        }

        match self.device_type {
            CompositeType::Raid0 => {
                for s in 0..num_sectors {
                    let chunk = &mut buf[(s * 512) as usize..((s + 1) * 512) as usize];
                    if let Some((drive_idx, phys_sector)) = self.map_raid0_sector(sector + s) {
                        let ret = chitin_blk_read(drive_idx, phys_sector, chunk);
                        if ret != 0 {
                            return ret;
                        }
                    } else {
                        return KernelError::InvalidArgument.as_i32();
                    }
                }
                0
            }
            CompositeType::Raid1 => {
                for s in 0..num_sectors {
                    let chunk = &mut buf[(s * 512) as usize..((s + 1) * 512) as usize];
                    let sector_off = sector + s;

                    let start = self.read_round_robin.fetch_add(1, Ordering::Relaxed) as usize
                        % self.child_count as usize;

                    let mut ok = false;
                    for offset in 0..self.child_count as usize {
                        let idx = (start + offset) % self.child_count as usize;
                        let ret = chitin_blk_read(self.child_drives[idx], sector_off, chunk);
                        if ret == 0 {
                            ok = true;
                            break;
                        }
                    }
                    if !ok {
                        return KernelError::IoError.as_i32();
                    }
                }
                0
            }
        }
    }

    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
        let num_sectors = (buf.len() / 512) as u64;
        if num_sectors == 0 {
            return KernelError::InvalidArgument.as_i32();
        }
        if sector + num_sectors > self.total_sectors {
            return KernelError::InvalidArgument.as_i32();
        }

        match self.device_type {
            CompositeType::Raid0 => {
                for s in 0..num_sectors {
                    let chunk = &buf[(s * 512) as usize..((s + 1) * 512) as usize];
                    if let Some((drive_idx, phys_sector)) = self.map_raid0_sector(sector + s) {
                        let ret = chitin_blk_write(drive_idx, phys_sector, chunk);
                        if ret != 0 {
                            return ret;
                        }
                    } else {
                        return KernelError::InvalidArgument.as_i32();
                    }
                }
                0
            }
            CompositeType::Raid1 => {
                for s in 0..num_sectors {
                    let chunk = &buf[(s * 512) as usize..((s + 1) * 512) as usize];
                    let sector_off = sector + s;

                    for i in 0..self.child_count as usize {
                        let ret = chitin_blk_write(self.child_drives[i], sector_off, chunk);
                        if ret != 0 {
                            return ret;
                        }
                    }
                }
                0
            }
        }
    }

    fn blk_is_present(&self) -> bool {
        match self.device_type {
            CompositeType::Raid0 => {
                for i in 0..self.child_count as usize {
                    if !chitin_blk_is_present(self.child_drives[i]) {
                        return false;
                    }
                }
                true
            }
            CompositeType::Raid1 => {
                for i in 0..self.child_count as usize {
                    if chitin_blk_is_present(self.child_drives[i]) {
                        return true;
                    }
                }
                false
            }
        }
    }

    fn blk_total_sectors(&self) -> u64 {
        self.total_sectors
    }
}

pub fn devtree_probe_composites() -> usize {
    let composite_compatibles: &[&str] = &["qx,raid0", "qx,raid1"];

    let mut candidate_nodes: alloc::vec::Vec<(u32, CompositeType)> = alloc::vec::Vec::new();

    devtree_walk(|node| {
        for &compat in composite_compatibles {
            if node.matches_compatible(compat) {
                if let Some(ct) = CompositeType::from_compatible(compat) {
                    candidate_nodes.push((node.id, ct));
                }
                break;
            }
        }
    });

    if candidate_nodes.is_empty() {
        klog_info!(Driver, "Chitin: no composite device tree nodes found");
        return 0;
    }

    let mut created = 0usize;

    for (node_id, composite_type) in candidate_nodes {
        let children = devtree_children(node_id);

        if children.is_empty() {
            klog_warn!(
                Driver,
                "Chitin: composite node {} has no children, skipping",
                node_id
            );
            continue;
        }

        let mut child_drives: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

        for &child_id in &children {
            let child_node = match devtree_get_node(child_id) {
                Some(n) => n,
                None => {
                    klog_warn!(
                        Driver,
                        "Chitin: composite child node {} not found, skipping",
                        child_id
                    );
                    continue;
                }
            };

            if child_node.proto != ChitinProto::Block {
                klog_warn!(
                    Driver,
                    "Chitin: composite child '{}' is not a block device, skipping",
                    child_node.name
                );
                continue;
            }

            let idx: usize = {
                if let Some(dev_id) = child_node.device_id {
                    match chitin_find_by_id(dev_id) {
                        Some(i) => i,
                        None => {
                            klog_warn!(
                                Driver,
                                "Chitin: child '{}' device_id={} not in registry",
                                child_node.name,
                                dev_id
                            );
                            continue;
                        }
                    }
                } else {
                    match chitin_find_by_name(child_node.name) {
                        Some(i) => i,
                        None => {
                            klog_warn!(
                                Driver,
                                "Chitin: child '{}' not found in Chitin registry",
                                child_node.name
                            );
                            continue;
                        }
                    }
                }
            };

            if idx > u8::MAX as usize {
                klog_warn!(
                    Driver,
                    "Chitin: child '{}' index {} exceeds u8 range, skipping",
                    child_node.name,
                    idx
                );
                continue;
            }

            child_drives.push(idx as u8);
        }

        if child_drives.is_empty() {
            klog_warn!(
                Driver,
                "Chitin: composite node {} has no valid block children",
                node_id
            );
            continue;
        }

        let stripe_size: u64 = {
            let node = match devtree_get_node(node_id) {
                Some(n) => n,
                None => continue,
            };
            node.get_prop("stripe_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(65536)
        };

        let orig = stripe_size;
        let stripe_size = stripe_size.max(MIN_STRIPE_SIZE);
        if orig < MIN_STRIPE_SIZE {
            klog_info!(
                Driver,
                "Chitin: stripe_size {} < 512, clamped to {}",
                orig,
                stripe_size
            );
        }

        let dev_name = alloc::format!(
            "{}-{}",
            composite_type.display_name().to_lowercase(),
            created
        );
        // Chitin register API 要求设备名为 `&'static str`.
        // 这是有界分配 —— 初始化时每个复合设备一份.
        let name_leaked: &'static str = dev_name.leak();

        let composite = match CompositeBlockDevice::new(composite_type, &child_drives, stripe_size)
        {
            Some(c) => c,
            None => {
                klog_warn!(
                    Driver,
                    "Chitin: failed to create composite device '{}'",
                    name_leaked
                );
                continue;
            }
        };

        let total_mb = composite.total_sectors / 2048;
        register_block_device(name_leaked, composite, None);

        klog_info!(
            Driver,
            "Chitin: {} virtual device '{}' registered ({} children, {} MB)",
            composite_type.display_name(),
            name_leaked,
            child_drives.len(),
            total_mb
        );

        created += 1;
    }

    created
}

#[no_mangle]
pub fn composite_probe() -> u32 {
    devtree_probe_composites() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raid0_sector_mapping() {
        let dev = CompositeBlockDevice {
            device_type: CompositeType::Raid0,
            child_drives: [0, 1, 0, 0, 0, 0, 0, 0],
            child_count: 2,
            stripe_sectors: 8,
            total_sectors: 16,
            read_round_robin: AtomicU32::new(0),
        };

        assert_eq!(dev.map_raid0_sector(0), Some((0, 0)));
        assert_eq!(dev.map_raid0_sector(7), Some((0, 7)));
        assert_eq!(dev.map_raid0_sector(8), Some((1, 0)));
        assert_eq!(dev.map_raid0_sector(15), Some((1, 7)));
    }

    #[test]
    fn test_raid0_sector_beyond_total() {
        let dev = CompositeBlockDevice {
            device_type: CompositeType::Raid0,
            child_drives: [0, 1, 0, 0, 0, 0, 0, 0],
            child_count: 2,
            stripe_sectors: 8,
            total_sectors: 16,
            read_round_robin: AtomicU32::new(0),
        };

        let mut buf = [0u8; 512];
        assert_eq!(dev.clone().blk_read(16, &mut buf), -1);
    }
}
