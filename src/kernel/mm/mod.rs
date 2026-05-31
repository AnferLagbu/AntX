//! Memory Management Subsystem
//!
//! Rust rewrite of kernel/mm (PMM, VMM, Kmalloc)
//! Provides physical memory management, virtual memory mapping,
//! and kernel heap allocation with memory safety guarantees.

extern crate alloc;

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};

pub mod pmm;

#[cfg(target_arch = "x86_64")]
pub mod vmm;
#[cfg(target_arch = "aarch64")]
#[path = "vmm_aarch64.rs"]
pub mod vmm;

pub mod arch;
pub mod cow;
pub mod ffi;
pub mod kmalloc;
pub mod kmalloc_slab;
pub mod page_fault;
pub mod pressure;
pub mod slab;
pub mod vma;

// Re-export commonly used types
pub use kmalloc::*;
pub use pmm::*;
pub use vmm::*;

/// Page size constants (matching C implementation)
pub const PAGE_SIZE: u64 = 4096;
pub const PAGE_SHIFT: u64 = 12;

/// Huge page sizes
pub const HUGE_PAGE_2M_SIZE: u64 = 2 * 1024 * 1024; // 2 MB
pub const HUGE_PAGE_1G_SIZE: u64 = 1024 * 1024 * 1024; // 1 GB
pub const HUGE_PAGE_2M_SHIFT: u64 = 21;
pub const HUGE_PAGE_1G_SHIFT: u64 = 30;

/// Memory layout constants
/// x86_64: high-half mapping (0xFFFF_8000_0000_0000)
/// aarch64: identity-mapped (PA=VA in low 2GB)
#[cfg(target_arch = "x86_64")]
pub const KERNEL_BASE: u64 = 0xFFFF800000000000u64;
#[cfg(target_arch = "aarch64")]
pub const KERNEL_BASE: u64 = 0;
pub const PHYSICAL_BASE: u64 = 0x0000000000000000u64;

/// Page table entry flags (matching C definitions)
pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_HUGE: u64 = 1 << 7; // Huge page flag
pub const PAGE_NX: u64 = 1u64 << 63;

/// Helper macros for page table indexing
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

/// Convert physical address to virtual address (kernel space)
#[inline(always)]
pub const fn phys_to_virt(phys: u64) -> u64 {
    phys + KERNEL_BASE
}

/// Convert virtual address to physical address (kernel space)
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

    /// Check if address is properly aligned for this page size
    pub fn is_aligned(&self, addr: u64) -> bool {
        let mask = self.size() - 1;
        (addr & mask) == 0
    }
}

/// Memory information structure (matching C struct)
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

    /// Align up to page boundary
    #[inline(always)]
    pub fn align_up(&self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }

    /// Align down to page boundary
    #[inline(always)]
    pub fn align_down(&self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }

    /// Convert to virtual address in kernel space
    pub fn to_virt(&self) -> VirtAddr {
        VirtAddr(phys_to_virt(self.0))
    }
}

/// Virtual address wrapper for type safety
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    pub fn new(addr: u64) -> Self {
        Self(addr)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Align up to page boundary
    #[inline(always)]
    pub fn align_up(&self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }

    /// Align down to page boundary
    #[inline(always)]
    pub fn align_down(&self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }

    /// Convert to physical address (assumes kernel space)
    pub fn to_phys(&self) -> PhysAddr {
        PhysAddr(virt_to_phys(self.0))
    }

    /// Get PML4 index for this address
    pub fn pml4_idx(&self) -> usize {
        pml4_index(self.0)
    }

    /// Get PDPT index for this address
    pub fn pdpt_idx(&self) -> usize {
        pdpt_index(self.0)
    }

    /// Get PD index for this address
    pub fn pd_idx(&self) -> usize {
        pd_index(self.0)
    }

    /// Get PT index for this address
    pub fn pt_idx(&self) -> usize {
        pt_index(self.0)
    }
}

// Page table entry flags (bitflags style)
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

/// Page Table Entry structure (matches C bitfield layout)
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

    /// Create from raw value
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
    use crate::kernel::mm::vmm::get_vmm;

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
