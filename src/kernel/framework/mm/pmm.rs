//! 物理内存管理器 (PMM) — Buddy 分配器
//!
//! 使用 buddy 分配器管理物理内存页, 提供 O(1) 空闲链表操作
//! 以及 O(log n) 释放合并.
//!
//! # 设计
//! - 阶数 0–9 (4 KB – 2 MB 连续块).
//! - Buddy 元数据建立前使用早期 (线性) 分配器.
//! - 保留位图用于 reserved 页跟踪和统计.
//! - 双向链表的侵入式空闲链表 (prev/next 存放在空闲页内).
//! - Buddy 合并使用按页的阶数元数据, 实现 O(1) 伙伴检查.
//!
//! # 安全
//! 所有修改都在内部 `AtomicBool` 自旋锁下进行.

macro_rules! klog_pmm {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

use super::*;
use crate::kernel::framework::sync::{disable_interrupts, restore_interrupts, IrqSaveFlags};
use core::cell::{Cell, UnsafeCell};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};


use crate::kernel::framework::sync::IrqSpinLock;

use crate::kernel::framework::sync::OnceLock;
const MAX_EARLY_ALLOCS: usize = 256;

/// 最大 buddy 阶数: 2^9 × 4 KB = 2 MB
const MAX_BUDDY_ORDER: u8 = 9;
/// buddy_meta 中的哨兵值: 页面已分配 / 不是空闲链表头
const BUDDY_ALLOCATED: u8 = 0xFF;

/// 物理 RAM 基地址
/// x86_64: 0 (multiboot 给出的物理内存从 0 开始)
/// aarch64: 0x40000000 (QEMU virt 机器 RAM 基址)
#[cfg(target_arch = "x86_64")]
const RAM_BASE: u64 = 0;
#[cfg(target_arch = "aarch64")]
const RAM_BASE: u64 = 0x40000000;

#[inline(always)]
fn phys_to_page(phys: u64) -> u64 {
    (phys - RAM_BASE) / PAGE_SIZE
}

#[inline(always)]
fn page_to_phys(page: u64) -> u64 {
    RAM_BASE + page * PAGE_SIZE
}

#[inline(always)]
fn pfn_to_virt(pfn: u64) -> *mut u8 {
    (page_to_phys(pfn) + KERNEL_BASE) as *mut u8
}

/// 将页数向上取整到 2 的幂 → 对应 buddy 阶数
///
/// T2-2: 策略已提取到 pmm_trait::PmmPolicy, 本函数保留为内部快捷路径
/// (直接调用 current_pmm_policy().count_to_order()).
#[inline]
fn count_to_order(count: usize) -> u8 {
    super::pmm_trait::current_pmm_policy().count_to_order(count, MAX_BUDDY_ORDER)
}

#[derive(Clone, Copy)]
struct EarlyAlloc {
    addr: u64,
    size: u64,
}

impl EarlyAlloc {
    pub const fn const_default() -> Self {
        Self { addr: 0, size: 0 }
    }
}

// ---- 侵入式双向空闲链表节点, 存放在空闲页内 ----
// SAFETY: 仅在 PMM 锁保护下访问; 每个空闲页提供 4096 字节
// 存储空间, 我们用前 16 字节存放 prev/next 指针.
#[repr(C)]
pub(crate) struct FreeNode {
    prev: *mut FreeNode,
    next: *mut FreeNode,
}

// === E3: unsafe 集中化 — 裸指针子模块 ===
//
// buddy 分配器内部涉及的所有裸指针解引用都
// 封装在这里.  外层 `PhysicalMemoryManager` 方法只调用
// safe 包装器, 使 buddy 分配算法本身保持 safe Rust.
pub(crate) mod raw {
    use super::*;

    // ---- FreeNode safe 包装器 ----
    // SAFETY 不变式: 指针指向物理 RAM 内空闲页中的合法 FreeNode,
    // 且 PMM 锁已持有.
    #[derive(Clone, Copy)]
    pub struct FreeNodeRef(*mut FreeNode);

    impl FreeNodeRef {
        /// # Safety
        /// - `ptr` 必须指向空闲页内合法的 `FreeNode`
        /// - 使用期间必须持有 PMM 锁
        #[inline(always)]
        pub unsafe fn new_unchecked(ptr: *mut FreeNode) -> Self {
            Self(ptr)
        }

        #[inline(always)]
        #[allow(dead_code)] // 待 PMM 调试/诊断路径启用后使用。
        pub fn is_null(self) -> bool {
            self.0.is_null()
        }

        #[inline(always)]
        #[allow(dead_code)] // 待 PMM 调试/诊断路径启用后使用。
        pub fn as_ptr(self) -> *mut FreeNode {
            self.0
        }

        #[inline(always)]
        pub fn prev(&self) -> *mut FreeNode {
            // SAFETY: FreeNodeRef 由 new_unchecked 保证指针有效, 读 prev 链指针 (PMM 锁持有)
            unsafe { (*self.0).prev }
        }

        #[inline(always)]
        pub fn next(&self) -> *mut FreeNode {
            // SAFETY: FreeNodeRef 由 new_unchecked 保证指针有效, 读 next 链指针 (PMM 锁持有)
            unsafe { (*self.0).next }
        }

        #[inline(always)]
        pub fn set_prev(&self, p: *mut FreeNode) {
            // SAFETY: FreeNodeRef 由 new_unchecked 保证指针有效, 写 prev 链指针 (PMM 锁持有)
            unsafe { (*self.0).prev = p; }
        }

        #[inline(always)]
        pub fn set_next(&self, p: *mut FreeNode) {
            // SAFETY: FreeNodeRef 由 new_unchecked 保证指针有效, 写 next 链指针 (PMM 锁持有)
            unsafe { (*self.0).next = p; }
        }
    }

