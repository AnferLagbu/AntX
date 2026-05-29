//! AArch64 Virtual Memory Manager
//!
//! Implements the same FFI interface as x86_64 vmm.rs, providing:
//! - Kernel high-half page table (TTBR1_EL1) management
//! - User space page table (TTBR0_EL1) creation/mapping
//! - Page table walk/clone/destroy
//!
//! Architecture: ARMv8-A 4KB granule, 48-bit VA
//! - TTBR0_EL1: User space  (0x0000_0000_0000_0000 .. 0x0000_FFFF_FFFF_FFFF)
//! - TTBR1_EL1: Kernel space (0xFFFF_0000_0000_0000 .. 0xFFFF_FFFF_FFFF_FFFF)
//!
//! Page table levels: L0 (512GB) → L1 (1GB) → L2 (2MB) → L3 (4KB)

use super::*;
use core::ffi::c_void;
use core::ptr;

// FFI imports from PMM
extern "C" {
    fn pmm_alloc_page() -> *mut c_void;
    fn pmm_free_page(addr: *mut c_void);
}

fn phys_to_virt(phys: u64) -> u64 {
    phys + super::KERNEL_BASE
}

// ─── ARM Descriptor Constants ────────────────────────────────────────

/// Descriptor types
const DESC_VALID: u64 = 1 << 0;
const DESC_TYPE_TABLE: u64 = 0b11;      // Table descriptor (L0/L1/L2)
const DESC_TYPE_BLOCK: u64 = 0b01;      // Block descriptor (L1/L2)
const DESC_TYPE_PAGE: u64 = 0b11;       // Page descriptor (L3, same bits as TABLE)

/// Memory attribute indices (matching MAIR_EL1 setup in mmu.rs)
const MAIR_DEVICE_nGnRnE: u64 = 0;      // Device memory
const MAIR_NORMAL_WBWA: u64 = 1;        // Normal cacheable (kernel)
const MAIR_NORMAL_NC: u64 = 2;          // Normal non-cacheable
const MAIR_USER_NORMAL: u64 = 4;        // Normal WBWA for user pages

/// Access Permission bits [7:6] in descriptor
const AP_EL1_RW: u64 = 0 << 6;          // EL1 read/write, EL0 no access
const AP_BOTH_RW: u64 = 1 << 6;         // EL1 read/write, EL0 read/write
const AP_EL1_RO: u64 = 2 << 6;          // EL1 read-only, EL0 no access
const AP_BOTH_RO: u64 = 3 << 6;         // EL1 read-only, EL0 read-only

/// Attribute index shift (bits [4:2])
const ATTR_SHIFT: u64 = 2;

/// Access flag (bit 10)
const AF: u64 = 1 << 10;

/// Execute never bits
const UXN: u64 = 1 << 54; // Unprivileged Execute Never (EL0)
const PXN: u64 = 1 << 53; // Privileged Execute Never (EL1)

/// Stage 1 shareability (bit 8 for inner, bit 9 for outer) — not strictly needed for stage 1

/// Page table entry count per level
const TABLE_ENTRIES: usize = 512;

// ─── Address Extraction Macros ───────────────────────────────────────

#[inline(always)]
fn l0_index(vaddr: u64) -> usize {
    ((vaddr >> 39) & 0x1FF) as usize
}

#[inline(always)]
fn l1_index(vaddr: u64) -> usize {
    ((vaddr >> 30) & 0x1FF) as usize
}

#[inline(always)]
fn l2_index(vaddr: u64) -> usize {
    ((vaddr >> 21) & 0x1FF) as usize
}

#[inline(always)]
fn l3_index(vaddr: u64) -> usize {
    ((vaddr >> 12) & 0x1FF) as usize
}

#[inline(always)]
fn is_kernel_addr(vaddr: u64) -> bool {
    vaddr >= 0xFFFF_0000_0000_0000
}

// ─── Page Flag Translation (x86 → ARM) ──────────────────────────────

