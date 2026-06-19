//! 内存管理子系统
//!
//! kernel/mm 的 Rust 重写 (PMM, VMM, Kmalloc)
//! 提供物理内存管理、虚拟内存映射以及内核堆分配, 附带内存安全保证.
//!
//! ## 依赖声明
//!
//! framework 内部依赖: sync, syscall, proc, tests
//! services 依赖: services::mm (安全代理)

extern crate alloc;

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};

pub mod pmm;

#[cfg(target_arch = "x86_64")]
#[path = "vmm_x86_64.rs"]
pub mod vmm;
#[cfg(target_arch = "aarch64")]
#[path = "vmm_aarch64.rs"]
pub mod vmm;

pub mod api;
pub mod arch;
pub mod copy_user;
pub mod cow;
pub mod frame;
pub mod kmalloc;
pub mod kmalloc_slab;
/// D3: NUMA 拓扑感知与内存策略
pub mod numa;
pub mod pcache;
pub mod page_fault;
pub mod pressure;
pub mod slab;
pub mod swap;
pub mod vma;
/// T-02: 物理页帧分配决策 trait
pub mod alloc_trait;
/// T2-2: PMM 策略决策 trait (阶数选择/碎片化/水位线)
pub mod pmm_trait;
/// T2-3: Slab 策略决策 trait (缓存大小选择/对象数计算/分配优先级)
pub mod slab_trait;
/// T2-4: Swap 策略决策 trait (LRU 管理/回收决策/kswapd 触发)
pub mod swap_trait;
/// L-03: 机制 API 集中导出 — 供 services 层策略实现调用
pub mod mechanism;

#[cfg(target_arch = "x86_64")]
pub mod kpti;
#[cfg(target_arch = "aarch64")]
#[path = "kpti_aarch64.rs"]
pub mod kpti;

// 重新导出常用类型
pub use kmalloc::*;
pub use pmm::*;
pub use vmm::*;

// api 公共接口 re-export — 避免跨子系统直接访问 mm::api 内部
// 注意: 不使用 glob re-export 因与 vmm::* 有名称冲突 (vmm_init 等)
pub use api::{
    KmallocStats,
    pmm_init, pmm_init_bitmap,
    pmm_alloc_page, pmm_free_page, pmm_alloc_pages, pmm_free_pages,
    pmm_alloc_page_phys, pmm_free_page_phys, pmm_alloc_pages_phys, pmm_free_pages_phys,
    pmm_alloc_huge_page_phys, pmm_alloc_huge_page, pmm_free_huge_page,
    pmm_is_aligned_for_huge, pmm_get_free_pages, pmm_get_total_pages, pmm_get_used_pages,
    pmm_dump_stats,
    vma_get_current_mm, vma_set_current_mm,
    vmm_clone_user_page_table_cow, vmm_destroy_page_table, vmm_switch_page_table,
    copy_to_user, copy_from_user, is_user_buf,
    update_pressure, MemoryPressure,
    PfResult, PageFaultInfo, handle_page_fault, handle_user_page_fault,
    k_malloc, k_free, kfree, kmalloc_stats,
};

// vma 公共类型 re-export — 避免跨子系统直接访问 mm::vma 内部
pub use vma::{MmStruct, Vma, VmaType};

// swap 公共接口 re-export — 避免跨子系统直接访问 mm::swap 内部
pub use swap::{kswapd_wakeup, set_page_locked};

// kpti 公共接口 re-export — 避免跨子系统直接访问 mm::kpti 内部
#[cfg(target_arch = "aarch64")]
pub use kpti::kpti_trampoline_ttbr1_or_kernel;

// alloc_trait 公共接口 re-export — T-02 策略-机制分离
pub use alloc_trait::{FrameAllocDecision, FallbackAllocPolicy, AllocContext, AllocDecision, register_alloc_decision, current_alloc_decision};

// pmm_trait 公共接口 re-export — T2-2 PMM 策略-机制分离
pub use pmm_trait::{PmmPolicy, FallbackPmmPolicy, PmmPolicyContext, Watermarks, register_pmm_policy, current_pmm_policy};

// slab_trait 公共接口 re-export — T2-3 Slab 策略-机制分离
pub use slab_trait::{SlabPolicy, FallbackSlabPolicy, SlabPolicyContext, SlabAllocSource, register_slab_policy, current_slab_policy};

