//! Demand Paging — 按需分页子系统
//!
//! 与 VMA、VMM、PMM 协作实现真正的按需分页：
//!
//! ## 处理流程
//!
//! ```text
//! #PF 异常 → handle_page_fault(mm, info)
//!   ├── find_vma(addr)
//!   │   ├── Found → handle_vma_fault
//!   │   │   ├── Write+ReadOnly → COW copy
//!   │   │   └── Normal → alloc + map
//!   │   └── Not Found
//!   │       ├── Stack region → handle_stack_expansion
//!   │       └── Else → SIGSEGV
//!   └── Return PfResult
//! ```
//!
//! ## SAFETY
//!
//! - 本模块在 #PF 中断上下文中运行。所有操作必须无阻塞。
//! - PMM 分配的物理页通过 KERNEL_BASE 转为有效虚拟地址后清零，
//!   确保用户态不会看到脏数据（信息泄漏防护）。
//! - PAGE_FAULT_COUNT 使用 AtomicU64, 无竞争条件。

use super::pmm;
use super::vma::{MmStruct, Vma, VmaType};
use super::vmm;
use super::*;
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PfResult {
    Fixed = 0,
    SignalSegv = 1,
    SignalBus = 2,
    Oom = 3,
    Unhandled = 4,
}

#[derive(Debug, Clone, Copy)]
pub struct PageFaultInfo {
    pub fault_addr: u64,
    pub present: bool,
    pub write: bool,
    pub user: bool,
    pub reserved: bool,
    pub instruction: bool,
}

impl PageFaultInfo {
    pub fn from_error_code(fault_addr: u64, error_code: u64) -> Self {
        Self {
            fault_addr,
            present: error_code & 0x01 != 0,
            write: error_code & 0x02 != 0,
            user: error_code & 0x04 != 0,
            reserved: error_code & 0x08 != 0,
            instruction: error_code & 0x10 != 0,
        }
    }
}

const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_F000;
const USER_STACK_DEFAULT_SIZE: u64 = 0x0080_0000; // 8MB
const USER_STACK_GUARD_PAGES: u64 = 1; // 1 page guard

pub fn handle_page_fault(mm: &MmStruct, info: PageFaultInfo) -> PfResult {
    let addr = info.fault_addr as usize;

    if info.reserved {
        return PfResult::SignalBus;
    }

    if let Some(vma) = mm.find_vma(addr) {
        if vma.is_guard() {
            return PfResult::SignalSegv;
        }
        return handle_vma_fault_with_mm(mm, &vma, &info);
    }

    if info.user && is_stack_expansion_candidate(addr) {
        return handle_stack_expansion(mm, addr);
    }

    PfResult::SignalSegv
}

/// 用户态缺页简化入口 (无需传递 MmStruct，直接分配并映射)
pub fn handle_user_page_fault(info: PageFaultInfo) -> PfResult {
    let addr = info.fault_addr as usize;

    if info.reserved {
        return PfResult::SignalBus;
    }

    // Swap-in: PTE 为 swap entry (present=0 但非零)
    if !info.present && info.user {
        let pml4 = vmm::get_current_pml4();
        let vmm_inst = vmm::get_vmm();
        if let Some(pte) = vmm_inst.get_pte_value(pml4, VirtAddr(info.fault_addr)) {
            if super::swap::is_swap_pte(pte) {
                let result = super::swap::handle_swap_fault(pml4, info.fault_addr);
                if result == PfResult::Fixed {
                    PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
                }
                return result;
            }
        }
    }

    // COW: 写已存在但只读的页
    if info.write && info.present {
        let pml4 = vmm::get_current_pml4();
        return match super::cow::cow_handle_fault(pml4, info.fault_addr) {
            Some(_) => {
                PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
                PfResult::Fixed
            }
            None => PfResult::SignalSegv,
        };
    }

    if info.user && is_stack_expansion_candidate(addr) {
        return handle_stack_expansion_simple(addr);
    }

    handle_simple_fault(addr, &info)
}

