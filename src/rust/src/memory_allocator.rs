use core::alloc::{GlobalAlloc, Layout};

extern "C" {
    fn pmm_alloc_page() -> *mut u8;
    fn pmm_free_page(addr: *mut u8);
    fn pmm_alloc_pages(count: u64) -> *mut u8;
    fn pmm_free_pages(addr: *mut u8, count: u64);
    fn kmalloc(size: u64) -> *mut u8;
    fn kfree(ptr: *mut u8);
}

pub struct KernelAllocator;

/// Page threshold: allocations ≤ PAGE_THRESHOLD use kmalloc,
/// larger allocations use pmm_alloc_pages directly.
const PAGE_THRESHOLD: usize = 2048;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        
        // Small allocations: use kmalloc free-list (no page waste)
        if size <= PAGE_THRESHOLD {
            let ptr = kmalloc(size as u64);
            if !ptr.is_null() {
                return ptr;
            }
        }
        
        // Large allocations / kmalloc fallback: direct page allocation
        let pages_needed = (size + 4095) / 4096;
        if pages_needed == 1 {
            pmm_alloc_page()
        } else {
            pmm_alloc_pages(pages_needed as u64)
        }
    }
    
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() { return; }
        
        let size = layout.size();
        if size <= PAGE_THRESHOLD {
            kfree(ptr);
        } else {
            let pages = ((size + 4095) / 4096) as u64;
            if pages <= 1 {
                pmm_free_page(ptr);
            } else {
                pmm_free_pages(ptr, pages);
            }
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;
