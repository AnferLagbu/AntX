//! Virtual Memory Manager (VMM)!
//!
//! Manages virtual memory mapping using 4-level page tables (x86_64).
//! Provides:
//! - Virtual to physical address translation
//! - Page table creation and management
//! - User space page tables
//! - Huge page support (2MB, 1GB)
//! - Memory protection and access control

/// Serial print macro (placeholder)
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

use super::*;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, Ordering};

/// Kernel PML4 base address (global, matching C implementation)
static KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

/// VMM lock for thread safety
static VMM_LOCK: AtomicBool = AtomicBool::new(false);

/// Maximum number of user page tables
const MAX_USER_PAGE_TABLES: usize = 256;

/// User page table tracking structure
#[derive(Clone, Copy)]
struct UserPageTable {
    pml4_phys: u64,
    in_use: bool,
}

/// Virtual Memory Manager state
pub struct VirtualMemoryManager {
    /// User page table array
    user_tables: [UserPageTable; MAX_USER_PAGE_TABLES],
    
    /// Number of active user page tables
    user_table_count: AtomicUsize,
    
    /// Statistics
    total_maps: AtomicU64,
    total_unmaps: AtomicU64,
    page_faults: AtomicU64,
}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            user_tables: [UserPageTable { pml4_phys: 0, in_use: false }; MAX_USER_PAGE_TABLES],
            user_table_count: AtomicUsize::new(0),
            total_maps: AtomicU64::new(0),
            total_unmaps: AtomicU64::new(0),
            page_faults: AtomicU64::new(0),
        }
    }

    /// Initialize the VMM
    pub fn init(&mut self) {
        let cr3 = unsafe { self.read_cr3() };
        
        KERNEL_PML4.store(cr3, Ordering::Release);
        
        serial_println!("[VMM] Initialized with kernel PML4 at 0x{:X}", cr3);
    }

    /// Map a virtual page to a physical page
    pub fn map_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        self.acquire_lock();
        
        let result = self.map_page_internal(virt, phys, flags);
        
        if result.is_ok() {
            self.total_maps.fetch_add(1, Ordering::Relaxed);
        }
        
        self.release_lock();
        result
    }

    /// Map a huge page (2MB or 1GB)
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

    /// Unmap a virtual page
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
            
            // Walk the page table hierarchy using individual entries
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
            
            // Check if it's a huge page (1GB)
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
                
                // Check for 2MB huge page
                if pde.is_huge() {
                    (*pd.add(virt.pd_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                } else {
                    // Regular 4KB page
                    let pt = pde.frame().to_virt().0 as *mut PageTableEntry;
                    (*pt.add(virt.pt_idx())).set_value(0);
                    self.flush_tlb(virt.0);
                }
            }
        }
        
        self.total_unmaps.fetch_add(1, Ordering::Relaxed);
        self.release_lock();
    }

    /// Get physical address for a given virtual address
    pub fn get_physical(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.get_physical_in_pml4(KERNEL_PML4.load(Ordering::Acquire), virt)
    }

    /// Get physical address in a specific page table context
    pub fn get_physical_in_pml4(&self, pml4: u64, virt: VirtAddr) -> Option<PhysAddr> {
        if pml4 == 0 {
            return None;
        }
        
        let pml4_virt = PhysAddr(pml4).to_virt();
        
        unsafe {
            let pml4 = pml4_virt.0 as *const PageTableEntry;
            
            // Check PML4 entry
            let pml4e = &*pml4.add(virt.pml4_idx());
            if !pml4e.is_present() {
                return None;
            }
            
            // Get PDPT
            let pdpt = pml4e.frame().to_virt().0 as *const PageTableEntry;
            let pdpte = &*pdpt.add(virt.pdpt_idx());
            if !pdpte.is_present() {
                return None;
            }
            
            // Check for 1GB huge page
            if pdpte.is_huge() {
                let frame = pdpte.frame();
                let offset = virt.0 & (HUGE_PAGE_1G_SIZE - 1);
                return Some(PhysAddr(frame.0 + offset));
            }
            
            // Get PD
            let pd = pdpte.frame().to_virt().0 as *const PageTableEntry;
            let pde = &*pd.add(virt.pd_idx());
            if !pde.is_present() {
                return None;
            }
            
            // Check for 2MB huge page
            if pde.is_huge() {
                let frame = pde.frame();
                let offset = virt.0 & (HUGE_PAGE_2M_SIZE - 1);
                return Some(PhysAddr(frame.0 + offset));
            }
            
            // Get PT
            let pt = pde.frame().to_virt().0 as *const PageTableEntry;
            let pte = &*pt.add(virt.pt_idx());
            if !pte.is_present() {
                return None;
            }
            
            let frame = pte.frame();
            let offset = virt.0 & (PAGE_SIZE - 1);
            Some(PhysAddr(frame.0 + offset))
        }
    }

    /// Switch to a different page table (load CR3)
    pub fn switch_page_table(&self, pml4: u64) {
        unsafe {
            self.write_cr3(pml4);
        }
    }

    /// Create a new user space page table
    pub fn create_user_page_table(&self) -> Option<u64> {
        let pmm = get_pmm();
        let pml4_phys = pmm.alloc_page()?;
        
        let pml4_virt = pml4_phys.to_virt();
        unsafe {
            core::ptr::write_bytes(pml4_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
        }
        
        // Copy kernel mappings from kernel PML4 (upper half)
        let kernel_pml4 = KERNEL_PML4.load(Ordering::Acquire);
        let kernel_pml4_virt = PhysAddr(kernel_pml4).to_virt();
        
        unsafe {
            let src = kernel_pml4_virt.0 as *const PageTableEntry;
            let dst = pml4_virt.0 as *mut PageTableEntry;
            
            for i in 256..512 {
                dst.write(src.add(i).read());
            }
        }
        
        let idx = self.find_free_user_slot();
        if idx < MAX_USER_PAGE_TABLES {
            unsafe {
                let tables_ptr = self.user_tables.as_ptr() as *mut UserPageTable;
                (*tables_ptr.add(idx)).pml4_phys = pml4_phys.as_u64();
                (*tables_ptr.add(idx)).in_use = true;
            }
            self.user_table_count.fetch_add(1, Ordering::Relaxed);
        }
        
        Some(pml4_phys.as_u64())
    }

    /// Map a page in a specific page table (for user space)
    pub fn map_page_in_table(&self, pml4: u64, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) {
        if pml4 == 0 {
            return;
        }
        
        let pml4_virt = PhysAddr(pml4).to_virt();
        
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            
            // Walk page table hierarchy
            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true);
            if pdpt.is_null() { return; }
            
            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true);
            if pd.is_null() { return; }
            
            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true);
            if pt.is_null() { return; }
            
            // Set up PTE
            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);
            
            self.flush_tlb(virt.0);
        }
    }

    /// Destroy a user page table and free all associated memory
    pub fn destroy_page_table(&self, pml4: u64) {
        if pml4 == 0 { return; }
        
        let pmm = get_pmm();
        let pml4_virt = PhysAddr(pml4).to_virt();
        
        unsafe {
            let pml4_ptr = pml4_virt.0 as *mut PageTableEntry;
            
            // Free all user-space page tables (entries 0-255)
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
        
        // Remove from tracking
        for i in 0..MAX_USER_PAGE_TABLES {
            if self.user_tables[i].pml4_phys == pml4 && self.user_tables[i].in_use {
                unsafe {
                    let tables_ptr = self.user_tables.as_ptr() as *mut UserPageTable;
                    (*tables_ptr.add(i)).in_use = false;
                    (*tables_ptr.add(i)).pml4_phys = 0;
                }
                self.user_table_count.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }
    }

    /// Get statistics
    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.total_maps.load(Ordering::Relaxed),
            self.total_unmaps.load(Ordering::Relaxed),
            self.page_faults.load(Ordering::Relaxed),
        )
    }

    // ==================== Private Methods ====================

    /// Internal page mapping implementation
    fn map_page_internal(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }
        
        let pml4_virt = PhysAddr(pml4_base).to_virt();
        
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            
            // Get or create PDPT
            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true);
            if pdpt.is_null() { return Err("Failed to allocate PDPT"); }
            
            // Get or create PD
            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true);
            if pd.is_null() { return Err("Failed to allocate PD"); }
            
            // Get or create PT
            let pt = self.get_or_create_table_entry(pd.add(virt.pd_idx()), true);
            if pt.is_null() { return Err("Failed to allocate PT"); }
            
            // Set up PTE
            let pte = &mut *pt.add(virt.pt_idx());
            pte.set_frame(phys);
            pte.set_flags(flags);
            
            self.flush_tlb(virt.0);
        }
        
        Ok(())
    }

    /// Map a 2MB huge page
    fn map_2mb_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }
        
        let pml4_virt = PhysAddr(pml4_base).to_virt();
        
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            
            // Get or create PDPT
            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true);
            if pdpt.is_null() { return Err("Failed to allocate PDPT"); }
            
            // Get or create PD
            let pd = self.get_or_create_table_entry(pdpt.add(virt.pdpt_idx()), true);
            if pd.is_null() { return Err("Failed to allocate PD"); }
            
            // Set up PD entry with PS bit
            let pde = &mut *pd.add(virt.pd_idx());
            pde.set_frame(phys);
            pde.set_flags(flags);
            
            self.flush_tlb(virt.0);
        }
        
        Ok(())
    }

    /// Map a 1GB huge page
    fn map_1gb_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), &'static str> {
        let pml4_base = KERNEL_PML4.load(Ordering::Acquire);
        if pml4_base == 0 {
            return Err("VMM not initialized");
        }
        
        let pml4_virt = PhysAddr(pml4_base).to_virt();
        
        unsafe {
            let pml4 = pml4_virt.0 as *mut PageTableEntry;
            
            // Get or create PDPT
            let pdpt = self.get_or_create_table_entry(pml4.add(virt.pml4_idx()), true);
            if pdpt.is_null() { return Err("Failed to allocate PDPT"); }
            
            // Set up PDPT entry with PS bit
            let pdpte = &mut *pdpt.add(virt.pdpt_idx());
            pdpte.set_frame(phys);
            pdpte.set_flags(flags);
            
            self.flush_tlb(virt.0);
        }
        
        Ok(())
    }

    /// Get or create a page table from an entry pointer
    ///
    /// # Arguments
    /// * `entry` - Pointer to a single PageTableEntry
    /// * `create` - If true, allocate new table if not present
    ///
    /// # Returns
    /// * Pointer to the next level page table (virtual address), or null on failure
    unsafe fn get_or_create_table_entry(&self, entry: *mut PageTableEntry, create: bool) -> *mut PageTableEntry {
        let e = &*entry;
        
        if e.is_present() {
            // Table already exists, return its virtual address
            e.frame().to_virt().0 as *mut PageTableEntry
        } else if create {
            // Need to create new table
            let pmm = get_pmm();
            
            if let Some(page) = pmm.alloc_page() {
                // Clear the new page table
                let page_virt = page.to_virt();
                core::ptr::write_bytes(page_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
                
                // Set up the entry pointing to the new table
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

    /// Find free slot in user page table tracking array
    fn find_free_user_slot(&self) -> usize {
        for i in 0..MAX_USER_PAGE_TABLES {
            if !self.user_tables[i].in_use {
                return i;
            }
        }
        MAX_USER_PAGE_TABLES
    }

    /// Acquire spinlock
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

    /// Release spinlock
    #[inline(always)]
    fn release_lock(&self) {
        VMM_LOCK.store(false, Ordering::Release);
    }

    /// Read CR3 register
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

    /// Write CR3 register
    #[inline(always)]
    unsafe fn write_cr3(&self, val: u64) {
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) val,
            options(nostack, preserves_flags)
        );
    }

    /// Invalidate TLB entry
    #[inline(always)]
    fn flush_tlb(&self, addr: u64) {
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) addr,
                options(nostack, preserves_flags)
            );
        }
    }
}

// Global VMM instance
static mut GLOBAL_VMM: VirtualMemoryManager = VirtualMemoryManager::new();

/// Get reference to global VMM instance
pub fn get_vmm() -> &'static VirtualMemoryManager {
    unsafe { &GLOBAL_VMM }
}

/// Get mutable reference to global VMM instance (for init operations)
///
/// # Safety
/// Should only be called during kernel initialization
pub unsafe fn get_vmm_mut() -> &'static mut VirtualMemoryManager {
    &mut GLOBAL_VMM
}

/// Get kernel PML4 physical address
pub fn get_kernel_pml4() -> u64 {
    KERNEL_PML4.load(Ordering::Acquire)
}
