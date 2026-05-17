//! Buddy System Allocator for Physical Memory Management
//!
//! Implements a power-of-2 buddy allocator that reduces external fragmentation
//! by ensuring blocks can always be split and merged efficiently.
//!
//! # Algorithm
//!
//! - Blocks are always power-of-2 in size (1, 2, 4, ..., 2^10 pages)
//! - Allocation: find smallest available block >= requested size, split if needed
//! - Deallocation: free block and recursively merge with buddy if free
//!
//! # Order Map Encoding (1 byte per page frame)
//!
//! - 0x00..=0x0A: first page of a FREE block of order 0..=10
//! - 0x80..=0x8A: first page of an ALLOCATED block of order 0..=10
//! - 0xFE: interior page of a free block (not the first page)
//! - 0xFF: interior page of an allocated block, or unmanaged page

use core::cell::Cell;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::ptr::NonNull;

use super::*;

pub const BUDDY_MAX_ORDER: usize = 10;
const BUDDY_ALLOCATED: u8 = 0x80;
const BUDDY_ORDER_MASK: u8 = 0x7F;
const BUDDY_INTERIOR_FREE: u8 = 0xFE;
const BUDDY_INTERIOR_USED: u8 = 0xFF;

macro_rules! klog_buddy {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

#[repr(C)]
struct FreeNode {
    next: u64,
    prev: u64,
}

pub struct BuddyAllocator {
    free_lists: [AtomicU64; BUDDY_MAX_ORDER + 1],
    order_map: Cell<Option<NonNull<u8>>>,
    total_pages: Cell<u64>,
    initialized: AtomicBool,
}

unsafe impl Sync for BuddyAllocator {}
unsafe impl Send for BuddyAllocator {}

impl BuddyAllocator {
    pub const fn new() -> Self {
        Self {
            free_lists: [const { AtomicU64::new(0) }; BUDDY_MAX_ORDER + 1],
            order_map: Cell::new(None),
            total_pages: Cell::new(0),
            initialized: AtomicBool::new(false),
        }
    }

    pub fn init(&self, total_pages: u64, bitmap_ptr: *const u32, bitmap_words: usize) {
        let map_bytes = total_pages as usize;
        let map_pages = (map_bytes + 4095) / 4096;

        extern "C" {
            fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void;
        }
        let map_ptr = unsafe { pmm_alloc_pages(map_pages as u64) };
        if map_ptr.is_null() {
            klog_buddy!("[BUDDY] Failed to allocate order map ({} bytes)", map_bytes);
            return;
        }

        let map_virt = map_ptr as u64 + KERNEL_BASE;
        unsafe {
            core::ptr::write_bytes(map_virt as *mut u8, BUDDY_INTERIOR_USED, map_bytes);
        }

        self.order_map.set(NonNull::new(map_virt as *mut u8));
        self.total_pages.set(total_pages);

        self.build_from_bitmap(total_pages, bitmap_ptr, bitmap_words);

        self.initialized.store(true, Ordering::Release);

        let free_count = self.count_free_pages();
        klog_buddy!(
            "[BUDDY] Initialized: {} total, {} free pages, max_order={}",
            total_pages,
            free_count,
            BUDDY_MAX_ORDER
        );
    }

    fn build_from_bitmap(&self, total_pages: u64, bitmap_ptr: *const u32, bitmap_words: usize) {
        let mut page: u64 = 0;
        while page < total_pages {
            if !self.bitmap_test_free(page, bitmap_ptr, bitmap_words) {
                page += 1;
                continue;
            }
            let region_start = page;
            let mut region_end = page + 1;
            while region_end < total_pages
                && self.bitmap_test_free(region_end, bitmap_ptr, bitmap_words)
            {
                region_end += 1;
            }
            self.add_region(region_start, region_end);
            page = region_end;
        }
    }

    fn bitmap_test_free(&self, page: u64, bitmap: *const u32, words: usize) -> bool {
        let bit = page as usize;
        let wi = bit / 32;
        let bi = bit % 32;
        if wi >= words {
            return false;
        }
        unsafe { (*bitmap.add(wi)) & (1u32 << bi) == 0 }
    }

    fn add_region(&self, start: u64, end: u64) {
        let mut page = start;
        while page < end {
            let remaining = end - page;
            let mut order = BUDDY_MAX_ORDER;
            while order > 0 {
                let bp = 1u64 << order;
                if page % bp == 0 && remaining >= bp {
                    break;
                }
                order -= 1;
            }
            let bp = 1u64 << order;
            self.list_push(page, order);
            self.om_set(page, order as u8);
            for i in 1..bp {
                self.om_set(page + i, BUDDY_INTERIOR_FREE);
            }
            page += bp;
        }
    }

    #[inline(always)]
    fn node_virt(&self, phys: u64) -> *mut FreeNode {
        (phys + KERNEL_BASE) as *mut FreeNode
    }

    fn list_push(&self, page: u64, order: usize) {
        let phys = page * PAGE_SIZE;
        let node = self.node_virt(phys);
        let old = self.free_lists[order].load(Ordering::Acquire);
        unsafe {
            (*node).next = old;
            (*node).prev = 0;
            if old != 0 {
                (*self.node_virt(old)).prev = phys;
            }
        }
        self.free_lists[order].store(phys, Ordering::Release);
    }

    fn list_remove(&self, phys: u64, order: usize) {
        let node = unsafe { &*self.node_virt(phys) };
        let (next, prev) = (node.next, node.prev);
        unsafe {
            if prev != 0 {
                (*self.node_virt(prev)).next = next;
            } else {
                self.free_lists[order].store(next, Ordering::Release);
            }
            if next != 0 {
                (*self.node_virt(next)).prev = prev;
            }
        }
    }

    #[inline(always)]
    fn om_set(&self, page: u64, val: u8) {
        if let Some(map) = self.order_map.get() {
            let idx = page as usize;
            if idx < self.total_pages.get() as usize {
                unsafe {
                    *map.as_ptr().add(idx) = val;
                }
            }
        }
    }

    #[inline(always)]
    fn om_get(&self, page: u64) -> u8 {
        if let Some(map) = self.order_map.get() {
            let idx = page as usize;
            if idx < self.total_pages.get() as usize {
                unsafe {
                    return *map.as_ptr().add(idx);
                }
            }
        }
        BUDDY_INTERIOR_USED
    }

    #[inline(always)]
    fn om_is_free(&self, page: u64, order: usize) -> bool {
        self.om_get(page) == order as u8
    }

    pub fn get_block_order(&self, page: u64) -> usize {
        let val = self.om_get(page);
        if val == BUDDY_INTERIOR_USED || val == BUDDY_INTERIOR_FREE {
            return usize::MAX;
        }
        (val & BUDDY_ORDER_MASK) as usize
    }

    pub fn alloc_order(&self, order: usize) -> Option<PhysAddr> {
        if order > BUDDY_MAX_ORDER || !self.initialized.load(Ordering::Acquire) {
            return None;
        }

        let mut found = order;
        while found <= BUDDY_MAX_ORDER && self.free_lists[found].load(Ordering::Acquire) == 0 {
            found += 1;
        }
        if found > BUDDY_MAX_ORDER {
            return None;
        }

        let phys = self.free_lists[found].load(Ordering::Acquire);
        let page = phys / PAGE_SIZE;
        self.list_remove(phys, found);

        let mut cur = found;
        while cur > order {
            cur -= 1;
            let buddy = page + (1u64 << cur);
            self.list_push(buddy, cur);
            self.om_set(buddy, cur as u8);
            for i in 1..(1u64 << cur) {
                self.om_set(buddy + i, BUDDY_INTERIOR_FREE);
            }
        }

        self.om_set(page, BUDDY_ALLOCATED | order as u8);
        for i in 1..(1u64 << order) {
            self.om_set(page + i, BUDDY_INTERIOR_USED);
        }

        Some(PhysAddr(phys))
    }

    pub fn alloc_pages(&self, count: usize) -> Option<PhysAddr> {
        self.alloc_order(Self::order_for_count(count))
    }

    pub fn free_page(&self, addr: PhysAddr) {
        self.free_block(addr, 0);
    }

    pub fn free_pages(&self, addr: PhysAddr, count: usize) {
        let page = addr.0 / PAGE_SIZE;
        let stored = self.get_block_order(page);
        if stored != usize::MAX {
            self.free_block(addr, stored);
        } else {
            for i in 0..count as u64 {
                self.free_block(PhysAddr(addr.0 + i * PAGE_SIZE), 0);
            }
        }
    }

    fn free_block(&self, addr: PhysAddr, order: usize) {
        if !self.initialized.load(Ordering::Acquire) {
            return;
        }

        let mut page = addr.0 / PAGE_SIZE;
        let mut cur = order;

        while cur < BUDDY_MAX_ORDER {
            let buddy = page ^ (1u64 << cur);
            if buddy >= self.total_pages.get() {
                break;
            }
            if !self.om_is_free(buddy, cur) {
                break;
            }

            self.list_remove(buddy * PAGE_SIZE, cur);
            let bs = 1u64 << cur;
            for i in 0..bs {
                self.om_set(buddy + i, BUDDY_INTERIOR_USED);
            }

            page = page.min(buddy);
            cur += 1;
        }

        self.list_push(page, cur);
        self.om_set(page, cur as u8);
        for i in 1..(1u64 << cur) {
            self.om_set(page + i, BUDDY_INTERIOR_FREE);
        }
    }

    pub fn order_for_count(count: usize) -> usize {
        if count <= 1 {
            return 0;
        }
        let (mut o, mut s) = (0usize, 1usize);
        while s < count {
            o += 1;
            s <<= 1;
        }
        o.min(BUDDY_MAX_ORDER)
    }

    #[inline(always)]
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    pub fn count_free_pages(&self) -> u64 {
        let mut total = 0u64;
        for order in 0..=BUDDY_MAX_ORDER {
            let mut count = 0u64;
            let mut cur = self.free_lists[order].load(Ordering::Acquire);
            while cur != 0 {
                count += 1;
                cur = unsafe { (*self.node_virt(cur)).next };
            }
            total += count * (1u64 << order);
        }
        total
    }

    pub fn dump_stats(&self) {
        klog_buddy!("=== Buddy Allocator ===");
        klog_buddy!("Total pages: {}", self.total_pages.get());
        klog_buddy!("Free pages:  {}", self.count_free_pages());
        for order in 0..=BUDDY_MAX_ORDER {
            let mut count = 0u64;
            let mut cur = self.free_lists[order].load(Ordering::Acquire);
            while cur != 0 {
                count += 1;
                cur = unsafe { (*self.node_virt(cur)).next };
            }
            if count > 0 {
                klog_buddy!(
                    "  Order {}: {} blocks ({} pages = {} KB)",
                    order,
                    count,
                    1u64 << order,
                    (1u64 << order) * 4
                );
            }
        }
        klog_buddy!("=======================");
    }
}

static GLOBAL_BUDDY: BuddyAllocator = BuddyAllocator::new();

pub fn buddy_init(total_pages: u64, bitmap_ptr: *const u32, bitmap_words: usize) {
    GLOBAL_BUDDY.init(total_pages, bitmap_ptr, bitmap_words);
}

pub fn get_buddy() -> &'static BuddyAllocator {
    &GLOBAL_BUDDY
}