    // ---- Buddy 元数据 safe 包装器 ----
    // SAFETY 不变式: meta 指针在 init_bitmap 后有效; idx < total_pages
    pub struct MetaRef {
        ptr: *mut u8,
    }

    impl MetaRef {
        /// # Safety
        /// - `ptr` 必须指向合法的 buddy 元数据数组
        /// - 使用期间必须持有 PMM 锁
        #[inline(always)]
        pub unsafe fn new_unchecked(ptr: *mut u8) -> Self {
            Self { ptr }
        }

        #[inline(always)]
        pub fn read(&self, idx: usize) -> u8 {
            // SAFETY: 调用方保证 idx < total_pages, ptr 合法
            unsafe { *self.ptr.add(idx) }
        }

        #[inline(always)]
        pub fn write(&self, idx: usize, val: u8) {
            // SAFETY: 调用方保证 idx < total_pages, ptr 合法
            unsafe { *self.ptr.add(idx) = val; }
        }
    }

    // ---- Bitmap safe 包装器 ----
    // SAFETY 不变式: bitmap 指针在 init_bitmap 后有效; word < bitmap_size
    pub struct BitmapRef {
        ptr: NonNull<u32>,
    }

    impl BitmapRef {
        #[inline(always)]
        pub fn new(ptr: NonNull<u32>) -> Self {
            Self { ptr }
        }

        #[inline(always)]
        pub fn set_bit(&self, bit: usize, bitmap_size: usize) {
            let word = bit / 32;
            if word < bitmap_size {
                // SAFETY: word < bitmap_size guarantees valid access
                unsafe {
                    let p = self.ptr.as_ptr().add(word) as *const AtomicU32;
                    (*p).fetch_or(1u32 << (bit % 32), Ordering::Relaxed);
                }
            }
        }

        #[inline(always)]
        pub fn clear_bit(&self, bit: usize, bitmap_size: usize) {
            let word = bit / 32;
            if word < bitmap_size {
                // SAFETY: word < bitmap_size guarantees valid access
                unsafe {
                    let p = self.ptr.as_ptr().add(word) as *const AtomicU32;
                    (*p).fetch_and(!(1u32 << (bit % 32)), Ordering::Relaxed);
                }
            }
        }

        #[inline(always)]
        pub fn test_bit(&self, bit: usize, bitmap_size: usize) -> bool {
            let word = bit / 32;
            if word < bitmap_size {
                // SAFETY: word < bitmap_size guarantees valid access
                unsafe {
                    let p = self.ptr.as_ptr().add(word) as *const AtomicU32;
                    (*p).load(Ordering::Relaxed) & (1u32 << (bit % 32)) != 0
                }
            } else {
                false
            }
        }

        #[inline(always)]
        pub fn count_free(&self, bitmap_size: usize) -> u64 {
            let mut free: u64 = 0;
            for w in 0..bitmap_size {
                // SAFETY: w < bitmap_size guarantees valid access
                unsafe {
                    let p = self.ptr.as_ptr().add(w) as *const AtomicU32;
                    free += (!(*p).load(Ordering::Relaxed)).count_ones() as u64;
                }
            }
            free
        }
    }

    // ---- Buddy heads safe 包装器 ----
    // SAFETY 不变式: buddy_heads 仅在 PMM 锁保护下访问
    pub struct HeadsRef {
        ptr: *mut [*mut FreeNode; MAX_BUDDY_ORDER as usize + 1],
    }

    impl HeadsRef {
        /// # Safety
        /// - `ptr` 必须指向合法的 buddy_heads 数组
        /// - 使用期间必须持有 PMM 锁
        #[inline(always)]
        pub unsafe fn new_unchecked(
            ptr: *mut [*mut FreeNode; MAX_BUDDY_ORDER as usize + 1],
        ) -> Self {
            Self { ptr }
        }

        #[inline(always)]
        pub fn head(&self, order: u8) -> *mut FreeNode {
            // SAFETY: order <= MAX_BUDDY_ORDER, ptr valid under lock
            unsafe { (*self.ptr)[order as usize] }
        }

        #[inline(always)]
        pub fn set_head(&self, order: u8, node: *mut FreeNode) {
            // SAFETY: order <= MAX_BUDDY_ORDER, ptr 持锁时合法
            unsafe { (*self.ptr)[order as usize] = node; }
        }
    }

    /// 清零一段内存.
    ///
    /// # Safety
    /// - `ptr` 必须指向 `len` 字节的合法可写区
    #[inline(always)]
    pub unsafe fn zero_memory(ptr: *mut u8, len: usize) { unsafe {
        core::ptr::write_bytes(ptr, 0, len);
    }}

    /// 用指定字节值填充一段内存.
    ///
    /// # Safety
    /// - `ptr` 必须指向 `len` 字节的合法可写区
    #[inline(always)]
    pub unsafe fn fill_memory(ptr: *mut u8, val: u8, len: usize) { unsafe {
        core::ptr::write_bytes(ptr, val, len);
    }}
}

use raw::{FreeNodeRef, MetaRef, BitmapRef, HeadsRef};

/// 物理内存管理器 — Buddy 分配器
///
/// 2026-07-02: 加 `#[repr(C)]` 防止 LTO 字段重排. 本次会话诊断发现
/// LTO 在 release 模式错位多个字段 (bitmap_size, buddy_meta, buddy_heads),
/// 虽有 addr_of! 修复, repr(C) 提供额外防御层.
#[repr(C)]
pub struct PhysicalMemoryManager {
    // ---- Bitmap (reserved 跟踪 + 统计) ----
    bitmap: Cell<Option<NonNull<u32>>>,
    bitmap_size: Cell<usize>,
    mem_size: Cell<u64>,
    kernel_end: Cell<u64>,
    info: Cell<MemoryInfo>,
    // ---- 锁与生命周期 ----
    lock: AtomicBool,
    initialized: AtomicBool,
    buddy_ready: AtomicBool,
    // ---- 早期 (线性) 分配器 ----
    early_allocs: UnsafeCell<[EarlyAlloc; MAX_EARLY_ALLOCS]>,
    early_count: AtomicUsize,
    early_current: AtomicU64,
    // ---- 统计 ----
    total_allocs: AtomicU64,
    total_frees: AtomicU64,
    failed_allocs: AtomicU64,
    // ---- Buddy 分配器 ----
    /// 按页阶数元数据: 0xFF = 已分配, 0..9 = 空闲块头阶数
    buddy_meta: Cell<Option<NonNull<u8>>>,
    /// 双向链表空闲块头, 每个阶数一个
    buddy_heads: UnsafeCell<[*mut FreeNode; MAX_BUDDY_ORDER as usize + 1]>,
}

