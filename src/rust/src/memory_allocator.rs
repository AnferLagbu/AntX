use core::alloc::{GlobalAlloc, Layout};

extern "C" {
    fn pmm_alloc_page() -> *mut u8;
    fn pmm_free_page(addr: *mut u8);
    fn pmm_alloc_pages(count: u64) -> *mut u8;
}

pub struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let pages_needed = (size + 4095) / 4096;
        
        if pages_needed == 1 {
            pmm_alloc_page()
        } else {
            pmm_alloc_pages(pages_needed as u64)
        }
    }
    
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        pmm_free_page(ptr);
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;