/// Translate x86-style page flags to an ARM L3 page descriptor (4KB page)
fn page_flags_to_descriptor(flags: u64, paddr: u64) -> u64 {
    let mut desc = paddr & 0x0000_FFFF_FFFF_F000; // Output address [47:12]
    desc |= DESC_TYPE_PAGE; // bits [1:0] = 0b11
    desc |= AF;             // Access flag

    // Access permission
    let user = (flags & PAGE_USER) != 0;
    let writable = (flags & PAGE_WRITABLE) != 0;

    if user && writable {
        desc |= AP_BOTH_RW;
    } else if user && !writable {
        desc |= AP_BOTH_RO;
    } else if !user && writable {
        desc |= AP_EL1_RW;
    } else {
        desc |= AP_EL1_RO;
    }

    // Memory type
    if user {
        desc |= MAIR_USER_NORMAL << ATTR_SHIFT;
    } else {
        desc |= MAIR_NORMAL_WBWA << ATTR_SHIFT;
    }

    // Execute never
    let nx = (flags & PAGE_NX) != 0;
    if nx {
        desc |= UXN;
        if !user {
            desc |= PXN;
        }
    }

    desc
}

/// Translate x86 flags to an ARM L1/L2 block descriptor (1GB/2MB block)
fn block_flags_to_descriptor(flags: u64, paddr: u64, _level: u8, output_mask: u64) -> u64 {
    let mut desc = paddr & output_mask;
    desc |= DESC_TYPE_BLOCK;
    desc |= AF;

    let user = (flags & PAGE_USER) != 0;
    let writable = (flags & PAGE_WRITABLE) != 0;

    if user && writable {
        desc |= AP_BOTH_RW;
    } else if user && !writable {
        desc |= AP_BOTH_RO;
    } else if !user && writable {
        desc |= AP_EL1_RW;
    } else {
        desc |= AP_EL1_RO;
    }

    // Kernel blocks use MAIR index 1 (WBWA), device blocks use 0
    if user {
        desc |= MAIR_USER_NORMAL << ATTR_SHIFT;
    } else {
        desc |= MAIR_NORMAL_WBWA << ATTR_SHIFT;
    }

    let nx = (flags & PAGE_NX) != 0;
    if nx {
        desc |= UXN;
        if !user {
            desc |= PXN;
        }
    }

    desc
}

/// Create a table descriptor pointing to next-level table
fn table_descriptor(next_table_paddr: u64) -> u64 {
    (next_table_paddr & 0x0000_FFFF_FFFF_F000) | DESC_TYPE_TABLE
}

// ─── AArch64 Virtual Memory Manager ──────────────────────────────────

pub struct Aarch64Vmm {
    /// Physical address of kernel L0 table (for TTBR1_EL1)
    kernel_l0: u64,
    /// User page table counter
    next_table_id: core::sync::atomic::AtomicU64,
}

impl Aarch64Vmm {
    pub fn new() -> Self {
        Self {
            kernel_l0: 0,
            next_table_id: core::sync::atomic::AtomicU64::new(0),
        }
    }

    // ─── Initialization ──────────────────────────────────────────────

    /// Initialize kernel high-half page tables (TTBR1_EL1).
    /// Does NOT replace the existing identity mapping set up by mmu.rs.
    pub fn init(&self) {
        // The kernel MMU identity mapping is already set up by mmu::init().
        // We keep it for low-level access (MMIO, etc.) and set up a proper
        // high-half kernel mapping in TTBR1_EL1 for general use.

        // Read current TTBR0_EL1 (which points to our L0 table from mmu.rs)
        let current_l0: u64;
        unsafe {
            core::arch::asm!("mrs {}, ttbr0_el1", out(reg) current_l0);
        }

        // Store the kernel L0 address
        // We reuse the existing page tables for now.
        // In a more complete implementation, we'd create separate kernel tables.
        let kernel_l0_ptr = &raw const self.kernel_l0 as *mut u64;
        unsafe { ptr::write_volatile(kernel_l0_ptr, current_l0); }

        // Ensure TTBR1_EL1 points to the same tables (for high-half access)
        unsafe {
            core::arch::asm!(
                "msr ttbr1_el1, {}",
                "isb",
                in(reg) current_l0,
            );
        }
    }

    // ─── Allocate a Page Table ───────────────────────────────────────

    fn alloc_table(&self) -> Option<u64> {
        let page = unsafe { pmm_alloc_page() };
        if page.is_null() {
            return None;
        }
        let paddr = page as u64;
        // Zero the table
        unsafe {
            ptr::write_bytes(page as *mut u8, 0, PAGE_SIZE as usize);
        }
        Some(paddr)
    }

    fn free_table(&self, paddr: u64) {
        if paddr != 0 {
            unsafe { pmm_free_page(paddr as *mut c_void); }
        }
    }

