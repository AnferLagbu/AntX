//! Physical Memory Manager (PMM) — Buddy Allocator
//!
//! Manages physical memory pages using a buddy allocator with O(1)
//! free-list operations and O(log n) merge-on-free.
//!
//! # Design
//! - Orders 0–9 (4 KB – 2 MB contiguous blocks).
//! - Early (linear) allocator until the buddy metadata is carved out.
//! - Bitmap retained for reserved-page tracking and statistics.
//! - Doubly-linked intrusive free lists (prev/next stored in free pages).
//! - Buddy-merge uses per-page order metadata for O(1) buddy checks.
//!
//! # Safety
//! All mutations happen under an internal `AtomicBool` spinlock.

macro_rules! klog_pmm {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

use super::*;
use crate::kernel::framework::sync_tcb_legacy::spinlock::{disable_interrupts, restore_interrupts, IrqSaveFlags};
use core::cell::{Cell, UnsafeCell};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

const MAX_EARLY_ALLOCS: usize = 256;

/// Maximum buddy order: 2^9 × 4 KB = 2 MB
const MAX_BUDDY_ORDER: u8 = 9;
/// Sentinel value in buddy_meta: page is allocated / not a free-list head
const BUDDY_ALLOCATED: u8 = 0xFF;

/// Physical RAM base address
/// x86_64: 0 (multiboot-provided physical memory starts at 0)
/// aarch64: 0x40000000 (QEMU virt machine RAM base)
#[cfg(target_arch = "x86_64")]
const RAM_BASE: u64 = 0;
#[cfg(target_arch = "aarch64")]
const RAM_BASE: u64 = 0x40000000;

#[inline(always)]
fn phys_to_page(phys: u64) -> u64 {
    (phys - RAM_BASE) / PAGE_SIZE
}

#[inline(always)]
fn page_to_phys(page: u64) -> u64 {
    RAM_BASE + page * PAGE_SIZE
}

#[inline(always)]
fn pfn_to_virt(pfn: u64) -> *mut u8 {
    (page_to_phys(pfn) + KERNEL_BASE) as *mut u8
}

/// Round count up to the next power of two → buddy order
#[inline]
fn count_to_order(count: usize) -> u8 {
    if count <= 1 {
        return 0;
    }
    // ceil(log2(count)) = floor(log2(count - 1)) + 1
    // = BITS - (count - 1).leading_zeros()
    let order = (usize::BITS - (count - 1).leading_zeros()) as u8;
    if order > MAX_BUDDY_ORDER {
        MAX_BUDDY_ORDER
    } else {
        order
    }
}

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

// ---- Intrusive doubly-linked free node stored inside free pages ----
// SAFETY: only accessed under the PMM lock; each free page provides 4096 B
// of storage — we use the first 16 B for prev / next pointers.
#[repr(C)]
struct FreeNode {
    prev: *mut FreeNode,
    next: *mut FreeNode,
}

pub struct PhysicalMemoryManager {
    // ---- Bitmap (reserved tracking + stats) ----
    bitmap: Cell<Option<NonNull<u32>>>,
    bitmap_size: Cell<usize>,
    mem_size: Cell<u64>,
    kernel_end: Cell<u64>,
    info: Cell<MemoryInfo>,
    // ---- Lock & lifecycle ----
    lock: AtomicBool,
    initialized: AtomicBool,
    buddy_ready: AtomicBool,
    // ---- Early (linear) allocator ----
    early_allocs: UnsafeCell<[EarlyAlloc; MAX_EARLY_ALLOCS]>,
    early_count: AtomicUsize,
    early_current: AtomicU64,
    // ---- Statistics ----
    total_allocs: AtomicU64,
    total_frees: AtomicU64,
    failed_allocs: AtomicU64,
    // ---- Buddy allocator ----
    /// Per-page order metadata: 0xFF = allocated, 0..9 = order of free block head
    buddy_meta: Cell<Option<NonNull<u8>>>,
    /// Doubly-linked free list heads, one per order
    buddy_heads: UnsafeCell<[*mut FreeNode; MAX_BUDDY_ORDER as usize + 1]>,
}

// SAFETY: PhysicalMemoryManager uses Cell/UnsafeCell for interior mutability.
// All public mutations go through pmm_alloc_pages/pmm_free_pages which
// acquire the internal lock (AtomicBool spinlock). The lock ensures
// mutual exclusion, so concurrent access from multiple threads is safe.
// buddy_heads is only accessed under the lock; bitmap/buddy_meta are
// set once during init and read-only afterwards.
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
            buddy_ready: AtomicBool::new(false),
            early_allocs: UnsafeCell::new([EarlyAlloc::const_default(); MAX_EARLY_ALLOCS]),
            early_count: AtomicUsize::new(0),
            early_current: AtomicU64::new(0),
            total_allocs: AtomicU64::new(0),
            total_frees: AtomicU64::new(0),
            failed_allocs: AtomicU64::new(0),
            buddy_meta: Cell::new(None),
            buddy_heads: UnsafeCell::new([core::ptr::null_mut(); MAX_BUDDY_ORDER as usize + 1]),
        }
    }

    // ==================== Public API (unchanged) ====================

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

        klog_pmm!(
            "[PMM] Init: {} MB, {} pages, kernel ends at 0x{:X}",
            mem_size / (1024 * 1024),
            total_pages,
            kernel_end
        );
    }

    pub fn init_bitmap(&self, reserved_after_kernel: u64) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }

        let reserved_aligned = (reserved_after_kernel + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current
            .fetch_add(reserved_aligned, Ordering::Relaxed);

        let info = self.info.get();
        let total_pages = info.total_pages as usize;
        let total_bits = total_pages;
        let bitmap_words = total_bits.div_ceil(32);
        let bitmap_bytes = bitmap_words * 4;

        // ---- Bitmap placement ----
        let bitmap_phys = self
            .early_current
            .fetch_add(bitmap_bytes as u64, Ordering::Relaxed);
        let bitmap_aligned = (bitmap_phys + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let bitmap_virt = bitmap_aligned + KERNEL_BASE;

        // SAFETY: bitmap_virt is phys_to_virt(PM) + KERNEL_BASE — valid kernel VA
        unsafe {
            core::ptr::write_bytes(bitmap_virt as *mut u8, 0, bitmap_bytes);
        }

        self.bitmap
            .set(match NonNull::new(bitmap_virt as *mut u32) {
                Some(ptr) => Some(ptr),
                None => {
                    klog_pmm!("[PMM] FATAL: bitmap null (0x{:X})", bitmap_virt);
                    return;
                }
            });
        self.bitmap_size.set(bitmap_words);

        // ---- Buddy metadata placement (right after bitmap, page-aligned) ----
        let buddy_meta_bytes = total_pages;
        let buddy_meta_phys =
            (bitmap_aligned + bitmap_bytes as u64 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let buddy_meta_virt = buddy_meta_phys + KERNEL_BASE;
        let buddy_meta_pages = buddy_meta_bytes.div_ceil(PAGE_SIZE as usize) as u64;

        // Update early_current past the buddy metadata
        self.early_current.store(
            buddy_meta_phys + buddy_meta_pages * PAGE_SIZE + PAGE_SIZE,
            Ordering::Relaxed,
        );

        // SAFETY: buddy_meta_virt = buddy_meta_phys + KERNEL_BASE — valid kernel VA
        unsafe {
            core::ptr::write_bytes(
                buddy_meta_virt as *mut u8,
                BUDDY_ALLOCATED,
                buddy_meta_bytes,
            );
        }

        self.buddy_meta
            .set(NonNull::new(buddy_meta_virt as *mut u8));
        klog_pmm!(
            "[PMM] Buddy meta: {} B at 0x{:X}",
            buddy_meta_bytes,
            buddy_meta_virt
        );

        // ---- Mark reserved regions in bitmap ----
        let kernel_end_val = self.kernel_end.get();
        let kernel_pages = phys_to_page(kernel_end_val + PAGE_SIZE - 1) as usize;
        let reserved_pages = (reserved_aligned / PAGE_SIZE) as usize;
        let total_reserved = kernel_pages + reserved_pages;
        for i in 0..total_reserved.min(total_pages) {
            self.set_bit(i);
        }
        if total_pages > 0 {
            self.set_bit(0); // page 0 must never be handed out
        }

        // Mark bitmap pages as used
        let bmp_start_page = phys_to_page(bitmap_aligned) as usize;
        let bmp_pages = (bitmap_bytes as u64).div_ceil(PAGE_SIZE) as usize;
        for i in bmp_start_page..(bmp_start_page + bmp_pages).min(total_pages) {
            self.set_bit(i);
        }

        // Mark buddy-meta pages as used
        let bm_start_page = phys_to_page(buddy_meta_phys) as usize;
        for i in bm_start_page..(bm_start_page + buddy_meta_pages as usize).min(total_pages) {
            self.set_bit(i);
        }

        // ---- Build buddy free lists from free bitmap pages ----
        self.buddy_init_free_lists(total_pages);

        self.buddy_ready.store(true, Ordering::Release);
        self.initialized.store(true, Ordering::Release);

        let free = self.count_free_pages();
        klog_pmm!(
            "[PMM] Buddy ready: {} total, {} free ({} MB), reserved {} pages",
            total_pages,
            free,
            free * 4 / 1024,
            total_reserved
        );

        self.update_stats();
    }

    pub fn alloc_page(&self) -> Option<PhysAddr> {
        let flags = self.acquire_lock();
        let result = self.do_alloc(0);
        match result {
            Some(_) => {
                self.total_allocs.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.release_lock(&flags);
        result
    }

    pub fn free_page(&self, addr: PhysAddr) {
        if addr.0 == 0 {
            return;
        }
        let flags = self.acquire_lock();
        self.do_free(addr, 0);
        self.total_frees.fetch_add(1, Ordering::Relaxed);
        self.release_lock(&flags);
    }

    pub fn alloc_pages(&self, count: usize) -> Option<PhysAddr> {
        if count == 0 {
            return None;
        }
        let order = count_to_order(count);
        let npages = 1usize << order as usize;

        let flags = self.acquire_lock();
        let result = self.do_alloc(order);
        match result {
            Some(_) => {
                self.total_allocs
                    .fetch_add(npages as u64, Ordering::Relaxed);
            }
            None => {
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.release_lock(&flags);
        result
    }

    pub fn free_pages(&self, addr: PhysAddr, count: usize) {
        if addr.0 == 0 || count == 0 {
            return;
        }
        let order = count_to_order(count);
        let npages = 1usize << order as usize;
        let flags = self.acquire_lock();
        self.do_free(addr, order);
        self.total_frees.fetch_add(npages as u64, Ordering::Relaxed);
        self.release_lock(&flags);
    }

    pub fn alloc_huge_page(&self, size_type: PageSize) -> Option<PhysAddr> {
        match size_type {
            PageSize::Size4K => self.alloc_page(),
            PageSize::Size2M => self.alloc_pages(512),
            PageSize::Size1G => {
                let np = (size_type.size() / PAGE_SIZE) as usize;
                let flags = self.acquire_lock();
                let result = self.buddy_direct_alloc_aligned(np, size_type.size());
                if result.is_some() {
                    self.total_allocs.fetch_add(np as u64, Ordering::Relaxed);
                } else {
                    self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                }
                self.release_lock(&flags);
                result
            }
        }
    }

    pub fn free_huge_page(&self, addr: PhysAddr, size_type: PageSize) {
        match size_type {
            PageSize::Size4K => self.free_page(addr),
            PageSize::Size2M => self.free_pages(addr, 512),
            PageSize::Size1G => {
                let np = (size_type.size() / PAGE_SIZE) as usize;
                let flags = self.acquire_lock();
                let start = phys_to_page(addr.0) as usize;
                for i in 0..np {
                    self.clear_bit(start + i);
                }
                self.total_frees.fetch_add(np as u64, Ordering::Relaxed);
                self.release_lock(&flags);
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
        klog_pmm!("=== PMM (Buddy) ===");
        klog_pmm!(
            "Total: {} MB  Pages: {} total / {} free / {} used",
            self.mem_size.get() / (1024 * 1024),
            info.total_pages,
            info.free_pages,
            info.used_pages
        );
        klog_pmm!("Kernel End: 0x{:X}", info.kernel_end);
        klog_pmm!(
            "Allocs: {}  Frees: {}  Failed: {}",
            self.total_allocs.load(Ordering::Relaxed),
            self.total_frees.load(Ordering::Relaxed),
            self.failed_allocs.load(Ordering::Relaxed)
        );
        klog_pmm!("===================");
    }

    // ==================== Lock helpers ====================

    /// Acquire the PMM lock with interrupt disabling (SMP-safe).
    ///
    /// Disables interrupts to prevent deadlock when an interrupt handler
    /// running on the same CPU attempts to allocate memory.
    #[inline(always)]
    fn acquire_lock(&self) -> IrqSaveFlags {
        let flags = disable_interrupts();
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        flags
    }

    #[inline(always)]
    fn release_lock(&self, flags: &IrqSaveFlags) {
        self.lock.store(false, Ordering::Release);
        restore_interrupts(flags);
    }

    // ==================== Bitmap helpers (stats + reserved) ====================

    fn set_bit(&self, bit: usize) {
        if let Some(bmp) = self.bitmap.get() {
            // SAFETY: bmp points to the bitmap (valid kernel VA from init).
            // word index checked against bitmap_size.
            // AtomicU32 operations are safe on properly aligned memory.
            unsafe {
                let word = bit / 32;
                if word < self.bitmap_size.get() {
                    let p = bmp.as_ptr().add(word) as *const AtomicU32;
                    (*p).fetch_or(1u32 << (bit % 32), Ordering::Relaxed);
                }
            }
        }
    }

    fn clear_bit(&self, bit: usize) {
        if let Some(bmp) = self.bitmap.get() {
            // SAFETY: bitmap pointer valid; word index bounds-checked;
            // AtomicU32 aligned access.
            unsafe {
                let word = bit / 32;
                if word < self.bitmap_size.get() {
                    let p = bmp.as_ptr().add(word) as *const AtomicU32;
                    (*p).fetch_and(!(1u32 << (bit % 32)), Ordering::Relaxed);
                }
            }
        }
    }

    fn test_bit(&self, bit: usize) -> bool {
        if let Some(bmp) = self.bitmap.get() {
            let word = bit / 32;
            if word < self.bitmap_size.get() {
                // SAFETY: bitmap pointer valid; word index checked; AtomicU32 load
                unsafe {
                    let p = bmp.as_ptr().add(word) as *const AtomicU32;
                    (*p).load(Ordering::Relaxed) & (1u32 << (bit % 32)) != 0
                }
            } else {
                false
            }
        } else {
            false
        }
    }

    fn count_free_pages(&self) -> u64 {
        let total = self.info.get().total_pages as usize;
        let mut free: u64 = 0;
        if let Some(bmp) = self.bitmap.get() {
            for w in 0..self.bitmap_size.get() {
                // SAFETY: bmp points to valid bitmap; w < bitmap_size bounds-checked
                unsafe {
                    let p = bmp.as_ptr().add(w) as *const AtomicU32;
                    free += (!(*p).load(Ordering::Relaxed)).count_ones() as u64;
                }
            }
        }
        // Clamp to total (bitmap may have extra bits beyond total_pages)
        let extra = (self.bitmap_size.get() * 32).saturating_sub(total) as u32;
        if extra > 0 {
            free = free.saturating_sub(extra as u64);
        }
        free
    }

    fn update_stats(&self) {
        let free = self.count_free_pages();
        let mut info = self.info.get();
        info.free_pages = free;
        info.used_pages = info.total_pages - free;
        self.info.set(info);
    }

    // ==================== Buddy allocator core ====================

    #[inline]
    fn buddy_meta_ptr(&self) -> *mut u8 {
        self.buddy_meta
            .get()
            .map(|n| n.as_ptr())
            .unwrap_or_default()
    }

    #[inline]
    fn buddy_heads_ptr(&self) -> *mut *mut FreeNode {
        // SAFETY: buddy_heads is UnsafeCell; accessed under bitmap lock;
        // pointer to the array is stable after init_bitmap.
        unsafe { (*self.buddy_heads.get()).as_mut_ptr() }
    }

    /// Try to merge freed page `pfn` at `order` with its buddy upwards.
    /// Returns (merged_pfn, final_order).
    fn buddy_try_merge(&self, mut pfn: u64, mut order: u8) -> (u64, u8) {
        let meta = self.buddy_meta_ptr();
        if meta.is_null() {
            return (pfn, order);
        }
        let total = self.info.get().total_pages;

        while order < MAX_BUDDY_ORDER {
            let buddy_pfn = pfn ^ (1u64 << order);
            if buddy_pfn >= total {
                break;
            }

            // SAFETY: buddy_pfn is within valid range; meta array entry is 1 byte
            let buddy_state = unsafe { *meta.add(buddy_pfn as usize) };
            if buddy_state != order {
                break;
            }

            // Remove buddy from its free list
            self.buddy_list_remove(buddy_pfn, order);
            // SAFETY: mark buddy as allocated in meta array
            unsafe {
                *meta.add(buddy_pfn as usize) = BUDDY_ALLOCATED;
            }

            pfn = core::cmp::min(pfn, buddy_pfn);
            order += 1;
        }

        // SAFETY: write final order into meta array
        unsafe {
            *meta.add(pfn as usize) = order;
        }
        (pfn, order)
    }

    /// Remove a free block from its doubly-linked list.
    fn buddy_list_remove(&self, pfn: u64, order: u8) {
        let heads = self.buddy_heads_ptr();
        let node = pfn_to_virt(pfn) as *mut FreeNode;
        // Defensive: validate node is within physical RAM range
        let node_phys = unsafe { (node as u64).wrapping_sub(KERNEL_BASE) };
        let mem_size = self.mem_size.get();
        #[allow(clippy::absurd_extreme_comparisons)]
        if node_phys < RAM_BASE || node_phys >= RAM_BASE + mem_size {
            klog_pmm!(
                "[PMM] Corrupt remove node at order {}: pfn=0x{:X} virt=0x{:X}",
                order,
                pfn,
                node as u64
            );
            return;
        }
        // SAFETY: node is inside a valid free page
        unsafe {
            let prev = (*node).prev;
            let next = (*node).next;
            if !prev.is_null() {
                (*prev).next = next;
            } else {
                *heads.add(order as usize) = next;
            }
            if !next.is_null() {
                (*next).prev = prev;
            }
        }
    }

    /// Push a block onto the free list head.
    fn buddy_list_push(&self, pfn: u64, order: u8) {
        let heads = self.buddy_heads_ptr();
        let node = pfn_to_virt(pfn) as *mut FreeNode;
        // SAFETY: free page is unused, we own the first 16 bytes
        unsafe {
            let old_head = *heads.add(order as usize);
            (*node).prev = core::ptr::null_mut();
            (*node).next = old_head;
            if !old_head.is_null() {
                (*old_head).prev = node;
            }
            *heads.add(order as usize) = node;
        }
    }

    /// Pop a block from the free list head. Returns pfn.
    fn buddy_list_pop(&self, order: u8) -> Option<u64> {
        let heads = self.buddy_heads_ptr();
        // SAFETY: heads points to stable array of FreeNode pointers
        let node = unsafe { *heads.add(order as usize) };
        if node.is_null() {
            return None;
        }

        // Defensive: validate node is within physical RAM range
        let node_phys = unsafe { (node as u64).wrapping_sub(KERNEL_BASE) };
        let mem_size = self.mem_size.get();
        #[allow(clippy::absurd_extreme_comparisons)]
        if node_phys < RAM_BASE || node_phys >= RAM_BASE + mem_size {
            klog_pmm!(
                "[PMM] Corrupt free list node at order {}: 0x{:X}",
                order,
                node as u64
            );
            return None;
        }

        // SAFETY: node is a valid free page within physical RAM
        let pfn = phys_to_page(node_phys);
        // SAFETY: updating doubly-linked list; node.next is valid or null
        unsafe {
            let next = (*node).next;
            *heads.add(order as usize) = next;
            if !next.is_null() {
                (*next).prev = core::ptr::null_mut();
            }
        }
        Some(pfn)
    }

    /// Core allocation at the given order.
    fn buddy_alloc(&self, order: u8) -> Option<(u64, u8)> {
        if order > MAX_BUDDY_ORDER {
            return None;
        }

        // Find smallest available order >= requested
        let mut avail_order: Option<u8> = None;
        for o in order..=MAX_BUDDY_ORDER {
            let heads = self.buddy_heads_ptr();
            // SAFETY: heads array is valid; o in valid range
            if unsafe { !(*heads.add(o as usize)).is_null() } {
                avail_order = Some(o);
                break;
            }
        }
        let alloc_order = avail_order?;

        let pfn = self.buddy_list_pop(alloc_order)?;
        let meta = self.buddy_meta_ptr();

        // SAFETY: mark as allocated in meta array
        unsafe {
            *meta.add(pfn as usize) = BUDDY_ALLOCATED;
        }

        // Split downward until we reach the requested order
        let cur_pfn = pfn;
        let mut cur_order = alloc_order;
        while cur_order > order {
            cur_order -= 1;
            let buddy_pfn = cur_pfn + (1u64 << cur_order);
            self.buddy_list_push(buddy_pfn, cur_order);
            // SAFETY: mark buddy as free at split order
            unsafe {
                *meta.add(buddy_pfn as usize) = cur_order;
            }
        }
        // SAFETY: mark final allocation in meta
        unsafe {
            *meta.add(cur_pfn as usize) = BUDDY_ALLOCATED;
        }

        Some((cur_pfn, order))
    }

    /// Main do_alloc: handles early alloc vs buddy.
    fn do_alloc(&self, order: u8) -> Option<PhysAddr> {
        if !self.initialized.load(Ordering::Acquire) {
            return if order == 0 {
                self.early_alloc_single()
            } else {
                self.early_alloc_multiple(1u64 << order as u64)
            };
        }

        if !self.buddy_ready.load(Ordering::Acquire) {
            // Between init and buddy_ready: fallback to bitmap scan
            let count = 1usize << order as usize;
            return self.alloc_from_bitmap_fallback(count);
        }

        let (pfn, _) = self.buddy_alloc(order)?;
        let addr = page_to_phys(pfn);
        for i in 0..(1usize << order as usize) {
            self.set_bit((pfn as usize) + i);
        }
        Some(PhysAddr(addr))
    }

    /// Main do_free: handles buddy or bitmap free.
    fn do_free(&self, addr: PhysAddr, order: u8) {
        if !self.initialized.load(Ordering::Acquire) {
            klog_pmm!("[PMM] Warn: free before bitmap init at 0x{:X}", addr.0);
            return;
        }

        let info = self.info.get();
        let pfn = phys_to_page(addr.0);
        if pfn >= info.total_pages {
            klog_pmm!("[PMM] Error: invalid page 0x{:X}", addr.0);
            return;
        }

        if !self.buddy_ready.load(Ordering::Acquire) {
            for i in 0..(1usize << order as usize) {
                self.clear_bit(pfn as usize + i);
            }
            return;
        }

        if !self.test_bit(pfn as usize) {
            klog_pmm!("[PMM] Warn: double free at pfn {}", pfn);
            return;
        }

        // Clear bitmap
        for i in 0..(1usize << order as usize) {
            self.clear_bit(pfn as usize + i);
        }

        // Merge and push onto free list
        let (merged_pfn, merged_order) = self.buddy_try_merge(pfn, order);
        self.buddy_list_push(merged_pfn, merged_order);
    }

    /// Scan all free pages (bits not set), coalesce into max-order buddy blocks.
    fn buddy_init_free_lists(&self, total_pages: usize) {
        let meta = self.buddy_meta_ptr();
        if meta.is_null() {
            return;
        }

        let mut pfn = 0usize;
        while pfn < total_pages {
            if self.test_bit(pfn) {
                pfn += 1;
                continue;
            }

            // Find contiguous free run
            let run_start = pfn;
            while pfn < total_pages && !self.test_bit(pfn) {
                pfn += 1;
            }
            let run_len = pfn - run_start;

            // Coalesce into max-order buddy blocks
            let mut cur = run_start as u64;
            let mut remaining = run_len;
            while remaining > 0 {
                // Largest power-of-two ≤ remaining, aligned to its own size
                let max_order = (usize::BITS - 1 - (remaining - 1).leading_zeros())
                    .min(MAX_BUDDY_ORDER as u32) as u8;
                // Find largest order where cur is naturally aligned AND 2^order ≤ remaining
                let mut order = max_order;
                while order > 0 {
                    let size = 1usize << order as usize;
                    if (cur as usize).is_multiple_of(size) && size <= remaining {
                        break;
                    }
                    order -= 1;
                }
                let block_size = 1usize << order as usize;

                // SAFETY: block is within free run and meta array bounds
                unsafe {
                    *meta.add(cur as usize) = order;
                }
                self.buddy_list_push(cur, order);

                cur += block_size as u64;
                remaining -= block_size;
            }
        }
    }

    /// Direct aligned alloc for 1GB pages (beyond buddy range).
    fn buddy_direct_alloc_aligned(&self, count: usize, alignment: u64) -> Option<PhysAddr> {
        let total = self.info.get().total_pages as usize;
        let align_pages = (alignment / PAGE_SIZE) as usize;
        let mut i = align_pages; // skip page 0
        while i + count <= total {
            if i.is_multiple_of(align_pages) {
                let mut ok = true;
                for j in 0..count {
                    if self.test_bit(i + j) {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    for j in 0..count {
                        self.set_bit(i + j);
                    }
                    return Some(PhysAddr(page_to_phys(i as u64)));
                }
            }
            i += align_pages;
        }
        None
    }

    /// Fallback bitmap scan (used between init and buddy_ready, or when buddy disabled).
    fn alloc_from_bitmap_fallback(&self, count: usize) -> Option<PhysAddr> {
        let total = self.info.get().total_pages as usize;
        for i in 0..total {
            if self.test_bit(i) {
                continue;
            }
            if i + count > total {
                return None;
            }
            let mut ok = true;
            for j in 1..count {
                if self.test_bit(i + j) {
                    ok = false;
                    break;
                }
            }
            if ok {
                for j in 0..count {
                    self.set_bit(i + j);
                }
                return Some(PhysAddr(page_to_phys(i as u64)));
            }
        }
        None
    }

    // ==================== Early allocator (pre-bitmap) ====================

    fn early_alloc_single(&self) -> Option<PhysAddr> {
        let current = self.early_current.fetch_add(PAGE_SIZE, Ordering::Relaxed);
        let aligned = (current + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current
            .store(aligned + PAGE_SIZE, Ordering::Relaxed);

        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            unsafe {
                let a = (*self.early_allocs.get()).as_mut_ptr().add(idx);
                (*a).addr = aligned;
                (*a).size = PAGE_SIZE;
            }
        }
        if aligned >= RAM_BASE + self.mem_size.get() {
            klog_pmm!("[PMM] Error: early alloc OOM");
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
                let a = (*self.early_allocs.get()).as_mut_ptr().add(idx);
                (*a).addr = aligned;
                (*a).size = size;
            }
        }
        if aligned + size > RAM_BASE + self.mem_size.get() {
            klog_pmm!("[PMM] Error: early multi alloc OOM");
            return None;
        }
        Some(PhysAddr(aligned))
    }
}

// ==================== Global singleton & init ====================

static GLOBAL_PMM: spin::Once<PhysicalMemoryManager> = spin::Once::new();

pub fn pmm_init(mem_size: u64, kernel_end: u64) -> &'static PhysicalMemoryManager {
    GLOBAL_PMM.call_once(|| {
        let pmm = PhysicalMemoryManager::new();
        pmm.init(mem_size, kernel_end);
        pmm
    })
}

pub fn pmm_init_bitmap(reserved_after_kernel: u64) {
    let pmm = GLOBAL_PMM
        .get()
        .expect("[PMM] pmm_init_bitmap before pmm_init");
    pmm.init_bitmap(reserved_after_kernel);
}

pub fn get_pmm() -> &'static PhysicalMemoryManager {
    GLOBAL_PMM.get().expect("[PMM] accessed before init")
}

// ==================== Barrier / Recovery ====================

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
        total_allocs: pmm.total_allocs.load(Ordering::Relaxed),
        total_frees: pmm.total_frees.load(Ordering::Relaxed),
        failed_allocs: pmm.failed_allocs.load(Ordering::Relaxed),
        info: pmm.info.get(),
    });
}

pub fn pmm_barrier_rollback() -> bool {
    let pmm = get_pmm();
    let snap = PMM_SNAPSHOT.lock();
    if let Some(ref s) = *snap {
        pmm.total_allocs.store(s.total_allocs, Ordering::Relaxed);
        pmm.total_frees.store(s.total_frees, Ordering::Relaxed);
        pmm.failed_allocs.store(s.failed_allocs, Ordering::Relaxed);
        pmm.info.set(s.info);
    }
    true
}

fn pmm_barrier_capture_cb() {
    pmm_barrier_capture();
}
fn pmm_barrier_rollback_cb() -> bool {
    pmm_barrier_rollback()
}

pub fn pmm_register_barrier_domain() {
    crate::kernel::framework::barrier::recovery_domain_register(3);
    if let Some(dom) = crate::kernel::framework::barrier::RECOVERY_MANAGER.lock().find(3) {
        *dom.capture_cb.lock() = Some(pmm_barrier_capture_cb);
        *dom.rollback_cb.lock() = Some(pmm_barrier_rollback_cb);
    }
}
