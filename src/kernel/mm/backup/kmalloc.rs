//! Kernel Heap Allocator (Kmalloc)
//!
//! Provides dynamic memory allocation for the kernel using a first-fit algorithm.
//! Features:
//! - First-fit free list allocation
//! - Block coalescing to reduce fragmentation
//! - Automatic heap expansion via VMM/PMM
//! - Early boot heap support
//! - Memory statistics and debugging

/// Serial print macro (placeholder)
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

use super::*;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, Ordering};
use core::cell::UnsafeCell;

/// Magic number for heap header validation
const HEAP_MAGIC: u32 = 0xDEADBEEF;

/// Minimum block size (to fit header + some data)
const MIN_BLOCK_SIZE: u64 = 64;

/// Alignment for allocations (must be power of 2)
const ALIGNMENT: u64 = 16;

/// Maximum early allocations to track
const MAX_EARLY_ALLOCS: usize = 128;

/// Early allocation entry
#[derive(Clone, Copy)]
struct EarlyHeapAlloc {
    ptr: *mut u8,
    size: usize,
}

impl EarlyHeapAlloc {
    pub const fn const_default() -> Self {
        Self { ptr: core::ptr::null_mut(), size: 0 }
    }
}

/// Heap header structure (matches C layout)
///
/// This structure is placed at the beginning of each allocated block,
/// both in use and free blocks. For free blocks, it's part of a doubly-linked list.
#[repr(C)]
pub struct HeapHeader {
    /// Size of this block (including header)
    size: u64,
    
    /// Is this block free? (1 = free, 0 = allocated)
    free: bool,
    
    /// Magic number for validation
    magic: u32,
    
    /// Pointer to next free block (only valid if free)
    next: *mut HeapHeader,
    
    /// Pointer to previous free block (only valid if free)
    prev: *mut HeapHeader,
}

impl HeapHeader {
    pub fn new(size: u64, is_free: bool) -> Self {
        Self {
            size,
            free: is_free,
            magic: HEAP_MAGIC,
            next: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
        }
    }

    /// Get pointer to data area after this header
    pub fn data_ptr(&self) -> *mut u8 {
        unsafe { (self as *const Self as *mut u8).add(core::mem::size_of::<Self>()) }
    }

    /// Get header from data pointer
    ///
    /// # Safety
    /// Caller must ensure `data` was returned by a valid allocation
    pub unsafe fn from_data_ptr(data: *mut u8) -> *mut Self {
        data.sub(core::mem::size_of::<Self>()) as *mut Self
    }
}

/// Kernel Heap Allocator state
pub struct KernelHeap {
    /// Start of heap region (virtual address)
    heap_start: VirtAddr,
    
    /// Current end of heap (virtual address)
    heap_end: VirtAddr,
    
    /// Head of free list
    free_list_head: UnsafeCell<*mut HeapHeader>,
    
    /// Lock for thread safety
    lock: AtomicBool,
    
    /// Is the heap initialized?
    initialized: AtomicBool,
    
    /// Early allocations before proper initialization
    early_allocs: [EarlyHeapAlloc; MAX_EARLY_ALLOCS],
    
    /// Number of early allocations
    early_count: AtomicUsize,
    
    /// Early heap buffer (static buffer for pre-init allocations)
    early_buffer: [u8; EARLY_BUFFER_SIZE],
    
    /// Current position in early buffer
    early_pos: AtomicUsize,
    
    /// Statistics
    total_allocated: AtomicU64,
    total_freed: AtomicU64,
    current_usage: AtomicU64,
    peak_usage: AtomicU64,
    alloc_count: AtomicU64,
    free_count: AtomicU64,
    failed_allocs: AtomicU64,
}

const EARLY_BUFFER_SIZE: usize = 4096;

impl KernelHeap {
    pub const fn new() -> Self {
        Self {
            heap_start: VirtAddr(0),
            heap_end: VirtAddr(0),
            free_list_head: UnsafeCell::new(core::ptr::null_mut()),
            lock: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            early_allocs: [EarlyHeapAlloc::const_default(); MAX_EARLY_ALLOCS],
            early_count: AtomicUsize::new(0),
            early_buffer: [0u8; EARLY_BUFFER_SIZE],
            early_pos: AtomicUsize::new(0),
            total_allocated: AtomicU64::new(0),
            total_freed: AtomicU64::new(0),
            current_usage: AtomicU64::new(0),
            peak_usage: AtomicU64::new(0),
            alloc_count: AtomicU64::new(0),
            free_count: AtomicU64::new(0),
            failed_allocs: AtomicU64::new(0),
        }
    }

