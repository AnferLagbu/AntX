//! 内存管理子系统 API 层
//!
//! 为内核其它模块提供统一的内存分配/释放/映射接口。
//! 所有公开函数都使用 `#[no_mangle]` 以保证符号名稳定,方便跨模块直接调用。
//!
//! ## 调用方契约
//! - `proc::api` —— 进程创建/销毁时的页表操作 (vmm_map_page / vmm_unmap_page)
//! - `crate::kernel::framework::proc::elf` —— ELF 加载时的 COW 页表克隆 (vmm_clone_user_page_table_cow)
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

/// 内核 malloc 统计结构 (C 兼容)
#[repr(C)]
pub struct KmallocStats {
    pub total_allocs: u64,
    pub total_frees: u64,
    pub current_usage: u64,
    pub peak_usage: u64,
}

// ============================================================
// PMM FFI 函数
// ============================================================

/// 初始化物理内存管理器
///
#[no_mangle]
pub fn pmm_init(mem_size: u64, kernel_end: u64) {
    super::pmm::pmm_init(mem_size, kernel_end);
}

/// 初始化位图以进行常规操作
///
#[no_mangle]
pub fn pmm_init_bitmap(reserved_after_kernel: u64) {
    super::pmm::pmm_init_bitmap(reserved_after_kernel);
}

/// 分配单个 4KB 页
///
#[no_mangle]
pub fn pmm_alloc_page() -> *mut u8 {
    let result = match get_pmm().alloc_page() {
        Some(addr) => addr.0 as *mut u8,
        None => core::ptr::null_mut(),
    };
    result
}

/// 释放单个页
///
#[no_mangle]
pub fn pmm_free_page(addr: *mut u8) {
    if !addr.is_null() {
        get_pmm().free_page(PhysAddr(addr as u64));
    }
}

/// 获取空闲页数量
///
#[no_mangle]
pub fn pmm_get_free_pages() -> u64 {
    get_pmm().get_free_pages()
}

/// 获取总页数
///
#[no_mangle]
pub fn pmm_get_total_pages() -> u64 {
    get_pmm().get_total_pages()
}

/// 获取已用页数
///
#[no_mangle]
pub fn pmm_get_used_pages() -> u64 {
    get_pmm().get_used_pages()
}

/// 分配多个连续页
///
#[no_mangle]
pub fn pmm_alloc_pages(count: usize) -> *mut u8 {
    let result = match get_pmm().alloc_pages(count) {
        Some(addr) => addr.0 as *mut u8,
        None => core::ptr::null_mut(),
    };
    result
}

/// 释放多个连续页
///
#[no_mangle]
pub fn pmm_free_pages(addr: *mut u8, count: usize) {
    if !addr.is_null() && count > 0 {
        get_pmm().free_pages(PhysAddr(addr as u64), count);
    }
}

/// 分配多个连续页, 返回物理地址.
///
/// 供需要 `PhysAddr` 类型安全的调用方使用.
pub fn pmm_alloc_pages_phys(count: usize) -> Option<super::PhysAddr> {
    get_pmm().alloc_pages(count)
}

/// 释放多个连续页 (物理地址).
pub fn pmm_free_pages_phys(addr: super::PhysAddr, count: usize) {
    get_pmm().free_pages(addr, count);
}

/// 打印 PMM 统计信息
///
#[no_mangle]
pub fn pmm_dump_stats() {
    get_pmm().dump_stats();
}

/// 分配一个大页 (2MB 或 1GB)
///
#[no_mangle]
pub fn pmm_alloc_huge_page(size_type: PageSize) -> *mut u8 {
    match get_pmm().alloc_huge_page(size_type) {
        Some(addr) => addr.0 as *mut u8,
        None => core::ptr::null_mut(),
    }
}

/// 释放一个大页
///
#[no_mangle]
pub fn pmm_free_huge_page(addr: *mut u8, size_type: PageSize) {
    if !addr.is_null() {
        get_pmm().free_huge_page(PhysAddr(addr as u64), size_type);
    }
}

/// 检查大页对齐
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
// VMM FFI 函数
// ============================================================

/// 初始化虚拟内存管理器
///
#[no_mangle]
pub fn vmm_init() {
    super::vmm::vmm_init();
}

/// 将虚拟页映射到物理页
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

/// 映射一个大页
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

/// 解除虚拟页映射
///
#[no_mangle]
pub fn vmm_unmap_page(virt: u64) {
    get_vmm().unmap_page(VirtAddr(virt));
}

