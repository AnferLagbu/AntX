//! Copy-on-Write (COW) Memory Management
//!
//! Implements COW semantics for fork() support:
//! - Page reference counting for shared physical pages
//! - COW page table cloning (mark writable pages as read-only + COW)
//! - COW fault handling (copy page on write, restore writability)
//!
//! # COW Flag
//!
//! Uses bit 9 (available bit 1) of x86_64 PTE as the COW marker.
//! When a COW page is written to, a page fault occurs and the handler
//! allocates a new physical page, copies the content, and remaps with
//! WRITABLE flag restored.

use core::sync::atomic::{AtomicU32, Ordering};

use super::*;

macro_rules! klog_cow {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

pub struct PageRefCounter {
    counts: AtomicU32,
    total_pages: AtomicU32,
    initialized: AtomicU32,
}

unsafe impl Sync for PageRefCounter {}
unsafe impl Send for PageRefCounter {}

static GLOBAL_REFCOUNTER: PageRefCounter = PageRefCounter {
    counts: AtomicU32::new(0),
    total_pages: AtomicU32::new(0),
    initialized: AtomicU32::new(0),
};

static mut REFCOUNT_ARRAY_PTR: *mut AtomicU32 = core::ptr::null_mut();
static mut REFCOUNT_ARRAY_PAGES: usize = 0;

pub fn cow_init(total_pages: u64) {
    let array_bytes = total_pages as usize * 4;
    let array_pages = (array_bytes + 4095) / 4096;

    extern "C" {
        fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void;
    }
    let ptr = unsafe { pmm_alloc_pages(array_pages as u64) };
    if ptr.is_null() {
        klog_cow!("[COW] Failed to allocate refcount array");
        return;
    }

    let virt = ptr as u64 + KERNEL_BASE;
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, array_bytes);
        REFCOUNT_ARRAY_PTR = virt as *mut AtomicU32;
        REFCOUNT_ARRAY_PAGES = array_pages;
    }

    GLOBAL_REFCOUNTER.total_pages.store(total_pages as u32, Ordering::Release);
    GLOBAL_REFCOUNTER.initialized.store(1, Ordering::Release);

    klog_cow!(
        "[COW] Initialized: {} pages, refcount array {} KB",
        total_pages,
        array_bytes / 1024
    );
}

#[inline(always)]
fn is_initialized() -> bool {
    GLOBAL_REFCOUNTER.initialized.load(Ordering::Acquire) != 0
}

pub fn page_get_ref(page: u64) -> u32 {
    if !is_initialized() || page >= GLOBAL_REFCOUNTER.total_pages.load(Ordering::Acquire) as u64 {
        return 0;
    }
    unsafe {
        let arr = REFCOUNT_ARRAY_PTR;
        if arr.is_null() { return 0; }
        (*arr.add(page as usize)).load(Ordering::Acquire)
    }
}

pub fn page_inc_ref(page: u64) -> u32 {
    if !is_initialized() || page >= GLOBAL_REFCOUNTER.total_pages.load(Ordering::Acquire) as u64 {
        return 0;
    }
    unsafe {
        let arr = REFCOUNT_ARRAY_PTR;
        if arr.is_null() { return 0; }
        (*arr.add(page as usize)).fetch_add(1, Ordering::Acquire) + 1
    }
}

pub fn page_dec_ref(page: u64) -> u32 {
    if !is_initialized() || page >= GLOBAL_REFCOUNTER.total_pages.load(Ordering::Acquire) as u64 {
        return 0;
    }
    unsafe {
        let arr = REFCOUNT_ARRAY_PTR;
        if arr.is_null() { return 0; }
        let old = (*arr.add(page as usize)).fetch_sub(1, Ordering::Acquire);
        if old <= 1 {
            (*arr.add(page as usize)).store(0, Ordering::Release);
            return 0;
        }
        old - 1
    }
}

pub fn page_set_ref(page: u64, count: u32) {
    if !is_initialized() || page >= GLOBAL_REFCOUNTER.total_pages.load(Ordering::Acquire) as u64 {
        return;
    }
    unsafe {
        let arr = REFCOUNT_ARRAY_PTR;
        if arr.is_null() { return; }
        (*arr.add(page as usize)).store(count, Ordering::Release);
    }
}