    /// Initialize the kernel heap
    pub fn init(&mut self, start: VirtAddr, initial_size: u64) {
        self.heap_start = start;
        self.heap_end = VirtAddr(start.0 + initial_size);
        
        let header = unsafe { &mut *(start.0 as *mut HeapHeader) };
        *header = HeapHeader::new(initial_size, true);
        
        unsafe { *self.free_list_head.get() = header; }
        
        self.initialized.store(true, Ordering::Release);
        
        serial_println!("[Kmalloc] Initialized: start=0x{:X}, size={} KB",
                       start.0, initial_size / 1024);
        
        self.process_early_allocations();
    }

    /// Allocate memory from kernel heap
    pub fn allocate(&self, size: usize) -> Option<*mut u8> {
        if size == 0 { return None; }
        
        let aligned_size = align_up(size as u64, ALIGNMENT);
        let total_size = aligned_size + core::mem::size_of::<HeapHeader>() as u64;
        let actual_size = total_size.max(MIN_BLOCK_SIZE);
        
        self.acquire_lock();
        
        let result = if !self.initialized.load(Ordering::Acquire) {
            self.early_allocate(actual_size as usize)
        } else {
            self.allocate_first_fit(actual_size)
        };
        
        match result {
            Some(ptr) => {
                self.alloc_count.fetch_add(1, Ordering::Relaxed);
                self.total_allocated.fetch_add(actual_size, Ordering::Relaxed);
                let usage = self.current_usage.fetch_add(actual_size, Ordering::Relaxed) + actual_size;
                
                let mut peak = self.peak_usage.load(Ordering::Relaxed);
                while usage > peak {
                    match self.peak_usage.compare_exchange_weak(
                        peak, usage,
                        Ordering::Relaxed,
                        Ordering::Relaxed
                    ) {
                        Ok(_) => break,
                        Err(p) => peak = p,
                    }
                }
                
                self.release_lock();
                Some(ptr)
            }
            None => {
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                self.release_lock();
                None
            }
        }
    }

    /// Free memory previously allocated by k_malloc
    pub fn deallocate(&self, ptr: *mut u8) {
        if ptr.is_null() { return; }
        
        self.acquire_lock();
        
        if !self.initialized.load(Ordering::Acquire) {
            serial_println!("[Kmalloc] Warning: Cannot free before initialization");
            self.release_lock();
            return;
        }
        
        let header = unsafe { HeapHeader::from_data_ptr(ptr) };
        
        unsafe {
            let h = &*header;
            if h.magic != HEAP_MAGIC {
                serial_println!("[Kmalloc] Error: Invalid magic (got 0x{:X}, expected 0x{:X})",
                               h.magic, HEAP_MAGIC);
                self.release_lock();
                return;
            }
            
            if h.free {
                serial_println!("[Kmalloc] Warning: Double free detected");
                self.release_lock();
                return;
            }
            
            (*header).free = true;
        }
        
        let freed_size = unsafe { (*header).size };
        self.free_count.fetch_add(1, Ordering::Relaxed);
        self.total_freed.fetch_add(freed_size, Ordering::Relaxed);
        self.current_usage.fetch_sub(freed_size, Ordering::Relaxed);
        
        self.coalesce(header);
        self.add_to_free_list(header);
        
        self.release_lock();
    }

