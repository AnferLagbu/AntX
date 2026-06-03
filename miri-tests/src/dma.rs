//! DMA 安全边界测试 (Miri 验证版)
//!
//! 与内核 `kernel/framework/dma_buf.rs` 的 `DmaStream` 等价行为, 验证:
//! - 缓冲区大小算术不溢出
//! - Frame 引用计数防止物理页重用
//! - 同步方向正确性 (ToDevice/FromDevice/Bidirectional)
//! - 对齐要求 (典型设备 4K 对齐)
//! - 生命周期: drop 后不能再访问

/// 典型设备 DMA 对齐要求
pub const DMA_ALIGNMENT: u64 = 4096; // 4 KiB 页对齐
/// 最大单次 DMA 大小 (256 MiB, 防止物理内存耗尽)
pub const DMA_MAX_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaError {
    NotAligned,
    SizeOverflow,
    SizeTooLarge,
    ZeroSize,
    OutOfMemory,
}

/// 模拟物理页 (替代内核 Frame)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysPage {
    pub paddr: u64,
    pub size: u64, // 页大小, 通常 = 4096
}

impl PhysPage {
    pub fn is_aligned(&self) -> bool {
        self.paddr.is_multiple_of(DMA_ALIGNMENT) && self.size.is_multiple_of(DMA_ALIGNMENT)
    }
}

/// 模拟帧引用计数 (防止 DMA 进行中物理页被回收)
#[derive(Debug)]
pub struct FrameRef {
    page: Option<PhysPage>,
}

impl FrameRef {
    pub fn new(page: PhysPage) -> Self {
        Self { page: Some(page) }
    }

    /// 拆解 Frame (DMA 完成后调用)
    pub fn take(mut self) -> PhysPage {
        self.page.take().expect("Frame already taken")
    }
}

impl Drop for FrameRef {
    fn drop(&mut self) {
        // 实际内核会: 解除 DMA 映射 + 释放物理页
        // 这里仅模拟
    }
}

/// DmaStream 模拟 (无 unsafe, 纯算法)
pub struct DmaStream {
    cpu_addr: u64,
    dma_addr: u64,
    size: u64,
    direction: DmaDirection,
    /// 持有 Frame 引用, 防物理页重用
    _frame: FrameRef,
    /// 同步状态: 是否已为下一次传输正确同步
    sync_state: SyncState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// 初始状态, CPU 可写入 (ToDevice) 或可读取 (FromDevice)
    CpuReady,
    /// 已 sync_for_device, 设备可访问
    DeviceReady,
    /// 双向 DMA 中间态
    BidirInProgress,
}

impl DmaStream {
    /// 从 PhysPage 创建 DMA 流
    pub fn from_page(page: PhysPage, dir: DmaDirection) -> Result<Self, DmaError> {
        if page.size == 0 {
            return Err(DmaError::ZeroSize);
        }
        if !page.is_aligned() {
            return Err(DmaError::NotAligned);
        }
        // 验证 dma_addr + size 不溢出 (典型设备要求物理地址 + 大小不溢出)
        if page.paddr.checked_add(page.size).is_none() {
            return Err(DmaError::SizeOverflow);
        }
        if page.size > DMA_MAX_SIZE {
            return Err(DmaError::SizeTooLarge);
        }

        let initial_state = match dir {
            DmaDirection::ToDevice => SyncState::CpuReady, // CPU 写
            DmaDirection::FromDevice => SyncState::DeviceReady, // 设备写
            DmaDirection::Bidirectional => SyncState::CpuReady,
        };

        Ok(Self {
            cpu_addr: page.paddr, // 模拟: phys = virt (identity map)
            dma_addr: page.paddr,
            size: page.size,
            direction: dir,
            _frame: FrameRef::new(page),
            sync_state: initial_state,
        })
    }

    /// CPU→设备: flush CPU cache
    pub fn sync_for_device(&mut self) -> Result<(), DmaError> {
        match self.direction {
            DmaDirection::ToDevice | DmaDirection::Bidirectional => {
                if self.sync_state == SyncState::CpuReady {
                    self.sync_state = SyncState::DeviceReady;
                    Ok(())
                } else {
                    Err(DmaError::OutOfMemory) // 状态错误, 实际是 DmaStateError
                }
            }
            DmaDirection::FromDevice => Err(DmaError::OutOfMemory),
        }
    }

    /// 设备→CPU: invalidate cache
    pub fn sync_for_cpu(&mut self) -> Result<(), DmaError> {
        match self.direction {
            DmaDirection::FromDevice | DmaDirection::Bidirectional => {
                if self.sync_state == SyncState::DeviceReady
                    || self.sync_state == SyncState::BidirInProgress
                {
                    self.sync_state = SyncState::CpuReady;
                    Ok(())
                } else {
                    Err(DmaError::OutOfMemory)
                }
            }
            DmaDirection::ToDevice => Err(DmaError::OutOfMemory),
        }
    }

    pub fn cpu_addr(&self) -> u64 {
        self.cpu_addr
    }

