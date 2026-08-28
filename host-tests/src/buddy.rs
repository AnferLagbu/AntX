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
    // reserve_range/unreserve_range 簿记: 1 = 预留/已用 (与真实 PMM 位图语义一致)
    reserved: Vec<bool>,
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
                reserved: vec![false; total_pages as usize],
                mock_memory: vec![0u8; mock_size],
            }
        }
        #[cfg(not(test))]
        Self {
            free_lists: [const { AtomicU64::new(0) }; BUDDY_MAX_ORDER + 1],
            order_map: vec![0; total_pages as usize],
            total_pages,
            initialized: AtomicBool::new(true),
            reserved: vec![false; total_pages as usize],
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

    fn is_reserved(&self, pfn: u64) -> bool {
        self.reserved.get(pfn as usize).copied().unwrap_or(true)
    }

    /// 将 [start, start+npages) 空闲页按自然对齐分裂为 buddy 块压入空闲链表.
    /// 与真实 PMM `buddy_free_insert_range` 的分裂逻辑一致.
    fn push_free_range(&mut self, start: u64, npages: u64) {
        let mut cur = start;
        let mut remaining = npages;
        while remaining > 0 {
            let mut order = (u64::BITS - 1 - remaining.leading_zeros()).min(BUDDY_MAX_ORDER as u32);
            while order > 0 {
                let size = 1u64 << order;
                if cur.is_multiple_of(size) && size <= remaining {
                    break;
                }
                order -= 1;
            }
            let block_size = 1u64 << order;
            self.list_push(cur, order as usize);
            self.om_set(cur, order as u8);
            cur += block_size;
            remaining -= block_size;
        }
    }

    /// 预留 [start, start+npages): 摘除空闲链表中所有重叠块, 不重叠部分重新压回,
    /// 并标记 reserved. 与真实 PMM `buddy_reserve_pfn_range` 语义一致.
    fn reserve_range(&mut self, start: u64, npages: u64) {
        let end = start + npages;
        for order in 0..=BUDDY_MAX_ORDER {
            let mut phys = self.free_lists[order].load(Ordering::Acquire);
            while phys != 0 {
                // SAFETY: 测试 mock, phys 始终指向 mock_memory 内合法 FreeNode
                let next = unsafe { (*self.node_virt(phys)).next };
                let block_pfn = phys / PAGE_SIZE;
                let block_size = 1u64 << order;
                if block_pfn < end && block_pfn + block_size > start {
                    self.list_remove(phys, order);
                    // 不重叠部分重新压回
                    if block_pfn < start {
                        self.push_free_range(block_pfn, start - block_pfn);
                    }
                    let block_end = block_pfn + block_size;
                    if block_end > end {
                        let right_start = block_pfn.max(end);
                        self.push_free_range(right_start, block_end - right_start);
                    }
                }
                phys = next;
            }
        }
        for i in start..end {
            if (i as usize) < self.reserved.len() {
                self.reserved[i as usize] = true;
            }
        }
    }

    /// 撤销 reserve_range: 清 reserved 标记, 将范围压回空闲链表.
    fn unreserve_range(&mut self, start: u64, npages: u64) {
        for i in start..(start + npages) {
            if (i as usize) < self.reserved.len() {
                self.reserved[i as usize] = false;
            }
        }
        self.push_free_range(start, npages);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buddy_constants() {
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(BUDDY_MAX_ORDER, 10);
        const { assert!(BUDDY_ALLOCATED & BUDDY_ORDER_MASK == 0); }
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
            assert_eq!(
                buddy.om_get(buddy_page) & BUDDY_ORDER_MASK,
                order as u8,
                "buddy page at order {} should be in free list",
                order
            );
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
        assert!(
            buddy.om_get(page0) & BUDDY_ALLOCATED != 0,
            "page 1 should be allocated"
        );
        for i in 1..4u64 {
            assert_eq!(
                buddy.om_get(page0 + i),
                BUDDY_INTERIOR_USED,
                "interior page {} should be INTERIOR_USED",
                i
            );
        }
    }

    #[test]
    fn buddy_free_list_operations() {
        let buddy = BuddyAllocator::new(1024);
        buddy.list_push(1, 5);
        buddy.list_push(33, 5);
        assert_ne!(buddy.free_lists[5].load(Ordering::Acquire), 0);
    }

    // 回归: PMM reserve_range 与 buddy 空闲链表严格同步.
    // 背景: 早期实现 reserve_range 只 set_bit 位图, 未从空闲链表摘除重叠块,
    // 导致含已预留/已分配页的块滞留在链表中, buddy_alloc 分裂时把已占用页
    // push 回空闲链表 → 二次分配 → 覆盖用户代码页 (fork #PF/Triple Fault).
    #[test]
    fn buddy_reserve_range_never_hands_out_reserved_pages() {
        let mut buddy = BuddyAllocator::new(1024);
        // 一个大 order-9 块覆盖 [1, 513) (mock 以 phys 0 为空链表哨兵, 块头须 ≥ pfn 1)
        buddy.list_push(1, 9);
        buddy.om_set(1, 9);
        // reserve [100, 150) (跨块内部的子范围)
        buddy.reserve_range(100, 50);

        // 1) 空闲链表任何块都不含 reserved 页
        for order in 0..=BUDDY_MAX_ORDER {
            let mut phys = buddy.free_lists[order].load(Ordering::Acquire);
            while phys != 0 {
                let pfn = phys / PAGE_SIZE;
                let block_size = 1u64 << order;
                for i in 0..block_size {
                    assert!(
                        !buddy.is_reserved(pfn + i),
                        "free list order {} contains reserved pfn {}",
                        order,
                        pfn + i
                    );
                }
                // SAFETY: 测试 mock, phys 指向 mock_memory 内合法 FreeNode
                let next = unsafe { (*buddy.node_virt(phys)).next };
                phys = next;
            }
        }

        // 2) 持续分配, 绝不分到 reserved 页
        let mut allocated = 0u64;
        loop {
            match buddy.alloc_order(0) {
                Some(phys) => {
                    let pfn = phys / PAGE_SIZE;
                    assert!(
                        !buddy.is_reserved(pfn),
                        "allocated reserved pfn {}",
                        pfn
                    );
                    allocated += 1;
                }
                None => break,
            }
        }
        // 从 [1,513) 中扣除 reserved 的 50 页, 其余全部应能分配
        assert_eq!(allocated, 512 - 50, "free pages count mismatch");
    }

    // 回归: unreserve_range 后, 预留页重新进入空闲链表可被分配.
    #[test]
    fn buddy_reserve_unreserve_roundtrip() {
        let mut buddy = BuddyAllocator::new(1024);
        buddy.list_push(1, 9);
        buddy.om_set(1, 9);
        buddy.reserve_range(100, 50);
        // 分配 [100,150) 之外的页直到耗尽, 均不含 reserved 页
        loop {
            match buddy.alloc_order(0) {
                Some(phys) => assert!(!buddy.is_reserved(phys / PAGE_SIZE)),
                None => break,
            }
        }

        // unreserve 后, [100,150) 应重新可分配
        buddy.unreserve_range(100, 50);
        let mut got = 0u64;
        for _ in 0..50 {
            match buddy.alloc_order(0) {
                Some(phys) => {
                    let pfn = phys / PAGE_SIZE;
                    assert!(
                        (100..150).contains(&pfn),
                        "unreserved alloc returned pfn {} outside [100,150)",
                        pfn
                    );
                    got += 1;
                }
                None => break,
            }
        }
        assert_eq!(got, 50, "unreserved range should be fully re-allocatable");
    }
}