pub fn clone_page_table(src_pml4: u64) -> Option<u64> {
    if src_pml4 == 0 { return None; }

    let pmm = get_pmm();
    let vmm = get_vmm();

    let dst_pml4_phys = pmm.alloc_page()?;
    let dst_pml4_virt = dst_pml4_phys.to_virt();
    unsafe {
        core::ptr::write_bytes(dst_pml4_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
    }

    let src_pml4_virt = PhysAddr(src_pml4).to_virt();

    unsafe {
        let src = src_pml4_virt.0 as *const PageTableEntry;
        let dst = dst_pml4_virt.0 as *mut PageTableEntry;

        for i in 256..512 {
            dst.add(i).write(src.add(i).read());
        }

        for i in 0..256usize {
            let src_pml4e = &*src.add(i);
            if !src_pml4e.is_present() { continue; }

            let src_pdpt_phys = src_pml4e.frame().as_u64();
            let dst_pdpt_phys = match pmm.alloc_page() {
                Some(p) => p,
                None => {
                    vmm.destroy_page_table(dst_pml4_phys.as_u64());
                    return None;
                }
            };
            let dst_pdpt_virt = dst_pdpt_phys.to_virt();
            core::ptr::write_bytes(dst_pdpt_virt.0 as *mut u8, 0, PAGE_SIZE as usize);

            (*dst.add(i)).set_frame(dst_pdpt_phys);
            let mut flags = src_pml4e.flags();
            flags.insert(PageFlags::PRESENT | PageFlags::WRITABLE);
            (*dst.add(i)).set_flags(flags);

            let src_pdpt = PhysAddr(src_pdpt_phys).to_virt().0 as *const PageTableEntry;
            let dst_pdpt = dst_pdpt_virt.0 as *mut PageTableEntry;

            for j in 0..512usize {
                let src_pdpte = &*src_pdpt.add(j);
                if !src_pdpte.is_present() { continue; }

                if src_pdpte.is_huge() {
                    let frame = src_pdpte.frame().as_u64();
                    let page_num = frame / PAGE_SIZE;
                    page_inc_ref(page_num);

                    let mut pte_flags = src_pdpte.flags();
                    pte_flags.remove(PageFlags::WRITABLE);
                    pte_flags.insert(PageFlags::COW);

                    (*dst_pdpt.add(j)).set_frame(PhysAddr(frame));
                    (*dst_pdpt.add(j)).set_flags(pte_flags);

                    let mut src_flags = src_pdpte.flags();
                    src_flags.remove(PageFlags::WRITABLE);
                    src_flags.insert(PageFlags::COW);
                    (*src_pdpt.add(j)).set_flags(src_flags);
                    continue;
                }

                let src_pd_phys = src_pdpte.frame().as_u64();
                let dst_pd_phys = match pmm.alloc_page() {
                    Some(p) => p,
                    None => {
                        vmm.destroy_page_table(dst_pml4_phys.as_u64());
                        return None;
                    }
                };
                let dst_pd_virt = dst_pd_phys.to_virt();
                core::ptr::write_bytes(dst_pd_virt.0 as *mut u8, 0, PAGE_SIZE as usize);

                (*dst_pdpt.add(j)).set_frame(dst_pd_phys);
                let mut flags = src_pdpte.flags();
                flags.insert(PageFlags::PRESENT | PageFlags::WRITABLE);
                (*dst_pdpt.add(j)).set_flags(flags);

                let src_pd = PhysAddr(src_pd_phys).to_virt().0 as *const PageTableEntry;
                let dst_pd = dst_pd_virt.0 as *mut PageTableEntry;

                for k in 0..512usize {
                    let src_pde = &*src_pd.add(k);
                    if !src_pde.is_present() { continue; }

                    if src_pde.is_huge() {
                        let frame = src_pde.frame().as_u64();
                        let page_num = frame / PAGE_SIZE;
                        page_inc_ref(page_num);

                        let mut pte_flags = src_pde.flags();
                        pte_flags.remove(PageFlags::WRITABLE);
                        pte_flags.insert(PageFlags::COW);

                        (*dst_pd.add(k)).set_frame(PhysAddr(frame));
                        (*dst_pd.add(k)).set_flags(pte_flags);

                        let mut src_flags = src_pde.flags();
                        src_flags.remove(PageFlags::WRITABLE);
                        src_flags.insert(PageFlags::COW);
                        (*src_pd.add(k)).set_flags(src_flags);
                        continue;
                    }

                    let src_pt_phys = src_pde.frame().as_u64();
                    let dst_pt_phys = match pmm.alloc_page() {
                        Some(p) => p,
                        None => {
                            vmm.destroy_page_table(dst_pml4_phys.as_u64());
                            return None;
                        }
                    };
                    let dst_pt_virt = dst_pt_phys.to_virt();
                    core::ptr::write_bytes(dst_pt_virt.0 as *mut u8, 0, PAGE_SIZE as usize);

                    (*dst_pd.add(k)).set_frame(dst_pt_phys);
                    let mut flags = src_pde.flags();
                    flags.insert(PageFlags::PRESENT | PageFlags::WRITABLE);
                    (*dst_pd.add(k)).set_flags(flags);

                    let src_pt = PhysAddr(src_pt_phys).to_virt().0 as *const PageTableEntry;
                    let dst_pt = dst_pt_virt.0 as *mut PageTableEntry;

                    for l in 0..512usize {
                        let src_pte = &*src_pt.add(l);
                        if !src_pte.is_present() { continue; }

                        let frame = src_pte.frame().as_u64();
                        let page_num = frame / PAGE_SIZE;

                        if src_pte.flags().contains(PageFlags::WRITABLE) {
                            page_inc_ref(page_num);

                            let mut cow_flags = src_pte.flags();
                            cow_flags.remove(PageFlags::WRITABLE);
                            cow_flags.insert(PageFlags::COW);

                            (*dst_pt.add(l)).set_frame(PhysAddr(frame));
                            (*dst_pt.add(l)).set_flags(cow_flags);

                            let mut src_flags = src_pte.flags();
                            src_flags.remove(PageFlags::WRITABLE);
                            src_flags.insert(PageFlags::COW);
                            (*src_pt.add(l)).set_flags(src_flags);
                        } else {
                            (*dst_pt.add(l)).set_frame(PhysAddr(frame));
                            (*dst_pt.add(l)).set_flags(src_pte.flags());

                            page_inc_ref(page_num);
                        }
                    }
                }
            }
        }
    }

    vmm.flush_tlb(0);

    Some(dst_pml4_phys.as_u64())
}

pub fn handle_cow_fault(virt_addr: u64, pml4: u64) -> bool {
    if pml4 == 0 { return false; }

    let v = VirtAddr(virt_addr);
    let pml4_virt = PhysAddr(pml4).to_virt();

    unsafe {
        let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

        let pml4e = &*pml4_ptr.add(v.pml4_idx());
        if !pml4e.is_present() { return false; }
        let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;

        let pdpte = &*pdpt.add(v.pdpt_idx());
        if !pdpte.is_present() { return false; }

        if pdpte.is_huge() {
            return handle_cow_huge_1gb(pdpt.add(v.pdpt_idx()), virt_addr);
        }

        let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
        let pde = &*pd.add(v.pd_idx());
        if !pde.is_present() { return false; }

        if pde.is_huge() {
            return handle_cow_huge_2mb(pd.add(v.pd_idx()), virt_addr);
        }

        let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
        let pte = &mut *pt.add(v.pt_idx());

        if !pte.is_present() { return false; }

        let flags = pte.flags();
        if !flags.contains(PageFlags::COW) { return false; }

        let old_phys = pte.frame().as_u64();
        let page_num = old_phys / PAGE_SIZE;
        let refcount = page_get_ref(page_num);

        if refcount <= 1 {
            let mut new_flags = flags;
            new_flags.remove(PageFlags::COW);
            new_flags.insert(PageFlags::WRITABLE);
            pte.set_flags(new_flags);
            get_vmm().flush_tlb(virt_addr);
            return true;
        }

        let pmm = get_pmm();
        let new_phys = match pmm.alloc_page() {
            Some(p) => p,
            None => return false,
        };

        let old_virt = old_phys + KERNEL_BASE;
        let new_virt = new_phys.as_u64() + KERNEL_BASE;
        core::ptr::copy_nonoverlapping(
            old_virt as *const u8,
            new_virt as *mut u8,
            PAGE_SIZE as usize,
        );

        page_dec_ref(page_num);

        pte.set_frame(new_phys);
        let mut new_flags = flags;
        new_flags.remove(PageFlags::COW);
        new_flags.insert(PageFlags::WRITABLE);
        pte.set_flags(new_flags);

        get_vmm().flush_tlb(virt_addr);
        true
    }
}

fn handle_cow_huge_2mb(pde_ptr: *mut PageTableEntry, virt_addr: u64) -> bool {
    unsafe {
        let pde = &mut *pde_ptr;
        let flags = pde.flags();
        if !flags.contains(PageFlags::COW) { return false; }

        let old_phys = pde.frame().as_u64();
        let page_num = old_phys / PAGE_SIZE;
        let refcount = page_get_ref(page_num);

        if refcount <= 1 {
            let mut new_flags = flags;
            new_flags.remove(PageFlags::COW);
            new_flags.insert(PageFlags::WRITABLE);
            pde.set_flags(new_flags);
            get_vmm().flush_tlb(virt_addr);
            return true;
        }

        let pmm = get_pmm();
        let new_phys = match pmm.alloc_page() {
            Some(p) => p,
            None => return false,
        };

        let old_virt = old_phys + KERNEL_BASE;
        let new_virt = new_phys.as_u64() + KERNEL_BASE;
        core::ptr::copy_nonoverlapping(
            old_virt as *const u8,
            new_virt as *mut u8,
            HUGE_PAGE_2M_SIZE as usize,
        );

        page_dec_ref(page_num);

        pde.set_frame(new_phys);
        let mut new_flags = flags;
        new_flags.remove(PageFlags::COW);
        new_flags.insert(PageFlags::WRITABLE);
        pde.set_flags(new_flags);

        get_vmm().flush_tlb(virt_addr);
        true
    }
}

fn handle_cow_huge_1gb(pdpte_ptr: *mut PageTableEntry, virt_addr: u64) -> bool {
    unsafe {
        let pdpte = &mut *pdpte_ptr;
        let flags = pdpte.flags();
        if !flags.contains(PageFlags::COW) { return false; }

        let old_phys = pdpte.frame().as_u64();
        let page_num = old_phys / PAGE_SIZE;
        let refcount = page_get_ref(page_num);

        if refcount <= 1 {
            let mut new_flags = flags;
            new_flags.remove(PageFlags::COW);
            new_flags.insert(PageFlags::WRITABLE);
            pdpte.set_flags(new_flags);
            get_vmm().flush_tlb(virt_addr);
            return true;
        }

        klog_cow!("[COW] 1GB huge page COW not fully supported, restoring writability");
        let mut new_flags = flags;
        new_flags.remove(PageFlags::COW);
        new_flags.insert(PageFlags::WRITABLE);
        pdpte.set_flags(new_flags);
        get_vmm().flush_tlb(virt_addr);
        true
    }
}

pub fn is_cow_page(virt_addr: u64, pml4: u64) -> bool {
    if pml4 == 0 { return false; }

    let v = VirtAddr(virt_addr);
    let pml4_virt = PhysAddr(pml4).to_virt();

    unsafe {
        let pml4_ptr = pml4_virt.0 as *const PageTableEntry;
        let pml4e = &*pml4_ptr.add(v.pml4_idx());
        if !pml4e.is_present() { return false; }

        let pdpt = pml4e.frame().to_virt().0 as *const PageTableEntry;
        let pdpte = &*pdpt.add(v.pdpt_idx());
        if !pdpte.is_present() { return false; }
        if pdpte.is_huge() { return pdpte.flags().contains(PageFlags::COW); }

        let pd = pdpte.frame().to_virt().0 as *const PageTableEntry;
        let pde = &*pd.add(v.pd_idx());
        if !pde.is_present() { return false; }
        if pde.is_huge() { return pde.flags().contains(PageFlags::COW); }

        let pt = pde.frame().to_virt().0 as *const PageTableEntry;
        let pte = &*pt.add(v.pt_idx());
        if !pte.is_present() { return false; }

        pte.flags().contains(PageFlags::COW)
    }
}
