//! DMA Engine core implementation
//!
//! Manages coherent DMA allocations, streaming mappings,
//! scatter-gather lists, and MMIO (ioremap) mapping.
//! Uses PhysAddr/VirtAddr type safety and lock-free atomics.

use super::*;
use crate::kernel::mm::{PhysAddr, VirtAddr, PageFlags};
use core::sync::atomic::{AtomicBool, Ordering};
use alloc::vec::Vec;
use spin::Mutex;

pub struct DmaEngine {
    initialized: AtomicBool,
    mappings: Mutex<Vec<DmaMapping>>,
    stats: DmaStats,
    mmio_regions: Mutex<Vec<(VirtAddr, PhysAddr, usize)>>,
}

unsafe impl Send for DmaEngine {}
unsafe impl Sync for DmaEngine {}

impl DmaEngine {
    pub const fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            mappings: Mutex::new(Vec::new()),
            stats: DmaStats::new(),
            mmio_regions: Mutex::new(Vec::new()),
        }
    }

    // =============== Lifecycle ===============

    pub fn init(&self) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }
        self.initialized.store(true, Ordering::Release);
    }

    pub fn shutdown(&self) {
        if !self.initialized.load(Ordering::Acquire) {
            return;
        }

        let mut mappings = self.mappings.lock();

        // Drop all coherent mappings which release their pages
        for m in mappings.drain(..) {
            if m.is_coherent {
                let pages = (m.size as u64 + PAGE_SIZE - 1) / PAGE_SIZE;
                get_pmm().free_pages(m.dma_addr, pages as usize);
            }
        }

        // Clear mmio regions
        let mut regions = self.mmio_regions.lock();
        for (virt, _phys, size) in regions.drain(..) {
            let pages = (size as u64 + PAGE_SIZE - 1) / PAGE_SIZE;
            for i in 0..pages {
                get_vmm().unmap_page(VirtAddr(virt.0 + i * PAGE_SIZE));
            }
        }

        self.stats.reset();
        self.initialized.store(false, Ordering::Release);
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    // =============== Coherent DMA Memory ===============

    /// Allocate physically contiguous DMA-coherent memory.
    /// Returns (cpu_virt_addr, dma_phys_addr).
    pub fn alloc_coherent(&self, size: usize) -> Option<(VirtAddr, PhysAddr)> {
        if size == 0 || !self.initialized.load(Ordering::Acquire) {
            return None;
        }

        let pages = (size as u64 + PAGE_SIZE - 1) / PAGE_SIZE;

        let phys = match get_pmm().alloc_pages(pages as usize) {
            Some(p) => p,
            None => {
                self.stats.coherence_fails.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };

        // Convert physical to kernel virtual (direct-map region)
        let virt = VirtAddr(phys.0 + KERNEL_BASE);

        // Zero the memory
        unsafe {
            ptr::write_bytes(virt.0 as *mut u8, 0, (pages * PAGE_SIZE) as usize);
        }

        self.stats.total_allocations.fetch_add(1, Ordering::Relaxed);
        self.stats.total_bytes_allocated.fetch_add(size as u64, Ordering::Relaxed);
        self.stats.current_bytes_used.fetch_add(size as u64, Ordering::Relaxed);

        // Track this allocation
        self.mappings.lock().push(DmaMapping {
            cpu_addr: virt,
            dma_addr: phys,
            size,
            direction: DmaDirection::Bidirectional,
            cache: DmaCachePolicy::None,
            is_coherent: true,
            is_mapped: true,
        });

        Some((virt, phys))
    }

    /// Free coherent DMA memory
    pub fn free_coherent(&self, cpu_addr: VirtAddr, size: usize) {
        if size == 0 || cpu_addr.0 == 0 || !self.initialized.load(Ordering::Acquire) {
            return;
        }

        let phys_addr = get_vmm().get_physical(cpu_addr);
        let pages = (size as u64 + PAGE_SIZE - 1) / PAGE_SIZE;

        if let Some(phys) = phys_addr {
            get_pmm().free_pages(phys, pages as usize);
        }

        self.stats.total_frees.fetch_add(1, Ordering::Relaxed);
        if self.stats.current_bytes_used.load(Ordering::Relaxed) >= size as u64 {
            self.stats.current_bytes_used.fetch_sub(size as u64, Ordering::Relaxed);
        }

        // Remove from tracking
        let mut mappings = self.mappings.lock();
        mappings.retain(|m| m.cpu_addr != cpu_addr);
    }

    /// Get the device (physical) DMA address for a CPU virtual address
    pub fn device_address(&self, cpu_addr: VirtAddr) -> Option<PhysAddr> {
        if cpu_addr.0 == 0 {
            return None;
        }
        get_vmm().get_physical(cpu_addr)
    }

    // =============== ioremap (MMIO) ===============

    /// Map physical MMIO region into kernel virtual address space.
    /// Uses uncacheable (UC) mapping suitable for device registers.
    pub fn ioremap(&self, phys_addr: PhysAddr, size: usize) -> Option<VirtAddr> {
        if phys_addr.0 == 0 || size == 0 || !self.initialized.load(Ordering::Acquire) {
            return None;
        }

        let pages = (size as u64 + PAGE_SIZE - 1) / PAGE_SIZE;
        let virt = alloc_mmio_virt(size);

        let flags = PageFlags::PRESENT
            | PageFlags::WRITABLE
            | PageFlags::from_bits_truncate(1 << 4)   // PCD: cache disable
            | PageFlags::from_bits_truncate(1 << 3);   // PWT: write-through

        for i in 0..pages {
            let page_phys = PhysAddr(phys_addr.0 + i * PAGE_SIZE);
            let page_virt = VirtAddr(virt.0 + i * PAGE_SIZE);

            if get_vmm().map_page(page_virt, page_phys, flags).is_err() {
                // Rollback
                for j in 0..i {
                    let unmap_virt = VirtAddr(virt.0 + j * PAGE_SIZE);
                    get_vmm().unmap_page(unmap_virt);
                }
                return None;
            }
        }

        self.mmio_regions.lock().push((virt, phys_addr, size));
        Some(virt)
    }

    /// Unmap MMIO region
    pub fn iounmap(&self, virt_addr: VirtAddr, size: usize) {
        if virt_addr.0 == 0 || size == 0 {
            return;
        }

        let pages = (size as u64 + PAGE_SIZE - 1) / PAGE_SIZE;
        for i in 0..pages {
            get_vmm().unmap_page(VirtAddr(virt_addr.0 + i * PAGE_SIZE));
        }

        let mut regions = self.mmio_regions.lock();
        regions.retain(|(v, _, _)| *v != virt_addr);
    }

    // =============== Streaming DMA Mappings ===============

    /// Map an existing kernel buffer for DMA
    pub fn map_single(&self, buffer: VirtAddr, size: usize, direction: DmaDirection) -> Option<*const DmaMapping> {
        if buffer.0 == 0 || size == 0 || !self.initialized.load(Ordering::Acquire) {
            return None;
        }

        let dma_addr = get_vmm().get_physical(buffer)?;

        let mut mappings = self.mappings.lock();

        let mapping = DmaMapping {
            cpu_addr: buffer,
            dma_addr,
            size,
            direction,
            cache: DmaCachePolicy::Writeback,
            is_coherent: false,
            is_mapped: true,
        };

        // Sync before giving to device
        if matches!(direction, DmaDirection::ToDevice) {
            Self::barrier_device();
        }

        mappings.push(mapping);
        let idx = mappings.len() - 1;

        let mapping_count = mappings.len() as u64;
        self.stats.total_mappings.fetch_add(1, Ordering::Relaxed);
        self.stats.current_in_use.store(mapping_count, Ordering::Relaxed);

        // Update max concurrent
        let mut max = self.stats.max_concurrent.load(Ordering::Relaxed);
        while mapping_count > max {
            match self.stats.max_concurrent.compare_exchange_weak(
                max, mapping_count, Ordering::Relaxed, Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(m) => max = m,
            }
        }

        // Return a pointer that can be passed to C — use the Vec's internal pointer
        Some(&mappings[idx] as *const DmaMapping)
    }

    /// Unmap a streaming DMA mapping
    pub fn unmap_single(&self, mapping: &DmaMapping) {
        if !mapping.is_mapped || !self.initialized.load(Ordering::Acquire) {
            return;
        }

        let mut mappings = self.mappings.lock();
        mappings.retain(|m| m.cpu_addr != mapping.cpu_addr || m.dma_addr != mapping.dma_addr);

        let count = mappings.len() as u64;
        self.stats.total_unmappings.fetch_add(1, Ordering::Relaxed);
        self.stats.current_in_use.store(count, Ordering::Relaxed);
    }

    // =============== Cache Synchronization ===============

    /// Sync for device access (CPU → Device)
    pub fn sync_for_device(&self, _mapping: &DmaMapping, _offset: usize, _size: usize) {
        Self::barrier_device();
    }

    /// Sync for CPU access (Device → CPU)
    pub fn sync_for_cpu(&self, _mapping: &DmaMapping, _offset: usize, _size: usize) {
        Self::barrier_cpu();
    }

    /// Bidirectional sync
    pub fn sync_both(&self, mapping: &DmaMapping, offset: usize, size: usize) {
        self.sync_for_device(mapping, offset, size);
        self.sync_for_cpu(mapping, offset, size);
    }

    // =============== Scatter-Gather ===============

    pub fn sg_init(&self, sglist: &mut DmaScatterList) {
        sglist.entry_count = 0;
        sglist.total_length = 0;
    }

    pub fn sg_add_entry(&self, sglist: &mut DmaScatterList, addr: VirtAddr, length: usize) -> i32 {
        if addr.0 == 0 || length == 0 {
            return -1;
        }
        if sglist.entry_count as usize >= DMA_MAX_SCATTER_ENTRIES {
            return -1;
        }

        let idx = sglist.entry_count as usize;
        let phys = get_vmm().get_physical(addr);

        sglist.entries[idx] = DmaScatterEntry {
            phys_addr: phys.map(|p| p.0).unwrap_or(0),
            length,
            page_addr: addr.0 as usize,
        };

        sglist.entry_count += 1;
        sglist.total_length += length;
        0
    }

    pub fn sg_total_length(&self, sglist: &DmaScatterList) -> usize {
        sglist.total_length
    }

    // =============== Statistics ===============

    pub fn get_stats(&self) -> DmaPoolStats {
        self.stats.snapshot()
    }

    pub fn reset_stats(&self) {
        self.stats.reset();
    }

    // =============== Private Helpers ===============

    #[inline(always)]
    fn barrier_device() {
        // sfence: ensure all stores are visible before DMA
        unsafe {
            core::arch::asm!("sfence", options(nomem, nostack));
        }
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    #[inline(always)]
    fn barrier_cpu() {
        // lfence: ensure all loads reflect DMA writes
        unsafe {
            core::arch::asm!("lfence", options(nomem, nostack));
        }
        core::sync::atomic::fence(Ordering::SeqCst);
    }
}

// Global DMA Engine instance
static mut GLOBAL_DMA: DmaEngine = DmaEngine::new();

pub fn get_dma() -> &'static DmaEngine {
    unsafe { &GLOBAL_DMA }
}

pub unsafe fn get_dma_mut() -> &'static mut DmaEngine {
    &mut GLOBAL_DMA
}

