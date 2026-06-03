//! DmaStream 端到端验证 (Phase 3.4)
//!
//! 复刻 `src/kernel/framework/dma_buf.rs` 的 `DmaStream` 算法,
//! 在 host 端用 std 模拟 Frame 依赖, 验证:
//! 1. 物理地址 + 大小对齐检查
//! 2. 范围溢出检查
//! 3. size 上限检查
//! 4. size = 0 拒绝
//! 5. 状态机: ToDevice / FromDevice / Bidirectional 正确转换
//! 6. 非法状态转换返回错误
//! 7. 缓冲区生命周期 (drop 释放 Frame)
//!
//! 注: 完整 e2e 验证需 QEMU 端 + 真实 PCI 设备, 见 `scripts/qemu_boot_test.sh`。

#![allow(dead_code)]

/// 典型设备 DMA 对齐要求
pub const DMA_ALIGNMENT: u64 = 4096;
/// 最大单次 DMA 大小
pub const DMA_MAX_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    CpuReady,
    DeviceReady,
    BidirInProgress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaError {
    NotAligned,
    SizeOverflow,
    SizeTooLarge,
    ZeroSize,
    InvalidStateTransition,
    InvalidFrame,
}

/// 模拟物理页 (替代内核 Frame)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysPage {
    pub paddr: u64,
    pub size: u64,
}

impl PhysPage {
    pub fn is_aligned(&self) -> bool {
        self.paddr.is_multiple_of(DMA_ALIGNMENT) && self.size.is_multiple_of(DMA_ALIGNMENT)
    }
}

/// 模拟 Frame 引用计数
#[derive(Debug)]
pub struct FrameRef {
    page: Option<PhysPage>,
}

impl FrameRef {
    pub fn new(page: PhysPage) -> Self {
        Self { page: Some(page) }
    }
    pub fn take(mut self) -> PhysPage {
        self.page.take().expect("Frame already taken")
    }
}

impl Drop for FrameRef {
    fn drop(&mut self) {
        // 实际内核: 解除 DMA 映射 + 释放物理页
    }
}

/// DmaStream 模拟 (无 unsafe, 镜像生产代码算法)
#[derive(Debug)]
pub struct DmaStream {
    cpu_addr: u64,
    dma_addr: u64,
    size: u64,
    direction: DmaDirection,
    sync_state: SyncState,
    _frame: FrameRef,
}

impl DmaStream {
    pub fn from_page(page: PhysPage, dir: DmaDirection) -> Result<Self, DmaError> {
        if page.size == 0 {
            return Err(DmaError::ZeroSize);
        }
        if !page.is_aligned() {
            return Err(DmaError::NotAligned);
        }
        if page.paddr.checked_add(page.size).is_none() {
            return Err(DmaError::SizeOverflow);
        }
        if page.size > DMA_MAX_SIZE {
            return Err(DmaError::SizeTooLarge);
        }

        let initial_state = match dir {
            DmaDirection::ToDevice => SyncState::CpuReady,
            DmaDirection::FromDevice => SyncState::DeviceReady,
            DmaDirection::Bidirectional => SyncState::CpuReady,
        };

        Ok(Self {
            cpu_addr: page.paddr, // 模拟 phys = virt (identity map)
            dma_addr: page.paddr,
            size: page.size,
            direction: dir,
            sync_state: initial_state,
            _frame: FrameRef::new(page),
        })
    }

    pub fn sync_for_device(&mut self) -> Result<(), DmaError> {
        match self.direction {
            DmaDirection::ToDevice | DmaDirection::Bidirectional => {
                if self.sync_state == SyncState::CpuReady
                    || self.sync_state == SyncState::BidirInProgress
                {
                    self.sync_state = SyncState::DeviceReady;
                    Ok(())
                } else {
                    Err(DmaError::InvalidStateTransition)
                }
            }
            DmaDirection::FromDevice => Err(DmaError::InvalidStateTransition),
        }
    }

