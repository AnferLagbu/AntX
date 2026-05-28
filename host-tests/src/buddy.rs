#![allow(dead_code)] // 测试基础设施模块 (BuddyAllocator 由 #[cfg(test)] 测试使用)

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PAGE_SIZE: u64 = 4096;
const BUDDY_MAX_ORDER: usize = 10;
const BUDDY_ALLOCATED: u8 = 0x80;
const BUDDY_ORDER_MASK: u8 = 0x7F;
const BUDDY_INTERIOR_FREE: u8 = 0xFE;
const BUDDY_INTERIOR_USED: u8 = 0xFF;

#[repr(C)]
struct FreeNode {
    next: u64,
    prev: u64,
}

struct BuddyAllocator {
    free_lists: [AtomicU64; BUDDY_MAX_ORDER + 1],
    order_map: Vec<u8>,
    total_pages: u64,
    initialized: AtomicBool,
    #[cfg(test)]
    mock_memory: Vec<u8>,
}

impl BuddyAllocator {
    fn new(total_pages: u64) -> Self {
        #[cfg(test)]
        {
            let mock_size = (total_pages as usize + 1) * PAGE_SIZE as usize;
            Self {
                free_lists: [const { AtomicU64::new(0) }; BUDDY_MAX_ORDER + 1],
                order_map: vec![0; total_pages as usize],
                total_pages,
                initialized: AtomicBool::new(true),
                mock_memory: vec![0u8; mock_size],
            }
        }
        #[cfg(not(test))]
        Self {
            free_lists: [const { AtomicU64::new(0) }; BUDDY_MAX_ORDER + 1],
            order_map: vec![0; total_pages as usize],
            total_pages,
            initialized: AtomicBool::new(true),
        }
    }

    #[cfg(test)]
    fn node_virt(&self, phys: u64) -> *mut FreeNode {
        let offset = phys as usize;
        if offset + std::mem::size_of::<FreeNode>() <= self.mock_memory.len() {
            self.mock_memory.as_ptr().wrapping_add(offset) as *mut FreeNode
        } else {
            std::ptr::null_mut()
        }
    }

    #[cfg(not(test))]
    fn node_virt(&self, phys: u64) -> *mut FreeNode {
        (phys + 0xFFFF800000000000u64) as *mut FreeNode
    }

    fn om_get(&self, page: u64) -> u8 {
        self.order_map.get(page as usize).copied().unwrap_or(0xFF)
    }

