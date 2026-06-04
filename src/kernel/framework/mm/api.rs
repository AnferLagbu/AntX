//! 内存管理子系统 API 层
//!
//! 为内核其它模块提供统一的内存分配/释放/映射接口。
//! 所有公开函数都使用 `#[no_mangle]` 以保证符号名稳定,方便跨模块直接调用。
//!
//! ## 调用方契约
//! - `proc::api` —— 进程创建/销毁时的页表操作 (vmm_map_page / vmm_unmap_page)
//! - `crate::kernel::framework::proc_legacy::elf` —— ELF 加载时的 COW 页表克隆 (vmm_clone_user_page_table_cow)
//! - `fs::ramfs` / `fs::hvfs` —— 文件系统页缓存分配 (pmm_alloc_page / pmm_free_page)
//! - `ipc::shm` —— 共享内存段的物理页映射
//! - `driver::*` —— 各驱动的 DMA 缓冲区分配
//! - `credo::storage` —— 持久化数据写入时的内存申请
//!
//! ## 安全约束
//! - 所有指针参数在函数入口处做 is_null 检查
//! - `pmm_alloc_*` 返回 null 时调用方必须处理 OOM
//! - `vmm_map_page` 不检查地址冲突,调用方负责确保不重复映射
//! - 物理页分配器 (PM) 由 spinlock 保护,可在中断上下文调用
//! - 页表操作 (VMM) 不可在中断上下文调用 (需要锁)
//!
//! ## 性能特征
//! - PM 分配: 位图扫描 O(N), Buddy 优化后 O(log N)
//! - Slab 分配: O(1) 缓存命中, 无锁 per-CPU 缓存
//! - VMM 映射: 四级页表遍历 O(4), 常数时间
//!
//! 设计目标:
//! - 隐藏内部实现细节(pmm/vmm/slab)
//! - 提供纯 Rust 抽象,无 C ABI 依赖
//! - 异常路径:空指针 / 越界 / 内存不足一律返回 0 / -1 / null,调用方按需检查

use super::*;
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
#[no_mangle]
pub fn pmm_init(mem_size: u64, kernel_end: u64) {
    super::pmm::pmm_init(mem_size, kernel_end);
}

/// Initialize bitmap for normal operation
///
#[no_mangle]
pub fn pmm_init_bitmap(reserved_after_kernel: u64) {
    super::pmm::pmm_init_bitmap(reserved_after_kernel);
}

/// Allocate a single 4KB page
///
#[no_mangle]
pub fn pmm_alloc_page() -> *mut u8 {
    let result = match get_pmm().alloc_page() {
        Some(addr) => addr.0 as *mut u8,
        None => core::ptr::null_mut(),
    };
    result
}

/// Free a single page
///
#[no_mangle]
pub fn pmm_free_page(addr: *mut u8) {
    if !addr.is_null() {
        get_pmm().free_page(PhysAddr(addr as u64));
    }
}

/// Get number of free pages
///
#[no_mangle]
pub fn pmm_get_free_pages() -> u64 {
    get_pmm().get_free_pages()
}

/// Get total number of pages
///
#[no_mangle]
pub fn pmm_get_total_pages() -> u64 {
    get_pmm().get_total_pages()
}

/// Get number of used pages
///
#[no_mangle]
pub fn pmm_get_used_pages() -> u64 {
    get_pmm().get_used_pages()
}

/// Allocate multiple contiguous pages
///
#[no_mangle]
pub fn pmm_alloc_pages(count: usize) -> *mut u8 {
    let result = match get_pmm().alloc_pages(count) {
        Some(addr) => addr.0 as *mut u8,
        None => core::ptr::null_mut(),
    };
    result
}

/// Free multiple contiguous pages
///
#[no_mangle]
pub fn pmm_free_pages(addr: *mut u8, count: usize) {
    if !addr.is_null() && count > 0 {
        get_pmm().free_pages(PhysAddr(addr as u64), count);
    }
}

/// Print PMM statistics
///
#[no_mangle]
pub fn pmm_dump_stats() {
    get_pmm().dump_stats();
}

/// Allocate a huge page (2MB or 1GB)
///
#[no_mangle]
pub fn pmm_alloc_huge_page(size_type: PageSize) -> *mut u8 {
    match get_pmm().alloc_huge_page(size_type) {
        Some(addr) => addr.0 as *mut u8,
        None => core::ptr::null_mut(),
    }
}

/// Free a huge page
///
#[no_mangle]
pub fn pmm_free_huge_page(addr: *mut u8, size_type: PageSize) {
    if !addr.is_null() {
        get_pmm().free_huge_page(PhysAddr(addr as u64), size_type);
    }
}