/// 将 2MB 大页拆分为 512 个 4KB 页
///
#[no_mangle]
pub fn vmm_split_2mb_page(virt: u64) -> i32 {
    match get_vmm().split_2mb_page(virt) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 为虚拟地址对应的 PML4 项设置 USER 标志
///
#[no_mangle]
pub fn vmm_ensure_pml4_user(virt: u64) {
    get_vmm().ensure_pml4_user(virt);
}

/// 为路径上所有页表项设置 USER 标志, 以允许用户态访问
///
#[no_mangle]
pub fn vmm_ensure_path_user(virt: u64) {
    get_vmm().ensure_path_user(virt);
}

/// 获取虚拟地址对应的物理地址
///
#[no_mangle]
pub fn vmm_get_physical(virt: u64) -> u64 {
    match get_vmm().get_physical(VirtAddr(virt)) {
        Some(phys) => phys.as_u64(),
        None => 0,
    }
}

/// 在指定页表上下文中获取物理地址
///
#[no_mangle]
pub fn vmm_get_physical_in_table(pml4: u64, virt: u64) -> u64 {
    match get_vmm().get_physical_in_pml4(pml4, VirtAddr(virt)) {
        Some(phys) => phys.as_u64(),
        None => 0,
    }
}

/// 切换到其它页表 (加载 CR3)
///
#[no_mangle]
pub fn vmm_switch_page_table(cr3: u64) {
    get_vmm().switch_page_table(cr3);
}

/// 创建用户态页表
///
#[no_mangle]
pub fn vmm_create_user_page_table() -> u64 {
    match get_vmm().create_user_page_table() {
        Some(pml4) => pml4,
        None => 0,
    }
}

/// 在指定页表中映射 (用于用户态)
///
#[no_mangle]
pub fn vmm_map_page_in_table(pml4: u64, virt: u64, phys: u64, flags: u64) {
    let virt_addr = VirtAddr(virt);
    let phys_addr = PhysAddr(phys);
    let page_flags = PageFlags::from_bits_truncate(flags);

    get_vmm().map_page_in_table(pml4, virt_addr, phys_addr, page_flags);
}

/// 克隆用户页表 (深拷贝所有用户态映射)
///
#[no_mangle]
pub fn vmm_clone_user_page_table(parent_pml4: u64) -> u64 {
    get_vmm().clone_user_page_table(parent_pml4).unwrap_or(0)
}

/// 使用 COW (写时复制) 克隆用户页表
/// 共享页在父与子两侧均被标记为只读.
///
#[no_mangle]
pub fn vmm_clone_user_page_table_cow(parent_pml4: u64) -> u64 {
    super::cow::clone_user_page_table_cow(parent_pml4).unwrap_or(0)
}

/// 销毁页表并释放其关联的全部内存
///
#[no_mangle]
pub fn vmm_destroy_page_table(pml4: u64) {
    get_vmm().destroy_page_table(pml4);
}

/// 获取内核 PML4 地址 (访问全局变量)
///
/// 注: 这是对全局 KERNEL_PML4 变量的访问器
#[no_mangle]
pub static kernel_pml4: AtomicU64 = AtomicU64::new(0);

// ============================================================
// Kmalloc FFI 函数
// ============================================================

/// 从内核堆分配内存
///
#[no_mangle]
pub fn k_malloc(size: usize) -> *mut u8 {
    match get_kmalloc().allocate(size) {
        Some(ptr) => ptr as *mut u8,
        None => core::ptr::null_mut(),
    }
}

/// 释放 k_malloc 分配的内存
///
#[no_mangle]
pub fn k_free(ptr: *mut u8) {
    if !ptr.is_null() {
        get_kmalloc().deallocate(ptr as *mut u8);
    }
}

/// 重新分配内存块
///
#[no_mangle]
pub fn k_realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    match get_kmalloc().reallocate(ptr as *mut u8, size) {
        Some(new_ptr) => new_ptr as *mut u8,
        None => core::ptr::null_mut(),
    }
}

/// 初始化内核堆
///
#[no_mangle]
pub fn kmalloc_init(start: u64, initial_size: u64) {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        get_kmalloc_mut().init(VirtAddr(start), initial_size);
    }
}

/// 打印 kmalloc 统计
///
#[no_mangle]
pub fn kmalloc_dump_stats() {
    get_kmalloc().dump_stats();
}

/// 校验堆完整性 (调试用)
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
// 兼容性别名 (无下划线)
// 与原始 C API 函数名一致
// ============================================================

/// k_malloc 的别名 — 与原始 C API 一致: void* kmalloc(uint64_t size)
#[no_mangle]
pub fn kmalloc(size: u64) -> *mut u8 {
    k_malloc(size as usize)
}

/// k_free 的别名 — 与原始 C API 一致: void kfree(void* ptr)
#[no_mangle]
pub fn kfree(ptr: *mut u8) {
    k_free(ptr)
}

/// k_realloc 的别名 — 与原始 C API 一致: void* krealloc(void* ptr, uint64_t size)
#[no_mangle]
pub fn krealloc(ptr: *mut u8, size: u64) -> *mut u8 {
    k_realloc(ptr, size as usize)
}

/// 获取内核堆统计 — 与原始 C API 一致: void kmalloc_stats(struct kmalloc_stats* stats)
///
/// 注: 此为简化版本, 暂不填充结构
#[no_mangle]
pub fn kmalloc_stats(stats: *mut u8) {
    if stats.is_null() {
        return;
    }

    let heap_stats = get_kmalloc().get_stats();
    
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let stats_ptr = stats as *mut KmallocStats;
        (*stats_ptr).total_allocs = heap_stats.alloc_count;
        (*stats_ptr).total_frees = heap_stats.free_count;
        (*stats_ptr).current_usage = heap_stats.current_usage;
        (*stats_ptr).peak_usage = heap_stats.peak_usage;
    }
}

/// 转储内核堆信息 — 与原始 C API 一致: void kmalloc_dump(void)
#[no_mangle]
pub fn kmalloc_dump() {
    get_kmalloc().dump_stats();
}

// ============================================================
// VMA 公共接口
// ============================================================

/// 获取当前进程的内存描述符.
///
/// 通过公共 api 层访问, 避免直接引用 `mm::vma::get_current_mm`.
pub fn vma_get_current_mm() -> Option<&'static super::vma::MmStruct> {
    super::vma::get_current_mm()
}

// VMA 类型 re-export — 避免跨子系统直接引用 mm::vma 内部类型
pub use super::vma::{MmStruct, Vma, VmaType};

// copy_user re-export — 避免跨子系统直接引用 mm::copy_user 内部
pub use super::copy_user::{copy_to_user, copy_from_user, is_user_buf};

// pressure re-export — 避免跨子系统直接引用 mm::pressure 内部
pub use super::pressure::{update_pressure, MemoryPressure};

// page_fault re-export — 避免跨子系统直接引用 mm::page_fault 内部
pub use super::page_fault::{PfResult, PageFaultInfo, handle_page_fault, handle_user_page_fault};
