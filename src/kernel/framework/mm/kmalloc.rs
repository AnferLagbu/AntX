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
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

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
        Self {
            ptr: core::ptr::null_mut(),
            size: 0,
        }
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
        // SAFETY: self is a valid reference to HeapHeader; adding size_of::<Self>()
        // stays within the same allocation block (header is followed by data).
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

// === E4: Unsafe Concentration — raw sub-module ===
//
// All bare-pointer dereferences for heap header internals are
// encapsulated here.  The outer `KernelHeap` methods call only safe
// wrappers, keeping the allocation algorithm logic itself safe Rust.
pub(crate) mod raw {
    use super::*;

    /// Safe wrapper around a `*mut HeapHeader`.
    ///
    /// SAFETY invariant: the pointer points to a valid HeapHeader inside
    /// the heap region, and the heap lock is held.
    #[derive(Clone, Copy)]
    pub struct HeaderRef(*mut HeapHeader);

    impl HeaderRef {
        /// # Safety
        /// - `ptr` must point to a valid `HeapHeader` in the heap
        /// - Heap lock must be held for the duration of use
        #[inline(always)]
        pub unsafe fn new_unchecked(ptr: *mut HeapHeader) -> Self {
            Self(ptr)
        }

        /// Construct from a data pointer returned by allocate.
        ///
        /// # Safety
        /// - `data` must have been returned by a prior allocation
        /// - Heap lock must be held
        #[inline(always)]
        pub unsafe fn from_data_ptr(data: *mut u8) -> Self {
            Self(HeapHeader::from_data_ptr(data))
        }

        #[inline(always)]
        pub fn as_ptr(self) -> *mut HeapHeader {
            self.0
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn is_null(self) -> bool {
            self.0.is_null()
        }

        #[inline(always)]
        pub fn size(&self) -> u64 {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).size }
        }

        #[inline(always)]
        pub fn set_size(&self, val: u64) {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).size = val; }
        }

        #[inline(always)]
        pub fn is_free(&self) -> bool {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).free }
        }

        #[inline(always)]
        pub fn set_free(&self, val: bool) {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).free = val; }
        }

        #[inline(always)]
        pub fn magic(&self) -> u32 {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).magic }
        }

        #[inline(always)]
        pub fn next(&self) -> *mut HeapHeader {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).next }
        }

        #[inline(always)]
        pub fn set_next(&self, p: *mut HeapHeader) {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).next = p; }
        }

        #[inline(always)]
        pub fn prev(&self) -> *mut HeapHeader {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).prev }
        }

        #[inline(always)]
        pub fn set_prev(&self, p: *mut HeapHeader) {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).prev = p; }
        }

        #[inline(always)]
        pub fn data_ptr(&self) -> *mut u8 {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { (*self.0).data_ptr() }
        }

        /// Write a new HeapHeader value at this location.
        #[inline(always)]
        pub fn write(&self, val: HeapHeader) {
            // SAFETY: caller guarantees valid pointer under heap lock
            unsafe { *self.0 = val; }
        }

        /// Get the address of this header as a byte pointer.
        #[inline(always)]
        pub fn byte_ptr(self) -> *mut u8 {
            self.0 as *mut u8
        }

        /// Compute the next adjacent header by byte offset.
        #[inline(always)]
        pub fn adjacent_next(&self, offset: usize) -> Self {
            // SAFETY: caller guarantees offset stays within heap region
            unsafe { Self::new_unchecked(self.byte_ptr().add(offset) as *mut HeapHeader) }
        }
    }

    /// Safe wrapper for free_list_head access.
    pub struct FreeListHeadRef<'a> {
        ptr: &'a UnsafeCell<*mut HeapHeader>,
    }

    impl<'a> FreeListHeadRef<'a> {
        #[inline(always)]
        pub fn new(ptr: &'a UnsafeCell<*mut HeapHeader>) -> Self {
            Self { ptr }
        }

        #[inline(always)]
        pub fn get(&self) -> *mut HeapHeader {
            // SAFETY: heap lock is held
            unsafe { *self.ptr.get() }
        }

        #[inline(always)]
        pub fn set(&self, val: *mut HeapHeader) {
            // SAFETY: heap lock is held
            unsafe { *self.ptr.get() = val; }
        }
    }

    /// Zero a memory region.
    ///
    /// # Safety
    /// - `ptr` must point to a valid writable region of `len` bytes
    #[inline(always)]
    pub unsafe fn zero_memory(ptr: *mut u8, len: usize) {
        core::ptr::write_bytes(ptr, 0, len);
    }

    /// Copy memory non-overlapping.
    ///
    /// # Safety
    /// - src must be readable for `len` bytes
    /// - dst must be writable for `len` bytes
    /// - regions must not overlap
    #[inline(always)]
    pub unsafe fn copy_nonoverlapping(src: *const u8, dst: *mut u8, len: usize) {
        core::ptr::copy_nonoverlapping(src, dst, len);
    }
}