// SAFETY: PhysicalMemoryManager 使用 Cell/UnsafeCell 实现内部可变性.
// 所有公开修改都通过 pmm_alloc_pages/pmm_free_pages 进行, 它们
// 获取内部锁 (AtomicBool 自旋锁). 锁保证互斥, 多线程并发访问安全.
// buddy_heads 仅在持锁时访问; bitmap/buddy_meta 仅在初始化时设置,
// SAFETY: PhysicalMemoryManager 含 UnsafeCell, 但初始化完成后只读.
unsafe impl Sync for PhysicalMemoryManager {}
// SAFETY: 同上, 初始化后只读, 无并发写风险.
unsafe impl Send for PhysicalMemoryManager {}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            bitmap: Cell::new(None),
            bitmap_size: Cell::new(0),
            mem_size: Cell::new(0),
            kernel_end: Cell::new(0),
            info: Cell::new(MemoryInfo::const_default()),
            lock: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            buddy_ready: AtomicBool::new(false),
            early_allocs: UnsafeCell::new([EarlyAlloc::const_default(); MAX_EARLY_ALLOCS]),
            early_count: AtomicUsize::new(0),
            early_current: AtomicU64::new(0),
            total_allocs: AtomicU64::new(0),
            total_frees: AtomicU64::new(0),
            failed_allocs: AtomicU64::new(0),
            buddy_meta: Cell::new(None),
            buddy_heads: UnsafeCell::new([core::ptr::null_mut(); MAX_BUDDY_ORDER as usize + 1]),
        }
    }

    // ==================== 公开 API (不变) ====================

    pub fn init(&self, mem_size: u64, kernel_end: u64) {
        self.mem_size.set(mem_size);
        self.kernel_end.set(kernel_end);

        let total_pages = mem_size / PAGE_SIZE;

        let start = (kernel_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.store(start, Ordering::Relaxed);

        let mut info = self.info.get();
        info.total_pages = total_pages;
        info.kernel_end = kernel_end;
        self.info.set(info);

        klog_pmm!(
            "[PMM] Init: {} MB, {} pages, kernel ends at 0x{:X}",
            mem_size / (1024 * 1024),
            total_pages,
            kernel_end
        );
    }

    pub fn init_bitmap(&self, reserved_after_kernel: u64) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }

        let reserved_aligned = (reserved_after_kernel + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current
            .fetch_add(reserved_aligned, Ordering::Relaxed);

        let info = self.info.get();
        let total_pages = info.total_pages as usize;
        let total_bits = total_pages;
        let bitmap_words = total_bits.div_ceil(32);
        let bitmap_bytes = bitmap_words * 4;

        // ---- Bitmap placement ----
        let bitmap_phys = self
            .early_current
            .fetch_add(bitmap_bytes as u64, Ordering::Relaxed);
        let bitmap_aligned = (bitmap_phys + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let bitmap_virt = bitmap_aligned + KERNEL_BASE;

        // SAFETY: bitmap_virt is phys_to_virt(PM) + KERNEL_BASE — valid kernel VA
        unsafe {
            raw::zero_memory(bitmap_virt as *mut u8, bitmap_bytes);
        }

        self.bitmap
            .set(match NonNull::new(bitmap_virt as *mut u32) {
                Some(ptr) => Some(ptr),
                None => {
                    klog_pmm!("[PMM] FATAL: bitmap null (0x{:X})", bitmap_virt);
                    return;
                }
            });
        self.bitmap_size.set(bitmap_words);

        // ---- Buddy 元数据布局 (位于 bitmap 之后, 页对齐) ----
        let buddy_meta_bytes = total_pages;
        let buddy_meta_phys =
            (bitmap_aligned + bitmap_bytes as u64 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let buddy_meta_virt = buddy_meta_phys + KERNEL_BASE;
        let buddy_meta_pages = buddy_meta_bytes.div_ceil(PAGE_SIZE as usize) as u64;

        // 将 early_current 推过 buddy 元数据
        self.early_current.store(
            buddy_meta_phys + buddy_meta_pages * PAGE_SIZE + PAGE_SIZE,
            Ordering::Relaxed,
        );

        // SAFETY: buddy_meta_virt = buddy_meta_phys + KERNEL_BASE — 合法的内核 VA
        unsafe {
            raw::fill_memory(
                buddy_meta_virt as *mut u8,
                BUDDY_ALLOCATED,
                buddy_meta_bytes,
            );
        }

        // 2026-07-02: turn 28 排查 test 86 hang (LTO 错位 PMM 字段).
        // GDB dump 显示 do_alloc 内 `mov 0x8(%rsi), %rax` 触发 #PF,
        // rsi 来自 buddy_heads 数组中的非法 FreeNode 指针 (0x7F80000
        // 不在 heap 范围). 原因: init_bitmap 时 self.buddy_meta.set()
        // LTO 错位到 buddy_heads 数组. 修复: 用 core::ptr::addr_of! 获取
        // 真实字段地址, 强制编译器在 LTO 之前解析出正确偏移.
        // SAFETY: 单线程启动期, 无并发写.
        let nn = core::ptr::NonNull::new(buddy_meta_virt as *mut u8);
        unsafe {
            let meta_ptr: *mut Option<core::ptr::NonNull<u8>> =
                core::ptr::addr_of!(self.buddy_meta) as *const _ as *mut _;
            core::ptr::write_volatile(meta_ptr, nn);
        }
        klog_pmm!(
            "[PMM] Buddy meta: {} B at 0x{:X}",
            buddy_meta_bytes,
            buddy_meta_virt
        );

        // ---- 在 bitmap 中标记 reserved 区 ----
        let kernel_end_val = self.kernel_end.get();
        let kernel_pages = phys_to_page(kernel_end_val + PAGE_SIZE - 1) as usize;
        let reserved_pages = (reserved_aligned / PAGE_SIZE) as usize;
        let total_reserved = kernel_pages + reserved_pages;
        for i in 0..total_reserved.min(total_pages) {
            self.set_bit(i);
        }
        if total_pages > 0 {
            self.set_bit(0); // page 0 永远不能被分配出去
        }

        // 标记 bitmap 页已用
        let bmp_start_page = phys_to_page(bitmap_aligned) as usize;
        let bmp_pages = (bitmap_bytes as u64).div_ceil(PAGE_SIZE) as usize;
        for i in bmp_start_page..(bmp_start_page + bmp_pages).min(total_pages) {
            self.set_bit(i);
        }

        // 标记 buddy-meta 页已用
        let bm_start_page = phys_to_page(buddy_meta_phys) as usize;
        for i in bm_start_page..(bm_start_page + buddy_meta_pages as usize).min(total_pages) {
            self.set_bit(i);
        }

        // ---- 从空闲 bitmap 页构建 buddy 空闲链表 ----
        self.buddy_init_free_lists(total_pages);

        self.buddy_ready.store(true, Ordering::Release);
        self.initialized.store(true, Ordering::Release);

        let free = self.count_free_pages();
        klog_pmm!(
            "[PMM] Buddy ready: {} total, {} free ({} MB), reserved {} pages",
            total_pages,
            free,
            free * 4 / 1024,
            total_reserved
        );

        self.update_stats();
    }

    pub fn alloc_page(&self) -> Option<PhysAddr> {
        let flags = self.acquire_lock();
        let result = self.do_alloc(0);
        match result {
            Some(_) => {
                self.total_allocs.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.release_lock(&flags);
        result
    }

    pub fn free_page(&self, addr: PhysAddr) {
        if addr.0 == 0 {
            return;
        }
        let flags = self.acquire_lock();
        self.do_free(addr, 0);
        self.total_frees.fetch_add(1, Ordering::Relaxed);
        self.release_lock(&flags);
    }

    pub fn alloc_pages(&self, count: usize) -> Option<PhysAddr> {
        if count == 0 {
            return None;
        }
        let order = count_to_order(count);
        let npages = 1usize << order as usize;

        // T-02: 分配前策略决策
        // buddy 就绪后使用缓存的 free_pages 统计值, 避免每次分配都遍历 bitmap;
        // 这既提升性能, 又避免 count_free_pages 遍历 + klog 格式化导致的栈溢出风险.
        // 统计值在 do_alloc/do_free 的持锁路径中通过 update_stats 更新.
        let free = if self.buddy_ready.load(Ordering::Relaxed) {
            self.info.get().free_pages
        } else {
            self.count_free_pages()
        };
        let ctx = super::alloc_trait::AllocContext {
            requested_pages: npages,
            free_pages: free,
            total_pages: self.info.get().total_pages as u64,
            pressure_level: 0,
            preferred_node: None,
        };
        match super::alloc_trait::current_alloc_decision().decide_alloc(ctx) {
            super::alloc_trait::AllocDecision::Allow => {}
            super::alloc_trait::AllocDecision::Deny => {
                klog_pmm!("[PMM] alloc denied (policy)");
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            super::alloc_trait::AllocDecision::RetryAfterReclaim => {
                // 策略建议回收后重试, 但 PMM 不执行回收, 直接失败
                // services 层的 OOMD 会在上层处理回收逻辑
                klog_pmm!("[PMM] alloc retry-after-reclaim");
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        }
        let flags = self.acquire_lock();
        let result = self.do_alloc(order);
        match result {
            Some(_) => {
                self.total_allocs
                    .fetch_add(npages as u64, Ordering::Relaxed);
            }
            None => {
                self.failed_allocs.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.release_lock(&flags);
        result
    }

    pub fn free_pages(&self, addr: PhysAddr, count: usize) {
        if addr.0 == 0 || count == 0 {
            return;
        }
        let order = count_to_order(count);
        let npages = 1usize << order as usize;
        let flags = self.acquire_lock();
        self.do_free(addr, order);
        self.total_frees.fetch_add(npages as u64, Ordering::Relaxed);
        self.release_lock(&flags);
    }

    pub fn alloc_huge_page(&self, size_type: PageSize) -> Option<PhysAddr> {
        match size_type {
            PageSize::Size4K => self.alloc_page(),
            PageSize::Size2M => self.alloc_pages(512),
            PageSize::Size1G => {
                let np = (size_type.size() / PAGE_SIZE) as usize;
                let flags = self.acquire_lock();
                let result = self.buddy_direct_alloc_aligned(np, size_type.size());
                if result.is_some() {
                    self.total_allocs.fetch_add(np as u64, Ordering::Relaxed);
                } else {
                    self.failed_allocs.fetch_add(1, Ordering::Relaxed);
                }
                self.release_lock(&flags);
                result
            }
        }
    }

    pub fn free_huge_page(&self, addr: PhysAddr, size_type: PageSize) {
        match size_type {
            PageSize::Size4K => self.free_page(addr),
            PageSize::Size2M => self.free_pages(addr, 512),
            PageSize::Size1G => {
                let np = (size_type.size() / PAGE_SIZE) as usize;
                let flags = self.acquire_lock();
                let start = phys_to_page(addr.0) as usize;
                for i in 0..np {
                    self.clear_bit(start + i);
                }
                self.total_frees.fetch_add(np as u64, Ordering::Relaxed);
                self.release_lock(&flags);
            }
        }
    }

    pub fn is_aligned_for_huge(&self, addr: PhysAddr, size_type: PageSize) -> bool {
        size_type.is_aligned(addr.0)
    }

    pub fn get_free_pages(&self) -> u64 {
        self.info.get().free_pages
    }
    pub fn get_total_pages(&self) -> u64 {
        self.info.get().total_pages
    }
    pub fn get_used_pages(&self) -> u64 {
        self.info.get().used_pages
    }
    pub fn get_info(&self) -> MemoryInfo {
        self.info.get()
    }

    pub fn dump_stats(&self) {
        let info = self.info.get();
        klog_pmm!("=== PMM (Buddy) ===");
        klog_pmm!(
            "Total: {} MB  Pages: {} total / {} free / {} used",
            self.mem_size.get() / (1024 * 1024),
            info.total_pages,
            info.free_pages,
            info.used_pages
        );
        klog_pmm!("Kernel End: 0x{:X}", info.kernel_end);
        klog_pmm!(
            "Allocs: {}  Frees: {}  Failed: {}",
            self.total_allocs.load(Ordering::Relaxed),
            self.total_frees.load(Ordering::Relaxed),
            self.failed_allocs.load(Ordering::Relaxed)
        );
        klog_pmm!("===================");
    }

    // ==================== 锁辅助函数 ====================

    /// 获取 PMM 锁, 同时禁用中断 (SMP 安全).
    ///
    /// 禁用中断是为了避免当运行在同一 CPU 的中断处理程序
    /// 尝试分配内存时形成死锁.
    #[inline(always)]
    fn acquire_lock(&self) -> IrqSaveFlags {
        let flags = disable_interrupts();
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        flags
    }

    #[inline(always)]
    fn release_lock(&self, flags: &IrqSaveFlags) {
        self.lock.store(false, Ordering::Release);
        restore_interrupts(flags);
    }

    // ==================== Bitmap 辅助函数 (统计 + reserved) ====================

    // 2026-07-01 test 110 hang 修复: 防止 LTO 字段错位.
    //
    // dump 显示 set_bit 汇编 `cmp GLOBAL_PMM+0x1078(%rip),%rdx` 实际读
    // failed_allocs 字段, 而非 bitmap_size 字段 (差 0x1060 字节).
    // LTO 在 inline 时把 self.bitmap_size.get() 错位到 self.failed_allocs,
    // 运行时 cmp 与 failed_allocs 实际值 (~0x3FF) 比较, 几乎总通过 jae,
    // 导致巨大 page index 的 set_bit 不被跳过, 越界写入触发 #PF.
    //
    // 修复: 通过 raw pointer 直接读 self.bitmap_size, 强制 LTO 看到
    // 真实字段偏移, 不可错位. volatile read 防止任何 caching.
    fn set_bit(&self, bit: usize) {
        if let Some(bmp) = self.bitmap.get() {
            // SAFETY: bitmap 已 init 时 self.bitmap_size 也是已 set 的有效值.
            let bitmap_size = unsafe {
                let p = self as *const Self as *const u64;
                // bitmap 字段在 offset 0, size 16; bitmap_size 字段在 offset 16
                core::ptr::read_volatile(p.add(2) as *const usize)
            };
            BitmapRef::new(bmp).set_bit(bit, bitmap_size);
        }
    }

    // 2026-07-01: 同样防止 LTO 错位 (见 set_bit 注释)
    fn clear_bit(&self, bit: usize) {
        if let Some(bmp) = self.bitmap.get() {
            let bitmap_size = unsafe {
                let p = self as *const Self as *const u64;
                core::ptr::read_volatile(p.add(2) as *const usize)
            };
            BitmapRef::new(bmp).clear_bit(bit, bitmap_size);
        }
    }

    // 2026-07-01: 同样防止 LTO 错位 (见 set_bit 注释)
    fn test_bit(&self, bit: usize) -> bool {
        if let Some(bmp) = self.bitmap.get() {
            let bitmap_size = unsafe {
                let p = self as *const Self as *const u64;
                core::ptr::read_volatile(p.add(2) as *const usize)
            };
            BitmapRef::new(bmp).test_bit(bit, bitmap_size)
        } else {
            false
        }
    }

    // 2026-07-01: 同样防止 LTO 错位 (见 set_bit 注释)
    fn count_free_pages(&self) -> u64 {
        let total = self.info.get().total_pages as usize;
        // SAFETY: bitmap 已 init 时 self.bitmap_size 有效
        let bmp_size = unsafe {
            let p = self as *const Self as *const u64;
            core::ptr::read_volatile(p.add(2) as *const usize)
        };
        let free = if let Some(bmp) = self.bitmap.get() {
            let f = BitmapRef::new(bmp).count_free(bmp_size);
            f
        } else {
            0
        };
        // 截断到 total (bitmap 在 total_pages 之外可能还有剩余位)
        let extra = (self.bitmap_size.get() * 32).saturating_sub(total) as u32;
        if extra > 0 {
            free.saturating_sub(extra as u64)
        } else {
            free
        }
    }

    fn update_stats(&self) {
        let free = self.count_free_pages();
        let mut info = self.info.get();
        info.free_pages = free;
        info.used_pages = info.total_pages - free;
        self.info.set(info);
    }

    /// 轻量统计增量: 分配 npages 页后更新 free/used 计数.
    /// 调用方必须持有 PMM 锁.
    #[inline]
    fn stats_alloc(&self, npages: u64) {
        let mut info = self.info.get();
        info.free_pages = info.free_pages.saturating_sub(npages);
        info.used_pages = info.used_pages.saturating_add(npages);
        self.info.set(info);
    }

    /// 轻量统计增量: 释放 npages 页后更新 free/used 计数.
    /// 调用方必须持有 PMM 锁.
    #[inline]
    fn stats_free(&self, npages: u64) {
        let mut info = self.info.get();
        info.free_pages = info.free_pages.saturating_add(npages);
        info.used_pages = info.used_pages.saturating_sub(npages);
        self.info.set(info);
    }

    // ==================== Buddy 分配器核心 ====================

    #[inline]
    fn buddy_meta_ref(&self) -> Option<MetaRef> {
        // 2026-07-02: turn 28 排查. LTO 错位 buddy_meta 字段访问.
        // 用 core::ptr::addr_of! 获取真实字段地址, 防 LTO 错位.
        // Cell<T> 是 repr(transparent), 指针 cast 到 T 安全.
        let meta_field_ptr = core::ptr::addr_of!(self.buddy_meta);
        let buddy_meta: Option<core::ptr::NonNull<u8>> = unsafe {
            core::ptr::read_volatile(meta_field_ptr as *const Option<core::ptr::NonNull<u8>>)
        };
        buddy_meta.map(|n| {
            // SAFETY: buddy_meta 在 init_bitmap 中设置一次, 此后只读;
            // 所有 buddy 操作都持有 PMM 锁.
            unsafe { MetaRef::new_unchecked(n.as_ptr()) }
        })
    }

    #[inline]
    fn buddy_heads_ref(&self) -> HeadsRef {
        // 2026-07-02: turn 28 排查. LTO 错位 buddy_heads 字段访问.
        // 用 core::ptr::addr_of! 获取真实字段地址, 防 LTO 错位.
        // UnsafeCell<T> 是 repr(transparent), 地址 = T 地址.
        let field_addr = core::ptr::addr_of!(self.buddy_heads) as *const u8;
        let heads_ptr: *mut [*mut FreeNode; MAX_BUDDY_ORDER as usize + 1] =
            field_addr as *mut _;
        // SAFETY: buddy_heads 在 PMM 锁保护下访问; init_bitmap 之后稳定.
        unsafe { HeadsRef::new_unchecked(heads_ptr) }
    }

    /// 尝试将 `order` 处释放的 `pfn` 与其上方的 buddy 合并.
    /// 返回 (merged_pfn, final_order).
    fn buddy_try_merge(&self, mut pfn: u64, mut order: u8) -> (u64, u8) {
        let meta = match self.buddy_meta_ref() {
            Some(m) => m,
            None => return (pfn, order),
        };
        let total = self.info.get().total_pages;

        while order < MAX_BUDDY_ORDER {
            let buddy_pfn = pfn ^ (1u64 << order);
            if buddy_pfn >= total {
                break;
            }

            let buddy_state = meta.read(buddy_pfn as usize);
            if buddy_state != order {
                break;
            }

            // 从空闲链表中移除 buddy
            self.buddy_list_remove(buddy_pfn, order);
            meta.write(buddy_pfn as usize, BUDDY_ALLOCATED);

            pfn = core::cmp::min(pfn, buddy_pfn);
            order += 1;
        }

        meta.write(pfn as usize, order);
        (pfn, order)
    }

    /// 从双向链表中移除一个空闲块.
    fn buddy_list_remove(&self, pfn: u64, order: u8) {
        let heads = self.buddy_heads_ref();
        let node = pfn_to_virt(pfn) as *mut FreeNode;
        // 防御性: 校验 node 是否在物理 RAM 范围内
        let node_phys = (node as u64).wrapping_sub(KERNEL_BASE);
        let mem_size = self.mem_size.get();
        #[allow(clippy::absurd_extreme_comparisons)]
        if node_phys < RAM_BASE || node_phys >= RAM_BASE + mem_size {
            klog_pmm!(
                "[PMM] Corrupt remove node at order {}: pfn=0x{:X} virt=0x{:X}",
                order,
                pfn,
                node as u64
            );
            return;
        }
        // SAFETY: node is inside a valid free page, PMM lock held
        let n = unsafe { FreeNodeRef::new_unchecked(node) };
        let prev = n.prev();
        let next = n.next();
        if !prev.is_null() {
            // SAFETY: prev is a valid FreeNode in the list
            let p = unsafe { FreeNodeRef::new_unchecked(prev) };
            p.set_next(next);
        } else {
            heads.set_head(order, next);
        }
        if !next.is_null() {
            // SAFETY: next is a valid FreeNode in the list
            let nx = unsafe { FreeNodeRef::new_unchecked(next) };
            nx.set_prev(prev);
        }
    }

    /// 将一个块压入空闲链表头.
    fn buddy_list_push(&self, pfn: u64, order: u8) {
        let heads = self.buddy_heads_ref();
        let node = pfn_to_virt(pfn) as *mut FreeNode;
        // SAFETY: 空闲页未被使用, 我们拥有其前 16 字节, PMM 锁已持有
        let n = unsafe { FreeNodeRef::new_unchecked(node) };
        let old_head = heads.head(order);
        n.set_prev(core::ptr::null_mut());
        n.set_next(old_head);
        if !old_head.is_null() {
            // SAFETY: old_head 是链表中合法的 FreeNode
            let oh = unsafe { FreeNodeRef::new_unchecked(old_head) };
            oh.set_prev(node);
        }
        heads.set_head(order, node);
    }

    /// 从空闲链表头弹出一个块, 返回 pfn.
    fn buddy_list_pop(&self, order: u8) -> Option<u64> {
        let heads = self.buddy_heads_ref();
        let node = heads.head(order);
        if node.is_null() {
            return None;
        }

        // 防御性: 校验 node 是否在物理 RAM 范围内
        let node_phys = (node as u64).wrapping_sub(KERNEL_BASE);
        let mem_size = self.mem_size.get();
        #[allow(clippy::absurd_extreme_comparisons)]
        if node_phys < RAM_BASE || node_phys >= RAM_BASE + mem_size {
            return None;
        }

        let pfn = phys_to_page(node_phys);
        // SAFETY: node is a valid free page, PMM lock held
        let n = unsafe { FreeNodeRef::new_unchecked(node) };
        let next = n.next();
        heads.set_head(order, next);
        if !next.is_null() {
            // SAFETY: next is a valid FreeNode in the list
            let nx = unsafe { FreeNodeRef::new_unchecked(next) };
            nx.set_prev(core::ptr::null_mut());
        }
        Some(pfn)
    }

    /// 指定阶数执行核心分配.
    fn buddy_alloc(&self, order: u8) -> Option<(u64, u8)> {
        if order > MAX_BUDDY_ORDER {
            return None;
        }

        // 寻找 >= 请求阶数的最小可用阶
        let mut avail_order: Option<u8> = None;
        for o in order..=MAX_BUDDY_ORDER {
            let heads = self.buddy_heads_ref();
            let h = heads.head(o);
            if !h.is_null() {
                avail_order = Some(o);
                break;
            }
        }
        let alloc_order = avail_order?;

        let pfn = self.buddy_list_pop(alloc_order)?;
        let meta = match self.buddy_meta_ref() {
            Some(m) => m,
            None => return None,
        };

        meta.write(pfn as usize, BUDDY_ALLOCATED);

        // 向下分裂, 直至达到请求阶数
        let cur_pfn = pfn;
        let mut cur_order = alloc_order;
        while cur_order > order {
            cur_order -= 1;
            let buddy_pfn = cur_pfn + (1u64 << cur_order);
            self.buddy_list_push(buddy_pfn, cur_order);
            meta.write(buddy_pfn as usize, cur_order);
        }
        meta.write(cur_pfn as usize, BUDDY_ALLOCATED);

        Some((cur_pfn, order))
    }

    /// 主 do_alloc: 处理早期分配与 buddy 分配.
    fn do_alloc(&self, order: u8) -> Option<PhysAddr> {
        if !self.initialized.load(Ordering::Acquire) {
            return if order == 0 {
                self.early_alloc_single()
            } else {
                self.early_alloc_multiple(1u64 << order as u64)
            };
        }

        if !self.buddy_ready.load(Ordering::Acquire) {
            // init 完成但 buddy 还未就绪: 回退到 bitmap 扫描
            let count = 1usize << order as usize;
            return self.alloc_from_bitmap_fallback(count);
        }

        let (pfn, _) = self.buddy_alloc(order)?;
        let addr = page_to_phys(pfn);
        let npages = 1u64 << order as u64;
        for i in 0..(npages as usize) {
            self.set_bit((pfn as usize) + i);
        }
        self.stats_alloc(npages);
        Some(PhysAddr(addr))
    }

    /// 主 do_free: 处理 buddy 或 bitmap 释放.
    fn do_free(&self, addr: PhysAddr, order: u8) {
        if !self.initialized.load(Ordering::Acquire) {
            klog_pmm!("[PMM] Warn: free before bitmap init at 0x{:X}", addr.0);
            return;
        }

        let info = self.info.get();
        let pfn = phys_to_page(addr.0);
        if pfn >= info.total_pages {
            klog_pmm!("[PMM] Error: invalid page 0x{:X}", addr.0);
            return;
        }

        if !self.buddy_ready.load(Ordering::Acquire) {
            let npages = 1u64 << order as u64;
            for i in 0..(npages as usize) {
                self.clear_bit(pfn as usize + i);
            }
            self.stats_free(npages);
            return;
        }

        if !self.test_bit(pfn as usize) {
            klog_pmm!("[PMM] Warn: double free at pfn {}", pfn);
            return;
        }

        // Clear bitmap
        let npages = 1u64 << order as u64;
        for i in 0..(npages as usize) {
            self.clear_bit(pfn as usize + i);
        }

        // 合并并压入空闲链表
        let (merged_pfn, merged_order) = self.buddy_try_merge(pfn, order);
        self.buddy_list_push(merged_pfn, merged_order);
        self.stats_free(npages);
    }

    /// 扫描所有空闲页 (位未置位), 合并为最大阶的 buddy 块.
    fn buddy_init_free_lists(&self, total_pages: usize) {
        let meta = match self.buddy_meta_ref() {
            Some(m) => m,
            None => return,
        };

        let mut pfn = 0usize;
        while pfn < total_pages {
            if self.test_bit(pfn) {
                pfn += 1;
                continue;
            }

            // 寻找连续空闲段
            let run_start = pfn;
            while pfn < total_pages && !self.test_bit(pfn) {
                pfn += 1;
            }
            let run_len = pfn - run_start;

            // 合并为最大阶 buddy 块
            let mut cur = run_start as u64;
            let mut remaining = run_len;
            while remaining > 0 {
                // ≤ remaining 的最大 2 的幂, 对齐到自身大小
                let max_order = (usize::BITS - 1 - (remaining - 1).leading_zeros())
                    .min(MAX_BUDDY_ORDER as u32) as u8;
                // 寻找 cur 自然对齐 且 2^order ≤ remaining 的最大阶
                let mut order = max_order;
                while order > 0 {
                    let size = 1usize << order as usize;
                    if (cur as usize).is_multiple_of(size) && size <= remaining {
                        break;
                    }
                    order -= 1;
                }
                let block_size = 1usize << order as usize;

                meta.write(cur as usize, order);
                self.buddy_list_push(cur, order);

                cur += block_size as u64;
                remaining -= block_size;
            }
        }
    }

    /// 1GB 页直接对齐分配 (超出 buddy 范围).
    fn buddy_direct_alloc_aligned(&self, count: usize, alignment: u64) -> Option<PhysAddr> {
        let total = self.info.get().total_pages as usize;
        let align_pages = (alignment / PAGE_SIZE) as usize;
        let mut i = align_pages; // 跳过 page 0
        while i + count <= total {
            if i.is_multiple_of(align_pages) {
                let mut ok = true;
                for j in 0..count {
                    if self.test_bit(i + j) {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    for j in 0..count {
                        self.set_bit(i + j);
                    }
                    return Some(PhysAddr(page_to_phys(i as u64)));
                }
            }
            i += align_pages;
        }
        None
    }

    /// 回退 bitmap 扫描 (在 init 完成但 buddy 还未就绪, 或 buddy 关闭时使用).
    fn alloc_from_bitmap_fallback(&self, count: usize) -> Option<PhysAddr> {
        let total = self.info.get().total_pages as usize;
        for i in 0..total {
            if self.test_bit(i) {
                continue;
            }
            if i + count > total {
                return None;
            }
            let mut ok = true;
            for j in 1..count {
                if self.test_bit(i + j) {
                    ok = false;
                    break;
                }
            }
            if ok {
                for j in 0..count {
                    self.set_bit(i + j);
                }
                self.stats_alloc(count as u64);
                return Some(PhysAddr(page_to_phys(i as u64)));
            }
        }
        None
    }

    // ==================== 早期分配器 (bitmap 之前) ====================

    fn early_alloc_single(&self) -> Option<PhysAddr> {
        let current = self.early_current.fetch_add(PAGE_SIZE, Ordering::Relaxed);
        let aligned = (current + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current
            .store(aligned + PAGE_SIZE, Ordering::Relaxed);

        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            // SAFETY: idx < MAX_EARLY_ALLOCS 上界检查保证 early_allocs.add(idx) 不越界;
            // early_allocs 由构造时 OnceCell 初始化为定长数组, 类型为 EarlyAlloc.
            unsafe {
                let a = (*self.early_allocs.get()).as_mut_ptr().add(idx);
                (*a).addr = aligned;
                (*a).size = PAGE_SIZE;
            }
        }
        if aligned >= RAM_BASE + self.mem_size.get() {
            klog_pmm!("[PMM] Error: early alloc OOM");
            return None;
        }
        Some(PhysAddr(aligned))
    }

    fn early_alloc_multiple(&self, count: u64) -> Option<PhysAddr> {
        let size = count * PAGE_SIZE;
        let current = self.early_current.fetch_add(size, Ordering::Relaxed);
        let aligned = (current + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        self.early_current.store(aligned + size, Ordering::Relaxed);

        let idx = self.early_count.fetch_add(1, Ordering::Relaxed);
        if idx < MAX_EARLY_ALLOCS {
            // SAFETY: idx < MAX_EARLY_ALLOCS 守护, size = count*PAGE_SIZE, 记录多页范围.
            unsafe {
                let a = (*self.early_allocs.get()).as_mut_ptr().add(idx);
                (*a).addr = aligned;
                (*a).size = size;
            }
        }
        if aligned + size > RAM_BASE + self.mem_size.get() {
            klog_pmm!("[PMM] Error: early multi alloc OOM");
            return None;
        }
        Some(PhysAddr(aligned))
    }
}

// ==================== 全局单例与初始化 ====================

static GLOBAL_PMM: OnceLock<PhysicalMemoryManager> = OnceLock::new();

pub fn pmm_init(mem_size: u64, kernel_end: u64) -> &'static PhysicalMemoryManager {
    GLOBAL_PMM.get_or_init(|slot| {
        let pmm = PhysicalMemoryManager::new();
        pmm.init(mem_size, kernel_end);
        slot.write(pmm);
    })
}

pub fn pmm_init_bitmap(reserved_after_kernel: u64) {
    let pmm = GLOBAL_PMM
        .get()
        .expect("[PMM] pmm_init_bitmap before pmm_init");
    pmm.init_bitmap(reserved_after_kernel);
}

pub fn get_pmm() -> &'static PhysicalMemoryManager {
    GLOBAL_PMM.get().expect("[PMM] accessed before init")
}

// ==================== 屏障与回滚 ====================

#[derive(Clone, Copy)]
struct PmmSnapshot {
    total_allocs: u64,
    total_frees: u64,
    failed_allocs: u64,
    info: super::MemoryInfo,
}

static PMM_SNAPSHOT: IrqSpinLock<Option<PmmSnapshot>> = IrqSpinLock::new(None);

pub fn pmm_barrier_capture() {
    let pmm = get_pmm();
    let mut snap = PMM_SNAPSHOT.lock();
    *snap = Some(PmmSnapshot {
        total_allocs: pmm.total_allocs.load(Ordering::Relaxed),
        total_frees: pmm.total_frees.load(Ordering::Relaxed),
        failed_allocs: pmm.failed_allocs.load(Ordering::Relaxed),
        info: pmm.info.get(),
    });
}

pub fn pmm_barrier_rollback() -> bool {
    let pmm = get_pmm();
    let snap = PMM_SNAPSHOT.lock();
    if let Some(ref s) = *snap {
        pmm.total_allocs.store(s.total_allocs, Ordering::Relaxed);
        pmm.total_frees.store(s.total_frees, Ordering::Relaxed);
        pmm.failed_allocs.store(s.failed_allocs, Ordering::Relaxed);
        pmm.info.set(s.info);
    }
    true
}

fn pmm_barrier_capture_cb() {
    pmm_barrier_capture();
}
fn pmm_barrier_rollback_cb() -> bool {
    pmm_barrier_rollback()
}

pub fn pmm_register_barrier_domain() {
    crate::kernel::framework::barrier::recovery_domain_register(3);
    if let Some(dom) = crate::kernel::framework::barrier::RECOVERY_MANAGER.lock().find(3) {
        *dom.capture_cb.lock() = Some(pmm_barrier_capture_cb);
        *dom.rollback_cb.lock() = Some(pmm_barrier_rollback_cb);
    }
}