// swap_trait 公共接口 re-export — T2-4 Swap 策略-机制分离
pub use swap_trait::{SwapPolicy, FallbackSwapPolicy, SwapPolicyContext, LruPageInfo, register_swap_policy, current_swap_policy};

// cow 公共接口 re-export — 避免跨子系统直接访问 mm::cow 内部
pub use cow::{cow_init, cow_ref_count, cow_inc_ref, cow_dec_ref};

// mechanism 模块供 services 层通过 framework::mm::mechanism::* 访问机制 API
// 不使用 glob re-export 因与现有 api re-export 产生歧义

/// Page size and huge-page constants (统一从 config.rs 引用)
pub use crate::kernel::framework::config::{
    PAGE_SIZE, PAGE_SHIFT, HUGE_PAGE_2M_SIZE, HUGE_PAGE_1G_SIZE, HUGE_PAGE_2M_SHIFT,
    HUGE_PAGE_1G_SHIFT,
};

/// 内存布局常量
/// x86_64: 高半核映射 (0xFFFF_8000_0000_0000)
/// aarch64: 直接恒等映射 (PA=VA, 低 2GB)
#[cfg(target_arch = "x86_64")]
pub const KERNEL_BASE: u64 = 0xFFFF800000000000u64;
#[cfg(target_arch = "aarch64")]
pub const KERNEL_BASE: u64 = 0;
pub const PHYSICAL_BASE: u64 = 0x0000000000000000u64;

/// 缓存行大小 (字节). x86_64 与 aarch64 通用值为 64.
/// 用于 DMA 缓存刷写对齐、false sharing 避免等场景.
pub const CACHE_LINE_SIZE: u64 = 64;

/// 内核文本段基址 (符号地址).
/// x86_64: 高半核最高 2GB 区域 (-2GB 符号地址), 用于内核代码绝对寻址
///   和 RIP/地址分类 (内核文本段 vs 直接映射区).
/// aarch64: 恒等映射, 内核文本段基址由 bootloader 决定 (典型 0x40080000).
#[cfg(target_arch = "x86_64")]
pub const KERNEL_TEXT_BASE: u64 = 0xFFFFFFFF80000000;
#[cfg(target_arch = "aarch64")]
pub const KERNEL_TEXT_BASE: u64 = 0x40080000;

/// 页表项标志 (与 C 定义一致)
pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_HUGE: u64 = 1 << 7; // Huge page flag
pub const PAGE_NX: u64 = 1u64 << 63;

/// 页表索引辅助宏
#[inline(always)]
pub const fn pml4_index(addr: u64) -> usize {
    ((addr >> 39) & 0x1FF) as usize
}

#[inline(always)]
pub const fn pdpt_index(addr: u64) -> usize {
    ((addr >> 30) & 0x1FF) as usize
}

#[inline(always)]
pub const fn pd_index(addr: u64) -> usize {
    ((addr >> 21) & 0x1FF) as usize
}

#[inline(always)]
pub const fn pt_index(addr: u64) -> usize {
    ((addr >> 12) & 0x1FF) as usize
}

/// 物理地址转虚拟地址 (内核空间)
#[inline(always)]
pub const fn phys_to_virt(phys: u64) -> u64 {
    phys + KERNEL_BASE
}

/// 虚拟地址转物理地址 (内核空间)
#[inline(always)]
pub const fn virt_to_phys(virt: u64) -> u64 {
    virt - KERNEL_BASE
}

/// Page size type enum
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PageSize {
    Size4K = 0, // Standard 4KB page
    Size2M = 1, // 2MB huge page
    Size1G = 2, // 1GB giant page
}

impl PageSize {
    pub fn size(&self) -> u64 {
        match self {
            PageSize::Size4K => PAGE_SIZE,
            PageSize::Size2M => HUGE_PAGE_2M_SIZE,
            PageSize::Size1G => HUGE_PAGE_1G_SIZE,
        }
    }

    pub fn shift(&self) -> u64 {
        match self {
            PageSize::Size4K => PAGE_SHIFT,
            PageSize::Size2M => HUGE_PAGE_2M_SHIFT,
            PageSize::Size1G => HUGE_PAGE_1G_SHIFT,
        }
    }

    /// 检查地址是否按当前页大小正确对齐
    pub fn is_aligned(&self, addr: u64) -> bool {
        let mask = self.size() - 1;
        (addr & mask) == 0
    }
}

