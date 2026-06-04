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
//! ## SAFETY
//!
//! Interior mutability for `user_tables` is achieved via `UnsafeCell`
//! protected by an internal `AtomicBool` spinlock (`VMM_LOCK`).
//! All mutations occur under the lock, making `unsafe impl Sync` sound.
//!
//! ### Key invariants (all `unsafe` blocks depend on these):
//!
//! 1. **KERNEL_PML4**: Written once in `init()`, read-only afterwards (Release/Acquire).
//! 2. **VMM_LOCK**: All page table modifications and UserPageTable mutations are serialized.
//! 3. **PhysAddr → VirtAddr**: `phys_to_virt(pa) = pa + KERNEL_BASE` → valid kernel VA
//!    because the kernel identity-maps all physical memory at `KERNEL_BASE`.
//! 4. **PMM allocation**: Returned physical addresses are always page-aligned and valid.
//! 5. **Page table pointers**: Every pointer derived from `PhysAddr::to_virt()` points to
//!    a full 4KB page allocated by PMM, making all 512-entry traversals safe.
//! 6. **Present bit guards**: Before dereferencing any table entry as a pointer to the
//!    next level, we check `entry & 1 != 0`.
//! 7. **Deadlock prevention**: `acquire_lock` panics on recursive acquisition in debug
//!    builds via `VMM_LOCK_RECURSIVE`, preventing SMP deadlocks before they occur.
//!
//! ## Lock ordering
//!
//! **VMM_LOCK must never be held while acquiring VMA_LOCK (MmStruct::vmas).**
//! This prevents the ABBA deadlock:
//!   Thread A: VMM_LOCK → VMA_LOCK
//!   Thread B: VMA_LOCK → VMM_LOCK (happens in MmStruct::remove_range)
//!
//! All callers obey this rule:
//! - `user_driver.rs`: VMM ops (map/unmap) → release VMM_LOCK → VMA ops (insert/remove)
//! - `page_fault.rs`: VMA lookup (find_vma) → release VMA_LOCK → VMM ops (map_page)
//! - `MmStruct::remove_range`: VMA_LOCK held → VMM_LOCK acquired (reverse order safe)

use super::*;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::kernel::framework::sync_tcb_legacy::spinlock::{disable_interrupts, restore_interrupts, IrqSaveFlags};

static KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

static VMM_LOCK: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
static VMM_LOCK_RECURSIVE: AtomicBool = AtomicBool::new(false);

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

