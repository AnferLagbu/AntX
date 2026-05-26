//! Copy-on-Write (COW) — fork 内存共享与写时复制
//!
//! ## 核心思想
//!
//! fork() 时父子进程**共享**物理页，均标记为只读。
//! 任意一方写入时触发 #PF → COW handler → 复制物理页。
//!
//! ## 引用计数
//!
//! 使用 `BTreeMap<PhysFrame, u32>` 跟踪每帧的共享计数:
//! - fork: 父子各 +1 = 2
//! - COW fault: 子分配新页，父引用 -1
//! - munmap / exit: 引用 -1，归零时释放
//!
//! ## SAFETY
//!
//! COW 跟踪表由 `COW_LOCK` 自旋锁保护。fork 时 VMM lock 已持有。

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};

use super::*;
use super::vmm;

const PHYS_FRAME_SHIFT: u64 = 12;

static COW_LOCK: AtomicBool = AtomicBool::new(false);

static mut COW_REFS: Option<BTreeMap<u64, u32>> = None;

pub fn cow_init() {
    unsafe { COW_REFS = Some(BTreeMap::new()); }
}

fn cow_lock() {
    while COW_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

fn cow_unlock() {
    COW_LOCK.store(false, Ordering::Release);
}

fn frame_key(phys: u64) -> u64 {
    phys & !(PAGE_SIZE - 1)
}

pub fn cow_inc_ref(phys: u64) {
    let key = frame_key(phys);
    cow_lock();
    let refs = unsafe { COW_REFS.as_mut().unwrap() };
    *refs.entry(key).or_insert(0) += 1;
    cow_unlock();
}

pub fn cow_dec_ref(phys: u64) -> bool {
    let key = frame_key(phys);
    cow_lock();
    let refs = unsafe { COW_REFS.as_mut().unwrap() };
    if let Some(count) = refs.get_mut(&key) {
        *count -= 1;
        if *count == 0 {
            refs.remove(&key);
            cow_unlock();
            return true; // caller should free the page
        }
    }
    cow_unlock();
    false
}

pub fn cow_ref_count(phys: u64) -> u32 {
    let key = frame_key(phys);
    cow_lock();
    let refs = unsafe { COW_REFS.as_ref().unwrap() };
    let count = refs.get(&key).copied().unwrap_or(0);
    cow_unlock();
    count
}

/// 将页表项标记为 COW (清除 WRITABLE, 设置 COW 标志)
pub fn cow_mark_pte_readonly(pte: &mut u64) {
    *pte &= !(PAGE_WRITABLE as u64);
    // COW 页仍然 PRESENT + USER, 只是不可写
}

/// COW 感知的页表克隆: 共享用户空间物理页, 双方标记只读
/// 相比 deep copy 版本, 该版本不分配新物理页, 也不复制数据
///
/// 调用者负责在调用前后持有 VMM lock。
pub fn clone_user_page_table_cow(parent_pml4: u64) -> Option<u64> {
    if parent_pml4 == 0 {
        return None;
    }

    let pmm = super::pmm::get_pmm();
    let child_pml4_phys = pmm.alloc_page()?;
    let kernel_pml4 = vmm::get_kernel_pml4();

    let child_pml4_virt = child_pml4_phys.to_virt().0 as *mut u64;
    unsafe { core::ptr::write_bytes(child_pml4_virt, 0, PAGE_SIZE as usize); }

    // 复制内核空间条目 (256-511)
    let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt().0 as *const u64;
    unsafe {
        core::ptr::copy_nonoverlapping(kernel_pml4_virt.add(256), child_pml4_virt.add(256), 256);
    }

    let parent_pml4_virt = PhysAddr(parent_pml4).to_virt().0 as *const u64;

    // 遍历用户空间条目 (0-255)
    for i in 0..256usize {
        let parent_pml4e = unsafe { parent_pml4_virt.add(i).read_volatile() };
        if (parent_pml4e & 1) == 0 {
            continue;
        }

        let child_pdpt_phys = pmm.alloc_page()?;
        let child_pdpt_virt = child_pdpt_phys.to_virt().0 as *mut u64;
        unsafe { core::ptr::write_bytes(child_pdpt_virt, 0, PAGE_SIZE as usize); }

        let mut child_pml4e = parent_pml4e;
        child_pml4e = (child_pml4e & 0xFFF) | (child_pdpt_phys.as_u64() & 0x000FFFFFFFFFF000);
        unsafe { child_pml4_virt.add(i).write_volatile(child_pml4e); }

        let parent_pdpt_virt = PhysAddr((parent_pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;

        for j in 0..512usize {
            let parent_pdpte = unsafe { parent_pdpt_virt.add(j).read_volatile() };
            if (parent_pdpte & 1) == 0 || (parent_pdpte & 0x80) != 0 {
                continue;
            }

            let child_pd_phys = pmm.alloc_page()?;
            let child_pd_virt = child_pd_phys.to_virt().0 as *mut u64;
            unsafe { core::ptr::write_bytes(child_pd_virt, 0, PAGE_SIZE as usize); }

            let mut child_pdpte = parent_pdpte;
            child_pdpte = (child_pdpte & 0xFFF) | (child_pd_phys.as_u64() & 0x000FFFFFFFFFF000);
            unsafe { child_pdpt_virt.add(j).write_volatile(child_pdpte); }

            let parent_pd_virt = PhysAddr((parent_pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;

            for k in 0..512usize {
                let parent_pde = unsafe { parent_pd_virt.add(k).read_volatile() };
                if (parent_pde & 1) == 0 {
                    continue;
                }

                if (parent_pde & 0x80) != 0 {
                    let child_pde_v = parent_pde;
                    unsafe { child_pd_virt.add(k).write_volatile(child_pde_v); }
                    continue;
                }

                let child_pt_phys = pmm.alloc_page()?;
                let child_pt_virt = child_pt_phys.to_virt().0 as *mut u64;
                unsafe { core::ptr::write_bytes(child_pt_virt, 0, PAGE_SIZE as usize); }

                let mut child_pde = parent_pde;
                child_pde = (child_pde & 0xFFF) | (child_pt_phys.as_u64() & 0x000FFFFFFFFFF000);
                unsafe { child_pd_virt.add(k).write_volatile(child_pde); }

                let parent_pt_virt = PhysAddr((parent_pde & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;

                for l in 0..512usize {
                    let parent_pte = unsafe { parent_pt_virt.add(l).read_volatile() };
                    if (parent_pte & 1) == 0 {
                        continue;
                    }

                    let parent_phys = parent_pte & 0x000FFFFFFFFFF000;
                    let parent_flags = parent_pte & 0xFFF;

                    // COW: 如果页面可写, 标记为只读, 增加引用计数
                    if (parent_flags & 2) != 0 {
                        // 标记父页只读
                        let parent_pte_ptr = parent_pt_virt as *mut u64;
                        unsafe {
                            let mut pte = parent_pte_ptr.add(l).read_volatile();
                            pte &= !2u64; // clear WRITABLE
                            parent_pte_ptr.add(l).write_volatile(pte);
                        }

                        // 子页共享相同物理帧 (只读)
                        let mut child_pte = parent_pte;
                        child_pte &= !2u64; // clear WRITABLE
                        unsafe { child_pt_virt.add(l).write_volatile(child_pte); }

                        // 增加引用计数 (父 + 子)
                        cow_inc_ref(parent_phys);
                        cow_inc_ref(parent_phys);
                    } else {
                        // 已只读的页直接共享
                        unsafe { child_pt_virt.add(l).write_volatile(parent_pte); }
                    }
                }
            }
        }
    }

    Some(child_pml4_phys.as_u64())
}

/// COW fault 处理: 为写入分配新页
pub fn cow_handle_fault(pml4: u64, fault_addr: u64) -> Option<u64> {
    let vmm_inst = vmm::get_vmm();
    let page_aligned = fault_addr & !(PAGE_SIZE - 1);

    let old_phys = vmm_inst.get_physical_in_pml4(pml4, VirtAddr(page_aligned))?;
    let old_frame = old_phys.as_u64() & 0x000FFFFFFFFFF000;

    // 检查引用计数
    let count = cow_ref_count(old_frame);

    if count <= 1 {
        let pte = unsafe {
            let pml4_v = PhysAddr(pml4).to_virt().0 as *const u64;
            let pml4e = pml4_v.add(virt_pml4_idx(page_aligned)).read_volatile();
            if (pml4e & 1) == 0 { return None; }

            let pdpt_p = PhysAddr((pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;
            let pdpte = pdpt_p.add(virt_pdpt_idx(page_aligned)).read_volatile();
            if (pdpte & 1) == 0 { return None; }

            let pd_p = PhysAddr((pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;
            let pde = pd_p.add(virt_pd_idx(page_aligned)).read_volatile();
            if (pde & 1) == 0 { return None; }

            let pt_p = PhysAddr((pde & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *mut u64;
            &mut *pt_p.add(virt_pt_idx(page_aligned))
        };

        *pte |= 2; // WRITABLE
        crate::arch!(tlb_flush_page(page_aligned as usize));
        return Some(old_phys.as_u64());
    }

    // 共享页: 分配新帧, 复制数据
    let pmm_inst = super::pmm::get_pmm();
    let new_phys = pmm_inst.alloc_page()?;
    let new_virt = new_phys.to_virt();

    let old_virt = PhysAddr(old_frame + KERNEL_BASE).to_virt();
    unsafe {
        core::ptr::copy_nonoverlapping(
            old_virt.0 as *const u8,
            new_virt.0 as *mut u8,
            PAGE_SIZE as usize,
        );
    }

    // 减少旧帧引用
    cow_dec_ref(old_frame);

    // 映射新页 (可写)
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    vmm_inst.map_page_in_table(pml4, VirtAddr(page_aligned), new_phys, flags);

    Some(new_phys.as_u64())
}

#[inline]
fn virt_pml4_idx(v: u64) -> usize { ((v >> 39) & 0x1FF) as usize }
#[inline]
fn virt_pdpt_idx(v: u64) -> usize { ((v >> 30) & 0x1FF) as usize }
#[inline]
fn virt_pd_idx(v: u64) -> usize  { ((v >> 21) & 0x1FF) as usize }
#[inline]
fn virt_pt_idx(v: u64) -> usize  { ((v >> 12) & 0x1FF) as usize }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_key_alignment() {
        assert_eq!(frame_key(0x1000), 0x1000);
        assert_eq!(frame_key(0x1FFF), 0x1000);
        assert_eq!(frame_key(0x2000), 0x2000);
    }

    #[test]
    fn test_virt_index_functions() {
        let v = 0x0000_7FFF_0000_0000u64;
        assert_eq!(virt_pml4_idx(v), 0);
        assert_eq!(virt_pt_idx(v), 0);
    }
}