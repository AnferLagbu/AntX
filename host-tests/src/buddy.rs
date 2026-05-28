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
}
