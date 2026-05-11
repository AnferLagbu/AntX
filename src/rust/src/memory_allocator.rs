use core::alloc::{GlobalAlloc, Layout};

extern "C" {
    fn pmm_alloc_page() -> *mut core::ffi::c_void;
    fn pmm_free_page(addr: *mut core::ffi::c_void);
    fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void;
    fn pmm_free_pages(addr: *mut core::ffi::c_void, count: u64);
    fn kmalloc(size: u64) -> *mut core::ffi::c_void;
    fn kfree(ptr: *mut core::ffi::c_void);
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
                return ptr as *mut u8;
            }
        }

        // Large allocations / kmalloc fallback: direct page allocation
        let pages_needed = (size + 4095) / 4096;
        if pages_needed == 1 {
            pmm_alloc_page() as *mut u8
        } else {
            pmm_alloc_pages(pages_needed as u64) as *mut u8
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() { return; }

        let size = layout.size();
        if size <= PAGE_THRESHOLD {
            kfree(ptr as *mut core::ffi::c_void);
        } else {
            let pages = ((size + 4095) / 4096) as u64;
            if pages <= 1 {
                pmm_free_page(ptr as *mut core::ffi::c_void);
            } else {
                pmm_free_pages(ptr as *mut core::ffi::c_void, pages);
            }
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;
