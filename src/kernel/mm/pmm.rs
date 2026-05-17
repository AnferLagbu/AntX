//! Physical Memory Manager (PMM)
//!
//! Manages physical memory pages using a bitmap allocator.
//! Provides page allocation/deallocation with support for:
//! - Single page allocation
//! - Multiple contiguous page allocation
//! - Huge page allocation (2MB, 1GB)
//! - Early boot allocation (before bitmap initialization)
//! - Statistics tracking
//!
//! # Safety
//! Interior mutability is achieved via `Cell`/`UnsafeCell` protected by
//! an internal `AtomicBool` spinlock. All mutations occur under the lock,
//! making `unsafe impl Sync` sound.

macro_rules! klog_pmm {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

use super::*;
use core::cell::{Cell, UnsafeCell};
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, Ordering};

const MAX_EARLY_ALLOCS: usize = 256;

#[derive(Clone, Copy)]
struct EarlyAlloc {
    addr: u64,
    size: u64,
}

impl EarlyAlloc {
    pub const fn const_default() -> Self {
        Self { addr: 0, size: 0 }
    }
}

pub struct PhysicalMemoryManager {
    bitmap: Cell<Option<NonNull<u32>>>,
    bitmap_size: Cell<usize>,
    mem_size: Cell<u64>,
    kernel_end: Cell<u64>,
    info: Cell<MemoryInfo>,
    lock: AtomicBool,
    initialized: AtomicBool,
    early_allocs: UnsafeCell<[EarlyAlloc; MAX_EARLY_ALLOCS]>,
    early_count: AtomicUsize,
    early_current: AtomicU64,
    total_allocs: AtomicU64,
    total_frees: AtomicU64,
    failed_allocs: AtomicU64,
}

