use core::alloc::{GlobalAlloc, Layout};

extern "C" {
    fn pmm_alloc_page() -> *mut core::ffi::c_void;
    fn pmm_free_page(addr: *mut core::ffi::c_void);
    fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void;
    fn pmm_free_pages(addr: *mut core::ffi::c_void, count: u64);
    fn kmalloc(size: u64) -> *mut core::ffi::c_void;
    fn kfree(ptr: *mut core::ffi::c_void);
}

#[cfg(target_arch = "aarch64")]
const KERNEL_BASE: u64 = 0u64;

#[cfg(not(target_arch = "aarch64"))]
const KERNEL_BASE: u64 = 0xFFFF800000000000u64;

pub struct KernelAllocator;

const PAGE_THRESHOLD: usize = 2048;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();

        if size <= PAGE_THRESHOLD {
            let ptr = kmalloc(size as u64);
            if !ptr.is_null() {
                return ptr as *mut u8;
            }
        }

        let pages_needed = (size + 4095) / 4096;
        if pages_needed == 1 {
            let phys = pmm_alloc_page() as u64;
            (phys + KERNEL_BASE) as *mut u8
        } else {
            let phys = pmm_alloc_pages(pages_needed as u64) as u64;
            (phys + KERNEL_BASE) as *mut u8
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() { return; }

        let size = layout.size();
        if size <= PAGE_THRESHOLD {
            kfree(ptr as *mut core::ffi::c_void);
        } else {
            let pages = ((size + 4095) / 4096) as u64;
            let phys_addr = (ptr as u64) - KERNEL_BASE;
            if pages <= 1 {
                pmm_free_page(phys_addr as *mut core::ffi::c_void);
            } else {
                pmm_free_pages(phys_addr as *mut core::ffi::c_void, pages);
            }
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;