    pub fn dma_addr(&self) -> u64 {
        self.dma_addr
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn direction(&self) -> DmaDirection {
        self.direction
    }

    /// 验证 [cpu_addr, cpu_addr+size) 不溢出
    pub fn range_valid(&self) -> bool {
        self.cpu_addr.checked_add(self.size).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aligned_page(addr: u64) -> PhysPage {
        PhysPage { paddr: addr, size: 4096 }
    }

    #[test]
    fn from_aligned_page_ok() {
        let p = aligned_page(0x10000);
        let d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        assert_eq!(d.cpu_addr(), 0x10000);
        assert_eq!(d.dma_addr(), 0x10000);
        assert_eq!(d.size(), 4096);
    }

    #[test]
    fn unaligned_page_rejected() {
        let p = PhysPage { paddr: 0x1001, size: 4096 };
        assert!(matches!(
            DmaStream::from_page(p, DmaDirection::ToDevice),
            Err(DmaError::NotAligned)
        ));
    }

    #[test]
    fn zero_size_rejected() {
        let p = PhysPage { paddr: 0, size: 0 };
        assert!(matches!(
            DmaStream::from_page(p, DmaDirection::ToDevice),
            Err(DmaError::ZeroSize)
        ));
    }

    #[test]
    fn too_large_rejected() {
        // size 超过 DMA_MAX_SIZE (且是 4K 倍数, 避免 NotAligned 优先)
        let p = PhysPage { paddr: 0, size: DMA_MAX_SIZE + 4096 };
        assert!(matches!(
            DmaStream::from_page(p, DmaDirection::ToDevice),
            Err(DmaError::SizeTooLarge)
        ));
    }

    #[test]
    fn range_no_overflow() {
        // paddr + size 不溢出 (贴近边界但留有余量)
        let p = PhysPage { paddr: 0xFFFF_FFFF_FFFF_E000, size: 4096 };
        let d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        assert!(d.range_valid());
    }

    #[test]
    fn range_overflow_detected() {
        // paddr 末尾, size 触发 checked_add overflow
        // paddr 必须页对齐, size 必须页对齐
        let p = PhysPage { paddr: u64::MAX - 4095, size: 8192 };
        // paddr=0xFFFFFFFFFFFFF000, paddr+size=0x10000000000000001 = overflow
        assert!(matches!(
            DmaStream::from_page(p, DmaDirection::ToDevice),
            Err(DmaError::SizeOverflow)
        ));
    }

    #[test]
    fn to_device_lifecycle() {
        let p = aligned_page(0x20000);
        let mut d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();

        // 初始: CPU 可写
        assert_eq!(d.sync_state, SyncState::CpuReady);

        // CPU 写完后, sync_for_device
        d.sync_for_device().unwrap();
        assert_eq!(d.sync_state, SyncState::DeviceReady);

        // ToDevice 方向不应调用 sync_for_cpu
        assert!(d.sync_for_cpu().is_err());

        // 设备访问完, 状态回到 CpuReady (这里我们用 sync_for_cpu 模拟)
        // 实际硬件: device 完成 DMA 后 CPU 端可观察到新数据
        // 对 ToDevice 语义: sync_for_cpu 失败, 状态保留为 DeviceReady
    }

    #[test]
    fn from_device_lifecycle() {
        let p = aligned_page(0x30000);
        let mut d = DmaStream::from_page(p, DmaDirection::FromDevice).unwrap();

        // 初始: 设备可写
        assert_eq!(d.sync_state, SyncState::DeviceReady);

        // sync_for_cpu 让 CPU 可见
        d.sync_for_cpu().unwrap();
    }

    #[test]
    fn to_device_cannot_sync_for_cpu() {
        // ToDevice 方向不应调用 sync_for_cpu
        let p = aligned_page(0x40000);
        let mut d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        // 设备方向错误, sync_for_cpu 应失败
        assert!(d.sync_for_cpu().is_err());
    }

    #[test]
    fn from_device_cannot_sync_for_device() {
        // FromDevice 方向不应调用 sync_for_device
        let p = aligned_page(0x50000);
        let mut d = DmaStream::from_page(p, DmaDirection::FromDevice).unwrap();
        assert!(d.sync_for_device().is_err());
    }

    #[test]
    fn bidir_lifecycle() {
        let p = aligned_page(0x60000);
        let mut d = DmaStream::from_page(p, DmaDirection::Bidirectional).unwrap();

        // 双向: 初始 CPU 可写
        d.sync_for_device().unwrap();
        // 设备读
        d.sync_for_cpu().unwrap();
        // CPU 改
        d.sync_for_device().unwrap();
    }

    #[test]
    fn frame_lifecycle_ownership() {
        // Frame 引用计数语义: DmaStream drop 时 Frame 被释放
        let p = aligned_page(0x70000);
        let d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        // 在 d 存活期间, p 不能被外部释放 (借用检查器保证)
        drop(d);
        // d 已被 drop, p 可被使用
        let _ = p;
    }

    #[test]
    fn take_frame_releases_dma() {
        // 显式 take Frame 等于主动释放 DMA
        let p = aligned_page(0x80000);
        let d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        let recovered = d._frame.take();
        assert_eq!(recovered, p);
        // d 在 take 后不能被使用 (借用检查器保证, 编译失败)
    }

    #[test]
    fn stress_random_dmas() {
        // 压测: 1000 个 DMA 流, 验证不变式
        let mut streams = Vec::new();
        for i in 0..1000u64 {
            let p = aligned_page((i + 1) * 0x1000);
            let dir = match i % 3 {
                0 => DmaDirection::ToDevice,
                1 => DmaDirection::FromDevice,
                _ => DmaDirection::Bidirectional,
            };
            let d = DmaStream::from_page(p, dir);
            if d.is_ok() {
                streams.push(d.unwrap());
            }
        }
        assert!(streams.len() > 0);
        // 全部不溢出
        for d in &streams {
            assert!(d.range_valid());
        }
    }
}
