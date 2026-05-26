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
//! - COW 跟踪表由 `COW_LOCK` 自旋锁保护。
//! - 所有 `unsafe` 页表访问基于 PMM 分配的有效物理帧，通过 `KERNEL_BASE`
//!   转换为内核虚拟地址，不会产生悬垂指针。
//! - volatile 读写确保编译器不重排 MMIO 相关的页表操作。

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};

use super::*;
use super::vmm;

static COW_LOCK: AtomicBool = AtomicBool::new(false);

static mut COW_REFS: Option<BTreeMap<u64, u32>> = None;

pub fn cow_init() {
    // SAFETY: 单次初始化, 启动早期调用, 无并发访问
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
    // SAFETY: COW_LOCK 持有中, 独占访问 COW_REFS
    let refs = unsafe { COW_REFS.as_mut().unwrap() };
    *refs.entry(key).or_insert(0) += 1;
    cow_unlock();
}

pub fn cow_dec_ref(phys: u64) -> bool {
    let key = frame_key(phys);
    cow_lock();
    // SAFETY: COW_LOCK 持有中, 独占访问 COW_REFS
    let refs = unsafe { COW_REFS.as_mut().unwrap() };
    if let Some(count) = refs.get_mut(&key) {
        *count -= 1;
        if *count == 0 {
            refs.remove(&key);
            cow_unlock();
            return true;
        }
    }
    cow_unlock();
    false
}

pub fn cow_ref_count(phys: u64) -> u32 {
    let key = frame_key(phys);
    cow_lock();
    // SAFETY: COW_LOCK 持有中, 只读访问 COW_REFS
    let refs = unsafe { COW_REFS.as_ref().unwrap() };
    let count = refs.get(&key).copied().unwrap_or(0);
    cow_unlock();
    count
}