    // ─── Kernel Page Map ─────────────────────────────────────────────

    pub fn map_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<(), ()> {
        // On aarch64, TTBR0 (user) and TTBR1 (kernel) use separate page tables.
        // User-space addresses (bit 47 = 0) are accessed via TTBR0_EL1 and are
        // never used from kernel mode. The kernel accesses user memory via
        // identity-mapped physical addresses instead.
        //
        // Attempting to map user-space VAs into the kernel page table is not
        // only unnecessary but DANGEROUS: the kernel L0 table (shared with the
        // identity mapping) contains BLOCK descriptors in L1_IDMAP. Walking into
        // them with ensure_next_level() overwrites the block descriptor without
        // ARM's required Break-Before-Make (BBM) sequence, corrupting the page
        // walk cache for adjacent entries in the same cache line.
        //
        // Skip user-space VA mappings entirely.
        if virt.as_u64() >> 48 == 0 {
            return Ok(());
        }
        self.map_page_in_table(self.kernel_l0, virt, phys, flags);
        Ok(())
    }

    pub fn map_huge_page(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags, size_type: PageSize) -> Result<(), ()> {
        // Skip user-space VA mappings (same rationale as map_page above)
        if virt.as_u64() >> 48 == 0 {
            return Ok(());
        }
        match size_type {
            PageSize::Size4K => self.map_page(virt, phys, flags),
            PageSize::Size2M => {
                // Use L2 block descriptor for 2MB huge page
                let vaddr = virt.as_u64();
                let paddr = phys.as_u64();
                let raw_flags = flags.bits();

                let l0 = phys_to_virt(self.kernel_l0) as *mut u64;
                let l0_idx = l0_index(vaddr);
                let l1 = self.ensure_next_level(l0, l0_idx);
                let l1_idx = l1_index(vaddr);
                let l2 = self.ensure_next_level(l1, l1_idx);
                let l2_idx = l2_index(vaddr);

                let desc = block_flags_to_descriptor(raw_flags, paddr, 2, 0x0000_FFFF_FFE0_0000);
                unsafe { ptr::write_volatile(l2.add(l2_idx), desc); }
                Ok(())
            }
            PageSize::Size1G => {
                // Use L1 block descriptor for 1GB huge page
                let vaddr = virt.as_u64();
                let paddr = phys.as_u64();
                let raw_flags = flags.bits();

                let l0 = phys_to_virt(self.kernel_l0) as *mut u64;
                let l0_idx = l0_index(vaddr);
                let l1 = self.ensure_next_level(l0, l0_idx);
                let l1_idx = l1_index(vaddr);

                let desc = block_flags_to_descriptor(raw_flags, paddr, 1, 0x0000_FFFF_C000_0000);
                unsafe { ptr::write_volatile(l1.add(l1_idx), desc); }
                Ok(())
            }
        }
    }

    pub fn unmap_page(&self, _virt: VirtAddr) {
        // TODO: Implement TLB-aware unmap
    }

    pub fn split_2mb_page(&self, _virt: u64) -> Result<(), ()> {
        // On aarch64, L2 blocks (2MB) are the default for block mappings.
        // We don't need to split them — we can always allocate L3 tables
        // and use 4KB pages directly.
        // This function exists for x86 compatibility.
        Ok(())
    }

    // ─── Page Table Walk / Map ───────────────────────────────────────

