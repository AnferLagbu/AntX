//! DMA Engine 核心实现
//!
//! 管理一致性 DMA 分配、流式映射、散聚表 (scatter-gather)
//! 以及 MMIO (ioremap) 映射.
//! 采用 `PhysAddr`/`VirtAddr` 类型安全与无锁原子变量.

use super::*;
use crate::kernel::framework::mm::{PageFlags, PhysAddr, VirtAddr};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock as Mutex;
pub struct DmaEngine {
    initialized: AtomicBool,
    mappings: Mutex<Vec<DmaMapping>>,
    stats: DmaStats,
    mmio_regions: Mutex<Vec<(VirtAddr, PhysAddr, usize)>>,
}

// SAFETY: DmaEngine 对 mappings/mmio_regions 使用 Mutex, 对 initialized 使用 AtomicBool.
// DmaStats 仅为普通 Copy 类型. 不存在未配同步原语的 UnsafeCell.
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

        // 释放所有一致性映射并回收其页面
        for m in mappings.drain(..) {
            if m.is_coherent {
                let pages = (m.size as u64).div_ceil(PAGE_SIZE);
                get_pmm().free_pages(m.dma_addr, pages as usize);
            }
        }

        // 清空 MMIO 映射区
        let mut regions = self.mmio_regions.lock();
        for (virt, _phys, size) in regions.drain(..) {
            let pages = (size as u64).div_ceil(PAGE_SIZE);
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

    /// 分配物理上连续的 DMA 一致性内存.
    /// 返回 `(cpu_virt_addr, dma_phys_addr)`.
    pub fn alloc_coherent(&self, size: usize) -> Option<(VirtAddr, PhysAddr)> {
        if size == 0 || !self.initialized.load(Ordering::Acquire) {
            return None;
        }

        let pages = (size as u64).div_ceil(PAGE_SIZE);

        let phys = match get_pmm().alloc_pages(pages as usize) {
            Some(p) => p,
            None => {
                self.stats.coherence_fails.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };

        // 将物理地址转换为内核虚拟地址 (direct-map 区)
        let virt = VirtAddr(phys.0 + KERNEL_BASE);

        // 清零内存
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            ptr::write_bytes(virt.0 as *mut u8, 0, (pages * PAGE_SIZE) as usize);
        }

        // 确保清零对设备可见
        self.cache_flush(virt, size);

        self.stats.total_allocations.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_bytes_allocated
            .fetch_add(size as u64, Ordering::Relaxed);
        self.stats
            .current_bytes_used
            .fetch_add(size as u64, Ordering::Relaxed);

        // 记录本次分配
        self.mappings.lock().push(DmaMapping {
            cpu_addr: virt,
            dma_addr: phys,
            size,
            direction: DmaDirection::Bidirectional,
            cache: DmaCachePolicy::Writeback,
            is_coherent: true,
            is_mapped: true,
        });

        Some((virt, phys))
    }

    /// 释放 DMA 一致性内存
    pub fn free_coherent(&self, cpu_addr: VirtAddr, size: usize) {
        if size == 0 || cpu_addr.0 == 0 || !self.initialized.load(Ordering::Acquire) {
            return;
        }

        let mut mappings = self.mappings.lock();

        let phys_addr = mappings
            .iter()
            .find(|m| m.cpu_addr == cpu_addr && m.is_coherent)
            .map(|m| m.dma_addr);

        let pages = (size as u64).div_ceil(PAGE_SIZE);

        if let Some(phys) = phys_addr {
            get_pmm().free_pages(phys, pages as usize);
        }

        mappings.retain(|m| m.cpu_addr != cpu_addr);

        drop(mappings);

        self.stats.total_frees.fetch_add(1, Ordering::Relaxed);
        if self.stats.current_bytes_used.load(Ordering::Relaxed) >= size as u64 {
            self.stats
                .current_bytes_used
                .fetch_sub(size as u64, Ordering::Relaxed);
        }
    }

    /// 获取 CPU 虚拟地址对应的设备 (物理) DMA 地址
    pub fn device_address(&self, cpu_addr: VirtAddr) -> Option<PhysAddr> {
        if cpu_addr.0 == 0 {
            return None;
        }
        get_vmm().get_physical(cpu_addr)
    }

    // =============== ioremap (MMIO) ===============

    /// 将物理 MMIO 区映射到内核虚拟地址空间.
    /// 采用不可缓存 (UC) 映射, 适合设备寄存器访问.
    pub fn ioremap(&self, phys_addr: PhysAddr, size: usize) -> Option<VirtAddr> {
        if phys_addr.0 == 0 || size == 0 || !self.initialized.load(Ordering::Acquire) {
            return None;
        }

        let pages = (size as u64).div_ceil(PAGE_SIZE);
        let virt = alloc_mmio_virt(size);

        let flags = PageFlags::PRESENT
            | PageFlags::WRITABLE
            | PageFlags::from_bits_truncate(1 << 4)   // PCD: cache disable
            | PageFlags::from_bits_truncate(1 << 3); // PWT: write-through

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

    /// 解除 MMIO 区映射
    pub fn iounmap(&self, virt_addr: VirtAddr, size: usize) {
        if virt_addr.0 == 0 || size == 0 {
            return;
        }

        let pages = (size as u64).div_ceil(PAGE_SIZE);
        for i in 0..pages {
            get_vmm().unmap_page(VirtAddr(virt_addr.0 + i * PAGE_SIZE));
        }

        let mut regions = self.mmio_regions.lock();
        regions.retain(|(v, _, _)| *v != virt_addr);
    }

    // =============== 流式 DMA 映射 ===============

    /// 将已有内核缓冲区映射为 DMA 缓冲区
    pub fn map_single(
        &self,
        buffer: VirtAddr,
        size: usize,
        direction: DmaDirection,
    ) -> Option<*const DmaMapping> {
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

        // 交给设备前同步
        if matches!(direction, DmaDirection::ToDevice) {
            Self::barrier_device();
        }

        mappings.push(mapping);
        let idx = mappings.len() - 1;

        let mapping_count = mappings.len() as u64;
        self.stats.total_mappings.fetch_add(1, Ordering::Relaxed);
        self.stats
            .current_in_use
            .store(mapping_count, Ordering::Relaxed);

        // 更新最大并发数
        let mut max = self.stats.max_concurrent.load(Ordering::Relaxed);
        while mapping_count > max {
            match self.stats.max_concurrent.compare_exchange_weak(
                max,
                mapping_count,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(m) => max = m,
            }
        }

        // 返回可传给 C 的指针 — 使用 Vec 内部指针
        Some(&mappings[idx] as *const DmaMapping)
    }

    /// 解除流式 DMA 映射
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

    // =============== 缓存同步 ===============

    /// 为设备访问同步 (CPU → Device)
    pub fn sync_for_device(&self, _mapping: &DmaMapping, _offset: usize, _size: usize) {
        Self::barrier_device();
    }

    /// 为 CPU 访问同步 (Device → CPU)
    pub fn sync_for_cpu(&self, _mapping: &DmaMapping, _offset: usize, _size: usize) {
        Self::barrier_cpu();
    }

    /// 双向同步
    pub fn sync_both(&self, mapping: &DmaMapping, offset: usize, size: usize) {
        self.sync_for_device(mapping, offset, size);
        self.sync_for_cpu(mapping, offset, size);
    }

    // =============== 散聚表 (Scatter-Gather) ===============

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

    // =============== 统计 ===============

    pub fn get_stats(&self) -> DmaPoolStats {
        self.stats.snapshot()
    }

    pub fn reset_stats(&self) {
        self.stats.reset();
    }

    // =============== 私有辅助函数 ===============

    /// 刷新 CPU 缓存以确保 DMA 一致性.
    /// 该函数与架构相关, 对非一致性 DMA 至关重要.
    #[inline(always)]
    #[allow(unused_variables)]
    fn cache_flush(&self, addr: VirtAddr, size: usize) {
        // 架构相关缓存刷新
        #[cfg(target_arch = "x86_64")]
        {
            // x86_64: 若可用则使用 CLFLUSH 指令
            // 当前使用内存栅栏, 因为 x86_64 多数情况下已一致
            core::sync::atomic::fence(Ordering::SeqCst);

            // TODO(TRACK-A99EBB): 在较新 CPU 上使用 CLFLUSHOPT 以获取更好性能
            // if cpu_has_clflushopt() {
            //     flush_cache_clflushopt(addr, size);
            // }
        }

        #[cfg(target_arch = "aarch64")]
        {
            // aarch64: 使用缓存维护指令
            // DCCMVAC — 按虚拟地址清理数据缓存至一致性点
            // 确保 CPU 写入对 DMA 设备可见
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                let start = addr.0 as usize;
                let end = start + size;
                let cache_line_size = 64; // 典型缓存行大小

                // 将起始地址向下对齐到缓存行边界
                let aligned_start = start & !(cache_line_size - 1);

                for offset in (aligned_start..end).step_by(cache_line_size) {
                    // DCCVAC — 按虚拟地址将数据缓存清理至一致性点
                    // dc cvac: 将脏缓存行写回内存
                    core::arch::asm!(
                        "dc cvac, {addr}",
                        addr = in(reg) offset,
                        options(nostack, nomem),
                    );
                }

                // 数据同步屏障
                // 确保所有缓存维护操作完成后再继续
                core::arch::asm!("dsb sy", options(nostack, nomem));
            }
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            // 其他架构: 假设一致性, 或退化为栅栏
            core::sync::atomic::fence(Ordering::SeqCst);
        }
    }

    /// 在 DMA 读之前失效 CPU 缓存.
    /// 确保 CPU 能看到设备的写入.
    #[inline(always)]
    #[allow(unused_variables)]
    fn cache_invalidate(&self, addr: VirtAddr, size: usize) {
        #[cfg(target_arch = "x86_64")]
        {
            // x86_64: 缓存通常一致, 只需栅栏
            core::sync::atomic::fence(Ordering::SeqCst);
        }

        #[cfg(target_arch = "aarch64")]
        {
            // aarch64: 使用 DC IVAC 指令
            // 失效缓存行, 以便 CPU 从内存读入最新数据
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                let start = addr.0 as usize;
                let end = start + size;
                let cache_line_size = 64;

                // 将起始地址向下对齐到缓存行边界
                let aligned_start = start & !(cache_line_size - 1);

                for offset in (aligned_start..end).step_by(cache_line_size) {
                    // DCIVAC — 按虚拟地址将数据缓存失效至一致性点
                    // dc ivac: 失效缓存行, 强制下次读从内存加载
                    core::arch::asm!(
                        "dc ivac, {addr}",
                        addr = in(reg) offset,
                        options(nostack, nomem),
                    );
                }

                // 数据同步屏障
                // 确保所有缓存失效操作完成后再继续
                core::arch::asm!("dsb sy", options(nostack, nomem));
            }
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            core::sync::atomic::fence(Ordering::SeqCst);
        }
    }

    #[inline(always)]
    fn barrier_device() {
        // sfence: 确保所有 store 在 DMA 之前可见
        crate::arch!(fence_w());
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    #[inline(always)]
    fn barrier_cpu() {
        // lfence: 确保所有 load 反映 DMA 写入
        crate::arch!(fence_r());
        core::sync::atomic::fence(Ordering::SeqCst);
    }
}