/// COW 感知的页表克隆: 共享用户空间物理页, 双方标记只读
/// 相比 deep copy 版本, 该版本不分配新物理页, 也不复制数据
pub fn clone_user_page_table_cow(parent_pml4: u64) -> Option<u64> {
    if parent_pml4 == 0 {
        return None;
    }

    let pmm = super::pmm::get_pmm();
    let child_pml4_phys = pmm.alloc_page()?;
    let kernel_pml4 = vmm::get_kernel_pml4();

    // SAFETY: child_pml4_phys 刚由 PMM 分配, 物理地址有效;
    // KERNEL_BASE 偏移后得到可访问的内核虚拟地址
    let child_pml4_virt = child_pml4_phys.to_virt().0 as *mut u64;
    unsafe { core::ptr::write_bytes(child_pml4_virt, 0, PAGE_SIZE as usize); }

    // SAFETY: kernel_pml4 由 vmm_init 写入, 指向有效页表;
    // 复制高半区 (索引 256-511) 使子进程页表共享内核映射
    let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt().0 as *const u64;
    unsafe {
        core::ptr::copy_nonoverlapping(kernel_pml4_virt.add(256), child_pml4_virt.add(256), 256);
    }

    // SAFETY: parent_pml4 是已注册用户页表的物理地址
    let parent_pml4_virt = PhysAddr(parent_pml4).to_virt().0 as *const u64;

    for i in 0..256usize {
        // SAFETY: 索引 0-255 在 PML4 页范围内; volatile 确保读取真实的页表内容
        let parent_pml4e = unsafe { parent_pml4_virt.add(i).read_volatile() };
        if (parent_pml4e & 1) == 0 {
            continue;
        }

        let child_pdpt_phys = pmm.alloc_page()?;
        // SAFETY: 刚分配的页, 通过 KERNEL_BASE 映射有效
        let child_pdpt_virt = child_pdpt_phys.to_virt().0 as *mut u64;
        unsafe { core::ptr::write_bytes(child_pdpt_virt, 0, PAGE_SIZE as usize); }

        let mut child_pml4e = parent_pml4e;
        child_pml4e = (child_pml4e & 0xFFF) | (child_pdpt_phys.as_u64() & 0x000FFFFFFFFFF000);
        // SAFETY: child_pml4_virt 指向有效 PML4 页
        unsafe { child_pml4_virt.add(i).write_volatile(child_pml4e); }

        // SAFETY: parent_pml4e 已检验 present, phys_to_virt 映射有效
        let parent_pdpt_virt = PhysAddr((parent_pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;

        for j in 0..512usize {
            // SAFETY: pdpt 索引在页范围内
            let parent_pdpte = unsafe { parent_pdpt_virt.add(j).read_volatile() };
            if (parent_pdpte & 1) == 0 || (parent_pdpte & 0x80) != 0 {
                continue;
            }

            let child_pd_phys = pmm.alloc_page()?;
            // SAFETY: 刚分配的页
            let child_pd_virt = child_pd_phys.to_virt().0 as *mut u64;
            unsafe { core::ptr::write_bytes(child_pd_virt, 0, PAGE_SIZE as usize); }

            let mut child_pdpte = parent_pdpte;
            child_pdpte = (child_pdpte & 0xFFF) | (child_pd_phys.as_u64() & 0x000FFFFFFFFFF000);
            unsafe { child_pdpt_virt.add(j).write_volatile(child_pdpte); }

            let parent_pd_virt = PhysAddr((parent_pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;

            for k in 0..512usize {
                // SAFETY: pd 索引在页范围内
                let parent_pde = unsafe { parent_pd_virt.add(k).read_volatile() };
                if (parent_pde & 1) == 0 {
                    continue;
                }

                if (parent_pde & 0x80) != 0 {
                    // 2MB huge page: 保持原样, 不参与 COW
                    let child_pde_v = parent_pde;
                    // SAFETY: child_pd_virt 指向有效 PD 页
                    unsafe { child_pd_virt.add(k).write_volatile(child_pde_v); }
                    continue;
                }

                let child_pt_phys = pmm.alloc_page()?;
                let child_pt_virt = child_pt_phys.to_virt().0 as *mut u64;
                unsafe { core::ptr::write_bytes(child_pt_virt, 0, PAGE_SIZE as usize); }

                let mut child_pde = parent_pde;
                child_pde = (child_pde & 0xFFF) | (child_pt_phys.as_u64() & 0x000FFFFFFFFFF000);
                unsafe { child_pd_virt.add(k).write_volatile(child_pde); }

                // SAFETY: parent_pt 物理地址来自有效 PDE
                let parent_pt_virt = PhysAddr((parent_pde & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *const u64;

                for l in 0..512usize {
                    // SAFETY: pt 索引在页范围内
                    let parent_pte = unsafe { parent_pt_virt.add(l).read_volatile() };
                    if (parent_pte & 1) == 0 {
                        continue;
                    }

                    let parent_phys = parent_pte & 0x000FFFFFFFFFF000;
                    let parent_flags = parent_pte & 0xFFF;

                    if (parent_flags & 2) != 0 {
                        // SAFETY: parent_pt_virt 是只读指针 (immutable ref),
                        // 但 COW 需要写回标记只读位; 这里转为 *mut 是安全的,
                        // 因为该 PTE 由本函数独占访问 (调用方持有 VMM lock)
                        let parent_pte_ptr = parent_pt_virt as *mut u64;
                        unsafe {
                            let mut pte = parent_pte_ptr.add(l).read_volatile();
                            pte &= !2u64; // clear WRITABLE
                            parent_pte_ptr.add(l).write_volatile(pte);
                        }

                        let mut child_pte = parent_pte;
                        child_pte &= !2u64;
                        // SAFETY: child_pt_virt 指向有效 PT 页
                        unsafe { child_pt_virt.add(l).write_volatile(child_pte); }

                        cow_inc_ref(parent_phys);
                        cow_inc_ref(parent_phys);
                    } else {
                        // SAFETY: 已只读的页直接共享 PTE 内容
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

    let count = cow_ref_count(old_frame);

    if count <= 1 {
        if count == 1 {
            cow_lock();
            // SAFETY: COW_LOCK 持有中
            let refs = unsafe { COW_REFS.as_mut().unwrap() };
            refs.remove(&old_frame);
            cow_unlock();
        }

        // SAFETY: 通过页表遍历找到 PTE 物理地址;
        // 每级 entry 都经过 present 校验, KERNEL_BASE 偏移有效
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

            // SAFETY: pt 物理地址有效, PTE 偏移在页范围内
            let pt_p = PhysAddr((pde & 0x000FFFFFFFFFF000) + KERNEL_BASE).to_virt().0 as *mut u64;
            &mut *pt_p.add(virt_pt_idx(page_aligned))
        };

        *pte |= 2; // WRITABLE
        crate::arch!(tlb_flush_page(page_aligned as usize));
        return Some(old_phys.as_u64());
    }

    let pmm_inst = super::pmm::get_pmm();
    let new_phys = pmm_inst.alloc_page()?;
    let new_virt = new_phys.to_virt();

    // SAFETY: old_frame 来自有效页表项, KERNEL_BASE 映射可用;
    // new_virt 来自刚分配的 PMM 页, 两个 4KB 区域均有效
    let old_virt = PhysAddr(old_frame + KERNEL_BASE).to_virt();
    unsafe {
        core::ptr::copy_nonoverlapping(
            old_virt.0 as *const u8,
            new_virt.0 as *mut u8,
            PAGE_SIZE as usize,
        );
    }

    if cow_dec_ref(old_frame) {
        pmm_inst.free_page(PhysAddr(old_frame));
    }

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