use raw::{HeaderRef, FreeListHeadRef};

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

        // SAFETY: caller (kmem_init) provides a valid mapped heap region of
        // size >= sizeof(HeapHeader); start is page-aligned and exclusive.
        let header = unsafe { HeaderRef::new_unchecked(start.0 as *mut HeapHeader) };
        header.write(HeapHeader::new(initial_size, true));

        let head = FreeListHeadRef::new(&self.free_list_head);
        head.set(header.as_ptr());

        self.initialized.store(true, Ordering::Release);

        serial_println!(
            "[Kmalloc] Initialized: start=0x{:X}, size={} KB",
            start.0,
            initial_size / 1024
        );

        self.process_early_allocations();
    }

    /// Allocate memory from kernel heap
    pub fn allocate(&self, size: usize) -> Option<*mut u8> {
        if size == 0 {
            return None;
        }

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
                self.total_allocated
                    .fetch_add(actual_size, Ordering::Relaxed);
                let usage =
                    self.current_usage.fetch_add(actual_size, Ordering::Relaxed) + actual_size;

                let mut peak = self.peak_usage.load(Ordering::Relaxed);
                while usage > peak {
                    match self.peak_usage.compare_exchange_weak(
                        peak,
                        usage,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
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
        if ptr.is_null() {
            return;
        }

        self.acquire_lock();

        if !self.initialized.load(Ordering::Acquire) {
            serial_println!("[Kmalloc] Warning: Cannot free before initialization");
            self.release_lock();
            return;
        }

        // SAFETY: caller guarantees ptr was returned by a valid allocation
        let header = unsafe { HeaderRef::from_data_ptr(ptr) };

        if header.magic() != HEAP_MAGIC {
            serial_println!(
                "[Kmalloc] Error: Invalid magic (got 0x{:X}, expected 0x{:X})",
                header.magic(),
                HEAP_MAGIC
            );
            self.release_lock();
            return;
        }

        if header.is_free() {
            serial_println!("[Kmalloc] Warning: Double free detected");
            self.release_lock();
            return;
        }

        header.set_free(true);

        let freed_size = header.size();
        self.free_count.fetch_add(1, Ordering::Relaxed);
        self.total_freed.fetch_add(freed_size, Ordering::Relaxed);
        self.current_usage.fetch_sub(freed_size, Ordering::Relaxed);

        let effective = self.coalesce(header);
        self.add_to_free_list(effective);

        self.release_lock();
    }

    /// Reallocate memory block
    pub fn reallocate(&self, ptr: *mut u8, size: usize) -> Option<*mut u8> {
        if size == 0 {
            self.deallocate(ptr);
            return None;
        }

        if ptr.is_null() {
            return self.allocate(size);
        }

        self.acquire_lock();

        // SAFETY: ptr was returned by kmalloc; magic/size validated below
        let header = unsafe { HeaderRef::from_data_ptr(ptr) };
        if header.magic() != HEAP_MAGIC || header.is_free() {
            self.release_lock();
            return None;
        }

        let old_data_size = (header.size() - core::mem::size_of::<HeapHeader>() as u64) as usize;
        let new_aligned = align_up(size as u64, ALIGNMENT) as usize;

        if new_aligned <= old_data_size {
            self.release_lock();
            return Some(ptr);
        }

        // Allocate new block while holding the lock to prevent
        // ptr from being invalidated before copy completes
        let actual_size =
            (new_aligned as u64 + core::mem::size_of::<HeapHeader>() as u64).max(MIN_BLOCK_SIZE);
        let new_ptr = match self.allocate_first_fit(actual_size) {
            Some(p) => {
                self.alloc_count.fetch_add(1, Ordering::Relaxed);
                self.total_allocated
                    .fetch_add(actual_size, Ordering::Relaxed);
                let usage =
                    self.current_usage.fetch_add(actual_size, Ordering::Relaxed) + actual_size;
                let mut peak = self.peak_usage.load(Ordering::Relaxed);
                while usage > peak {
                    match self.peak_usage.compare_exchange_weak(
                        peak,
                        usage,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(p) => peak = p,
                    }
                }
                p
            }
            None => {
                self.release_lock();
                return None;
            }
        };

        // SAFETY: ptr is a valid pointer to old_data_size bytes; new_ptr is a
        // distinct allocation of the same size; regions cannot overlap.
        unsafe {
            raw::copy_nonoverlapping(ptr, new_ptr, old_data_size);
        }

        // Deallocate old block inline while holding the lock
        header.set_free(true);
        let freed_size = header.size();
        self.free_count.fetch_add(1, Ordering::Relaxed);
        self.total_freed.fetch_add(freed_size, Ordering::Relaxed);
        self.current_usage.fetch_sub(freed_size, Ordering::Relaxed);
        let effective = self.coalesce(header);
        self.add_to_free_list(effective);

        self.release_lock();
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
        serial_println!(
            "Heap Range:     0x{:X} - 0x{:X}",
            stats.heap_start.0,
            stats.heap_end.0
        );
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
        if !self.initialized.load(Ordering::Acquire) {
            return true;
        }

        self.acquire_lock();

        let mut count = 0usize;
        let head = FreeListHeadRef::new(&self.free_list_head);
        let mut current = head.get();

        while !current.is_null() {
            // SAFETY: current is a non-null *mut HeapHeader in the free list
            let cur = unsafe { HeaderRef::new_unchecked(current) };

            if cur.magic() != HEAP_MAGIC {
                serial_println!("[Kmalloc] Validate: Bad magic at {:p}", cur.as_ptr());
                self.release_lock();
                return false;
            }

            if !cur.is_free() {
                serial_println!("[Kmalloc] Validate: Non-free block in free list");
                self.release_lock();
                return false;
            }

            let next_ptr = cur.next();
            if !next_ptr.is_null() {
                let next = unsafe { HeaderRef::new_unchecked(next_ptr) };
                let next_prev = next.prev();
                if !next_prev.is_null() && next_prev != current {
                    serial_println!("[Kmalloc] Validate: Broken backward link");
                    self.release_lock();
                    return false;
                }
            }

            count += 1;
            current = next_ptr;

            if count > 10000 {
                serial_println!("[Kmalloc] Validate: Too many nodes (possible cycle)");
                self.release_lock();
                return false;
            }
        }

        self.release_lock();
        true
    }

    // ==================== Private Methods ====================

    /// First-fit allocation algorithm
    fn allocate_first_fit(&self, size: u64) -> Option<*mut u8> {
        let head = FreeListHeadRef::new(&self.free_list_head);
        let mut current = head.get();

        while !current.is_null() {
            // SAFETY: current is a non-null *mut HeapHeader in the free list
            let cur = unsafe { HeaderRef::new_unchecked(current) };
            let block_size = cur.size();

            if block_size >= size {
                if block_size
                    >= size + MIN_BLOCK_SIZE + core::mem::size_of::<HeapHeader>() as u64
                {
                    self.split_block(cur, size);
                }

                cur.set_free(false);
                self.remove_from_free_list(cur);

                return Some(cur.data_ptr());
            }

            current = cur.next();
        }

        self.expand_heap(size)
    }

    /// Split a block into two parts
    fn split_block(&self, header: HeaderRef, size: u64) {
        let original_size = header.size();
        let remaining = original_size - size;

        let second_part = header.adjacent_next(size as usize);
        second_part.write(HeapHeader::new(remaining, true));

        header.set_size(size);

        let header_next = header.next();
        if !header_next.is_null() {
            let hn = unsafe { HeaderRef::new_unchecked(header_next) };
            hn.set_prev(second_part.as_ptr());
        }
        second_part.set_next(header_next);
        second_part.set_prev(header.as_ptr());
        header.set_next(second_part.as_ptr());
    }

    /// Coalesce adjacent free blocks
    fn coalesce(&self, header: HeaderRef) -> HeaderRef {
        self.coalesce_forward(header);
        self.coalesce_backward(header)
    }

    fn coalesce_forward(&self, header: HeaderRef) {
        let next_addr = header.adjacent_next(header.size() as usize);
        let heap_end = self.heap_end.0 as *mut u8;

        if next_addr.byte_ptr() < heap_end {
            if next_addr.magic() == HEAP_MAGIC && next_addr.is_free() {
                self.remove_from_free_list(next_addr);
                header.set_size(header.size() + next_addr.size());
            }
        }
    }

    fn coalesce_backward(&self, header: HeaderRef) -> HeaderRef {
        let head = FreeListHeadRef::new(&self.free_list_head);
        let mut current = head.get();

        while !current.is_null() {
            // SAFETY: current is a valid node in the free list
            let candidate = unsafe { HeaderRef::new_unchecked(current) };
            let candidate_end = candidate.byte_ptr() as usize + candidate.size() as usize;

            if candidate_end == header.byte_ptr() as usize {
                self.remove_from_free_list(candidate);
                candidate.set_size(candidate.size() + header.size());
                candidate.set_free(true);
                return candidate;
            }

            current = candidate.next();
        }

        header
    }

    /// Add a block to the free list
    fn add_to_free_list(&self, header: HeaderRef) {
        let head = FreeListHeadRef::new(&self.free_list_head);
        let head_ptr = head.get();

        if !head_ptr.is_null() {
            let old_head = unsafe { HeaderRef::new_unchecked(head_ptr) };
            header.set_next(old_head.as_ptr());
            old_head.set_prev(header.as_ptr());
        } else {
            header.set_next(core::ptr::null_mut());
        }
        header.set_prev(core::ptr::null_mut());
        head.set(header.as_ptr());
    }

    /// Remove a block from the free list
    fn remove_from_free_list(&self, header: HeaderRef) {
        let prev = header.prev();
        let next = header.next();
        let head = FreeListHeadRef::new(&self.free_list_head);

        if !prev.is_null() {
            let p = unsafe { HeaderRef::new_unchecked(prev) };
            p.set_next(next);
        } else {
            head.set(next);
        }

        if !next.is_null() {
            let n = unsafe { HeaderRef::new_unchecked(next) };
            n.set_prev(prev);
        }

        header.set_next(core::ptr::null_mut());
        header.set_prev(core::ptr::null_mut());
    }

    /// Expand the heap by requesting more pages from VMM/PMM
    fn expand_heap(&self, size: u64) -> Option<*mut u8> {
        let pages_needed = size.div_ceil(PAGE_SIZE);
        let expand_by = pages_needed * PAGE_SIZE;

        let vmm = get_vmm();
        let pmm = get_pmm();

        let phys = pmm.alloc_pages(pages_needed as usize)?;

        let new_start = self.heap_end;
        let _new_end = VirtAddr(self.heap_end.0 + expand_by);

        for i in 0..pages_needed {
            let page_phys = PhysAddr(phys.as_u64() + i * PAGE_SIZE);
            let page_virt = VirtAddr(new_start.0 + i * PAGE_SIZE);

            if vmm
                .map_page(
                    page_virt,
                    page_phys,
                    PageFlags::PRESENT | PageFlags::WRITABLE,
                )
                .is_err()
            {
                for j in 0..i {
                    let unmap_virt = VirtAddr(new_start.0 + j * PAGE_SIZE);
                    vmm.unmap_page(unmap_virt);
                }
                pmm.free_pages(phys, pages_needed as usize);
                return None;
            }
        }

        // SAFETY: new pages are mapped writable by vmm.map_page above;
        // new_start is the mapped region start, exclusive access held under lock.
        let new_block = unsafe { HeaderRef::new_unchecked(new_start.0 as *mut HeapHeader) };
        new_block.write(HeapHeader::new(expand_by, true));

        self.add_to_free_list(new_block);

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

        // SAFETY: current is checked above to be within early_buffer bounds
        let ptr = unsafe { self.early_buffer.as_ptr().add(current) as *mut u8 };

        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            // SAFETY: idx < MAX_EARLY_ALLOCS guards the bounds
            unsafe {
                let alloc_ptr = self.early_allocs.as_ptr().add(idx) as *mut EarlyHeapAlloc;
                raw::zero_memory(alloc_ptr as *mut u8, core::mem::size_of::<EarlyHeapAlloc>());
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
                // SAFETY: early alloc ptr is valid, new_ptr is a distinct allocation
                unsafe {
                    raw::copy_nonoverlapping(alloc.ptr, new_ptr, alloc.size);
                }
            }
        }
    }

    #[inline(always)]
    fn acquire_lock(&self) {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
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
///
/// # Safety
/// GLOBAL_KMALLOC is a static initialized via `KernelHeap::new()` (const).
/// Reading a static reference is always safe.
pub fn get_kmalloc() -> &'static KernelHeap {
    // SAFETY: GLOBAL_KMALLOC is a static; the reference is valid for the
    // program lifetime, no aliasing concerns for shared access.
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