unsafe impl Sync for PhysicalMemoryManager {}
unsafe impl Send for PhysicalMemoryManager {}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            bitmap: Cell::new(None),
            bitmap_size: Cell::new(0),
            mem_size: Cell::new(0),
            kernel_end: Cell::new(0),
            info: Cell::new(MemoryInfo::const_default()),
            lock: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            early_allocs: UnsafeCell::new([EarlyAlloc::const_default(); MAX_EARLY_ALLOCS]),
            early_count: AtomicUsize::new(0),
            early_current: AtomicU64::new(0),
            total_allocs: AtomicU64::new(0),
            total_frees: AtomicU64::new(0),
            failed_allocs: AtomicU64::new(0),
        }
    }

    pub fn init(&self, mem_size: u64, kernel_end: u64) {
        self.mem_size.set(mem_size);
        self.kernel_end.set(kernel_end);

        let total_pages = mem_size / PAGE_SIZE;

        let start = (kernel_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.store(start, Ordering::Relaxed);

        let mut info = self.info.get();
        info.total_pages = total_pages;
        info.kernel_end = kernel_end;
        self.info.set(info);

        klog_pmm!("[PMM] Initialized: {} MB, {} pages, kernel ends at 0x{:X}",
                       mem_size / (1024 * 1024), total_pages, kernel_end);
    }

    pub fn init_bitmap(&self, reserved_after_kernel: u64) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }

        let reserved_aligned = (reserved_after_kernel + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.fetch_add(reserved_aligned, Ordering::Relaxed);

        let info = self.info.get();
        let total_bits = info.total_pages as usize;
        let bitmap_words = (total_bits + 31) / 32;
        let bitmap_bytes = bitmap_words * 4;

        let bitmap_phys = self.early_current.fetch_add(bitmap_bytes as u64, Ordering::Relaxed);
        let bitmap_aligned = (bitmap_phys + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.store(bitmap_aligned + bitmap_bytes as u64 + PAGE_SIZE, Ordering::Relaxed);

        let bitmap_virt = bitmap_aligned + KERNEL_BASE;

        unsafe {
            core::ptr::write_bytes(bitmap_virt as *mut u8, 0, bitmap_bytes);
        }

        self.bitmap.set(match NonNull::new(bitmap_virt as *mut u32) {
            Some(ptr) => {
                klog_pmm!("[PMM] Bitmap allocated successfully at 0x{:X}", bitmap_virt);
                Some(ptr)
            },
            None => {
                klog_pmm!("[PMM] FATAL: Bitmap allocation returned null address (0x{:X})", bitmap_virt);
                return;
            }
        });
        self.bitmap_size.set(bitmap_words);

        let kernel_end_val = self.kernel_end.get();
        let kernel_pages = ((kernel_end_val + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        let reserved_pages = (reserved_aligned / PAGE_SIZE) as usize;
        let total_reserved = kernel_pages + reserved_pages;
        for i in 0..total_reserved.min(total_bits) {
            self.set_bit(i);
        }

        // Always reserve page 0 — PhysAddr(0) must never be handed out
        // because FFI layers convert it to a null pointer.
        if total_bits > 0 {
            self.set_bit(0);
        }

        let bitmap_start_page = (bitmap_aligned / PAGE_SIZE) as usize;
        let bitmap_pages = ((bitmap_bytes as u64 + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        for i in bitmap_start_page..(bitmap_start_page + bitmap_pages).min(total_bits) {
            self.set_bit(i);
        }

        self.initialized.store(true, Ordering::Release);

        self.update_stats();

        let info = self.info.get();
        klog_pmm!("[PMM] Bitmap initialized: {} total, {} free ({} MB), reserved {} pages, bmp @0x{:X}",
                       info.total_pages, info.free_pages,
                       info.free_pages * 4 / 1024,
                       total_reserved, bitmap_aligned);
    }

    pub fn alloc_page(&self) -> Option<PhysAddr> {
        self.acquire_lock();

        let result = if !self.initialized.load(Ordering::Acquire) {
            self.early_alloc_single()
        } else {
            self.alloc_from_bitmap(1)
        };

        match result {
            Some(addr) => {
                self.total_allocs.fetch_add(1, Ordering::Relaxed);
                self.release_lock();
                Some(addr)
            }
            None => {
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                self.release_lock();
                None
            }
        }
    }

    pub fn free_page(&self, addr: PhysAddr) {
        if addr.0 == 0 { return; }

        self.acquire_lock();

        if !self.initialized.load(Ordering::Acquire) {
            klog_pmm!("[PMM] Warning: Cannot free page at 0x{:X} before bitmap init", addr.0);
            self.release_lock();
            return;
        }

        let info = self.info.get();
        let page_num = addr.0 / PAGE_SIZE;

        if page_num >= info.total_pages {
            klog_pmm!("[PMM] Error: Invalid page address 0x{:X}", addr.0);
            self.release_lock();
            return;
        }

        if !self.test_bit(page_num as usize) {
            klog_pmm!("[PMM] Warning: Double free detected at page {}", page_num);
            self.release_lock();
            return;
        }

        self.clear_bit(page_num as usize);

        self.total_frees.fetch_add(1, Ordering::Relaxed);
        self.release_lock();
    }

    pub fn alloc_pages(&self, count: usize) -> Option<PhysAddr> {
        if count == 0 { return None; }
        if count == 1 { return self.alloc_page(); }

        self.acquire_lock();

        let result = if !self.initialized.load(Ordering::Acquire) {
            self.early_alloc_multiple(count as u64)
        } else {
            self.alloc_from_bitmap(count)
        };

        match result {
            Some(addr) => {
                self.total_allocs.fetch_add(count as u64, Ordering::Relaxed);
                self.release_lock();
                Some(addr)
            }
            None => {
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                self.release_lock();
                None
            }
        }
    }

    pub fn free_pages(&self, addr: PhysAddr, count: usize) {
        if addr.0 == 0 || count == 0 { return; }

        self.acquire_lock();

        if !self.initialized.load(Ordering::Acquire) {
            klog_pmm!("[PMM] Warning: Cannot free pages before bitmap init");
            self.release_lock();
            return;
        }

        let info = self.info.get();
        let start_page = addr.0 / PAGE_SIZE;

        for i in 0..count {
            let page_num = start_page + i as u64;

            if page_num >= info.total_pages { break; }

            if !self.test_bit(page_num as usize) {
                klog_pmm!("[PMM] Warning: Double free at page {}", page_num);
                continue;
            }

            self.clear_bit(page_num as usize);
        }

        self.total_frees.fetch_add(count as u64, Ordering::Relaxed);
        self.release_lock();
    }

    pub fn alloc_huge_page(&self, size_type: PageSize) -> Option<PhysAddr> {
        match size_type {
            PageSize::Size4K => self.alloc_page(),
            PageSize::Size2M | PageSize::Size1G => {
                let num_4k_pages = (size_type.size() / PAGE_SIZE) as usize;

                self.acquire_lock();

                let result = if !self.initialized.load(Ordering::Acquire) {
                    self.early_alloc_huge(size_type)
                } else {
                    self.alloc_aligned(num_4k_pages, size_type.size())
                };

                match result {
                    Some(addr) => {
                        self.total_allocs.fetch_add(num_4k_pages as u64, Ordering::Relaxed);
                        self.release_lock();
                        Some(addr)
                    }
                    None => {
                        self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                        self.release_lock();
                        None
                    }
                }
            }
        }
    }

    pub fn free_huge_page(&self, addr: PhysAddr, size_type: PageSize) {
        match size_type {
            PageSize::Size4K => self.free_page(addr),
            _ => {
                let num_4k_pages = (size_type.size() / PAGE_SIZE) as usize;
                self.free_pages(addr, num_4k_pages);
            }
        }
    }

    pub fn is_aligned_for_huge(&self, addr: PhysAddr, size_type: PageSize) -> bool {
        size_type.is_aligned(addr.0)
    }

    pub fn get_free_pages(&self) -> u64 {
        self.info.get().free_pages
    }

    pub fn get_total_pages(&self) -> u64 {
        self.info.get().total_pages
    }

    pub fn get_used_pages(&self) -> u64 {
        self.info.get().used_pages
    }

    pub fn get_info(&self) -> MemoryInfo {
        self.info.get()
    }

    pub fn dump_stats(&self) {
        let info = self.info.get();
        klog_pmm!("=== PMM Statistics ===");
        klog_pmm!("Total Memory: {} MB", self.mem_size.get() / (1024 * 1024));
        klog_pmm!("Total Pages: {}", info.total_pages);
        klog_pmm!("Free Pages:  {}", info.free_pages);
        klog_pmm!("Used Pages:  {}", info.used_pages);
        klog_pmm!("Kernel End:  0x{:X}", info.kernel_end);
        klog_pmm!("Total Allocs: {}", self.total_allocs.load(Ordering::Relaxed));
        klog_pmm!("Total Frees:  {}", self.total_frees.load(Ordering::Relaxed));
        klog_pmm!("Failed Allocs:{}", self.failed_allocs.load(Ordering::Relaxed));
        klog_pmm!("Initialized:  {}", self.initialized.load(Ordering::Relaxed));
        klog_pmm!("=====================");
    }

    // ==================== Private Methods ====================

    #[inline(always)]
    fn acquire_lock(&self) {
        while self.lock.compare_exchange_weak(
            false, true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn release_lock(&self) {
        self.lock.store(false, Ordering::Release);
    }

    fn set_bit(&self, bit: usize) {
        if let Some(bitmap_ptr) = self.bitmap.get() {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let word_index = bit / 32;
                let bit_index = bit % 32;

                if word_index < self.bitmap_size.get() {
                    let atomic_ptr = bitmap.add(word_index) as *const core::sync::atomic::AtomicU32;
                    (*atomic_ptr).fetch_or(1u32 << bit_index, core::sync::atomic::Ordering::SeqCst);
                }
            }
        }
    }

    fn clear_bit(&self, bit: usize) {
        if let Some(bitmap_ptr) = self.bitmap.get() {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let word_index = bit / 32;
                let bit_index = bit % 32;

                if word_index < self.bitmap_size.get() {
                    let atomic_ptr = bitmap.add(word_index) as *const core::sync::atomic::AtomicU32;
                    (*atomic_ptr).fetch_and(!(1u32 << bit_index), core::sync::atomic::Ordering::SeqCst);
                }
            }
        }
    }

    fn test_bit(&self, bit: usize) -> bool {
        if let Some(bitmap_ptr) = self.bitmap.get() {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let word_index = bit / 32;
                let bit_index = bit % 32;

                if word_index < self.bitmap_size.get() {
                    let atomic_ptr = bitmap.add(word_index) as *const core::sync::atomic::AtomicU32;
                    let value = (*atomic_ptr).load(core::sync::atomic::Ordering::SeqCst);
                    (value & (1u32 << bit_index)) != 0
                } else {
                    false
                }
            }
        } else {
            false
        }
    }

    fn find_first_free(&self, start: usize) -> Option<usize> {
        if let Some(bitmap_ptr) = self.bitmap.get() {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let total_bits = self.bitmap_size.get() * 32;

                for i in start..total_bits {
                    let word_index = i / 32;
                    let bit_index = i % 32;

                    let atomic_ptr = bitmap.add(word_index) as *const core::sync::atomic::AtomicU32;
                    let value = (*atomic_ptr).load(core::sync::atomic::Ordering::SeqCst);
                    if (value & (1u32 << bit_index)) == 0 {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    fn find_contiguous_free(&self, count: usize, align: u64) -> Option<usize> {
        if let Some(bitmap_ptr) = self.bitmap.get() {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let total_bits = self.bitmap_size.get() * 32;
                let mut consecutive = 0usize;
                let mut start_candidate = 0usize;

                let align_pages = (align / PAGE_SIZE) as usize;
                let mut i = 0;

                while i < total_bits {
                    if consecutive == 0 && align_pages > 1 {
                        if i % align_pages != 0 {
                            i += align_pages - (i % align_pages);
                            continue;
                        }
                    }

                    let word_index = i / 32;
                    let bit_index = i % 32;
                    let atomic_ptr = bitmap.add(word_index) as *const core::sync::atomic::AtomicU32;
                    let value = (*atomic_ptr).load(core::sync::atomic::Ordering::SeqCst);

                    if (value & (1u32 << bit_index)) == 0 {
                        if consecutive == 0 {
                            start_candidate = i;
                        }
                        consecutive += 1;

                        if consecutive >= count {
                            return Some(start_candidate);
                        }
                    } else {
                        consecutive = 0;
                    }

                    i += 1;
                }
            }
        }
        None
    }

    fn alloc_from_bitmap(&self, count: usize) -> Option<PhysAddr> {
        let required_alignment = PAGE_SIZE;

        if count == 1 {
            if let Some(bit) = self.find_first_free(0) {
                self.set_bit(bit);
                let addr = PhysAddr((bit as u64) * PAGE_SIZE);
                self.update_stats();
                return Some(addr);
            }
        } else {
            if let Some(start_bit) = self.find_contiguous_free(count, required_alignment) {
                for i in 0..count {
                    self.set_bit(start_bit + i);
                }

                let addr = PhysAddr((start_bit as u64) * PAGE_SIZE);
                self.update_stats();
                return Some(addr);
            }
        }

        None
    }

    fn alloc_aligned(&self, count: usize, alignment: u64) -> Option<PhysAddr> {
        if let Some(start_bit) = self.find_contiguous_free(count, alignment) {
            let addr = (start_bit as u64) * PAGE_SIZE;
            if addr % alignment == 0 {
                for i in 0..count {
                    self.set_bit(start_bit + i);
                }

                self.update_stats();
                return Some(PhysAddr(addr));
            }
        }

        None
    }

    fn early_alloc_single(&self) -> Option<PhysAddr> {
        let current = self.early_current.fetch_add(PAGE_SIZE, Ordering::Relaxed);
        let aligned = (current + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        self.early_current.store(aligned + PAGE_SIZE, Ordering::Relaxed);

        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let alloc_ptr = (*self.early_allocs.get()).as_mut_ptr().add(idx);
                (*alloc_ptr).addr = aligned;
                (*alloc_ptr).size = PAGE_SIZE;
            }
        }

        if aligned >= self.mem_size.get() {
            klog_pmm!("[PMM] Error: Early allocation out of memory!");
            return None;
        }

        Some(PhysAddr(aligned))
    }

    fn early_alloc_multiple(&self, count: u64) -> Option<PhysAddr> {
        let size = count * PAGE_SIZE;
        let current = self.early_current.fetch_add(size, Ordering::Relaxed);
        let aligned = (current + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        self.early_current.store(aligned + size, Ordering::Relaxed);

        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let alloc_ptr = (*self.early_allocs.get()).as_mut_ptr().add(idx);
                (*alloc_ptr).addr = aligned;
                (*alloc_ptr).size = size;
            }
        }

        if aligned + size > self.mem_size.get() {
            klog_pmm!("[PMM] Error: Early multi-page alloc out of memory!");
            return None;
        }

        Some(PhysAddr(aligned))
    }

    fn early_alloc_huge(&self, size_type: PageSize) -> Option<PhysAddr> {
        let size = size_type.size();
        let current = self.early_current.load(Ordering::Relaxed);

        let aligned = (current + size - 1) & !(size - 1);

        self.early_current.store(aligned + size, Ordering::Relaxed);

        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let alloc_ptr = (*self.early_allocs.get()).as_mut_ptr().add(idx);
                (*alloc_ptr).addr = aligned;
                (*alloc_ptr).size = size;
            }
        }

        if aligned + size > self.mem_size.get() {
            klog_pmm!("[PMM] Error: Early huge page alloc out of memory!");
            return None;
        }

        Some(PhysAddr(aligned))
    }

    fn update_stats(&self) {
        if let Some(bitmap_ptr) = self.bitmap.get() {
            let bitmap_size = self.bitmap_size.get();
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let mut used_count: u64 = 0;

                for i in 0..bitmap_size {
                    let atomic_ptr = bitmap.add(i) as *const core::sync::atomic::AtomicU32;
                    let value = (*atomic_ptr).load(core::sync::atomic::Ordering::SeqCst);
                    used_count += value.count_ones() as u64;
                }

                let mut info = self.info.get();
                info.used_pages = used_count;
                info.free_pages = info.total_pages - used_count;
                self.info.set(info);
            }
        }
    }
}

static GLOBAL_PMM: spin::Once<PhysicalMemoryManager> = spin::Once::new();

pub fn pmm_init(mem_size: u64, kernel_end: u64) -> &'static PhysicalMemoryManager {
    GLOBAL_PMM.call_once(|| {
        let pmm = PhysicalMemoryManager::new();
        pmm.init(mem_size, kernel_end);
        pmm
    })
}

