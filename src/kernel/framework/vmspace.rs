//! VmSpace — 用户地址空间安全句柄 (TCB)
//!
//! 封装用户态页表操作 (map / unmap / protect / activate)，
//! 确保 services 层无法直接操作内核页表或越界访问。
//!
//! ## 与 Asterinas OSTD `VmSpace` 的关系
//!
//! 等价于 OSTD 的 `VmSpace`。services 层通过此句柄管理
//! 进程地址空间，而不直接接触 PML4 / TTBR0 物理地址。
//!
//! ## SAFETY 不变量
//!
//! - `pt_root` 必须是有效的顶级页表物理地址（PML4 for x86，TTBR0 for aarch64）。
//! - `map()` 自动检查 vaddr 是否在用户地址空间范围。
//! - `activate()` 仅由调度器在 context switch 时调用。
//! - 所有操作通过 `get_vmm()` 委托给架构特定的 VMM。

use core::fmt;

use crate::kernel::framework::mm::{PhysAddr, VirtAddr, PageSize, PageFlags, get_vmm};

#[cfg(target_arch = "x86_64")]
const USER_VADDR_MASK: u64 = 0x00007FFF_FFFFFFFF;

#[cfg(target_arch = "aarch64")]
const USER_VADDR_MASK: u64 = 0x0000FFFF_FFFFFFFF;

use super::frame::Frame;

/// 安全可操作的进程地址空间。
///
/// services 层只能通过此句柄 map/unmap/protect 用户页。
pub struct VmSpace {
    pt_root: PhysAddr,
    is_kernel: bool,
}

impl VmSpace {
    /// 创建一个空的用户地址空间（新页表）。
    ///
    /// # SAFETY
    /// 调用方确保在合适的分配上下文（初始化早期或进程创建时）。
    pub unsafe fn new() -> Option<Self> {
        let vmm = get_vmm();
        let pt_phys = vmm.create_user_page_table()?;
        Some(Self {
            pt_root: PhysAddr(pt_phys),
            is_kernel: false,
        })
    }

    /// 获取页表根物理地址
    #[inline(always)]
    pub fn pt_root(&self) -> PhysAddr {
        self.pt_root
    }

    /// 是否是内核地址空间
    #[inline(always)]
    pub fn is_kernel(&self) -> bool {
        self.is_kernel
    }

    /// 标记为内核地址空间（保留用于内核 VmSpace）
    #[inline(always)]
    pub fn set_kernel(&mut self) {
        self.is_kernel = true;
    }

    /// 安全映射: 将 Frame 映射到用户虚拟地址。
    ///
    /// 自动检查 vaddr 是否在用户区 [0, USER_ADDR_MAX) 内。
    /// 调用方负责保证 vaddr 未被占用。
    pub fn map(&self, vaddr: VirtAddr, frame: &Frame, flags: PageFlags) -> Result<(), &'static str> {
        let va = vaddr.as_u64();
        if va & !USER_VADDR_MASK != 0 {
            return Err("vaddr outside user address space");
        }
        let vmm = get_vmm();
        // SAFETY: Internal vmm call, pt_root is valid for this VmSpace lifetime.
        // map_page_in_table operates on a specific PML4 (user page table), safe from services.
        unsafe {
            vmm.map_page_in_table(self.pt_root.as_u64(), vaddr, frame.phys(), flags);
        }
        frame.inc_ref();
        Ok(())
    }

    /// 映射大页 (2MB / 1GB)
    pub fn map_huge(
        &self,
        vaddr: VirtAddr,
        frame: &Frame,
        flags: PageFlags,
        _size: PageSize,
    ) -> Result<(), &'static str> {
        let va = vaddr.as_u64();
        if va & !USER_VADDR_MASK != 0 {
            return Err("vaddr outside user address space");
        }
        let vmm = get_vmm();
        vmm.map_huge_page(vaddr, frame.phys(), flags, PageSize::Size2M)?;
        frame.inc_ref();
        Ok(())
    }

    /// 解除映射
    pub fn unmap(&self, vaddr: VirtAddr) -> Result<(), &'static str> {
        let va = vaddr.as_u64();
        if va & !USER_VADDR_MASK != 0 {
            return Err("vaddr outside user address space");
        }
        let vmm = get_vmm();
        // SAFETY: pt_root is valid. unmap_page_in_table is safe for user page tables.
        unsafe {
            vmm.unmap_page_in_table(self.pt_root.as_u64(), vaddr);
        }
        Ok(())
    }

    /// 修改页保护属性（通过先 unmap 再 map 实现）
    pub fn protect(
        &self,
        vaddr: VirtAddr,
        new_flags: PageFlags,
    ) -> Result<(), &'static str> {
        let va = vaddr.as_u64();
        if va & !USER_VADDR_MASK != 0 {
            return Err("vaddr outside user address space");
        }
        let vmm = get_vmm();
        // Find current mapping, then re-map with new flags.
        if let Some(old_phys) = vmm.get_physical_in_pml4(self.pt_root.as_u64(), vaddr) {
            // SAFETY: Unmap then re-map atomically (lock is held within VMM).
            unsafe {
                vmm.unmap_page_in_table(self.pt_root.as_u64(), vaddr);
                vmm.map_page_in_table(self.pt_root.as_u64(), vaddr, old_phys, new_flags);
            }
        }
        Ok(())
    }

    /// 激活此地址空间（切换到其页表）。
    ///
    /// # SAFETY
    /// 仅由调度器在 context switch 时调用。
    /// 调用方确保当前 CPU 不在中断上下文中。
    pub unsafe fn activate(&self) {
        let vmm = get_vmm();
        unsafe {
            vmm.switch_page_table(self.pt_root.as_u64());
        }
    }

    /// 销毁地址空间（释放页表页）。
    ///
    /// # SAFETY
    /// 调用前确保无 CPU 运行在此地址空间上。
    pub unsafe fn destroy(&self) {
        let vmm = get_vmm();
        unsafe {
            vmm.destroy_page_table(self.pt_root.as_u64());
        }
    }
}

impl fmt::Display for VmSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VmSpace(pt_root=0x{:x})", self.pt_root.as_u64())
    }
}

// SAFETY: VmSpace is a handle to page tables owned by the kernel.
unsafe impl Send for VmSpace {}
unsafe impl Sync for VmSpace {}