    fn om_set(&mut self, page: u64, val: u8) {
        if (page as usize) < self.order_map.len() {
            self.order_map[page as usize] = val;
        }
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

    fn alloc_order(&mut self, order: usize) -> Option<u64> {
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
        Some(phys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buddy_constants() {
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(BUDDY_MAX_ORDER, 10);
        assert!(BUDDY_ALLOCATED & BUDDY_ORDER_MASK == 0);
    }

    #[test]
    fn buddy_allocator_creation() {
        let buddy = BuddyAllocator::new(1024);
        assert_eq!(buddy.total_pages, 1024);
        assert!(buddy.initialized.load(Ordering::Acquire));
    }

    #[test]
    fn buddy_alloc_order_too_large() {
        let mut buddy = BuddyAllocator::new(1024);
        let result = buddy.alloc_order(BUDDY_MAX_ORDER + 1);
        assert!(result.is_none());
    }

    #[test]
    fn buddy_order_map_basic() {
        let mut buddy = BuddyAllocator::new(16);
        buddy.om_set(0, 5);
        assert_eq!(buddy.om_get(0), 5);
        assert_eq!(buddy.om_get(1), 0);
    }

    #[test]
    fn buddy_order_map_interior() {
        let mut buddy = BuddyAllocator::new(16);
        buddy.om_set(0, BUDDY_ALLOCATED | 3);
        assert!(buddy.om_get(0) & BUDDY_ALLOCATED != 0);
        assert_eq!(buddy.om_get(0) & BUDDY_ORDER_MASK, 3);
    }

    #[test]
    fn buddy_alloc_order_0() {
        let mut buddy = BuddyAllocator::new(1024);
        buddy.list_push(1, BUDDY_MAX_ORDER);
        buddy.om_set(1, BUDDY_MAX_ORDER as u8);
        let result = buddy.alloc_order(0);
        assert!(result.is_some(), "alloc_order(0) should succeed");
        let page = result.unwrap() / PAGE_SIZE;
        assert_eq!(buddy.om_get(page) & BUDDY_ORDER_MASK, 0);
        assert!(buddy.om_get(page) & BUDDY_ALLOCATED != 0);
    }

    #[test]
    fn buddy_alloc_split_verification() {
        let mut buddy = BuddyAllocator::new(1024);
        buddy.list_push(1, BUDDY_MAX_ORDER);
        buddy.om_set(1, BUDDY_MAX_ORDER as u8);
        let result = buddy.alloc_order(0);
        assert!(result.is_some());
        for order in 0..BUDDY_MAX_ORDER {
            let buddy_page = 1 + (1u64 << order);
            assert_eq!(buddy.om_get(buddy_page) & BUDDY_ORDER_MASK, order as u8,
                "buddy page at order {} should be in free list", order);
        }
    }

    #[test]
    fn buddy_alloc_exhaustion() {
        let mut buddy = BuddyAllocator::new(4);
        buddy.list_push(1, 1);
        buddy.om_set(1, 1);
        buddy.om_set(2, 1);
        let first = buddy.alloc_order(0);
        assert!(first.is_some(), "first alloc should succeed");
        let second = buddy.alloc_order(0);
        assert!(second.is_some(), "second alloc from buddy should succeed");
        let third = buddy.alloc_order(0);
        assert!(third.is_none(), "third alloc should fail (exhausted)");
    }

    #[test]
    fn buddy_alloc_all_orders() {
        for order in 0..=BUDDY_MAX_ORDER {
            let mut buddy = BuddyAllocator::new(1024);
            buddy.list_push(1, BUDDY_MAX_ORDER);
            buddy.om_set(1, BUDDY_MAX_ORDER as u8);
            let result = buddy.alloc_order(order);
            assert!(result.is_some(), "alloc_order({}) should succeed", order);
        }
    }

    #[test]
    fn buddy_alloc_uninitialized() {
        let mut buddy = BuddyAllocator::new(1024);
        buddy.initialized.store(false, Ordering::Release);
        let result = buddy.alloc_order(0);
        assert!(result.is_none(), "alloc on uninitialized should fail");
    }

    #[test]
    fn buddy_om_set_get_boundary() {
        let mut buddy = BuddyAllocator::new(16);
        buddy.om_set(15, 0xAB);
        assert_eq!(buddy.om_get(15), 0xAB);
        assert_eq!(buddy.om_get(16), 0xFF);
    }

    #[test]
    fn buddy_interior_pages_marked() {
        let mut buddy = BuddyAllocator::new(1024);
        buddy.list_push(1, BUDDY_MAX_ORDER);
        buddy.om_set(1, BUDDY_MAX_ORDER as u8);
        let _ = buddy.alloc_order(2);
        let page0 = 1u64;
        assert!(buddy.om_get(page0) & BUDDY_ALLOCATED != 0, "page 1 should be allocated");
        for i in 1..4u64 {
            assert_eq!(buddy.om_get(page0 + i), BUDDY_INTERIOR_USED,
                "interior page {} should be INTERIOR_USED", i);
        }
    }

    #[test]
    fn buddy_free_list_operations() {
        let buddy = BuddyAllocator::new(1024);
        buddy.list_push(1, 5);
        buddy.list_push(33, 5);
        assert_ne!(buddy.free_lists[5].load(Ordering::Acquire), 0);
    }
}
