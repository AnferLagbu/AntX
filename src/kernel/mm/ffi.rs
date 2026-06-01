//! FFI Interface Layer for Memory Management Subsystem
//!
//! Provides C-compatible interface functions that wrap the Rust implementation.
//! All functions use `#[no_mangle]` and `extern "C"` to ensure ABI compatibility.
//!
//! This layer maintains the same API as the original C implementation,
//! allowing a drop-in replacement without modifying existing C code.

use super::*;
use core::ffi::c_void;
use core::sync::atomic::AtomicU64;

/// Kernel malloc statistics structure (C-compatible)
#[repr(C)]
pub struct KmallocStats {
    pub total_allocs: u64,
    pub total_frees: u64,
    pub current_usage: u64,
    pub peak_usage: u64,
}

// ============================================================
// PMM FFI Functions
// ============================================================

/// Initialize physical memory manager
///
/// C signature: void pmm_init(uint64_t mem_size, uint64_t kernel_end)
#[no_mangle]
pub extern "C" fn pmm_init(mem_size: u64, kernel_end: u64) {
    super::pmm::pmm_init(mem_size, kernel_end);
}

/// Initialize bitmap for normal operation
///
/// C signature: void pmm_init_bitmap(uint64_t reserved_after_kernel)
#[no_mangle]
pub extern "C" fn pmm_init_bitmap(reserved_after_kernel: u64) {
    super::pmm::pmm_init_bitmap(reserved_after_kernel);
}

/// Allocate a single 4KB page
///
/// C signature: void* pmm_alloc_page(void)
#[no_mangle]
pub extern "C" fn pmm_alloc_page() -> *mut c_void {
    let result = match get_pmm().alloc_page() {
        Some(addr) => addr.0 as *mut c_void,
        None => core::ptr::null_mut(),
    };
    result
}

/// Free a single page
///
/// C signature: void pmm_free_page(void* addr)
#[no_mangle]
pub extern "C" fn pmm_free_page(addr: *mut c_void) {
    if !addr.is_null() {
        get_pmm().free_page(PhysAddr(addr as u64));
    }
}

/// Get number of free pages
///
/// C signature: uint64_t pmm_get_free_pages(void)
#[no_mangle]
pub extern "C" fn pmm_get_free_pages() -> u64 {
    get_pmm().get_free_pages()
}

/// Get total number of pages
///
/// C signature: uint64_t pmm_get_total_pages(void)
#[no_mangle]
pub extern "C" fn pmm_get_total_pages() -> u64 {
    get_pmm().get_total_pages()
}

/// Get number of used pages
///
/// C signature: uint64_t pmm_get_used_pages(void)
#[no_mangle]
pub extern "C" fn pmm_get_used_pages() -> u64 {
    get_pmm().get_used_pages()
}

/// Allocate multiple contiguous pages
///
/// C signature: void* pmm_alloc_pages(size_t count)
#[no_mangle]
pub extern "C" fn pmm_alloc_pages(count: usize) -> *mut c_void {
    let result = match get_pmm().alloc_pages(count) {
        Some(addr) => addr.0 as *mut c_void,
        None => core::ptr::null_mut(),
    };
    result
}

/// Free multiple contiguous pages
///
/// C signature: void pmm_free_pages(void* addr, size_t count)
#[no_mangle]
pub extern "C" fn pmm_free_pages(addr: *mut c_void, count: usize) {
    if !addr.is_null() && count > 0 {
        get_pmm().free_pages(PhysAddr(addr as u64), count);
    }
}

/// Print PMM statistics
///
/// C signature: void pmm_dump_stats(void)
#[no_mangle]
pub extern "C" fn pmm_dump_stats() {
    get_pmm().dump_stats();
}

/// Allocate a huge page (2MB or 1GB)
///
/// C signature: void* pmm_alloc_huge_page(page_size_t size_type)
#[no_mangle]
pub extern "C" fn pmm_alloc_huge_page(size_type: PageSize) -> *mut c_void {
    match get_pmm().alloc_huge_page(size_type) {
        Some(addr) => addr.0 as *mut c_void,
        None => core::ptr::null_mut(),
    }
}

/// Free a huge page
///
/// C signature: void pmm_free_huge_page(void* addr, page_size_t size_type)
#[no_mangle]
pub extern "C" fn pmm_free_huge_page(addr: *mut c_void, size_type: PageSize) {
    if !addr.is_null() {
        get_pmm().free_huge_page(PhysAddr(addr as u64), size_type);
    }
}

/// Check alignment for huge page
///
/// C signature: int pmm_is_aligned_for_huge(void* addr, page_size_t size_type)
#[no_mangle]
pub extern "C" fn pmm_is_aligned_for_huge(addr: *const c_void, size_type: PageSize) -> i32 {
    if addr.is_null() {
        return 0;
    }

    if get_pmm().is_aligned_for_huge(PhysAddr(addr as u64), size_type) {
        1
    } else {
        0
    }
}