/// 内存信息结构 (与 C 结构体一致)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryInfo {
    pub total_pages: u64,
    pub free_pages: u64,
    pub used_pages: u64,
    pub kernel_end: u64,
}

impl Default for MemoryInfo {
    fn default() -> Self {
        Self {
            total_pages: 0,
            free_pages: 0,
            used_pages: 0,
            kernel_end: 0,
        }
    }
}

impl MemoryInfo {
    pub const fn const_default() -> Self {
        Self {
            total_pages: 0,
            free_pages: 0,
            used_pages: 0,
            kernel_end: 0,
        }
    }
}

/// Physical address wrapper for type safety
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    pub fn new(addr: u64) -> Self {
        Self(addr)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// 向上对齐到页边界
    #[inline(always)]
    pub fn align_up(&self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }

    /// 向下对齐到页边界
    #[inline(always)]
    pub fn align_down(&self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }

    /// 转为内核空间虚拟地址
    pub fn to_virt(&self) -> VirtAddr {
        VirtAddr(phys_to_virt(self.0))
    }
}

/// 虚拟地址 (类型安全包装)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    pub fn new(addr: u64) -> Self {
        Self(addr)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// 向上对齐到页边界
    #[inline(always)]
    pub fn align_up(&self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }

    /// 向下对齐到页边界
    #[inline(always)]
    pub fn align_down(&self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }

    /// 转为物理地址 (假定为内核空间)
    pub fn to_phys(&self) -> PhysAddr {
        PhysAddr(virt_to_phys(self.0))
    }

    /// 获取该地址的 PML4 索引
    pub fn pml4_idx(&self) -> usize {
        pml4_index(self.0)
    }

    /// 获取该地址的 PDPT 索引
    pub fn pdpt_idx(&self) -> usize {
        pdpt_index(self.0)
    }

    /// 获取该地址的 PD 索引
    pub fn pd_idx(&self) -> usize {
        pd_index(self.0)
    }

    /// 获取该地址的 PT 索引
    pub fn pt_idx(&self) -> usize {
        pt_index(self.0)
    }
}

// 页表项标志 (bitflags 风格)
bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PageFlags: u64 {
        const PRESENT     = 1 << 0;
        const WRITABLE    = 1 << 1;
        const USER        = 1 << 2;
        const WRITE_THROUGH = 1 << 3;
        const CACHE_DISABLE = 1 << 4;
        const ACCESSED    = 1 << 5;
        const DIRTY       = 1 << 6;
        const HUGE_PAGE   = 1 << 7;
        const GLOBAL      = 1 << 8;
        const NX          = 1u64 << 63;
    }
}

impl Default for PageFlags {
    fn default() -> Self {
        Self::PRESENT | Self::WRITABLE
    }
}

/// 页表项结构 (与 C 位域布局一致)
#[repr(C)]
pub struct PageTableEntry {
    bits: AtomicU64,
}

impl PageTableEntry {
    pub fn new() -> Self {
        Self {
            bits: AtomicU64::new(0),
        }
    }

    /// 从裸值创建
    pub fn from_value(value: u64) -> Self {
        Self {
            bits: AtomicU64::new(value),
        }
    }

    /// Get raw value
    pub fn value(&self) -> u64 {
        self.bits.load(Ordering::Acquire)
    }

    /// Set raw value
    pub fn set_value(&self, value: u64) {
        self.bits.store(value, Ordering::Release);
    }

    /// Is present?
    pub fn is_present(&self) -> bool {
        self.bits.load(Ordering::Acquire) & PAGE_PRESENT != 0
    }

    /// Set present flag
    pub fn set_present(&self, present: bool) {
        let mut val = self.bits.load(Ordering::Acquire);
        if present {
            val |= PAGE_PRESENT;
        } else {
            val &= !PAGE_PRESENT;
        }
        self.bits.store(val, Ordering::Release);
    }

    /// Is writable?
    pub fn is_writable(&self) -> bool {
        self.bits.load(Ordering::Acquire) & PAGE_WRITABLE != 0
    }

    /// Set writable flag
    pub fn set_writable(&self, writable: bool) {
        let mut val = self.bits.load(Ordering::Acquire);
        if writable {
            val |= PAGE_WRITABLE;
        } else {
            val &= !PAGE_WRITABLE;
        }
        self.bits.store(val, Ordering::Release);
    }

