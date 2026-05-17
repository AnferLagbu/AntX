use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PAGE_SIZE: u64 = 4096;
const BUDDY_MAX_ORDER: usize = 10;
const BUDDY_ALLOCATED: u8 = 0x80;
const BUDDY_ORDER_MASK: u8 = 0x7F;
const BUDDY_INTERIOR_FREE: u8 = 0xFE;
const BUDDY_INTERIOR_USED: u8 = 0xFF;
const DMA_MAX_SCATTER_ENTRIES: usize = 16;

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
}

impl BuddyAllocator {
    fn new(total_pages: u64) -> Self {
        Self {
            free_lists: [const { AtomicU64::new(0) }; BUDDY_MAX_ORDER + 1],
            order_map: vec![0; total_pages as usize],
            total_pages,
            initialized: AtomicBool::new(true),
        }
    }

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

    fn om_is_free(&self, page: u64, order: usize) -> bool {
        let v = self.om_get(page);
        v == order as u8 || v == BUDDY_INTERIOR_FREE
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

    fn free_block(&mut self, addr: u64, order: usize) {
        if !self.initialized.load(Ordering::Acquire) { return; }
        let mut page = addr / PAGE_SIZE;
        let mut cur = order;
        while cur < BUDDY_MAX_ORDER {
            let buddy = page ^ (1u64 << cur);
            if buddy >= self.total_pages { break; }
            if !self.om_is_free(buddy, cur) { break; }
            self.list_remove(buddy * PAGE_SIZE, cur);
            let bs = 1u64 << cur;
            for i in 0..bs { self.om_set(buddy + i, BUDDY_INTERIOR_USED); }
            page = page.min(buddy);
            cur += 1;
        }
        self.list_push(page, cur);
        self.om_set(page, cur as u8);
        for i in 1..(1u64 << cur) { self.om_set(page + i, BUDDY_INTERIOR_FREE); }
    }

    fn free_page(&mut self, addr: u64) {
        let page = addr / PAGE_SIZE;
        let v = self.om_get(page);
        if v & BUDDY_ALLOCATED == 0 { return; }
        let order = (v & BUDDY_ORDER_MASK) as usize;
        self.free_block(addr, order);
    }

    fn count_free_pages(&self) -> u64 {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_buddy(pages: u64) -> BuddyAllocator {
        let mut buddy = BuddyAllocator::new(pages);
        let max_order = BUDDY_MAX_ORDER;
        let mut start = 0u64;
        while start < pages {
            let mut order = max_order;
            while order > 0 && start + (1u64 << order) > pages {
                order -= 1;
            }
            buddy.list_push(start, order);
            buddy.om_set(start, order as u8);
            for i in 1..(1u64 << order) {
                buddy.om_set(start + i, BUDDY_INTERIOR_FREE);
            }
            start += 1u64 << order;
        }
        buddy
    }

    #[test]
    fn buddy_alloc_single_page() {
        let mut buddy = setup_buddy(1024);
        let initial_free = buddy.count_free_pages();
        let addr = buddy.alloc_order(0);
        assert!(addr.is_some());
        assert_eq!(buddy.count_free_pages(), initial_free - 1);
    }

    #[test]
    fn buddy_alloc_and_free() {
        let mut buddy = setup_buddy(1024);
        let initial_free = buddy.count_free_pages();
        let addr = buddy.alloc_order(0).unwrap();
        assert_eq!(buddy.count_free_pages(), initial_free - 1);
        buddy.free_page(addr);
        assert_eq!(buddy.count_free_pages(), initial_free);
    }

    #[test]
    fn buddy_alloc_multiple_orders() {
        let mut buddy = setup_buddy(1024);
        let a1 = buddy.alloc_order(0);
        let a2 = buddy.alloc_order(1);
        let a3 = buddy.alloc_order(2);
        assert!(a1.is_some());
        assert!(a2.is_some());
        assert!(a3.is_some());
    }

    #[test]
    fn buddy_coalescing() {
        let mut buddy = setup_buddy(1024);
        let initial_free = buddy.count_free_pages();
        let a = buddy.alloc_order(0).unwrap();
        let b = buddy.alloc_order(0).unwrap();
        buddy.free_page(a);
        buddy.free_page(b);
        assert_eq!(buddy.count_free_pages(), initial_free);
    }

    #[test]
    fn buddy_exhaustion() {
        let mut buddy = setup_buddy(4);
        let mut addrs = Vec::new();
        for _ in 0..4 {
            let addr = buddy.alloc_order(0);
            if addr.is_some() {
                addrs.push(addr.unwrap());
            }
        }
        let should_fail = buddy.alloc_order(0);
        assert!(should_fail.is_none());
        for addr in addrs {
            buddy.free_page(addr);
        }
        assert_eq!(buddy.count_free_pages(), 4);
    }

    #[test]
    fn buddy_alloc_order_too_large() {
        let buddy = setup_buddy(1024);
        let mut b = buddy;
        let result = b.alloc_order(BUDDY_MAX_ORDER + 1);
        assert!(result.is_none());
    }

    #[test]
    fn buddy_repeated_alloc_free() {
        let mut buddy = setup_buddy(256);
        for _ in 0..100 {
            let mut addrs = Vec::new();
            for _ in 0..10 {
                if let Some(addr) = buddy.alloc_order(0) {
                    addrs.push(addr);
                }
            }
            for addr in addrs {
                buddy.free_page(addr);
            }
        }
        assert_eq!(buddy.count_free_pages(), 256);
    }
}