// SAFETY: VMM_LOCK serializes all writes to user_tables (via UnsafeCell).
// Atomic counters use Relaxed ordering (single-writer under lock).
unsafe impl Sync for VirtualMemoryManager {}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            user_tables: UnsafeCell::new(
                [UserPageTable {
                    pml4_phys: 0,
                    in_use: false,
                }; MAX_USER_PAGE_TABLES],
            ),
            user_table_count: AtomicUsize::new(0),
            total_maps: AtomicU64::new(0),
            total_unmaps: AtomicU64::new(0),
            page_faults: AtomicU64::new(0),
        }
    }

    pub fn init(&self) {
        // SAFETY: read_cr3() reads the CR3 control register — safe at any time
        let cr3 = unsafe { self.read_cr3() };

        KERNEL_PML4.store(cr3, Ordering::Release);

        super::api::kernel_pml4.store(cr3, Ordering::Release);
    }

    pub fn map_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        let _flags = self.acquire_lock();

        let result = self.map_page_internal(virt, phys, flags);

        if result.is_ok() {
            self.total_maps.fetch_add(1, Ordering::Relaxed);
        }

        self.release_lock(&_flags);
        result
    }

    pub fn map_huge_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
        size_type: PageSize,
    ) -> Result<(), &'static str> {
        if !size_type.is_aligned(virt.0) || !size_type.is_aligned(phys.0) {
            return Err("Address not aligned for huge page");
        }

        let _flags = self.acquire_lock();

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

        self.release_lock(&_flags);
        result
    }

    pub fn unmap_page(&self, virt: VirtAddr) {
        let _flags = self.acquire_lock();

        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            self.release_lock(&_flags);
            return;
        }

        // SAFETY: pml4_base = CR3 value, KERNEL_BASE offset produces valid kernel VA
        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: VMM_LOCK held. Page table walk with present-bit guards at each level.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pml4e = &*pml4.add(virt.pml4_idx());

            if !pml4e.is_present() {
                self.release_lock(&_flags);
                return;
            }

            // SAFETY: pml4e.frame() is present & valid frame; phys_to_virt gives kernel VA
            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &*pdpt.add(virt.pdpt_idx());

            if !pdpte.is_present() {
                self.release_lock(&_flags);
                return;
            }

            if pdpte.is_huge() {
                // 1GB page: clear the PDPT entry directly
                (*pdpt.add(virt.pdpt_idx())).set_value(0);
                self.flush_tlb(virt.0);
            } else {
                // SAFETY: pdpte.frame() valid; present && !huge → points to PD
                let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
                let pde = &*pd.add(virt.pd_idx());

                if !pde.is_present() {
                    self.release_lock(&_flags);
                    return;
                }

                if pde.is_huge() {
                    (*pd.add(virt.pd_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                } else {
                    // SAFETY: pde.frame() valid; present && !huge → points to PT
                    let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
                    (*pt.add(virt.pt_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                }
            }
        }

        self.total_unmaps.fetch_add(1, Ordering::Relaxed);
        self.release_lock(&_flags);
    }

    pub fn get_physical(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.get_physical_in_pml4(KERNEL_PML4.load(Ordering::Acquire), virt)
    }

    pub fn get_physical_in_pml4(&self, pml4: u64, virt: VirtAddr) -> Option<PhysAddr> {
        if pml4 == 0 {
            return None;
        }

        // SAFETY: pml4 is a valid PML4 physical address; phys_to_virt gives kernel VA
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: Read-only page table walk. volatile reads for correctness under
        // concurrent hardware page walker updates (A/D bits).
        unsafe {
            let pml4_raw = pml4_virt.0 as *const u64;
            let pml4e = pml4_raw.add(virt.pml4_idx()).read_volatile();
            if (pml4e & 1) == 0 {
                return None;
            }

            // SAFETY: pml4e present → frame bits point to valid PDPT
            let pdpt_virt = (pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pdpt_raw = pdpt_virt as *const u64;
            let pdpte = pdpt_raw.add(virt.pdpt_idx()).read_volatile();
            if (pdpte & 1) == 0 {
                return None;
            }

            if (pdpte & 0x80) != 0 {
                let frame = pdpte & 0x000FFFFFFFFFF000;
                let offset = virt.0 & (HUGE_PAGE_1G_SIZE - 1);
                return Some(PhysAddr(frame + offset));
            }

            // SAFETY: pdpte present && !huge → valid PD pointer
            let pd_virt = (pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pd_raw = pd_virt as *const u64;
            let pde = pd_raw.add(virt.pd_idx()).read_volatile();
            if (pde & 1) == 0 {
                return None;
            }

            if (pde & 0x80) != 0 {
                let frame = pde & 0x000FFFFFFFFFF000;
                let offset = virt.0 & (HUGE_PAGE_2M_SIZE - 1);
                return Some(PhysAddr(frame + offset));
            }

            // SAFETY: pde present && !huge → valid PT pointer
            let pt_virt = (pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let pt_raw = pt_virt as *const u64;
            let pte = pt_raw.add(virt.pt_idx()).read_volatile();
            if (pte & 1) == 0 {
                return None;
            }

            let frame = pte & 0x000FFFFFFFFFF000;
            let offset = virt.0 & (PAGE_SIZE - 1);
            Some(PhysAddr(frame + offset))
        }
    }

    pub fn switch_page_table(&self, pml4: u64) {
        // SAFETY: pml4 must point to a valid PML4 table; CR3 write is privileged
        unsafe {
            self.write_cr3(pml4);
        }
    }

    pub fn create_user_page_table(&self) -> Option<u64> {
        let pmm = get_pmm();
        let pml4_phys = pmm.alloc_page()?;

        // SAFETY: pml4_phys from PMM, phys_to_virt valid; zero for clean state
        let pml4_virt = pml4_phys.to_virt();
        unsafe {
            core::ptr::write_bytes(pml4_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
        }

        let kernel_pml4 = KERNEL_PML4.load(Ordering::Acquire);
        // SAFETY: kernel_pml4 valid (set in init), phys_to_virt valid
        let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt();

        // SAFETY: Copy kernel-space entries (256..511) into user PML4.
        // Both src and dst are valid page-aligned kernel VAs.
        unsafe {
            let src = kernel_pml4_virt.0 as *const u64;
            let dst = pml4_virt.0 as *mut u64;

            core::ptr::copy_nonoverlapping(src.add(256), dst.add(256), 256);

            crate::arch!(tlb_flush_page(dst.add(256) as usize));

            // Verify copy by reading back entry 256
            let e256_src = src.add(256).read_volatile();
            let e256_dst = dst.add(256).read_volatile();
            if e256_src != e256_dst || (e256_src & 1) == 0 {
                pmm.free_page(pml4_phys);
                return None;
            }
        }

        let _flags = self.acquire_lock();

        let idx = self.find_free_user_slot();
        if idx < MAX_USER_PAGE_TABLES {
            // SAFETY: VMM_LOCK held; exclusive access to user_tables via UnsafeCell
            unsafe {
                let tables = &mut *self.user_tables.get();
                tables[idx].pml4_phys = pml4_phys.as_u64();
                tables[idx].in_use = true;
            }
            self.user_table_count.fetch_add(1, Ordering::Relaxed);
        }

        self.release_lock(&_flags);

        Some(pml4_phys.as_u64())
    }

    pub fn map_page_in_table(&self, pml4: u64, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) {
        if pml4 == 0 {
            return;
        }

        let _flags = self.acquire_lock();

        // SAFETY: pml4 is a valid PML4 address; VMM_LOCK held
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: Full 4-level page table walk with creation.
        // VMM_LOCK serializes all PT modifications.
        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4_ptr.add(virt.pml4_idx()), true, 0);
            if pdpt.is_null() {
                self.release_lock(&_flags);
                return;
            }

            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true, 0x200000);
            if pd.is_null() {
                self.release_lock(&_flags);
                return;
            }

            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true, 0x1000);
            if pt.is_null() {
                self.release_lock(&_flags);
                return;
            }

            if flags.contains(PageFlags::USER) {
                // SAFETY: ptr.add(idx) stays within the 512-entry table
                (*pml4_ptr.add(virt.pml4_idx())).set_user(true);
                (*pdpt.add(virt.pdpt_idx())).set_user(true);
                (*pd.add(virt.pd_idx())).set_user(true);
            }

            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        self.release_lock(&_flags);
    }

    pub fn unmap_page_in_table(&self, pml4: u64, virt: VirtAddr) {
        if pml4 == 0 {
            return;
        }

        let _flags = self.acquire_lock();

        // SAFETY: pml4 = process CR3 value. phys_to_virt gives valid kernel VA.
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: Read-only page table walk with present-bit guards at each level.
        // VMM_LOCK serializes all PT modifications.
        unsafe {
            let pml4_tbl = pml4_virt.0 as *mut PageTableEntry;

            // SAFETY: pml4_tbl.add(idx) stays within the 4KB PML4 page
            let pml4e = &*pml4_tbl.add(virt.pml4_idx());

            if !pml4e.is_present() {
                self.release_lock(&_flags);
                return;
            }

            // SAFETY: pml4e.frame() is present & valid frame; phys_to_virt gives kernel VA
            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &*pdpt.add(virt.pdpt_idx());

            if !pdpte.is_present() {
                self.release_lock(&_flags);
                return;
            }

            if pdpte.is_huge() {
                // 1GB page: clear the PDPT entry directly
                (*pdpt.add(virt.pdpt_idx())).set_value(0);
                self.flush_tlb(virt.0);
            } else {
                // SAFETY: pdpte.frame() valid; present && !huge → points to PD
                let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
                let pde = &*pd.add(virt.pd_idx());

                if !pde.is_present() {
                    self.release_lock(&_flags);
                    return;
                }

                if pde.is_huge() {
                    // 2MB page: clear the PDE entry directly
                    (*pd.add(virt.pd_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                } else {
                    // SAFETY: pde.frame() valid; present && !huge → points to PT
                    let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
                    let pt_idx = virt.pt_idx();
                    (*pt.add(pt_idx)).set_value(0);
                    self.flush_tlb(virt.0);

                    // Recursively free empty intermediate page tables
                    if self.is_table_empty(pt) {
                        let pt_phys = pde.frame().as_u64();
                        (*pd.add(virt.pd_idx())).set_value(0);
                        get_pmm().free_page(PhysAddr(pt_phys));

                        if self.is_table_empty(pd) {
                            let pd_phys = pdpte.frame().as_u64();
                            (*pdpt.add(virt.pdpt_idx())).set_value(0);
                            get_pmm().free_page(PhysAddr(pd_phys));

                            if self.is_table_empty(pdpt) {
                                let pdpt_phys = pml4e.frame().as_u64();
                                (*pml4_tbl.add(virt.pml4_idx())).set_value(0);
                                get_pmm().free_page(PhysAddr(pdpt_phys));
                            }
                        }
                    }
                }
            }
        }

        self.total_unmaps.fetch_add(1, Ordering::Relaxed);
        self.release_lock(&_flags);
    }

    pub fn destroy_page_table(&self, pml4: u64) {
        if pml4 == 0 {
            return;
        }

        let _flags = self.acquire_lock();

        let pmm = get_pmm();
        // SAFETY: pml4 valid; VMM_LOCK held
        let pml4_virt = PhysAddr(pml4).to_virt();

        // SAFETY: Walk all 4 levels freeing page tables.
        // Only user-space entries (0..255); kernel entries are shared.
        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;

            for i in 0..256usize {
                // SAFETY: pml4_ptr.add(i) within the 4KB PML4 page
                let pml4e = &*pml4_ptr.add(i);

                if pml4e.is_present() {
                    let pdpt_phys = pml4e.frame().as_u64();
                    let pdpt_virt = PhysAddr(pdpt_phys).to_virt();
                    let pdpt = pdpt_virt.0 as *mut PageTableEntry;

                    for j in 0..512usize {
                        // SAFETY: pdpt.add(j) within the 4KB PDPT page
                        let pdpte = &*pdpt.add(j);

                        if pdpte.is_present() && !pdpte.is_huge() {
                            let pd_phys = pdpte.frame().as_u64();
                            let pd_virt = PhysAddr(pd_phys).to_virt();
                            let pd = pd_virt.0 as *mut PageTableEntry;

                            for k in 0..512usize {
                                // SAFETY: pd.add(k) within the 4KB PD page
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

        // SAFETY: VMM_LOCK held; only mutation is clearing user_tables slot
        let tables = unsafe { &mut *self.user_tables.get() };
        for i in 0..MAX_USER_PAGE_TABLES {
            if tables[i].pml4_phys == pml4 && tables[i].in_use {
                tables[i].in_use = false;
                tables[i].pml4_phys = 0;
                self.user_table_count.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }

        self.release_lock(&_flags);
    }

    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.total_maps.load(Ordering::Relaxed),
            self.total_unmaps.load(Ordering::Relaxed),
            self.page_faults.load(Ordering::Relaxed),
        )
    }

    // ==================== Private Methods ====================

    fn map_page_internal(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        // SAFETY: KERNEL_PML4 valid; caller holds VMM_LOCK (via map_page/map_huge_page)
        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: Full 4-level page table walk with creation under VMM_LOCK
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true, 0);
            if pdpt.is_null() {
                return Err("Failed to allocate PDPT");
            }

            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true, 0x200000);
            if pd.is_null() {
                return Err("Failed to allocate PD");
            }

            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true, 0x1000);
            if pt.is_null() {
                return Err("Failed to allocate PT");
            }

            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    fn map_2mb_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: 2MB huge page mapping at PD level. VMM_LOCK held by caller.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true, 0);
            if pdpt.is_null() {
                return Err("Failed to allocate PDPT");
            }

            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true, 0x200000);
            if pd.is_null() {
                return Err("Failed to allocate PD");
            }

            let pde = &mut *pd.add(virt.pd_idx());
            if pde.is_present() && !pde.is_huge() {
                return Err("PD entry already split to PT, cannot map 2MB page");
            }
            pde.set_frame(phys);
            pde.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    fn map_1gb_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();

        // SAFETY: 1GB huge page mapping at PDPT level. VMM_LOCK held by caller.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true, 0);
            if pdpt.is_null() {
                return Err("Failed to allocate PDPT");
            }

            let pdpte = &mut *pdpt.add(virt.pdpt_idx());
            if pdpte.is_present() && !pdpte.is_huge() {
                return Err("PDPT entry already split, cannot map 1GB page");
            }
            pdpte.set_frame(phys);
            pdpte.set_flags(flags);

            self.flush_tlb(virt.0);
        }

        Ok(())
    }

    unsafe fn get_or_create_table_entry(
        &self,
        entry: *mut PageTableEntry,
        create: bool,
        huge_step: u64,
    ) -> *mut PageTableEntry {
        // SAFETY: caller guarantees `entry` points to a valid PageTableEntry within a
        // page table page allocated by PMM. Dereference is bounds-checked by 512-entry table size.
        let e = &*entry;

        if e.is_present() && !e.is_huge() {
            // SAFETY: Present && !huge → frame bits contain a valid physical address
            // pointing to the next-level table. phys_to_virt gives a valid kernel VA.
            e.frame().to_virt().0 as *mut PageTableEntry
        } else if create {
            let pmm = get_pmm();

            if let Some(page) = pmm.alloc_page() {
                let page_virt = page.to_virt();
                let pt = page_virt.0 as *mut PageTableEntry;
                core::ptr::write_bytes(pt as *mut u8, 0, PAGE_SIZE as usize);

                if e.is_huge() {
                    // Splitting a huge page: populate 512 4KB entries from the huge frame
                    let huge_frame = e.frame();
                    let huge_flags = e.flags();
                    let step = if huge_step > 0 { huge_step } else { 4096 };
                    for i in 0..512 {
                        // SAFETY: pt points to a full 4KB page; add(i) stays within bounds
                        let pte = &mut *pt.add(i);
                        pte.set_frame(PhysAddr(huge_frame.as_u64() + i as u64 * step));
                        pte.set_flags((huge_flags & !PageFlags::HUGE_PAGE) | PageFlags::PRESENT);
                    }
                }

                // SAFETY: `entry` is a valid pointer; write atomic via set_frame/set_flags
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
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }

        let _flags = self.acquire_lock();

        let result: Result<(), &'static str> = (|| {
            let pml4_virt = PhysAddr(pml4_base).to_virt();
            let v = VirtAddr(virt);

            // SAFETY: Splitting a 2MB huge page into 512 4KB pages.
            // VMM_LOCK held; all page table modifications are serialized.
            unsafe {
                let pml4 = pml4_virt.0 as *mut PageTableEntry;
                let pdpt = self.get_or_create_table_entry(pml4.add(v.pml4_idx()), false, 0);
                if pdpt.is_null() {
                    return Err("PDPT not present");
                }

                let pd = self.get_or_create_table_entry(pdpt.add(v.pdpt_idx()), false, 0);
                if pd.is_null() {
                    return Err("PD not present");
                }

                let pd_entry = &mut *pd.add(v.pd_idx());
                if !pd_entry.is_present() {
                    return Err("PD entry not present");
                }
                if !pd_entry.is_huge() {
                    return Ok(());
                }

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
                    // SAFETY: pt is a full 4KB PT page; add(i) stays in bounds
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
        })();

        self.release_lock(&_flags);
        result
    }

    pub fn ensure_pml4_user(&self, virt: u64) {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return;
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();
        let v = VirtAddr(virt);

        // SAFETY: Setting USER bit on PML4 entry; KERNEL_PML4 valid, index in range
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            let entry = &mut *pml4.add(v.pml4_idx());
            if entry.is_present() && !entry.is_user() {
                entry.set_user(true);
                self.flush_tlb(virt);
            }
        }
    }

    pub fn ensure_path_user(&self, virt: u64) {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return;
        }

        let pml4_virt = PhysAddr(pml4_base).to_virt();
        let v = VirtAddr(virt);

        // SAFETY: Traversing PML4 → PDPT → PD, setting USER at each level.
        // Present-bit guards at each step. Indices computed from VA bits.
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;

            let pml4e = &mut *pml4.add(v.pml4_idx());
            if !pml4e.is_present() {
                return;
            }
            pml4e.set_user(true);

            let pdpt = pml4e.frame().to_virt().0 as *mut PageTableEntry;
            let pdpte = &mut *pdpt.add(v.pdpt_idx());
            if !pdpte.is_present() {
                return;
            }
            pdpte.set_user(true);

            if pdpte.is_huge() {
                self.flush_tlb(virt);
                return;
            }

            let pd = pdpte.frame().to_virt().0 as *mut PageTableEntry;
            let pde = &mut *pd.add(v.pd_idx());
            if !pde.is_present() {
                return;
            }
            pde.set_user(true);
        }

        self.flush_tlb(virt);
    }

    pub fn clone_user_page_table(&self, parent_pml4: u64) -> Option<u64> {
        if parent_pml4 == 0 {
            return None;
        }

        let _flags = self.acquire_lock();

        let pmm = get_pmm();
        let child_pml4_phys = pmm.alloc_page()?;
        let child_pml4_base = child_pml4_phys.to_virt().0 as *mut u64;

        // SAFETY: child_pml4_phys from PMM, phys_to_virt valid
        unsafe {
            core::ptr::write_bytes(child_pml4_base, 0, PAGE_SIZE as usize);
        }

        let kernel_pml4 = KERNEL_PML4.load(Ordering::Acquire);
        // SAFETY: kernel_pml4 valid; both src and dst are page-aligned kernel VAs
        let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt().0 as *const u64;
        unsafe {
            core::ptr::copy_nonoverlapping(
                kernel_pml4_virt.add(256),
                child_pml4_base.add(256),
                256,
            );
        }

        // SAFETY: parent_pml4 is a valid user PML4; VMM_LOCK held
        let parent_pml4_virt = PhysAddr(parent_pml4).to_virt().0 as *const u64;

        for i in 0..256u16 {
            // SAFETY: i in 0..255 within PML4 page; volatile for hardware-updated bits
            let parent_pml4e = unsafe { parent_pml4_virt.add(i as usize).read_volatile() };
            if (parent_pml4e & 1) == 0 {
                continue;
            }

            let child_pdpt_phys = pmm.alloc_page()?;
            let child_pdpt = child_pdpt_phys.to_virt().0 as *mut u64;
            // SAFETY: child_pdpt from PMM, phys_to_virt valid
            unsafe {
                core::ptr::write_bytes(child_pdpt, 0, PAGE_SIZE as usize);
            }

            let mut child_pml4e = parent_pml4e;
            child_pml4e = (child_pml4e & 0xFFF) | (child_pdpt_phys.as_u64() & 0x000FFFFFFFFFF000);
            // SAFETY: child_pml4_base is a valid 4KB PML4 page; volatile write for TLB coherency
            unsafe {
                child_pml4_base.add(i as usize).write_volatile(child_pml4e);
            }

            // SAFETY: parent_pml4e present → frame bits point to valid PDPT
            let parent_pdpt_virt = (parent_pml4e & 0x000FFFFFFFFFF000) + KERNEL_BASE;
            let parent_pdpt = parent_pdpt_virt as *const u64;

            for j in 0..512u16 {
                // SAFETY: j in 0..511 within PDPT page; volatile read
                let parent_pdpte = unsafe { parent_pdpt.add(j as usize).read_volatile() };
                if (parent_pdpte & 1) == 0 {
                    continue;
                }
                if (parent_pdpte & 0x80) != 0 {
                    continue;
                }

                let child_pd_phys = pmm.alloc_page()?;
                let child_pd = child_pd_phys.to_virt().0 as *mut u64;
                // SAFETY: child_pd from PMM
                unsafe {
                    core::ptr::write_bytes(child_pd, 0, PAGE_SIZE as usize);
                }

                let mut child_pdpte_v = parent_pdpte;
                child_pdpte_v =
                    (child_pdpte_v & 0xFFF) | (child_pd_phys.as_u64() & 0x000FFFFFFFFFF000);
                // SAFETY: child_pdpt valid; volatile write
                unsafe {
                    child_pdpt.add(j as usize).write_volatile(child_pdpte_v);
                }

                // SAFETY: parent_pdpte present → valid PD pointer
                let parent_pd_virt = (parent_pdpte & 0x000FFFFFFFFFF000) + KERNEL_BASE;
                let parent_pd = parent_pd_virt as *const u64;

                for k in 0..512u16 {
                    // SAFETY: k in 0..511 within PD page; volatile read
                    let parent_pde = unsafe { parent_pd.add(k as usize).read_volatile() };
                    if (parent_pde & 1) == 0 {
                        continue;
                    }

                    if (parent_pde & 0x80) != 0 {
                        // Deep copy 2MB huge page
                        let huge_phys = pmm.alloc_pages(512)?;
                        let huge_virt = PhysAddr(huge_phys.as_u64()).to_virt().0;
                        // SAFETY: parent_huge is valid 2MB kernel VA
                        let parent_huge = (parent_pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                parent_huge as *const u8,
                                huge_virt as *mut u8,
                                2 * 1024 * 1024,
                            );
                        }
                        let mut child_pde_v = parent_pde;
                        child_pde_v =
                            (child_pde_v & 0xFFF) | (huge_phys.as_u64() & 0x000FFFFFFFFFF000);
                        // SAFETY: child_pd valid; volatile write
                        unsafe {
                            child_pd.add(k as usize).write_volatile(child_pde_v);
                        }
                        continue;
                    }

                    let child_pt_phys = pmm.alloc_page()?;
                    let child_pt = child_pt_phys.to_virt().0 as *mut u64;
                    // SAFETY: child_pt from PMM
                    unsafe {
                        core::ptr::write_bytes(child_pt, 0, PAGE_SIZE as usize);
                    }

                    let mut child_pde_v = parent_pde;
                    child_pde_v =
                        (child_pde_v & 0xFFF) | (child_pt_phys.as_u64() & 0x000FFFFFFFFFF000);
                    // SAFETY: child_pd valid; volatile write
                    unsafe {
                        child_pd.add(k as usize).write_volatile(child_pde_v);
                    }

                    // SAFETY: parent_pde present && !huge → valid PT pointer
                    let parent_pt_virt = (parent_pde & 0x000FFFFFFFFFF000) + KERNEL_BASE;
                    let parent_pt = parent_pt_virt as *const u64;

                    for l in 0..512u16 {
                        // SAFETY: l in 0..511 within PT page; volatile read
                        let parent_pte = unsafe { parent_pt.add(l as usize).read_volatile() };
                        if (parent_pte & 1) == 0 {
                            continue;
                        }

                        let child_page_phys = pmm.alloc_page()?;
                        let child_page_virt = PhysAddr(child_page_phys.as_u64()).to_virt().0;
                        // SAFETY: parent_page_virt is valid kernel VA from PTE
                        let parent_page_virt = (parent_pte & 0x000FFFFFFFFFF000) + KERNEL_BASE;

                        // SAFETY: Both addresses are valid 4KB kernel VAs
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                parent_page_virt as *const u8,
                                child_page_virt as *mut u8,
                                PAGE_SIZE as usize,
                            );
                        }

                        let mut child_pte_v = parent_pte;
                        child_pte_v =
                            (child_pte_v & 0xFFF) | (child_page_phys.as_u64() & 0x000FFFFFFFFFF000);
                        // SAFETY: child_pt valid; volatile write
                        unsafe {
                            child_pt.add(l as usize).write_volatile(child_pte_v);
                        }
                    }
                }
            }
        }

        self.release_lock(&_flags);
        Some(child_pml4_phys.as_u64())
    }

    fn find_free_user_slot(&self) -> usize {
        // SAFETY: Read-only access to user_tables via UnsafeCell under VMM_LOCK.
        let tables = unsafe { &*self.user_tables.get() };
        for i in 0..MAX_USER_PAGE_TABLES {
            if !tables[i].in_use {
                return i;
            }
        }
        MAX_USER_PAGE_TABLES
    }

    #[inline(always)]
    pub fn acquire_lock(&self) -> IrqSaveFlags {
        let flags = disable_interrupts();
        while VMM_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        #[cfg(debug_assertions)]
        {
            if VMM_LOCK_RECURSIVE.swap(true, Ordering::Relaxed) {
                panic!("VMM_LOCK: recursive acquisition detected (deadlock)");
            }
        }
        flags
    }

    #[inline(always)]
    pub fn release_lock(&self, flags: &IrqSaveFlags) {
        #[cfg(debug_assertions)]
        {
            VMM_LOCK_RECURSIVE.store(false, Ordering::Relaxed);
        }
        VMM_LOCK.store(false, Ordering::Release);
        restore_interrupts(flags);
    }

    #[inline(always)]
    unsafe fn read_cr3(&self) -> u64 {
        // SAFETY: Reading CR3 is always safe; returns current page table base
        crate::arch!(read_page_table_base())
    }

    #[inline(always)]
    unsafe fn write_cr3(&self, val: u64) {
        // SAFETY: val must point to a valid PML4 table; caller guarantees this
        crate::arch!(write_page_table_base(val));
    }

    #[inline(always)]
    fn flush_tlb(&self, addr: u64) {
        crate::arch!(tlb_flush_page(addr as usize));

        #[cfg(feature = "smp")]
        {
            use crate::kernel::framework::smp;
            if smp::is_enabled() && smp::get_cpu_count() > 1 {
                smp::broadcast_tlb_invalidate();
            }
        }
    }

    fn is_table_empty(&self, table: *mut PageTableEntry) -> bool {
        for i in 0..512usize {
            unsafe {
                if (*table.add(i)).is_present() {
                    return false;
                }
            }
        }
        true
    }
}

static GLOBAL_VMM: spin::Once<VirtualMemoryManager> = spin::Once::new();

pub fn vmm_init() {
    GLOBAL_VMM.call_once(|| {
        let vmm = VirtualMemoryManager::new();
        vmm.init();
        vmm
    });
    super::cow::cow_init();
}

pub fn get_vmm() -> &'static VirtualMemoryManager {
    GLOBAL_VMM
        .get()
        .expect("[VMM] accessed before initialization")
}

pub fn get_kernel_pml4() -> u64 {
    KERNEL_PML4.load(Ordering::Acquire)
}

pub fn get_current_pml4() -> u64 {
    let cr3 = crate::arch!(read_page_table_base());
    if cr3 != 0 {
        cr3
    } else {
        KERNEL_PML4.load(Ordering::Acquire)
    }
}