/// Check alignment for huge page
///
#[no_mangle]
pub fn pmm_is_aligned_for_huge(addr: *const u8, size_type: PageSize) -> i32 {
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
#[no_mangle]
pub fn vmm_init() {
    super::vmm::vmm_init();
}

/// Map a virtual page to physical page
///
#[no_mangle]
pub fn vmm_map_page(virt: u64, phys: u64, flags: u64) -> i32 {
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
#[no_mangle]
pub fn vmm_map_huge_page(virt: u64, phys: u64, flags: u64, size_type: PageSize) -> i32 {
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
#[no_mangle]
pub fn vmm_unmap_page(virt: u64) {
    get_vmm().unmap_page(VirtAddr(virt));
}

/// Split a 2MB huge page into 512 4KB pages
///
#[no_mangle]
pub fn vmm_split_2mb_page(virt: u64) -> i32 {
    match get_vmm().split_2mb_page(virt) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Set USER flag on PML4 entry for a virtual address
///
#[no_mangle]
pub fn vmm_ensure_pml4_user(virt: u64) {
    get_vmm().ensure_pml4_user(virt);
}

/// Set USER flag on all page table entries in path for user access
///
#[no_mangle]
pub fn vmm_ensure_path_user(virt: u64) {
    get_vmm().ensure_path_user(virt);
}

/// Get physical address for virtual address
///
#[no_mangle]
pub fn vmm_get_physical(virt: u64) -> u64 {
    match get_vmm().get_physical(VirtAddr(virt)) {
        Some(phys) => phys.as_u64(),
        None => 0,
    }
}

/// Get physical address in specific page table context
///
#[no_mangle]
pub fn vmm_get_physical_in_table(pml4: u64, virt: u64) -> u64 {
    match get_vmm().get_physical_in_pml4(pml4, VirtAddr(virt)) {
        Some(phys) => phys.as_u64(),
        None => 0,
    }
}

/// Switch to different page table (load CR3)
///
#[no_mangle]
pub fn vmm_switch_page_table(cr3: u64) {
    get_vmm().switch_page_table(cr3);
}

/// Create user space page table
///
#[no_mangle]
pub fn vmm_create_user_page_table() -> u64 {
    match get_vmm().create_user_page_table() {
        Some(pml4) => pml4,
        None => 0,
    }
}

/// Map page in specific table (for user space)
///
#[no_mangle]
pub fn vmm_map_page_in_table(pml4: u64, virt: u64, phys: u64, flags: u64) {
    let virt_addr = VirtAddr(virt);
    let phys_addr = PhysAddr(phys);
    let page_flags = PageFlags::from_bits_truncate(flags);

    get_vmm().map_page_in_table(pml4, virt_addr, phys_addr, page_flags);
}

/// Clone a user page table (deep copy of all user-space mappings)
///
#[no_mangle]
pub fn vmm_clone_user_page_table(parent_pml4: u64) -> u64 {
    get_vmm().clone_user_page_table(parent_pml4).unwrap_or(0)
}

/// Clone a user page table using COW (Copy-on-Write)
/// Shared pages are marked read-only in both parent and child.
///
#[no_mangle]
pub fn vmm_clone_user_page_table_cow(parent_pml4: u64) -> u64 {
    super::cow::clone_user_page_table_cow(parent_pml4).unwrap_or(0)
}

/// Destroy a page table and free all associated memory
///
#[no_mangle]
pub fn vmm_destroy_page_table(pml4: u64) {
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
#[no_mangle]
pub fn k_malloc(size: usize) -> *mut u8 {
    match get_kmalloc().allocate(size) {
        Some(ptr) => ptr as *mut u8,
        None => core::ptr::null_mut(),
    }
}

/// Free memory allocated by k_malloc
///
#[no_mangle]
pub fn k_free(ptr: *mut u8) {
    if !ptr.is_null() {
        get_kmalloc().deallocate(ptr as *mut u8);
    }
}

/// Reallocate memory block
///
#[no_mangle]
pub fn k_realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    match get_kmalloc().reallocate(ptr as *mut u8, size) {
        Some(new_ptr) => new_ptr as *mut u8,
        None => core::ptr::null_mut(),
    }
}

/// Initialize kernel heap
///
#[no_mangle]
pub fn kmalloc_init(start: u64, initial_size: u64) {
    unsafe {
        get_kmalloc_mut().init(VirtAddr(start), initial_size);
    }
}

/// Print kmalloc statistics
///
#[no_mangle]
pub fn kmalloc_dump_stats() {
    get_kmalloc().dump_stats();
}

/// Validate heap integrity (for debugging)
///
#[no_mangle]
pub fn kmalloc_validate() -> i32 {
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
pub fn kmalloc(size: u64) -> *mut u8 {
    k_malloc(size as usize)
}

/// Alias for k_free - matches original C API: void kfree(void* ptr)
#[no_mangle]
pub fn kfree(ptr: *mut u8) {
    k_free(ptr)
}

/// Alias for k_realloc - matches original C API: void* krealloc(void* ptr, uint64_t size)
#[no_mangle]
pub fn krealloc(ptr: *mut u8, size: u64) -> *mut u8 {
    k_realloc(ptr, size as usize)
}

/// Get kernel heap statistics - matches original C API: void kmalloc_stats(struct kmalloc_stats* stats)
///
/// Note: This is a simplified version that doesn't fill the struct yet
#[no_mangle]
pub fn kmalloc_stats(stats: *mut u8) {
    if stats.is_null() {
        return;
    }

    let heap_stats = get_kmalloc().get_stats();
    
    unsafe {
        let stats_ptr = stats as *mut KmallocStats;
        (*stats_ptr).total_allocs = heap_stats.alloc_count;
        (*stats_ptr).total_frees = heap_stats.free_count;
        (*stats_ptr).current_usage = heap_stats.current_usage;
        (*stats_ptr).peak_usage = heap_stats.peak_usage;
    }
}

/// Dump kernel heap information - matches original C API: void kmalloc_dump(void)
#[no_mangle]
pub fn kmalloc_dump() {
    get_kmalloc().dump_stats();
}
