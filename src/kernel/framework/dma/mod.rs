//! DMA Engine 子系统 (Rust 重写)
//!
//! 提供一致性 DMA 内存管理、MMIO 的 ioremap、流式 DMA 映射
//! 以及内存屏障操作.
//!
//! 取代 `src/kernel/dma.c` 中的 C 实现, 采用类型安全的
//! `PhysAddr`/`VirtAddr` 与无锁统计.

extern crate alloc;

use crate::kernel::framework::mm::get_vmm;
use crate::kernel::framework::mm::{PhysAddr, VirtAddr};
use core::ptr::{self};
use core::sync::atomic::{AtomicU64, Ordering};

pub mod api;
pub mod engine;

// Constants
pub const DMA_MAX_MAPPINGS: usize = 256;
pub const DMA_MAX_SCATTER_ENTRIES: usize = 64;
pub const MMIO_VIRT_BASE: u64 = 0xFFFF900000000000;

// Page helpers
const PAGE_SIZE: u64 = 4096;
const KERNEL_BASE: u64 = 0xFFFF800000000000;

/// DMA transfer direction
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DmaDirection {
    ToDevice = 0,
    FromDevice = 1,
    Bidirectional = 2,
}

/// DMA cache policy
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DmaCachePolicy {
    None = 0,
    Writeback = 1,
    Writethrough = 2,
}

/// DMA mapping entry
#[derive(Debug)]
pub struct DmaMapping {
    pub cpu_addr: VirtAddr,
    pub dma_addr: PhysAddr,
    pub size: usize,
    pub direction: DmaDirection,
    pub cache: DmaCachePolicy,
    pub is_coherent: bool,
    pub is_mapped: bool,
}

/// 散聚表 (scatter-gather) 列表条目
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct DmaScatterEntry {
    pub phys_addr: u64,
    pub length: usize,
    pub page_addr: usize,
}

/// 散聚表 (scatter-gather) 列表
#[derive(Debug)]
#[repr(C)]
pub struct DmaScatterList {
    pub entry_count: u32,
    pub entries: [DmaScatterEntry; DMA_MAX_SCATTER_ENTRIES],
    pub total_length: usize,
    pub direction: u32,
}

impl DmaScatterList {
    pub const fn new() -> Self {
        Self {
            entry_count: 0,
            entries: [DmaScatterEntry {
                phys_addr: 0,
                length: 0,
                page_addr: 0,
            }; DMA_MAX_SCATTER_ENTRIES],
            total_length: 0,
            direction: 0,
        }
    }
}

/// 传输完成回调类型
pub type DmaCallback = fn(*mut u8, i32);

/// DMA 传输请求 (opaque — C 端使用指针)
#[derive(Debug)]
pub struct DmaTransfer {
    pub src_addr: u64,
    pub dst_addr: u64,
    pub length: usize,
    pub direction: DmaDirection,
    pub synchronous: bool,
    pub completed: bool,
    pub result: i32,
    pub callback: Option<DmaCallback>,
    pub private_data: *mut u8,
}

/// DMA pool statistics
#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct DmaPoolStats {
    pub total_allocations: u64,
    pub total_frees: u64,
    pub total_mappings: u64,
    pub total_unmappings: u64,
    pub current_in_use: u64,
    pub max_concurrent: u64,
    pub coherence_fails: u64,
    pub total_bytes_allocated: u64,
    pub current_bytes_used: u64,
}

/// 无锁统计计数器
pub struct DmaStats {
    pub total_allocations: AtomicU64,
    pub total_frees: AtomicU64,
    pub total_mappings: AtomicU64,
    pub total_unmappings: AtomicU64,
    pub current_in_use: AtomicU64,
    pub max_concurrent: AtomicU64,
    pub coherence_fails: AtomicU64,
    pub total_bytes_allocated: AtomicU64,
    pub current_bytes_used: AtomicU64,
}

impl DmaStats {
    pub const fn new() -> Self {
        Self {
            total_allocations: AtomicU64::new(0),
            total_frees: AtomicU64::new(0),
            total_mappings: AtomicU64::new(0),
            total_unmappings: AtomicU64::new(0),
            current_in_use: AtomicU64::new(0),
            max_concurrent: AtomicU64::new(0),
            coherence_fails: AtomicU64::new(0),
            total_bytes_allocated: AtomicU64::new(0),
            current_bytes_used: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> DmaPoolStats {
        DmaPoolStats {
            total_allocations: self.total_allocations.load(Ordering::Relaxed),
            total_frees: self.total_frees.load(Ordering::Relaxed),
            total_mappings: self.total_mappings.load(Ordering::Relaxed),
            total_unmappings: self.total_unmappings.load(Ordering::Relaxed),
            current_in_use: self.current_in_use.load(Ordering::Relaxed),
            max_concurrent: self.max_concurrent.load(Ordering::Relaxed),
            coherence_fails: self.coherence_fails.load(Ordering::Relaxed),
            total_bytes_allocated: self.total_bytes_allocated.load(Ordering::Relaxed),
            current_bytes_used: self.current_bytes_used.load(Ordering::Relaxed),
        }
    }

    fn reset(&self) {
        self.total_allocations.store(0, Ordering::Relaxed);
        self.total_frees.store(0, Ordering::Relaxed);
        self.total_mappings.store(0, Ordering::Relaxed);
        self.total_unmappings.store(0, Ordering::Relaxed);
        self.current_in_use.store(0, Ordering::Relaxed);
        self.max_concurrent.store(0, Ordering::Relaxed);
        self.coherence_fails.store(0, Ordering::Relaxed);
        self.total_bytes_allocated.store(0, Ordering::Relaxed);
        self.current_bytes_used.store(0, Ordering::Relaxed);
    }
}

/// 从虚拟地址计算物理地址 (经页表走查, 待流式 DMA 映射启用后使用)。
#[inline]
#[allow(dead_code)]
fn virt_to_phys(virt: *const u8) -> u64 {
    if virt.is_null() {
        return 0;
    }
    get_vmm()
        .get_physical(VirtAddr(virt as u64))
        .map(|p| p.0)
        .unwrap_or(0)
}

/// MMIO virtual address allocator
static MMIO_NEXT: AtomicU64 = AtomicU64::new(MMIO_VIRT_BASE);

fn alloc_mmio_virt(size: usize) -> VirtAddr {
    let pages = (size as u64).div_ceil(PAGE_SIZE);
    let aligned_pages = pages.max(1);
    let addr = MMIO_NEXT.fetch_add(aligned_pages * PAGE_SIZE, Ordering::Relaxed);
    VirtAddr(addr)
}

// 重导出 engine 类型
pub use engine::get_dma;