    /// Reallocate memory block
    pub fn reallocate(&self, ptr: *mut u8, size: usize) -> Option<*mut u8> {
        if size == 0 {
            self.deallocate(ptr);
            return None;
        }
        
        if ptr.is_null() { return self.allocate(size); }
        
        let header = unsafe { HeapHeader::from_data_ptr(ptr) };
        let old_size = unsafe { (*header).size - core::mem::size_of::<HeapHeader>() as u64 };
        
        let new_aligned = align_up(size as u64, ALIGNMENT);
        if new_aligned <= old_size { return Some(ptr); }
        
        let new_ptr = self.allocate(size)?;
        
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, old_size as usize);
        }
        
        self.deallocate(ptr);
        
        Some(new_ptr)
    }

    /// Get heap statistics
    pub fn get_stats(&self) -> HeapStats {
        HeapStats {
            heap_start: self.heap_start,
            heap_end: self.heap_end,
            total_allocated: self.total_allocated.load(Ordering::Relaxed),
            total_freed: self.total_freed.load(Ordering::Relaxed),
            current_usage: self.current_usage.load(Ordering::Relaxed),
            peak_usage: self.peak_usage.load(Ordering::Relaxed),
            alloc_count: self.alloc_count.load(Ordering::Relaxed),
            free_count: self.free_count.load(Ordering::Relaxed),
            failed_allocs: self.failed_allocs.load(Ordering::Relaxed),
        }
    }

    /// Print heap statistics
    pub fn dump_stats(&self) {
        let _stats = self.get_stats();
        serial_println!("=== Kmalloc Statistics ===");
        serial_println!("Heap Range:     0x{:X} - 0x{:X}", stats.heap_start.0, stats.heap_end.0);
        serial_println!("Total Allocated: {} KB", stats.total_allocated / 1024);
        serial_println!("Total Freed:    {} KB", stats.total_freed / 1024);
        serial_println!("Current Usage:   {} KB", stats.current_usage / 1024);
        serial_println!("Peak Usage:      {} KB", stats.peak_usage / 1024);
        serial_println!("Alloc Count:    {}", stats.alloc_count);
        serial_println!("Free Count:     {}", stats.free_count);
        serial_println!("Failed Allocs:  {}", stats.failed_allocs);
        serial_println!("=========================");
    }

    /// Validate heap integrity (for debugging)
    pub fn validate(&self) -> bool {
        if !self.initialized.load(Ordering::Acquire) { return true; }
        
        self.acquire_lock();
        
        let mut count = 0usize;
        let mut current = unsafe { *self.free_list_head.get() };
        
        while !current.is_null() {
            unsafe {
                let header = &*current;
                
                if header.magic != HEAP_MAGIC {
                    serial_println!("[Kmalloc] Validate: Bad magic at {:p}", header);
                    self.release_lock();
                    return false;
                }
                
                if !header.free {
                    serial_println!("[Kmalloc] Validate: Non-free block in free list");
                    self.release_lock();
                    return false;
                }
                
                if !header.next.is_null() {
                    let next_header = &*header.next;
                    if !next_header.prev.is_null() && next_header.prev != current {
                        serial_println!("[Kmalloc] Validate: Broken backward link");
                        self.release_lock();
                        return false;
                    }
                }
                
                count += 1;
                current = header.next;
                
                if count > 10000 {
                    serial_println!("[Kmalloc] Validate: Too many nodes (possible cycle)");
                    self.release_lock();
                    return false;
                }
            }
        }
        
        self.release_lock();
        true
    }

    // ==================== Private Methods ====================

    /// First-fit allocation algorithm
    fn allocate_first_fit(&self, size: u64) -> Option<*mut u8> {
        let mut current = unsafe { *self.free_list_head.get() };
        
        while !current.is_null() {
            unsafe {
                let header = current;
                let block_size = (*header).size;
                
                if block_size >= size {
                    if block_size >= size + MIN_BLOCK_SIZE + core::mem::size_of::<HeapHeader>() as u64 {
                        self.split_block(header, size);
                    }
                    
                    (*header).free = false;
                    self.remove_from_free_list(header);
                    
                    return Some((*header).data_ptr());
                }
                
                current = (*header).next;
            }
        }
        
        self.expand_heap(size)
    }

    /// Split a block into two parts
    unsafe fn split_block(&self, header: *mut HeapHeader, size: u64) {
        let original_size = (*header).size;
        let remaining = original_size - size;
        
        let second_part = (header as *mut u8).add(size as usize) as *mut HeapHeader;
        *second_part = HeapHeader::new(remaining, true);
        
        (*header).size = size;
        
        if !(*header).next.is_null() {
            (*second_part).next = (*header).next;
            (*(*header).next).prev = second_part;
        }
        (*second_part).prev = header;
        (*header).next = second_part;
    }

    /// Coalesce adjacent free blocks
    fn coalesce(&self, header: *mut HeapHeader) {
        unsafe {
            self.coalesce_forward(header);
            self.coalesce_backward(header);
        }
    }

    /// Coalesce with next block if it's free
    unsafe fn coalesce_forward(&self, header: *mut HeapHeader) {
        let next_addr = (header as *mut u8).add((*header).size as usize) as *mut HeapHeader;
        let heap_end = self.heap_end.0 as *mut u8;
        
        if (next_addr as *mut u8) < heap_end {
            let next_header = &*next_addr;
            
            if next_header.magic == HEAP_MAGIC && next_header.free {
                self.remove_from_free_list(next_addr);
                
                (*header).size += next_header.size;
                
                if !next_header.next.is_null() {
                    (*header).next = next_header.next;
                    (*(*header).next).prev = header;
                } else {
                    (*header).next = core::ptr::null_mut();
                }
            }
        }
    }

    /// Coalesce with previous block if it's free
    unsafe fn coalesce_backward(&self, header: *mut HeapHeader) {
        if !(*header).prev.is_null() {
            let prev = (*header).prev;
            let expected_prev_end = (prev as *mut u8).add((*prev).size as usize);
            
            if expected_prev_end == (header as *mut u8) {
                self.remove_from_free_list(prev);
                
                (*prev).size += (*header).size;
                
                if !(*header).next.is_null() {
                    (*prev).next = (*header).next;
                    (*(*header).next).prev = prev;
                }
            }
        }
    }

    /// Add a block to the free list
    fn add_to_free_list(&self, header: *mut HeapHeader) {
        unsafe {
            let head_ptr = self.free_list_head.get();
            
            if !(*head_ptr).is_null() {
                (*header).next = *head_ptr;
                (*(*head_ptr)).prev = header;
            } else {
                (*header).next = core::ptr::null_mut();
            }
            (*header).prev = core::ptr::null_mut();
            *head_ptr = header;
        }
    }

    /// Remove a block from the free list
    fn remove_from_free_list(&self, header: *mut HeapHeader) {
        unsafe {
            let prev = (*header).prev;
            let next = (*header).next;
            let head_ptr = self.free_list_head.get();
            
            if !prev.is_null() {
                (*prev).next = next;
            } else {
                *head_ptr = next;
            }
            
            if !next.is_null() {
                (*next).prev = prev;
            }
            
            (*header).next = core::ptr::null_mut();
            (*header).prev = core::ptr::null_mut();
        }
    }

    /// Expand the heap by requesting more pages from VMM/PMM
    fn expand_heap(&self, size: u64) -> Option<*mut u8> {
        let pages_needed = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        let expand_by = pages_needed * PAGE_SIZE;
        
        let vmm = get_vmm();
        let pmm = get_pmm();
        
        let phys = pmm.alloc_pages(pages_needed as usize)?;
        
        let new_start = self.heap_end;
        let _new_end = VirtAddr(self.heap_end.0 + expand_by);
        
        for i in 0..pages_needed {
            let page_phys = PhysAddr(phys.as_u64() + i * PAGE_SIZE);
            let page_virt = VirtAddr(new_start.0 + i * PAGE_SIZE);
            
            if vmm.map_page(page_virt, page_phys, PageFlags::PRESENT | PageFlags::WRITABLE).is_err() {
                for j in 0..i {
                    let unmap_virt = VirtAddr(new_start.0 + j * PAGE_SIZE);
                    vmm.unmap_page(unmap_virt);
                }
                pmm.free_pages(phys, pages_needed as usize);
                return None;
            }
        }
        
        let new_block = unsafe { &mut *(new_start.0 as *mut HeapHeader) };
        *new_block = HeapHeader::new(expand_by, true);
        
        self.add_to_free_list(new_block as *mut HeapHeader);
        
        self.allocate_first_fit(size)
    }

    /// Early allocation (before heap initialization)
    fn early_allocate(&self, size: usize) -> Option<*mut u8> {
        let current = self.early_pos.fetch_add(size, Ordering::Relaxed);
        
        if current + size > self.early_buffer.len() {
            serial_println!("[Kmalloc] Error: Early allocation out of space!");
            self.early_pos.fetch_sub(size, Ordering::Relaxed);
            return None;
        }
        
        let ptr = unsafe { self.early_buffer.as_ptr().offset(current as isize) as *mut u8 };
        
        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let alloc_ptr = self.early_allocs.as_ptr().add(idx) as *mut EarlyHeapAlloc;
                (*alloc_ptr).ptr = ptr;
                (*alloc_ptr).size = size;
            }
        }
        
        Some(ptr)
    }

    /// Process early allocations after proper initialization
    fn process_early_allocations(&self) {
        let count = self.early_count.load(Ordering::Acquire);
        
        for i in 0..count.min(MAX_EARLY_ALLOCS) {
            let alloc = self.early_allocs[i];
            
            if let Some(new_ptr) = self.allocate_first_fit(alloc.size as u64) {
                unsafe {
                    core::ptr::copy_nonoverlapping(alloc.ptr, new_ptr, alloc.size);
                }
            }
        }
    }

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
}