// 全局 DMA Engine 实例
static mut GLOBAL_DMA: DmaEngine = DmaEngine::new();

pub fn get_dma() -> &'static DmaEngine {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { &GLOBAL_DMA }
}

/// # Safety
///
/// 在 DMA 子系统尚未生效前的初始化阶段调用. 调用方保证 `count` > 0.
pub unsafe fn get_dma_mut() -> &'static mut DmaEngine {
    &mut GLOBAL_DMA
}

/// FFI 层使用的访问器
pub(crate) fn dma() -> &'static DmaEngine {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { &GLOBAL_DMA }
}

// =============== DMA 传输引擎 ===============

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

pub fn submit_transfer(
    src: PhysAddr,
    dst: PhysAddr,
    size: usize,
    _dir: DmaDirection,
) -> Option<usize> {
    let _slot = (0..MAX_DMA_TRANSFERS).find(|i| {
        DMA_TRANSFERS[*i]
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    })?;

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let src_virt = ioremap(src.0, size)?;
        let dst_virt = ioremap(dst.0, size)?;

        core::ptr::copy_nonoverlapping(src_virt as *const u8, dst_virt as *mut u8, size);

        DmaEngine::barrier_device();
    }

    Some(0)
}

pub fn submit_transfer_async(
    src: PhysAddr,
    dst: PhysAddr,
    size: usize,
    dir: DmaDirection,
    callback: DmaCallback,
) -> Option<usize> {
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

// SAFETY: 调用方保证指针/类型有效 (详见上下文)
unsafe fn ioremap(phys: u64, size: usize) -> Option<u64> {
    dma().ioremap(PhysAddr(phys), size).map(|v| v.0)
}
