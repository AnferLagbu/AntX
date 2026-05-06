//! Physical Memory Manager (PMM)
//!
//! Manages physical memory pages using a bitmap allocator.
//! Provides page allocation/deallocation with support for:
//! - Single page allocation
//! - Multiple contiguous page allocation
//! - Huge page allocation (2MB, 1GB)
//! - Early boot allocation (before bitmap initialization)
//! - Statistics tracking

/// klog output macro for PMM
macro_rules! klog_pmm {
    ($($arg:tt)*) => {
        {
            extern "C" {
                fn klog_ffi_info(msg: *const u8);
            }
            use core::fmt::Write;
            let mut buf: [u8; 256] = [0u8; 256];
            let mut cursor = 0;
            let _ = core::fmt::write(&mut CursorWriter::new(&mut buf, &mut cursor), format_args!($($arg)*));
            if cursor > 0 {
                unsafe { klog_ffi_info(buf.as_ptr()); }
            }
        }
    };
}

struct CursorWriter<'a> {
    buf: &'a mut [u8],
    cursor: &'a mut usize,
}

impl<'a> CursorWriter<'a> {
    fn new(buf: &'a mut [u8], cursor: &'a mut usize) -> Self {
        Self { buf, cursor }
    }
}

impl<'a> core::fmt::Write for CursorWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len() - *self.cursor;
        let to_write = bytes.len().min(remaining);
        self.buf[*self.cursor..*self.cursor + to_write].copy_from_slice(&bytes[..to_write]);
        *self.cursor += to_write;
        Ok(())
    }
}

use super::*;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, Ordering};

/// Magic number for heap header validation (matching C implementation)
const HEAP_MAGIC: u32 = 0xDEADBEEF;

/// Maximum number of early allocations to track
const MAX_EARLY_ALLOCS: usize = 256;

/// Early allocation entry structure
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

/// Physical Memory Manager - global state
pub struct PhysicalMemoryManager {
    /// Bitmap array (each bit represents one 4KB page)
    bitmap: Option<NonNull<u32>>,
    
    /// Size of bitmap in 32-bit words
    bitmap_size: usize,
    
    /// Total memory size in bytes
    mem_size: u64,
    
    /// Kernel end address (where kernel code/data ends)
    kernel_end: u64,
    
    /// Memory information statistics
    info: MemoryInfo,
    
    /// Spinlock for thread safety
    lock: AtomicBool,
    
    /// Track if PMM is initialized
    initialized: AtomicBool,
    
    /// Early allocations before bitmap setup
    early_allocs: [EarlyAlloc; MAX_EARLY_ALLOCS],
    
    /// Number of early allocations
    early_count: AtomicUsize,
    
    /// Current position for early allocation
    early_current: AtomicU64,
    
    /// Allocation statistics
    total_allocs: AtomicU64,
    total_frees: AtomicU64,
    failed_allocs: AtomicU64,
}

impl PhysicalMemoryManager {
    /// Create a new uninitialized PMM instance
    pub const fn new() -> Self {
        Self {
            bitmap: None,
            bitmap_size: 0,
            mem_size: 0,
            kernel_end: 0,
            info: MemoryInfo::const_default(),
            lock: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            early_allocs: [EarlyAlloc::const_default(); MAX_EARLY_ALLOCS],
            early_count: AtomicUsize::new(0),
            early_current: AtomicU64::new(0),
            total_allocs: AtomicU64::new(0),
            total_frees: AtomicU64::new(0),
            failed_allocs: AtomicU64::new(0),
        }
    }