// Accessor used by FFI layer
pub(crate) fn dma() -> &'static DmaEngine {
    unsafe { &GLOBAL_DMA }
}

// =============== DMA Transfer Engine ===============

#[repr(C)]
pub struct DmaTransfer {
    pub src_addr: PhysAddr,
    pub dst_addr: PhysAddr,
    pub size: usize,
    pub direction: DmaDirection,
    pub completed: core::sync::atomic::AtomicBool,
    pub callback: Option<DmaCallback>,
}

pub type DmaCallback = fn(*const DmaTransfer);

impl DmaTransfer {
    pub const fn new(src: PhysAddr, dst: PhysAddr, size: usize, dir: DmaDirection) -> Self {
        Self {
            src_addr: src,
            dst_addr: dst,
            size,
            direction: dir,
            completed: core::sync::atomic::AtomicBool::new(false),
            callback: None,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }

    pub fn wait(&self) {
        while !self.is_complete() {
            core::hint::spin_loop();
        }
    }
}

const MAX_DMA_TRANSFERS: usize = 32;

static DMA_TRANSFERS: [core::sync::atomic::AtomicU8; MAX_DMA_TRANSFERS] = 
    [const { core::sync::atomic::AtomicU8::new(0) }; MAX_DMA_TRANSFERS];

pub fn submit_transfer(src: PhysAddr, dst: PhysAddr, size: usize, _dir: DmaDirection) -> Option<usize> {
    let _slot = (0..MAX_DMA_TRANSFERS).find(|i| {
        DMA_TRANSFERS[*i].compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed).is_ok()
    })?;

    unsafe {
        let src_virt = ioremap(src.0, size)?;
        let dst_virt = ioremap(dst.0, size)?;

        core::ptr::copy_nonoverlapping(src_virt as *const u8, dst_virt as *mut u8, size);

        DmaEngine::barrier_device();
    }

    Some(0)
}

pub fn submit_transfer_async(src: PhysAddr, dst: PhysAddr, size: usize, dir: DmaDirection, callback: DmaCallback) -> Option<usize> {
    let id = submit_transfer(src, dst, size, dir)?;
    let transfer = DmaTransfer {
        src_addr: src,
        dst_addr: dst,
        size,
        direction: dir,
        completed: core::sync::atomic::AtomicBool::new(true),
        callback: Some(callback),
    };
    callback(&transfer);
    Some(id)
}

unsafe fn ioremap(phys: u64, size: usize) -> Option<u64> {
    dma().ioremap(PhysAddr(phys), size).map(|v| v.0)
}