pub fn pmm_init_bitmap(reserved_after_kernel: u64) {
    let pmm = GLOBAL_PMM.get().expect("[PMM] pmm_init_bitmap called before pmm_init");
    pmm.init_bitmap(reserved_after_kernel);
}

pub fn get_pmm() -> &'static PhysicalMemoryManager {
    GLOBAL_PMM.get().expect("[PMM] accessed before initialization")
}

#[derive(Clone, Copy)]
struct PmmSnapshot {
    total_allocs: u64,
    total_frees: u64,
    failed_allocs: u64,
    info: super::MemoryInfo,
}

static PMM_SNAPSHOT: spin::Mutex<Option<PmmSnapshot>> = spin::Mutex::new(None);

pub fn pmm_barrier_capture() {
    let pmm = get_pmm();
    let mut snap = PMM_SNAPSHOT.lock();
    *snap = Some(PmmSnapshot {
        total_allocs: pmm.total_allocs.load(Ordering::SeqCst),
        total_frees: pmm.total_frees.load(Ordering::SeqCst),
        failed_allocs: pmm.failed_allocs.load(Ordering::SeqCst),
        info: pmm.info.get(),
    });
}

pub fn pmm_barrier_rollback() -> bool {
    let pmm = get_pmm();
    let snap = PMM_SNAPSHOT.lock();
    if let Some(ref s) = *snap {
        pmm.total_allocs.store(s.total_allocs, Ordering::SeqCst);
        pmm.total_frees.store(s.total_frees, Ordering::SeqCst);
        pmm.failed_allocs.store(s.failed_allocs, Ordering::SeqCst);
        pmm.info.set(s.info);
    }
    true
}

extern "C" fn pmm_barrier_capture_cb() {
    pmm_barrier_capture();
}

extern "C" fn pmm_barrier_rollback_cb() -> bool {
    pmm_barrier_rollback()
}

pub fn pmm_register_barrier_domain() {
    crate::kernel::barrier::recovery_domain_register(3);
    if let Some(dom) = crate::kernel::barrier::RECOVERY_MANAGER.lock().find(3) {
        *dom.capture_cb.lock() = Some(pmm_barrier_capture_cb);
        *dom.rollback_cb.lock() = Some(pmm_barrier_rollback_cb);
    }
}