    /// Initialize the PMM with memory size and kernel end address
    pub fn init(&mut self, mem_size: u64, kernel_end: u64) {
        self.mem_size = mem_size;
        self.kernel_end = kernel_end;
        
        let total_pages = mem_size / PAGE_SIZE;
        
        let start = (kernel_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.store(start, Ordering::Relaxed);
        
        self.info.total_pages = total_pages;
        self.info.kernel_end = kernel_end;
        
        klog_pmm!("[PMM] Initialized: {} MB, {} pages, kernel ends at 0x{:X}",
                       mem_size / (1024 * 1024), total_pages, kernel_end);
    }

    /// Initialize the bitmap for normal operation
    /// `reserved_after_kernel`: memory already in use AFTER kernel (e.g. kmalloc heap),
    /// the bitmap will skip this range and mark it as used.
    pub fn init_bitmap(&mut self, reserved_after_kernel: u64) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }

        // Advance early_current past the reserved (heap) region
        let reserved_aligned = (reserved_after_kernel + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.fetch_add(reserved_aligned, Ordering::Relaxed);

        let total_bits = self.info.total_pages as usize;
        let bitmap_words = (total_bits + 31) / 32;
        let bitmap_bytes = bitmap_words * 4;
        
        let bitmap_phys = self.early_current.fetch_add(bitmap_bytes as u64, Ordering::Relaxed);
        let bitmap_aligned = (bitmap_phys + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.store(bitmap_aligned + bitmap_bytes as u64 + PAGE_SIZE, Ordering::Relaxed);
        
        let bitmap_virt = bitmap_aligned + KERNEL_BASE;
        
        unsafe {
            core::ptr::write_bytes(bitmap_virt as *mut u8, 0, bitmap_bytes);
        }
        
        self.bitmap = Some(NonNull::new(bitmap_virt as *mut u32).unwrap());
        self.bitmap_size = bitmap_words;
        
        // Mark kernel pages + reserved (heap) pages as used
        let kernel_pages = ((self.kernel_end + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        let reserved_pages = (reserved_aligned / PAGE_SIZE) as usize;
        let total_reserved = kernel_pages + reserved_pages;
        for i in 0..total_reserved.min(total_bits) {
            self.set_bit(i);
        }
        
        // Mark bitmap pages themselves as used (they live after the heap)
        let bitmap_start_page = (bitmap_aligned / PAGE_SIZE) as usize;
        let bitmap_pages = ((bitmap_bytes as u64 + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        for i in bitmap_start_page..(bitmap_start_page + bitmap_pages).min(total_bits) {
            self.set_bit(i);
        }
        
        self.initialized.store(true, Ordering::Release);
        
        self.update_stats();
        
        klog_pmm!("[PMM] Bitmap initialized: {} total, {} free ({} MB), reserved {} pages, bmp @0x{:X}",
                       self.info.total_pages, self.info.free_pages,
                       self.info.free_pages * 4 / 1024,
                       total_reserved, bitmap_aligned);
    }

    /// Allocate a single 4KB physical page
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

    /// Free a single physical page
    pub fn free_page(&self, addr: PhysAddr) {
        if addr.0 == 0 { return; }
        
        self.acquire_lock();
        
        if !self.initialized.load(Ordering::Acquire) {
            klog_pmm!("[PMM] Warning: Cannot free page at 0x{:X} before bitmap init", addr.0);
            self.release_lock();
            return;
        }
        
        let page_num = addr.0 / PAGE_SIZE;
        
        if page_num >= self.info.total_pages {
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

    /// Allocate multiple contiguous physical pages
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

    /// Free multiple contiguous physical pages
    pub fn free_pages(&self, addr: PhysAddr, count: usize) {
        if addr.0 == 0 || count == 0 { return; }
        
        self.acquire_lock();
        
        if !self.initialized.load(Ordering::Acquire) {
            klog_pmm!("[PMM] Warning: Cannot free pages before bitmap init");
            self.release_lock();
            return;
        }
        
        let start_page = addr.0 / PAGE_SIZE;
        
        for i in 0..count {
            let page_num = start_page + i as u64;
            
            if page_num >= self.info.total_pages { break; }
            
            if !self.test_bit(page_num as usize) {
                klog_pmm!("[PMM] Warning: Double free at page {}", page_num);
                continue;
            }
            
            self.clear_bit(page_num as usize);
        }
        
        self.total_frees.fetch_add(count as u64, Ordering::Relaxed);
        self.release_lock();
    }

    /// Allocate a huge page (2MB or 1GB)
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

    /// Free a huge page
    pub fn free_huge_page(&self, addr: PhysAddr, size_type: PageSize) {
        match size_type {
            PageSize::Size4K => self.free_page(addr),
            _ => {
                let num_4k_pages = (size_type.size() / PAGE_SIZE) as usize;
                self.free_pages(addr, num_4k_pages);
            }
        }
    }

    /// Check if an address is properly aligned for a given page size
    pub fn is_aligned_for_huge(&self, addr: PhysAddr, size_type: PageSize) -> bool {
        size_type.is_aligned(addr.0)
    }

    /// Get number of free pages
    pub fn get_free_pages(&self) -> u64 {
        self.info.free_pages
    }

    /// Get total number of pages
    pub fn get_total_pages(&self) -> u64 {
        self.info.total_pages
    }

    /// Get number of used pages
    pub fn get_used_pages(&self) -> u64 {
        self.info.used_pages
    }

    /// Get memory information structure
    pub fn get_info(&self) -> MemoryInfo {
        self.info
    }

    /// Print PMM statistics
    pub fn dump_stats(&self) {
        klog_pmm!("=== PMM Statistics ===");
        klog_pmm!("Total Memory: {} MB", self.mem_size / (1024 * 1024));
        klog_pmm!("Total Pages: {}", self.info.total_pages);
        klog_pmm!("Free Pages:  {}", self.info.free_pages);
        klog_pmm!("Used Pages:  {}", self.info.used_pages);
        klog_pmm!("Kernel End:  0x{:X}", self.info.kernel_end);
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

    /// Set a bit in the bitmap (mark page as used)
    fn set_bit(&self, bit: usize) {
        if let Some(bitmap_ptr) = self.bitmap {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let word_index = bit / 32;
                let bit_index = bit % 32;
                
                if word_index < self.bitmap_size {
                    let ptr = bitmap.add(word_index);
                    let value = ptr.read_volatile();
                    ptr.write_volatile(value | (1u32 << bit_index));
                }
            }
        }
    }

    /// Clear a bit in the bitmap (mark page as free)
    fn clear_bit(&self, bit: usize) {
        if let Some(bitmap_ptr) = self.bitmap {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let word_index = bit / 32;
                let bit_index = bit % 32;
                
                if word_index < self.bitmap_size {
                    let ptr = bitmap.add(word_index);
                    let value = ptr.read_volatile();
                    ptr.write_volatile(value & !(1u32 << bit_index));
                }
            }
        }
    }

    /// Test if a bit is set (page is used)
    fn test_bit(&self, bit: usize) -> bool {
        if let Some(bitmap_ptr) = self.bitmap {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let word_index = bit / 32;
                let bit_index = bit % 32;
                
                if word_index < self.bitmap_size {
                    let value = bitmap.add(word_index).read_volatile();
                    (value & (1u32 << bit_index)) != 0
                } else {
                    false
                }
            }
        } else {
            false
        }
    }

    /// Find first zero bit (free page) starting from given position
    fn find_first_free(&self, start: usize) -> Option<usize> {
        if let Some(bitmap_ptr) = self.bitmap {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let total_bits = self.bitmap_size * 32;
                
                for i in start..total_bits {
                    let word_index = i / 32;
                    let bit_index = i % 32;
                    
                    let value = bitmap.add(word_index).read_volatile();
                    if (value & (1u32 << bit_index)) == 0 {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    /// Find contiguous free block of given size
    fn find_contiguous_free(&self, count: usize, align: u64) -> Option<usize> {
        if let Some(bitmap_ptr) = self.bitmap {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let total_bits = self.bitmap_size * 32;
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
                    let value = bitmap.add(word_index).read_volatile();
                    
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

    /// Allocate from bitmap (normal operation)
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

    /// Allocate with specific alignment requirement
    fn alloc_aligned(&self, count: usize, alignment: u64) -> Option<PhysAddr> {
        let align_pages = (alignment / PAGE_SIZE) as usize;
        
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

    /// Early allocation (before bitmap init) - single page
    fn early_alloc_single(&self) -> Option<PhysAddr> {
        let current = self.early_current.fetch_add(PAGE_SIZE, Ordering::Relaxed);
        let aligned = (current + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        
        self.early_current.store(aligned + PAGE_SIZE, Ordering::Relaxed);
        
        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let alloc_ptr = self.early_allocs.as_ptr().add(idx) as *mut EarlyAlloc;
                (*alloc_ptr).addr = aligned;
                (*alloc_ptr).size = PAGE_SIZE;
            }
        }
        
        if aligned >= self.mem_size {
            klog_pmm!("[PMM] Error: Early allocation out of memory!");
            return None;
        }
        
        Some(PhysAddr(aligned))
    }

    /// Early allocation - multiple pages
    fn early_alloc_multiple(&self, count: u64) -> Option<PhysAddr> {
        let size = count * PAGE_SIZE;
        let current = self.early_current.fetch_add(size, Ordering::Relaxed);
        let aligned = (current + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        
        self.early_current.store(aligned + size, Ordering::Relaxed);
        
        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let alloc_ptr = self.early_allocs.as_ptr().add(idx) as *mut EarlyAlloc;
                (*alloc_ptr).addr = aligned;
                (*alloc_ptr).size = size;
            }
        }
        
        if aligned + size > self.mem_size {
            klog_pmm!("[PMM] Error: Early multi-page alloc out of memory!");
            return None;
        }
        
        Some(PhysAddr(aligned))
    }

    /// Early allocation - huge page with alignment
    fn early_alloc_huge(&self, size_type: PageSize) -> Option<PhysAddr> {
        let size = size_type.size();
        let current = self.early_current.load(Ordering::Relaxed);
        
        let aligned = (current + size - 1) & !(size - 1);
        
        self.early_current.store(aligned + size, Ordering::Relaxed);
        
        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let alloc_ptr = self.early_allocs.as_ptr().add(idx) as *mut EarlyAlloc;
                (*alloc_ptr).addr = aligned;
                (*alloc_ptr).size = size;
            }
        }
        
        if aligned + size > self.mem_size {
            klog_pmm!("[PMM] Error: Early huge page alloc out of memory!");
            return None;
        }
        
        Some(PhysAddr(aligned))
    }

    /// Update memory statistics
    fn update_stats(&self) {
        if let Some(bitmap_ptr) = self.bitmap {
            unsafe {
                let bitmap = bitmap_ptr.as_ptr();
                let mut used_count: u64 = 0;
                
                for i in 0..self.bitmap_size {
                    let value = bitmap.add(i).read_volatile();
                    used_count += value.count_ones() as u64;
                }
                
                // Use raw pointer mutation for stats update
                let info_ptr = &self.info as *const MemoryInfo as *mut MemoryInfo;
                (*info_ptr).used_pages = used_count;
                (*info_ptr).free_pages = self.info.total_pages - used_count;
            }
        }
    }
}

// Global PMM instance (using static mutable pattern for no_std)
static mut GLOBAL_PMM: PhysicalMemoryManager = PhysicalMemoryManager::new();

/// Get reference to global PMM instance
pub fn get_pmm() -> &'static PhysicalMemoryManager {
    unsafe { &GLOBAL_PMM }
}

/// Get mutable reference to global PMM instance (for init operations)
///
/// # Safety
/// This function should only be called during kernel initialization
pub unsafe fn get_pmm_mut() -> &'static mut PhysicalMemoryManager {
    &mut GLOBAL_PMM
}