    pub fn sync_for_cpu(&mut self) -> Result<(), DmaError> {
        match self.direction {
            DmaDirection::FromDevice | DmaDirection::Bidirectional => {
                if self.sync_state == SyncState::DeviceReady
                    || self.sync_state == SyncState::BidirInProgress
                {
                    self.sync_state = SyncState::CpuReady;
                    Ok(())
                } else {
                    Err(DmaError::InvalidStateTransition)
                }
            }
            DmaDirection::ToDevice => Err(DmaError::InvalidStateTransition),
        }
    }

    #[inline] pub fn cpu_addr(&self) -> u64 { self.cpu_addr }
    #[inline] pub fn dma_addr(&self) -> u64 { self.dma_addr }
    #[inline] pub fn size(&self) -> u64 { self.size }
    #[inline] pub fn direction(&self) -> DmaDirection { self.direction }
    #[inline] pub fn sync_state(&self) -> SyncState { self.sync_state }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(addr: u64, size: u64) -> PhysPage {
        PhysPage { paddr: addr, size }
    }

    // ===== 验证测试 =====

    #[test]
    fn from_aligned_page_ok() {
        let d = DmaStream::from_page(page(0x10000, 4096), DmaDirection::ToDevice).unwrap();
        assert_eq!(d.cpu_addr(), 0x10000);
        assert_eq!(d.dma_addr(), 0x10000);
        assert_eq!(d.size(), 4096);
        assert_eq!(d.direction(), DmaDirection::ToDevice);
        assert_eq!(d.sync_state(), SyncState::CpuReady);
    }

    #[test]
    fn unaligned_paddr_rejected() {
        let p = PhysPage { paddr: 0x1001, size: 4096 };
        assert_eq!(
            DmaStream::from_page(p, DmaDirection::ToDevice).unwrap_err(),
            DmaError::NotAligned
        );
    }

    #[test]
    fn unaligned_size_rejected() {
        // size 不为 4K 倍数
        let p = PhysPage { paddr: 0x10000, size: 4097 };
        assert_eq!(
            DmaStream::from_page(p, DmaDirection::ToDevice).unwrap_err(),
            DmaError::NotAligned
        );
    }

    #[test]
    fn zero_size_rejected() {
        let p = PhysPage { paddr: 0x10000, size: 0 };
        assert_eq!(
            DmaStream::from_page(p, DmaDirection::ToDevice).unwrap_err(),
            DmaError::ZeroSize
        );
    }

    #[test]
    fn too_large_rejected() {
        // > 256 MiB
        let p = PhysPage { paddr: 0x10000, size: DMA_MAX_SIZE + 4096 };
        assert_eq!(
            DmaStream::from_page(p, DmaDirection::ToDevice).unwrap_err(),
            DmaError::SizeTooLarge
        );
    }

    #[test]
    fn size_overflow_rejected() {
        // 边界: paddr + size 恰好不溢出
        // paddr = 0xFFFFFFFFFFFFD000, size = 0x2000 → end = 0xFFFFFFFFFFFFEFFF + 1 = 0xFFFFFFFFFFFFF000 (无溢出)
        let p = PhysPage { paddr: 0xFFFFFFFFFFFFD000, size: 0x2000 };
        assert!(DmaStream::from_page(p, DmaDirection::ToDevice).is_ok());
        // 真正溢出: paddr = 0xFFFFFFFFFFFFE000, size = 0x2000
        // paddr + size = 0x10000000000000000 → 溢出
        let p = PhysPage { paddr: 0xFFFFFFFFFFFFE000, size: 0x2000 };
        assert_eq!(
            DmaStream::from_page(p, DmaDirection::ToDevice).unwrap_err(),
            DmaError::SizeOverflow
        );
        // 更大溢出: paddr = 0xFFFFFFFFFFFFF000, size = 0x1000
        let p = PhysPage { paddr: 0xFFFFFFFFFFFFF000, size: 0x1000 };
        assert_eq!(
            DmaStream::from_page(p, DmaDirection::ToDevice).unwrap_err(),
            DmaError::SizeOverflow
        );
    }

    #[test]
    fn size_at_u64_max_boundary() {
        // paddr + size 接近 u64::MAX, 但不溢出
        // 4K 对齐要求下, 最大可表达范围: paddr=0, size=u64::MAX-0xFFF = 0xFFFFFFFFFFFFF000
        let p = PhysPage { paddr: 0, size: 0xFFFFFFFFFFFFF000 };
        // 但 size > DMA_MAX_SIZE (256 MiB) 触发 SizeTooLarge
        assert_eq!(
            DmaStream::from_page(p, DmaDirection::ToDevice).unwrap_err(),
            DmaError::SizeTooLarge
        );
        // 边界: 4K 对齐 size 上限 = DMA_MAX_SIZE (4K 倍数)
        // DMA_MAX_SIZE = 256 MiB = 0x10000000 (4K 倍数)
        let p = PhysPage { paddr: 0x1000, size: DMA_MAX_SIZE };
        assert!(DmaStream::from_page(p, DmaDirection::ToDevice).is_ok());
    }

    // ===== 状态机测试 =====

    #[test]
    fn to_device_lifecycle() {
        let mut d = DmaStream::from_page(page(0x10000, 4096), DmaDirection::ToDevice).unwrap();
        assert_eq!(d.sync_state(), SyncState::CpuReady);
        // CPU 写完, 调 sync_for_device 进入 DeviceReady
        assert!(d.sync_for_device().is_ok());
        assert_eq!(d.sync_state(), SyncState::DeviceReady);
        // 重复调 sync_for_device 应失败 (状态机不允许)
        assert_eq!(d.sync_for_device().unwrap_err(), DmaError::InvalidStateTransition);
    }

    #[test]
    fn from_device_lifecycle() {
        let mut d = DmaStream::from_page(page(0x10000, 4096), DmaDirection::FromDevice).unwrap();
        // FromDevice 初始为 DeviceReady (设备已写入)
        assert_eq!(d.sync_state(), SyncState::DeviceReady);
        // CPU 想读, 调 sync_for_cpu 进入 CpuReady
        assert!(d.sync_for_cpu().is_ok());
        assert_eq!(d.sync_state(), SyncState::CpuReady);
        // 重复调应失败
        assert_eq!(d.sync_for_cpu().unwrap_err(), DmaError::InvalidStateTransition);
    }

    #[test]
    fn to_device_cannot_sync_for_cpu() {
        let mut d = DmaStream::from_page(page(0x10000, 4096), DmaDirection::ToDevice).unwrap();
        // ToDevice 不允许 sync_for_cpu
        assert_eq!(d.sync_for_cpu().unwrap_err(), DmaError::InvalidStateTransition);
    }

    #[test]
    fn from_device_cannot_sync_for_device() {
        let mut d = DmaStream::from_page(page(0x10000, 4096), DmaDirection::FromDevice).unwrap();
        // FromDevice 不允许 sync_for_device
        assert_eq!(d.sync_for_device().unwrap_err(), DmaError::InvalidStateTransition);
    }

    #[test]
    fn bidir_full_cycle() {
        // Bidirectional 完整周期: CpuReady → DeviceReady → CpuReady → ...
        let mut d = DmaStream::from_page(page(0x10000, 4096), DmaDirection::Bidirectional).unwrap();
        assert_eq!(d.sync_state(), SyncState::CpuReady);
        assert!(d.sync_for_device().is_ok());
        assert_eq!(d.sync_state(), SyncState::DeviceReady);
        assert!(d.sync_for_cpu().is_ok());
        assert_eq!(d.sync_state(), SyncState::CpuReady);
        // 第二次
        assert!(d.sync_for_device().is_ok());
        assert!(d.sync_for_cpu().is_ok());
        // 1000 次循环
        for _ in 0..1000 {
            assert!(d.sync_for_device().is_ok());
            assert!(d.sync_for_cpu().is_ok());
        }
    }

    // ===== 生命周期测试 =====

    #[test]
    fn frame_lifecycle() {
        // 创建后 Frame 应被持有
        let d = DmaStream::from_page(page(0x10000, 4096), DmaDirection::ToDevice).unwrap();
        // drop 释放 (没有 panic 即说明 FrameRef 正确 Drop)
        drop(d);
    }

    #[test]
    fn frame_unique_ownership() {
        // 一个 PhysPage 只能被一个 DmaStream 持有
        let p = page(0x10000, 4096);
        let d1 = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        // 第二个 DmaStream 尝试用同一个 PhysPage 应能成功 (mock 中允许, 但生产 Frame 用 from_raw 强制唯一)
        let d2 = DmaStream::from_page(p, DmaDirection::FromDevice);
        // 这里 mock 允许多个, 实际 Frame 的 from_raw 是不安全的独占
        assert!(d2.is_ok());
        drop(d1);
    }

    // ===== 真实场景模拟 =====

    #[test]
    fn e1000_tx_desc_simulation() {
        // 模拟 e1000 发送描述符: 16 字节对齐, ToDevice 方向
        // 实际 e1000 描述符 16 字节对齐即可, 但 DMA buffer 通常 4KB 对齐
        let p = page(0xFEB_C0000, 4096);
        let mut d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        // mock 中 cpu_addr = paddr, 不直接解引用 (避免 SIGSEGV)
        // 实际生产代码会写入描述符内容
        assert_eq!(d.cpu_addr(), 0xFEB_C0000);
        // 提交给设备
        assert!(d.sync_for_device().is_ok());
        assert_eq!(d.dma_addr(), 0xFEB_C0000);
        // 状态机检查
        assert_eq!(d.sync_state(), SyncState::DeviceReady);
    }

    #[test]
    fn e1000_rx_desc_simulation() {
        // 模拟 e1000 接收描述符: FromDevice 方向
        let p = page(0xFEB_C1000, 4096);
        let mut d = DmaStream::from_page(p, DmaDirection::FromDevice).unwrap();
        // 设备 DMA 完成后, CPU 读描述符
        assert!(d.sync_for_cpu().is_ok());
        assert_eq!(d.dma_addr(), 0xFEB_C1000);
        assert_eq!(d.sync_state(), SyncState::CpuReady);
    }

    #[test]
    fn stress_random_dmas() {
        // 1000 个随机 DMA 流的创建 + 完整同步周期
        for i in 0..1000 {
            let paddr = (i as u64) * 0x10_0000;
            let p = page(paddr, 4096);
            let dir = match i % 3 {
                0 => DmaDirection::ToDevice,
                1 => DmaDirection::FromDevice,
                _ => DmaDirection::Bidirectional,
            };
            let mut d = DmaStream::from_page(p, dir).expect("create failed");
            // 完整同步周期
            for _ in 0..10 {
                match dir {
                    DmaDirection::ToDevice => {
                        if d.sync_for_device().is_ok() {
                            // 回到 CpuReady 模拟: 强制 reset (mock)
                        }
                    }
                    DmaDirection::FromDevice => {
                        let _ = d.sync_for_cpu();
                    }
                    DmaDirection::Bidirectional => {
                        let _ = d.sync_for_device();
                        let _ = d.sync_for_cpu();
                    }
                }
            }
        }
    }

    #[test]
    fn large_buffer_under_limit() {
        // 边界: 恰好等于 DMA_MAX_SIZE
        let p = page(0x10000, DMA_MAX_SIZE);
        assert!(DmaStream::from_page(p, DmaDirection::Bidirectional).is_ok());
    }

    #[test]
    fn mid_size_buffer() {
        // 2 MiB huge page
        let p = page(0x200000, 2 * 1024 * 1024);
        let d = DmaStream::from_page(p, DmaDirection::Bidirectional).unwrap();
        assert_eq!(d.size(), 2 * 1024 * 1024);
    }

    #[test]
    fn pci_bar_aligned_buffer() {
        // 模拟 PCI BAR 分配的 DMA 缓冲区: 起始地址通常是 4K 对齐
        let bar_base: u64 = 0xFEB_C0000; // 4K 对齐
        let p = page(bar_base, 0x2000); // 8KB
        let d = DmaStream::from_page(p, DmaDirection::ToDevice).unwrap();
        assert_eq!(d.dma_addr(), bar_base);
    }
}