// ============================================================
// VMM FFI Functions
// ============================================================

/// Initialize virtual memory manager
///
/// C signature: void vmm_init(void)
#[no_mangle]
pub extern "C" fn vmm_init() {
    super::vmm::vmm_init();
}

/// Map a virtual page to physical page
///
/// C signature: int vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags)
#[no_mangle]
pub extern "C" fn vmm_map_page(virt: u64, phys: u64, flags: u64) -> i32 {
    let virt_addr = VirtAddr(virt);
    let phys_addr = PhysAddr(phys);
    let page_flags = PageFlags::from_bits_truncate(flags);

    match get_vmm().map_page(virt_addr, phys_addr, page_flags) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Map a huge page
///
/// C signature: int vmm_map_huge_page(uint64_t virt, uint64_t phys, uint64_t flags, page_size_t size_type)
#[no_mangle]
pub extern "C" fn vmm_map_huge_page(virt: u64, phys: u64, flags: u64, size_type: PageSize) -> i32 {
    let virt_addr = VirtAddr(virt);
    let phys_addr = PhysAddr(phys);
    let page_flags = PageFlags::from_bits_truncate(flags);

    match get_vmm().map_huge_page(virt_addr, phys_addr, page_flags, size_type) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Unmap a virtual page
///
/// C signature: void vmm_unmap_page(uint64_t virt)
#[no_mangle]
pub extern "C" fn vmm_unmap_page(virt: u64) {
    get_vmm().unmap_page(VirtAddr(virt));
}

/// Split a 2MB huge page into 512 4KB pages
///
/// C signature: int vmm_split_2mb_page(uint64_t virt)
#[no_mangle]
pub extern "C" fn vmm_split_2mb_page(virt: u64) -> i32 {
    match get_vmm().split_2mb_page(virt) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Set USER flag on PML4 entry for a virtual address
///
/// C signature: void vmm_ensure_pml4_user(uint64_t virt)
#[no_mangle]
pub extern "C" fn vmm_ensure_pml4_user(virt: u64) {
    get_vmm().ensure_pml4_user(virt);
}

/// Set USER flag on all page table entries in path for user access
///
/// C signature: void vmm_ensure_path_user(uint64_t virt)
#[no_mangle]
pub extern "C" fn vmm_ensure_path_user(virt: u64) {
    get_vmm().ensure_path_user(virt);
}

/// Get physical address for virtual address
///
/// C signature: uint64_t vmm_get_physical(uint64_t virt)
#[no_mangle]
pub extern "C" fn vmm_get_physical(virt: u64) -> u64 {
    match get_vmm().get_physical(VirtAddr(virt)) {
        Some(phys) => phys.as_u64(),
        None => 0,
    }
}

/// Get physical address in specific page table context
///
/// C signature: uint64_t vmm_get_physical_in_table(uint64_t pml4, uint64_t virt)
#[no_mangle]
pub extern "C" fn vmm_get_physical_in_table(pml4: u64, virt: u64) -> u64 {
    match get_vmm().get_physical_in_pml4(pml4, VirtAddr(virt)) {
        Some(phys) => phys.as_u64(),
        None => 0,
    }
}

/// Switch to different page table (load CR3)
///
/// C signature: void vmm_switch_page_table(uint64_t cr3)
#[no_mangle]
pub extern "C" fn vmm_switch_page_table(cr3: u64) {
    get_vmm().switch_page_table(cr3);
}

/// Create user space page table
///
/// C signature: uint64_t vmm_create_user_page_table(void)
#[no_mangle]
pub extern "C" fn vmm_create_user_page_table() -> u64 {
    match get_vmm().create_user_page_table() {
        Some(pml4) => pml4,
        None => 0,
    }
}

/// Map page in specific table (for user space)
///
/// C signature: void vmm_map_page_in_table(uint64_t pml4, uint64_t virt, uint64_t phys, uint64_t flags)
#[no_mangle]
pub extern "C" fn vmm_map_page_in_table(pml4: u64, virt: u64, phys: u64, flags: u64) {
    let virt_addr = VirtAddr(virt);
    let phys_addr = PhysAddr(phys);
    let page_flags = PageFlags::from_bits_truncate(flags);

    get_vmm().map_page_in_table(pml4, virt_addr, phys_addr, page_flags);
}

/// Clone a user page table (deep copy of all user-space mappings)
///
/// C signature: uint64_t vmm_clone_user_page_table(uint64_t parent_pml4)
#[no_mangle]
pub extern "C" fn vmm_clone_user_page_table(parent_pml4: u64) -> u64 {
    get_vmm().clone_user_page_table(parent_pml4).unwrap_or(0)
}

/// Clone a user page table using COW (Copy-on-Write)
/// Shared pages are marked read-only in both parent and child.
///
/// C signature: uint64_t vmm_clone_user_page_table_cow(uint64_t parent_pml4)
#[no_mangle]
pub extern "C" fn vmm_clone_user_page_table_cow(parent_pml4: u64) -> u64 {
    super::cow::clone_user_page_table_cow(parent_pml4).unwrap_or(0)
}

/// Destroy a page table and free all associated memory
///
/// C signature: void vmm_destroy_page_table(uint64_t pml4)
#[no_mangle]
pub extern "C" fn vmm_destroy_page_table(pml4: u64) {
    get_vmm().destroy_page_table(pml4);
}

/// Get kernel PML4 address (global variable access)
///
/// Note: This is an accessor for the global KERNEL_PML4 variable
#[no_mangle]
pub static kernel_pml4: AtomicU64 = AtomicU64::new(0);

// ============================================================
// Kmalloc FFI Functions
// ============================================================

/// Allocate memory from kernel heap
///
/// C signature: void* k_malloc(size_t size)
#[no_mangle]
pub extern "C" fn k_malloc(size: usize) -> *mut c_void {
    match get_kmalloc().allocate(size) {
        Some(ptr) => ptr as *mut c_void,
        None => core::ptr::null_mut(),
    }
}

/// Free memory allocated by k_malloc
///
/// C signature: void k_free(void* ptr)
#[no_mangle]
pub extern "C" fn k_free(ptr: *mut c_void) {
    if !ptr.is_null() {
        get_kmalloc().deallocate(ptr as *mut u8);
    }
}

/// Reallocate memory block
///
/// C signature: void* k_realloc(void* ptr, size_t size)
#[no_mangle]
pub extern "C" fn k_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    match get_kmalloc().reallocate(ptr as *mut u8, size) {
        Some(new_ptr) => new_ptr as *mut c_void,
        None => core::ptr::null_mut(),
    }
}

/// Initialize kernel heap
///
/// C signature: void kmalloc_init(uint64_t start, uint64_t initial_size)
#[no_mangle]
pub extern "C" fn kmalloc_init(start: u64, initial_size: u64) {
    unsafe {
        get_kmalloc_mut().init(VirtAddr(start), initial_size);
    }
}

/// Print kmalloc statistics
///
/// C signature: void kmalloc_dump_stats(void)
#[no_mangle]
pub extern "C" fn kmalloc_dump_stats() {
    get_kmalloc().dump_stats();
}

/// Validate heap integrity (for debugging)
///
/// C signature: int kmalloc_validate(void)
#[no_mangle]
pub extern "C" fn kmalloc_validate() -> i32 {
    if get_kmalloc().validate() {
        1
    } else {
        0
    }
}

// ============================================================
// Compatibility Aliases (without underscore)
// These match the original C API function names
// ============================================================

/// Alias for k_malloc - matches original C API: void* kmalloc(uint64_t size)
#[no_mangle]
pub extern "C" fn kmalloc(size: u64) -> *mut c_void {
    k_malloc(size as usize)
}

/// Alias for k_free - matches original C API: void kfree(void* ptr)
#[no_mangle]
pub extern "C" fn kfree(ptr: *mut c_void) {
    k_free(ptr)
}

/// Alias for k_realloc - matches original C API: void* krealloc(void* ptr, uint64_t size)
#[no_mangle]
pub extern "C" fn krealloc(ptr: *mut c_void, size: u64) -> *mut c_void {
    k_realloc(ptr, size as usize)
}

/// Get kernel heap statistics - matches original C API: void kmalloc_stats(struct kmalloc_stats* stats)
///
/// Note: This is a simplified version that doesn't fill the struct yet
#[no_mangle]
pub extern "C" fn kmalloc_stats(stats: *mut c_void) {
    if stats.is_null() {
        return;
    }

    let kmalloc = get_kmalloc();
    
    unsafe {
        let stats_ptr = stats as *mut KmallocStats;
        (*stats_ptr).total_allocs = kmalloc.alloc_count.load(Ordering::Relaxed);
        (*stats_ptr).total_frees = kmalloc.free_count.load(Ordering::Relaxed);
        (*stats_ptr).current_usage = kmalloc.current_usage.load(Ordering::Relaxed);
        (*stats_ptr).peak_usage = kmalloc.peak_usage.load(Ordering::Relaxed);
    }
}

/// Dump kernel heap information - matches original C API: void kmalloc_dump(void)
#[no_mangle]
pub extern "C" fn kmalloc_dump() {
    get_kmalloc().dump_stats();
}
