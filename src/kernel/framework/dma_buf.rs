//! DmaStream / DmaCoherent — 安全 DMA 映射 (TCB)
//!
//! 封装 DMA 缓冲区的分配/映射/释放, 确保:
//! - 物理地址和虚拟地址的一致性 (x86 自动, aarch64 显式 cache flush)
//! - DMA 缓冲区生命周期管理 (解除映射前无 CPU 写入)
//!
//! ## 与 Asterinas OSTD `DmaStream` / `DmaCoherent` 的关系
//!
//! 等价于 OSTD 的 `DmaStream` + `DmaCoherent`。
//!
//! ## SAFETY 不变量
//!
//! - `DmaStream::sync()` 必须在 CPU 写入后、设备读取前调用 (非一致性架构)。
//! - `DmaCoherent` 内存在整个生命周期内对设备和 CPU 都可见。
//! - DMA 缓冲区持有的 Frame 引用计数防止物理页被重用。

use core::fmt;
use core::ptr::NonNull;

use crate::kernel::mm::PhysAddr;

use super::frame::Frame;

/// DMA 传输方向
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
}

/// 流式 DMA 缓冲区 (非一致性)。
///
/// 映射生命周期: alloc → sync_for_device → DMA → sync_for_cpu → free。
/// x86_64 上 sync 为空操作 (硬件保证一致性)。
pub struct DmaStream {
    cpu_addr: NonNull<u8>,
    dma_addr: PhysAddr,
    size: usize,
    _frame: Frame, // 持有 Frame 引用, 防物理页重用
}

impl DmaStream {
    /// 创建流式 DMA 映射。
    ///
    /// 从 Frame 分配并 map 到 DMA 可访问的内存。
    /// x86_64: 直接使用 Frame 的物理地址 (硬件一致性)。
    /// aarch64: 可能需要 iommu map 或 cache flush (Phase 2 实现)。
    pub fn from_frame(frame: Frame, dir: DmaDirection) -> Option<Self> {
        let size = frame.size();
        let phys = frame.phys();
        let cpu_ptr = NonNull::new(frame.as_virt_ptr())?;
        let _ = dir; // 用于 sync 方向判定
        Some(Self {
            cpu_addr: cpu_ptr,
            dma_addr: phys,
            size,
            _frame: frame,
        })
    }

    /// CPU 可访问的虚拟地址
    #[inline(always)]
    pub fn cpu_addr(&self) -> NonNull<u8> {
        self.cpu_addr
    }

    /// 设备可访问的物理地址
    #[inline(always)]
    pub fn dma_addr(&self) -> PhysAddr {
        self.dma_addr
    }

    /// 缓冲区大小
    #[inline(always)]
    pub fn size(&self) -> usize {
        self.size
    }

    /// CPU→设备: flush CPU cache, 确保设备看到最新数据。
    ///
    /// x86_64: 空操作 (硬件保证一致性)。
    /// aarch64: 需要 DC CVAU (Data Cache Clean to Point of Unification)。
    pub fn sync_for_device(&self) {
        #[cfg(target_arch = "aarch64")]
        {
            // SAFETY: ARCH-specific cache maintenance for DMA.
            // This requires aarch64 inline asm: DC CVAU on the buffer range.
            // TODO: Phase 2 — integrate with aarch64 cache ops.
        }
    }

    /// 设备→CPU: invalidate CPU cache, 确保 CPU 看到设备写入。
    ///
    /// x86_64: 空操作。
    /// aarch64: 需要 DC IVAU (Data Cache Invalidate to Point of Unification)。
    pub fn sync_for_cpu(&self) {
        #[cfg(target_arch = "aarch64")]
        {
            // SAFETY: ARCH-specific cache maintenance.
            // TODO: Phase 2 — integrate with aarch64 cache ops.
        }
    }
}

impl fmt::Display for DmaStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DmaStream(cpu=0x{:x}, dma=0x{:x}, size={})",
            self.cpu_addr.as_ptr() as usize,
            self.dma_addr.as_u64(),
            self.size,
        )
    }
}

// SAFETY: DmaStream 持有独立的 Frame 和 DMA 映射, Send 安全。
unsafe impl Send for DmaStream {}
unsafe impl Sync for DmaStream {}