/// Heap statistics structure
#[derive(Debug, Clone, Copy)]
pub struct HeapStats {
    pub heap_start: VirtAddr,
    pub heap_end: VirtAddr,
    pub total_allocated: u64,
    pub total_freed: u64,
    pub current_usage: u64,
    pub peak_usage: u64,
    pub alloc_count: u64,
    pub free_count: u64,
    pub failed_allocs: u64,
}

// Global Kmalloc instance
static mut GLOBAL_KMALLOC: KernelHeap = KernelHeap::new();

/// Get reference to global Kmalloc instance
pub fn get_kmalloc() -> &'static KernelHeap {
    unsafe { &GLOBAL_KMALLOC }
}

/// Get mutable reference to global Kmalloc instance (for init operations)
///
/// # Safety
/// Should only be called during kernel initialization
pub unsafe fn get_kmalloc_mut() -> &'static mut KernelHeap {
    &mut GLOBAL_KMALLOC
}

// ==================== Helper Functions ====================

/// Align value up to given alignment (must be power of 2)
#[inline(always)]
pub const fn align_up(value: u64, alignment: u64) -> u64 {
    (value + alignment - 1) & !(alignment - 1)
}

/// Align value down to given alignment (must be power of 2)
#[inline(always)]
pub const fn align_down(value: u64, alignment: u64) -> u64 {
    value & !(alignment - 1)
}