fn handle_simple_fault(addr: usize, _info: &PageFaultInfo) -> PfResult {
    let aligned = addr & !(PAGE_SIZE as usize - 1);

    let pmm_inst = pmm::get_pmm();
    let phys = match pmm_inst.alloc_page() {
        Some(p) => p,
        None => return PfResult::Oom,
    };

    let phys_virt = phys.to_virt();
    // SAFETY: phys 由 PMM 分配, phys_to_virt 映射有效; 清零防止信息泄漏
    unsafe {
        core::ptr::write_bytes(phys_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let vmm_inst = vmm::get_vmm();
    let pml4 = vmm::get_current_pml4();

    vmm_inst.map_page_in_table(pml4, VirtAddr(aligned as u64), phys, flags);

    PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
    PfResult::Fixed
}

fn handle_stack_expansion_simple(addr: usize) -> PfResult {
    let page_aligned = addr & !(PAGE_SIZE as usize - 1);
    let stack_base = USER_STACK_TOP - USER_STACK_DEFAULT_SIZE;
    let guard_end = stack_base + USER_STACK_GUARD_PAGES * PAGE_SIZE;

    if (page_aligned as u64) < guard_end {
        return PfResult::SignalSegv;
    }

    let pmm_inst = pmm::get_pmm();
    let phys = match pmm_inst.alloc_page() {
        Some(p) => p,
        None => return PfResult::Oom,
    };

    let phys_virt = phys.to_virt();
    // SAFETY: PMM 分配的有效页
    unsafe {
        core::ptr::write_bytes(phys_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let vmm_inst = vmm::get_vmm();
    vmm_inst.map_page_in_table(
        vmm::get_current_pml4(),
        VirtAddr(page_aligned as u64),
        phys,
        flags,
    );

    PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
    PfResult::Fixed
}

fn handle_vma_fault_with_mm(mm: &MmStruct, vma: &Vma, info: &PageFaultInfo) -> PfResult {
    let aligned = (info.fault_addr as usize) & !(PAGE_SIZE as usize - 1);

    // ── FileBacked VMA: 从 Page Cache 获取缓存页 ──
    if vma.vma_type == VmaType::FileBacked && vma.inode_id != 0 {
        return handle_file_fault(mm, vma, info, aligned);
    }

    // ── COW: 写入只读页 ──
    if info.write && !vma.flags.contains(PageFlags::WRITABLE) {
        return do_cow_copy_with_mm(mm, vma, aligned);
    }

    // ── 普通匿名页分配 ──
    let pmm_inst = pmm::get_pmm();
    let phys = match pmm_inst.alloc_page() {
        Some(p) => p,
        None => return PfResult::Oom,
    };

    let phys_virt = phys.to_virt();
    // SAFETY: PMM 分配的有效页, 清零防信息泄漏
    unsafe {
        core::ptr::write_bytes(phys_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
    }

    let flags = vma.flags | PageFlags::PRESENT;
    let vmm_inst = vmm::get_vmm();
    let pml4 = vmm::get_current_pml4();

    vmm_inst.map_page_in_table(pml4, VirtAddr(aligned as u64), phys, flags);

    PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
    PfResult::Fixed
}

/// 文件映射缺页处理: 从 Page Cache 获取/创建缓存页
fn handle_file_fault(_mm: &MmStruct, vma: &Vma, info: &PageFaultInfo, aligned: usize) -> PfResult {
    let page_index = ((aligned - vma.start) as u64 + vma.offset) / PAGE_SIZE;

    // 从 Page Cache 获取缓存页
    let cache_phys = match super::pcache::pcache_get(vma.inode_id, page_index) {
        Some(p) => p,
        None => return PfResult::Oom,
    };

    let vmm_inst = vmm::get_vmm();
    let pml4 = vmm::get_current_pml4();

    if vma.shared {
        // MAP_SHARED: 可写映射, 写入回写 Page Cache
        let flags = vma.flags | PageFlags::PRESENT | PageFlags::WRITABLE;
        vmm_inst.map_page_in_table(
            pml4,
            VirtAddr(aligned as u64),
            PhysAddr(cache_phys),
            flags,
        );

        // 写入时标记脏页
        if info.write {
            super::pcache::pcache_mark_dirty(vma.inode_id, page_index);
        }
    } else {
        // MAP_PRIVATE: 只读映射, 写入时触发 COW
        let flags = (vma.flags | PageFlags::PRESENT) & !PageFlags::WRITABLE;
        vmm_inst.map_page_in_table(
            pml4,
            VirtAddr(aligned as u64),
            PhysAddr(cache_phys),
            flags,
        );

        // 写入时 COW: 分配新页, 复制数据, 可写映射
        if info.write {
            let pmm_inst = pmm::get_pmm();
            let new_phys = match pmm_inst.alloc_page() {
                Some(p) => p,
                None => return PfResult::Oom,
            };

            // 从缓存页复制数据到新页
            let src_virt = PhysAddr(cache_phys).to_virt();
            let dst_virt = new_phys.to_virt();
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                core::ptr::copy_nonoverlapping(
                    src_virt.0 as *const u8,
                    dst_virt.0 as *mut u8,
                    PAGE_SIZE as usize,
                );
            }

            // 释放 Page Cache 引用
            super::pcache::pcache_put(vma.inode_id, page_index);

            // 用新页替换映射 (可写)
            let cow_flags = vma.flags | PageFlags::PRESENT | PageFlags::WRITABLE;
            vmm_inst.map_page_in_table(
                pml4,
                VirtAddr(aligned as u64),
                new_phys,
                cow_flags,
            );
        }
    }

    PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
    PfResult::Fixed
}

fn is_stack_expansion_candidate(addr: usize) -> bool {
    let a = addr as u64;
    (USER_STACK_TOP - USER_STACK_DEFAULT_SIZE..USER_STACK_TOP).contains(&a)
}

fn handle_stack_expansion(mm: &MmStruct, addr: usize) -> PfResult {
    let page_aligned = addr & !(PAGE_SIZE as usize - 1);
    let stack_base = USER_STACK_TOP - USER_STACK_DEFAULT_SIZE;
    let guard_end = stack_base + USER_STACK_GUARD_PAGES * PAGE_SIZE;

    if (page_aligned as u64) < guard_end {
        return PfResult::SignalSegv;
    }

    if mm.find_vma(page_aligned).is_some() {
        return PfResult::SignalSegv;
    }

    let pmm_inst = pmm::get_pmm();
    let phys = match pmm_inst.alloc_page() {
        Some(p) => p,
        None => return PfResult::Oom,
    };

    let phys_virt = phys.to_virt();
    // SAFETY: PMM 分配的有效页, 清零防信息泄漏
    unsafe {
        core::ptr::write_bytes(phys_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let vmm_inst = vmm::get_vmm();
    vmm_inst.map_page_in_table(
        vmm::get_current_pml4(),
        VirtAddr(page_aligned as u64),
        phys,
        flags,
    );

    let stack_vma = Vma::new(
        page_aligned,
        page_aligned + PAGE_SIZE as usize,
        flags,
        VmaType::Stack,
    );
    // LATER: 栈扩展 VMA 插入失败时页面已映射但无 VMA 跟踪,
    // 当前忽略返回值是权衡 (不会崩溃但 VMA 不完整)
    let _ = mm.insert_vma(stack_vma);

    PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
    PfResult::Fixed
}

// ── COW (Copy-on-Write) ──

fn do_cow_copy_with_mm(_mm: &MmStruct, _vma: &Vma, addr: usize) -> PfResult {
    let vmm_inst = vmm::get_vmm();
    let pml4 = vmm::get_current_pml4();

    let old_phys = match vmm_inst.get_physical_in_pml4(pml4, VirtAddr(addr as u64)) {
        Some(p) => p,
        None => return PfResult::SignalSegv,
    };

    let pmm_inst = pmm::get_pmm();
    let new_phys = match pmm_inst.alloc_page() {
        Some(p) => p,
        None => return PfResult::Oom,
    };

    let old_virt = old_phys.to_virt();
    let new_virt = new_phys.to_virt();
    // SAFETY: old_phys/new_phys 均为有效物理页, KERNEL_BASE 映射可用;
    // 两个 4KB 区域不重叠 (PMM 保证)
    unsafe {
        core::ptr::copy_nonoverlapping(
            old_virt.0 as *const u8,
            new_virt.0 as *mut u8,
            PAGE_SIZE as usize,
        );
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    vmm_inst.map_page_in_table(pml4, VirtAddr(addr as u64), new_phys, flags);

    PAGE_FAULT_COUNT.fetch_add(1, Ordering::Relaxed);
    PfResult::Fixed
}

// ── 统计 ──

pub static PAGE_FAULT_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn page_fault_count() -> u64 {
    PAGE_FAULT_COUNT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pf_info_from_error_code() {
        let info = PageFaultInfo::from_error_code(0x4000, 0x06);
        assert_eq!(info.fault_addr, 0x4000);
        assert!(info.write);
        assert!(info.user);
        assert!(!info.present);
        assert!(!info.reserved);
        assert!(!info.instruction);
    }

    #[test]
    fn test_pf_info_not_present() {
        let info = PageFaultInfo::from_error_code(0x1000, 0x00);
        assert!(!info.present);
        assert!(!info.write);
        assert!(!info.user);
    }

    #[test]
    fn test_stack_expansion_candidate() {
        let inside = (USER_STACK_TOP - 4096) as usize;
        assert!(is_stack_expansion_candidate(inside));

        let outside = (USER_STACK_TOP - USER_STACK_DEFAULT_SIZE - 4096) as usize;
        assert!(!is_stack_expansion_candidate(outside));

        assert!(!is_stack_expansion_candidate(0x1000));
    }

    #[test]
    fn test_pf_result_values() {
        assert_eq!(PfResult::Fixed as u8, 0);
        assert_eq!(PfResult::SignalSegv as u8, 1);
        assert_eq!(PfResult::Oom as u8, 3);
    }
}