    /// Is user accessible?
    pub fn is_user(&self) -> bool {
        self.bits.load(Ordering::Acquire) & PAGE_USER != 0
    }

    /// Set user flag
    pub fn set_user(&self, user: bool) {
        let mut val = self.bits.load(Ordering::Acquire);
        if user {
            val |= PAGE_USER;
        } else {
            val &= !PAGE_USER;
        }
        self.bits.store(val, Ordering::Release);
    }

    /// Is dirty?
    pub fn is_dirty(&self) -> bool {
        self.bits.load(Ordering::Acquire) & (1 << 6) != 0
    }

    /// Set dirty flag
    pub fn set_dirty(&self, dirty: bool) {
        let mut val = self.bits.load(Ordering::Acquire);
        if dirty {
            val |= 1 << 6;
        } else {
            val &= !(1 << 6);
        }
        self.bits.store(val, Ordering::Release);
    }

    /// Is accessed?
    pub fn is_accessed(&self) -> bool {
        self.bits.load(Ordering::Acquire) & (1 << 5) != 0
    }

    /// Set accessed flag
    pub fn set_accessed(&self, accessed: bool) {
        let mut val = self.bits.load(Ordering::Acquire);
        if accessed {
            val |= 1 << 5;
        } else {
            val &= !(1 << 5);
        }
        self.bits.store(val, Ordering::Release);
    }

    /// Is huge page?
    pub fn is_huge(&self) -> bool {
        self.bits.load(Ordering::Acquire) & PAGE_HUGE != 0
    }

    /// Get frame address (physical address of the page)
    pub fn frame(&self) -> PhysAddr {
        PhysAddr(self.bits.load(Ordering::Acquire) & 0x000FFFFFFFFFF000)
    }

    /// Set frame address
    pub fn set_frame(&self, frame: PhysAddr) {
        let mut val = self.bits.load(Ordering::Acquire);
        val = (val & !0x000FFFFFFFFFF000) | (frame.0 & 0x000FFFFFFFFFF000);
        self.bits.store(val, Ordering::Release);
    }

    /// Is no-execute?
    pub fn is_nx(&self) -> bool {
        self.bits.load(Ordering::Acquire) & PAGE_NX != 0
    }

    /// Set no-execute flag
    pub fn set_nx(&self, nx: bool) {
        let mut val = self.bits.load(Ordering::Acquire);
        if nx {
            val |= PAGE_NX;
        } else {
            val &= !PAGE_NX;
        }
        self.bits.store(val, Ordering::Release);
    }

    /// Get all flags as PageFlags
    pub fn flags(&self) -> PageFlags {
        PageFlags::from_bits_truncate(self.bits.load(Ordering::Acquire))
    }

    /// Set flags
    pub fn set_flags(&self, flags: PageFlags) {
        let mut val = self.bits.load(Ordering::Acquire);
        val = (val & !(PageFlags::all().bits())) | flags.bits();
        self.bits.store(val, Ordering::Release);
    }
}

impl Default for PageTableEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// 将帧缓冲物理地址映射到内核虚拟地址空间
///
/// 帧缓冲位于 PCI MMIO 区域（高位物理地址），启动页表的恒等映射
/// 仅覆盖低 1GB 物理内存。此函数通过 VMM 动态建立 2MB 大页映射，
/// 确保帧缓冲可被内核访问。
///
/// G1 阶段暂不配置 Write-Combining 缓存模式 (通过 PAT)，
/// 这不会阻止像素正常显示；WC 优化将在 G3 阶段添加。
pub fn map_framebuffer(phys_addr: u64, size: u64) -> *mut u8 {
    use crate::kernel::framework::mm::get_vmm;

    let vmm = get_vmm();

    let page_2m: u64 = 0x200000;
    let start_page = phys_addr & !(page_2m - 1);
    let end = phys_addr + size;
    let end_page = (end + page_2m - 1) & !(page_2m - 1);

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::GLOBAL;

    let mut pa = start_page;
    while pa < end_page {
        let va = phys_to_virt(pa);
        let _ = vmm.map_huge_page(VirtAddr(va), PhysAddr(pa), flags, PageSize::Size2M);
        pa += page_2m;
    }

    phys_to_virt(phys_addr) as *mut u8
}