    /// Walk the page table starting at `root_paddr`, creating intermediate
    /// levels as needed, and set the final page descriptor.
    pub fn map_page_in_table(
        &self,
        root_paddr: u64,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) {
        let vaddr = virt.as_u64();
        let paddr = phys.as_u64();
        let raw_flags = flags.bits();

        // SAFETY: phys_to_virt converts root_paddr (page table PA) to kernel VA
        let l0 = phys_to_virt(root_paddr) as *mut u64;
        let l0_idx = l0_index(vaddr);

        let l1 = self.ensure_next_level(l0, l0_idx);
        let l1_idx = l1_index(vaddr);

        let l2 = self.ensure_next_level(l1, l1_idx);
        let l2_idx = l2_index(vaddr);

        let l3 = self.ensure_next_level(l2, l2_idx);
        let l3_idx = l3_index(vaddr);

        // Set the L3 page descriptor
        let desc = page_flags_to_descriptor(raw_flags, paddr);
        unsafe {
            ptr::write_volatile(l3.add(l3_idx), desc);
        }

        // TLB invalidate
        unsafe {
            core::arch::asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) vaddr);
        }
    }

    /// Ensure the next-level page table exists at `table[idx]`.
    /// Returns a pointer to the next-level table.
    fn ensure_next_level(&self, table: *mut u64, idx: usize) -> *mut u64 {
        unsafe {
            let entry = ptr::read_volatile(table.add(idx));
            if entry & 0b11 == 0b11 {
                // Already a table descriptor
                let paddr = entry & 0x0000_FFFF_FFFF_F000;
                // SAFETY: phys_to_virt(paddr) yields valid kernel VA because
                // KERNEL_BASE (=0 on aarch64 identity map) maps all physical memory.
                phys_to_virt(paddr) as *mut u64
            } else {
                // Allocate new table
                let new_paddr = self.alloc_table().expect("[VMM] Out of physical memory for page table");
                let desc = table_descriptor(new_paddr);
                ptr::write_volatile(table.add(idx), desc);
                core::arch::asm!("dsb ishst");
                // SAFETY: new_paddr from PMM allocator; phys_to_virt yields kernel VA.
                phys_to_virt(new_paddr) as *mut u64
            }
        }
    }

    // ─── User Page Table Operations ──────────────────────────────────

    pub fn create_user_page_table(&self) -> Option<u64> {
        // Allocate a clean L0 table for user space (TTBR0_EL1)
        let user_l0 = self.alloc_table()?;

        // Copy kernel identity-mapped TTBR0 entries into the user page table.
        // The kernel on aarch64 uses identity mapping (VA==PA), so all kernel
        // code and devices are accessed via TTBR0_EL1. After switching TTBR0_EL1
        // to the user page table, kernel code must remain reachable —
        // otherwise the code following the TTBR0 switch cannot execute.
        //
        // Note: shared L1/L2 tables are safe because user mappings only
        // overlay unused address ranges (L2_DEVICE[0] → new L3 table).
        let kernel_l0 = phys_to_virt(self.kernel_l0) as *const u64;
        let user_l0_ptr = phys_to_virt(user_l0) as *mut u64;
        unsafe {
            for i in 0..TABLE_ENTRIES {
                let entry = ptr::read_volatile(kernel_l0.add(i));
                ptr::write_volatile(user_l0_ptr.add(i), entry);
            }
        }

        Some(user_l0)
    }

    pub fn ensure_pml4_user(&self, _virt: u64) {
        // On aarch64, kernel and user tables are separate (TTBR0 vs TTBR1).
        // We don't need a USER bit on kernel entries — user accesses go
        // through TTBR0, kernel through TTBR1.
        // This is a no-op for aarch64.
    }

    pub fn ensure_path_user(&self, virt: u64) {
        // On aarch64, this is only relevant if the path is in the user
        // page table. Since user tables are in TTBR0 and naturally
        // user-accessible, we just need to ensure all intermediate table
        // descriptors are present (which map_page_in_table already does).
        if is_kernel_addr(virt) {
            // No need for USER flag on kernel pages
            return;
        }
        // For user-space addresses in user tables, ensure entries are valid
        // (already done by map_page_in_table)
    }

    pub fn switch_page_table(&self, ttbr0: u64) {
        unsafe {
            core::arch::asm!(
                "dsb ish",
                "msr ttbr0_el1, {}",
                "isb",
                "tlbi vmalle1is",
                "dsb ish",
                "isb",
                in(reg) ttbr0,
            );
        }
    }

    pub fn get_physical(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.get_physical_in_pml4(self.kernel_l0, virt)
    }

    pub fn get_physical_in_pml4(&self, root_paddr: u64, virt: VirtAddr) -> Option<PhysAddr> {
        let vaddr = virt.as_u64();

        let l0 = root_paddr as *const u64;
        let l0_idx = l0_index(vaddr);
        let l0_entry = unsafe { ptr::read_volatile(l0.add(l0_idx)) };
        if l0_entry & 0b11 != 0b11 {
            return None;
        }

        // SAFETY: table descriptor frame bits contain valid PA → phys_to_virt → kernel VA
        let l1 = phys_to_virt(l0_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l1_idx = l1_index(vaddr);
        let l1_entry = unsafe { ptr::read_volatile(l1.add(l1_idx)) };
        if l1_entry & 0b11 == 0b01 {
            // L1 block (1GB)
            return Some(PhysAddr((l1_entry & 0x0000_FFFF_C000_0000) | (vaddr & 0x3FFF_FFFF)));
        }
        if l1_entry & 0b11 != 0b11 {
            return None;
        }

        // SAFETY: L1 table descriptor frame → phys_to_virt → kernel VA
        let l2 = phys_to_virt(l1_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l2_idx = l2_index(vaddr);
        let l2_entry = unsafe { ptr::read_volatile(l2.add(l2_idx)) };
        if l2_entry & 0b11 == 0b01 {
            // L2 block (2MB)
            return Some(PhysAddr((l2_entry & 0x0000_FFFF_FFE0_0000) | (vaddr & 0x1F_FFFF)));
        }
        if l2_entry & 0b11 != 0b11 {
            return None;
        }

        // SAFETY: L2 table descriptor frame → phys_to_virt → kernel VA
        let l3 = phys_to_virt(l2_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l3_idx = l3_index(vaddr);
        let l3_entry = unsafe { ptr::read_volatile(l3.add(l3_idx)) };
        if l3_entry & 0b11 != 0b11 {
            return None;
        }

        Some(PhysAddr((l3_entry & 0x0000_FFFF_FFFF_F000) | (vaddr & 0xFFF)))
    }

    // ─── Clone / Destroy User Page Table ────────────────────────────

    pub fn clone_user_page_table(&self, parent_paddr: u64) -> Option<u64> {
        let child_paddr = self.alloc_table()?;

        // SAFETY: phys_to_virt converts page table physical addresses to kernel VAs
        let parent = phys_to_virt(parent_paddr) as *const u64;
        let child = phys_to_virt(child_paddr) as *mut u64;

        for i in 0..256 {
            unsafe {
                let entry = ptr::read_volatile(parent.add(i));
                ptr::write_volatile(child.add(i), entry);
            }
        }

        Some(child_paddr)
    }

    pub fn destroy_page_table(&self, root_paddr: u64) {
        if root_paddr == 0 {
            return;
        }

        // SAFETY: phys_to_virt converts root_paddr to kernel VA
        let l0 = phys_to_virt(root_paddr) as *mut u64;

        for i in 0..256 {
            unsafe {
                let entry = ptr::read_volatile(l0.add(i));
                if entry & 0b11 == 0b11 {
                    let l1_paddr = entry & 0x0000_FFFF_FFFF_F000;
                    self.destroy_l1_table(l1_paddr);
                }
            }
        }

        self.free_table(root_paddr);
    }

    fn destroy_l1_table(&self, paddr: u64) {
        let l1 = phys_to_virt(paddr) as *mut u64;
        for i in 0..512 {
            unsafe {
                let entry = ptr::read_volatile(l1.add(i));
                if entry & 0b11 == 0b11 {
                    let l2_paddr = entry & 0x0000_FFFF_FFFF_F000;
                    self.destroy_l2_table(l2_paddr);
                }
            }
        }
        self.free_table(paddr);
    }

    fn destroy_l2_table(&self, paddr: u64) {
        let l2 = phys_to_virt(paddr) as *mut u64;
        for i in 0..512 {
            unsafe {
                let entry = ptr::read_volatile(l2.add(i));
                if entry & 0b11 == 0b11 {
                    let l3_paddr = entry & 0x0000_FFFF_FFFF_F000;
                    self.free_table(l3_paddr);
                }
            }
        }
        self.free_table(paddr);
    }
}

// ─── Global VMM Instance ─────────────────────────────────────────────

static GLOBAL_VMM: spin::Once<Aarch64Vmm> = spin::Once::new();

pub fn vmm_init() {
    GLOBAL_VMM.call_once(|| {
        let vmm = Aarch64Vmm::new();
        vmm.init();
        vmm
    });
}

pub fn get_vmm() -> &'static Aarch64Vmm {
    GLOBAL_VMM.get().expect("[VMM] aarch64 VMM accessed before initialization")
}

pub fn get_kernel_pml4() -> u64 {
    get_vmm().kernel_l0
}

pub fn get_current_pml4() -> u64 {
    let ttbr0: u64;
    unsafe { core::arch::asm!("mrs {}, TTBR0_EL1", out(reg) ttbr0); }
    if ttbr0 != 0 {
        ttbr0
    } else {
        get_vmm().kernel_l0
    }
}