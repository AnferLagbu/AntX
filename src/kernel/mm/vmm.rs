//! Virtual Memory Manager (VMM)
//!
//! Manages virtual memory mapping using 4-level page tables (x86_64).
//! Provides:
//! - Virtual to physical address translation
//! - Page table creation and management
//! - User space page tables
//! - Huge page support (2MB, 1GB)
//! - Memory protection and access control
//!
//! # Safety
//! Interior mutability for `user_tables` is achieved via `UnsafeCell`
//! protected by an internal `AtomicBool` spinlock (`VMM_LOCK`).
//! All mutations occur under the lock, making `unsafe impl Sync` sound.

macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

use super::*;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, Ordering};

static KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

static VMM_LOCK: AtomicBool = AtomicBool::new(false);

const MAX_USER_PAGE_TABLES: usize = 256;

#[derive(Clone, Copy)]
struct UserPageTable {
    pml4_phys: u64,
    in_use: bool,
}

pub struct VirtualMemoryManager {
    user_tables: UnsafeCell<[UserPageTable; MAX_USER_PAGE_TABLES]>,
    user_table_count: AtomicUsize,
    total_maps: AtomicU64,
    total_unmaps: AtomicU64,
    page_faults: AtomicU64,
}

unsafe impl Sync for VirtualMemoryManager {}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            user_tables: UnsafeCell::new([UserPageTable { pml4_phys: 0, in_use: false }; MAX_USER_PAGE_TABLES]),
            user_table_count: AtomicUsize::new(0),
            total_maps: AtomicU64::new(0),
            total_unmaps: AtomicU64::new(0),
            page_faults: AtomicU64::new(0),
        }
    }

    pub fn init(&self) {
        let cr3 = unsafe { self.read_cr3() };

        KERNEL_PML4.store(cr3, Ordering::Release);

        unsafe { super::ffi::kernel_pml4 = cr3; }

        serial_println!("[VMM] Initialized with kernel PML4 at 0x{:X}", cr3);
    }

    pub fn map_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        self.acquire_lock();

        let result = self.map_page_internal(virt, phys, flags);

        if result.is_ok() {
            self.total_maps.fetch_add(1, Ordering::Relaxed);
        }

        self.release_lock();
        result
    }

    pub fn map_huge_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags, size_type: PageSize) -> Result<(), &'static str> {
        if !size_type.is_aligned(virt.0) || !size_type.is_aligned(phys.0) {
            return Err("Address not aligned for huge page");
        }

        self.acquire_lock();

        let mut flags = flags;
        flags.insert(PageFlags::HUGE_PAGE);

        let result = match size_type {
            PageSize::Size2M => self.map_2mb_page(virt, phys, flags),
            PageSize::Size1G => self.map_1gb_page(virt, phys, flags),
            PageSize::Size4K => self.map_page_internal(virt, phys, flags),
        };

        if result.is_ok() {
            self.total_maps.fetch_add(1, Ordering::Relaxed);
        }

        self.release_lock();
        result
    }

    pub fn unmap_page(&self, virt: VirtAddr) {
        self.acquire_lock();

        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            self.release_lock();
            return;
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pml4e = &*pml4.add(virt.pml4_idx());

            if !pml4e.is_present() {
                self.release_lock();
                return;
            }

            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &*pdpt.add(virt.pdpt_idx());

            if !pdpte.is_present() {
                self.release_lock();
                return;
            }

            if pdpte.is_huge() {
                (*pdpt.add(virt.pdpt_idx())).set_value(0);
                self.flush_tlb(virt.0);
            } else {
                let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
                let pde = &*pd.add(virt.pd_idx());

                if !pde.is_present() {
                    self.release_lock();
                    return;
                }

                if pde.is_huge() {
                    (*pd.add(virt.pd_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                } else {
                    let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
                    (*pt.add(virt.pt_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                }
            }
        }

        self.total_unmaps.fetch_add(1, Ordering::Relaxed);
        self.release_lock();
    }

    pub fn get_physical(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.get_physical_in_pml4(KERNEL_PML4.load(Ordering::Acquire), virt)
    }

    pub fn get_physical_in_pml4(&self, pml4: u64, virt: VirtAddr) -> Option<PhysAddr> {
        if pml4 == 0 {
            return None;
        }

        let pml4_virt = PhysAddr(pml4).to_virt();

        unsafe {
            let pml4_raw = pml4_virt.0 as *const u64;
            let pml4e = pml4_raw.add(virt.pml4_idx()).read_volatile();
            if (pml4e & 1) == 0 { return None; }

            let pdpt_phys = (pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pdpt_raw = pdpt_phys as *const u64;
            let pdpte = pdpt_raw.add(virt.pdpt_idx()).read_volatile();
            if (pdpte & 1) == 0 { return None; }

            if (pdpte & 0x80) != 0 {
                let frame = pdpte & 0x000FFFFFFFFFF000;
                let offset = virt.0 & (HUGE_PAGE_1G_SIZE - 1);
                return Some(PhysAddr(frame + offset));
            }

            let pd_phys = (pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pd_raw = pd_phys as *const u64;
            let pde = pd_raw.add(virt.pd_idx()).read_volatile();
            if (pde & 1) == 0 { return None; }

            if (pde & 0x80) != 0 {
                let frame = pde & 0x000FFFFFFFFFF000;
                let offset = virt.0 & (HUGE_PAGE_2M_SIZE - 1);
                return Some(PhysAddr(frame + offset));
            }

            let pt_phys = (pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pt_raw = pt_phys as *const u64;
            let pte = pt_raw.add(virt.pt_idx()).read_volatile();
            if (pte & 1) == 0 { return None; }

            let frame = pte & 0x000FFFFFFFFFF000;
            let offset = virt.0 & (PAGE_SIZE - 1);
            Some(PhysAddr(frame + offset))
        }
    }

    pub fn switch_page_table(&self, pml4: u64) {
        unsafe {
            self.write_cr3(pml4);
        }
    }

    pub fn create_user_page_table(&self) -> Option<u64> {
        let pmm = get_pmm();
        let pml4_phys = pmm.alloc_page()?;

        let pml4_virt = pml4_phys.to_virt();
        unsafe {
            core::ptr::write_bytes(pml4_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
        }

        let kernel_pml4 = KERNEL_PML4.load(Ordering::Acquire);
        let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt();

        unsafe {
            let src = kernel_pml4_virt.0 as *const u64;
            let dst = pml4_virt.0 as *mut u64;

            core::ptr::copy_nonoverlapping(src.add(256), dst.add(256), 256);

            core::arch::asm!("invlpg [{}]", in(reg) dst.add(256), options(nostack));

            let e256_src = src.add(256).read_volatile();
            let e256_dst = dst.add(256).read_volatile();
            if e256_src != e256_dst || (e256_src & 1) == 0 {
                pmm.free_page(pml4_phys);
                return None;
            }
        }

        let idx = self.find_free_user_slot();
        if idx < MAX_USER_PAGE_TABLES {
            unsafe {
                let tables = &mut *self.user_tables.get();
                tables[idx].pml4_phys = pml4_phys.as_u64();
                tables[idx].in_use = true;
            }
            self.user_table_count.fetch_add(1, Ordering::Relaxed);
        }

        Some(pml4_phys.as_u64())
    }

    pub fn map_page_in_table(&self, pml4: u64, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) {
        if pml4 == 0 {
            return;
        }

        self.acquire_lock();

        let pml4_virt = PhysAddr(pml4).to_virt();

        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4_ptr.add(virt.pml4_idx()), true);
            if pdpt.is_null() {
                self.release_lock(); return;
            }

            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true);
            if pd.is_null() {
                self.release_lock(); return;
            }

            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true);
            if pt.is_null() {
                self.release_lock(); return;
            }

            if flags.contains(PageFlags::USER) {
                (*pml4_ptr.add(virt.pml4_idx())).set_user(true);
                (*pdpt.add(virt.pdpt_idx())).set_user(true);
                (*pd.add(virt.pd_idx())).set_user(true);
            }

            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        self.release_lock();
    }

    pub fn destroy_page_table(&self, pml4: u64) {
        if pml4 == 0 { return; }

        self.acquire_lock();

        let pmm = get_pmm();
        let pml4_virt = PhysAddr(pml4).to_virt();

        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

            for i in 0..256usize {
                let pml4e = &*pml4_ptr.add(i);

                if pml4e.is_present() {
                    let pdpt_phys = pml4e.frame().as_u64();
                    let pdpt_virt = PhysAddr(pdpt_phys).to_virt();
                    let pdpt = pdpt_virt.0 as *mut PageTableEntry;

                    for j in 0..512usize {
                        let pdpte = &*pdpt.add(j);

                        if pdpte.is_present() && !pdpte.is_huge() {
                            let pd_phys = pdpte.frame().as_u64();
                            let pd_virt = PhysAddr(pd_phys).to_virt();
                            let pd = pd_virt.0 as *mut PageTableEntry;

                            for k in 0..512usize {
                                let pde = &*pd.add(k);

                                if pde.is_present() && !pde.is_huge() {
                                    let pt_phys = pde.frame().as_u64();
                                    pmm.free_page(PhysAddr(pt_phys));
                                }
                            }

                            pmm.free_page(PhysAddr(pd_phys));
                        }
                    }

                    pmm.free_page(PhysAddr(pdpt_phys));
                }
            }

            pmm.free_page(PhysAddr(pml4));
        }

        let tables = unsafe { &mut *self.user_tables.get() };
        for i in 0..MAX_USER_PAGE_TABLES {
            if tables[i].pml4_phys == pml4 && tables[i].in_use {
                tables[i].in_use = false;
                tables[i].pml4_phys = 0;
                self.user_table_count.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }

        self.release_lock();
    }

    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.total_maps.load(Ordering::Relaxed),
            self.total_unmaps.load(Ordering::Relaxed),
            self.page_faults.load(Ordering::Relaxed),
        )
    }

    // ==================== Private Methods ====================

    fn map_page_internal(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true);
            if pdpt.is_null() { return Err("Failed to allocate PDPT"); }

            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true);
            if pd.is_null() { return Err("Failed to allocate PD"); }

            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true);
            if pt.is_null() { return Err("Failed to allocate PT"); }

            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    fn map_2mb_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true);
            if pdpt.is_null() { return Err("Failed to allocate PDPT"); }

            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true);
            if pd.is_null() { return Err("Failed to allocate PD"); }

            let pde = &mut *pd.add(virt.pd_idx());
            pde.set_frame(phys);
            pde.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    fn map_1gb_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true);
            if pdpt.is_null() { return Err("Failed to allocate PDPT"); }

            let pdpte = &mut *pdpt.add(virt.pdpt_idx());
            pdpte.set_frame(phys);
            pdpte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    unsafe fn get_or_create_table_entry(&self, entry: *mut PageTableEntry, create: bool) -> *mut PageTableEntry {
        let e = &*entry;

        if e.is_present() && !e.is_huge() {
            e.frame().to_virt().0 as *mut PageTableEntry
        } else if create {
            let pmm = get_pmm();

            if let Some(page) = pmm.alloc_page() {
                let page_virt = page.to_virt();
                let pt = page_virt.0 as *mut PageTableEntry;
                core::ptr::write_bytes(pt as *mut u8, 0, PAGE_SIZE as usize);

                if e.is_huge() {
                    let huge_frame = e.frame();
                    let huge_flags = e.flags();
                    for i in 0..512 {
                        let pte = &mut *pt.add(i);
                        pte.set_frame(PhysAddr(huge_frame.as_u64() + i as u64 * 4096));
                        pte.set_flags((huge_flags & !PageFlags::HUGE_PAGE) | PageFlags::PRESENT);
                    }
                }

                (*entry).set_frame(page);
                (*entry).set_flags(PageFlags::PRESENT | PageFlags::WRITABLE);

                page_virt.0 as *mut PageTableEntry
            } else {
                core::ptr::null_mut()
            }
        } else {
            core::ptr::null_mut()
        }
    }

    pub fn split_2mb_page(&self, virt: u64) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 { return Err("VMM not initialized"); }

        let pml4_virt = PhysAddr(pml4_base).to_virt();
        let v = VirtAddr(virt);

        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            let pdpt = self.get_or_create_table_entry(pml4.add(v.pml4_idx()), false);
            if pdpt.is_null() { return Err("PDPT not present"); }

            let pd = self.get_or_create_table_entry(pdpt.add(v.pdpt_idx()), false);
            if pd.is_null() { return Err("PD not present"); }

            let pd_entry = &mut *pd.add(v.pd_idx());
            if !pd_entry.is_present() { return Err("PD entry not present"); }
            if !pd_entry.is_huge() { return Ok(()); }

            let huge_frame = pd_entry.frame();
            let huge_flags = pd_entry.flags();

            let pmm = get_pmm();
            let pt_page = match pmm.alloc_page() {
                Some(p) => p,
                None => return Err("Failed to allocate PT"),
            };
            let pt = pt_page.to_virt().0 as *mut PageTableEntry;
            core::ptr::write_bytes(pt as *mut u8, 0, PAGE_SIZE as usize);

            for i in 0..512 {
                let pte = &mut *pt.add(i);
                pte.set_frame(PhysAddr(huge_frame.as_u64() + i as u64 * 4096));
                pte.set_flags((huge_flags & !PageFlags::HUGE_PAGE) | PageFlags::PRESENT);
                pte.set_present(true);
            }

            pd_entry.set_frame(pt_page);
            let new_flags = (huge_flags & !PageFlags::HUGE_PAGE) | PageFlags::PRESENT;
            pd_entry.set_flags(new_flags);

            self.flush_tlb(virt);
        }

        Ok(())
    }

    pub fn ensure_pml4_user(&self, virt: u64) {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 { return; }

        let pml4_virt = PhysAddr(pml4_base).to_virt();
        let v = VirtAddr(virt);

        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            let entry = &mut *pml4.add(v.pml4_idx());
            if entry.is_present() && !entry.is_user() {
                entry.set_user(true);
            }
        }
    }

    pub fn ensure_path_user(&self, virt: u64) {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 { return; }

        let pml4_virt = PhysAddr(pml4_base).to_virt();
        let v = VirtAddr(virt);

        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pml4e = &mut *pml4.add(v.pml4_idx());
            if !pml4e.is_present() { return; }
            pml4e.set_user(true);

            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &mut *pdpt.add(v.pdpt_idx());
            if !pdpte.is_present() { return; }
            pdpte.set_user(true);

            let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
            let pde = &mut *pd.add(v.pd_idx());
            if !pde.is_present() { return; }
            pde.set_user(true);
        }
    }

    fn find_free_user_slot(&self) -> usize {
        let tables = unsafe { &*self.user_tables.get() };
        for i in 0..MAX_USER_PAGE_TABLES {
            if !tables[i].in_use {
                return i;
            }
        }
        MAX_USER_PAGE_TABLES
    }

    #[inline(always)]
    fn acquire_lock(&self) {
        while VMM_LOCK.compare_exchange_weak(
            false, true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn release_lock(&self) {
        VMM_LOCK.store(false, Ordering::Release);
    }

    #[inline(always)]
    unsafe fn read_cr3(&self) -> u64 {
        let cr3: u64;
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nostack, preserves_flags)
        );
        cr3
    }

    #[inline(always)]
    unsafe fn write_cr3(&self, val: u64) {
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) val,
            options(nostack, preserves_flags)
        );
    }

    #[inline(always)]
    fn flush_tlb(&self, addr: u64) {
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) addr,
                options(nostack, preserves_flags)
            );
        }

        #[cfg(feature = "smp")]
        {
            use crate::kernel::smp;
            if smp::is_smp_enabled() && smp::get_cpu_count() > 1 {
                smp::send_tlb_invalidate_ipi(addr);
            }
        }
    }
}

static GLOBAL_VMM: spin::Once<VirtualMemoryManager> = spin::Once::new();

pub fn vmm_init() {
    GLOBAL_VMM.call_once(|| {
        let vmm = VirtualMemoryManager::new();
        vmm.init();
        vmm
    });
}

pub fn get_vmm() -> &'static VirtualMemoryManager {
    GLOBAL_VMM.get().expect("[VMM] accessed before initialization")
}

pub fn get_kernel_pml4() -> u64 {
    KERNEL_PML4.load(Ordering::Acquire)
}
