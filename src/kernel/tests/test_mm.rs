use crate::kernel::mm::{PhysAddr, VirtAddr, PageSize, PageFlags, PageTableEntry, MemoryInfo};
use crate::kernel::mm::{PAGE_SIZE, KERNEL_BASE, PAGE_PRESENT, PAGE_WRITABLE, PAGE_USER, PAGE_NX};
use crate::kernel::mm::{pml4_index, pdpt_index, pd_index, pt_index, phys_to_virt, virt_to_phys};
use crate::kernel::tests::runner;
use crate::check;

fn test_phys_addr() -> Result<(), &'static str> {
    let pa = PhysAddr::new(0x1000);
    check!(pa.as_u64() == 0x1000, "PhysAddr as_u64 mismatch");

    let aligned = pa.align_up(0x200000);
    check!(aligned.as_u64() == 0x200000, "align_up 2M mismatch");

    let down = pa.align_down(0x200000);
    check!(down.as_u64() == 0, "align_down 2M mismatch");

    let va = pa.to_virt();
    check!(va.as_u64() == KERNEL_BASE + 0x1000, "phys_to_virt mismatch");
    Ok(())
}

fn test_virt_addr() -> Result<(), &'static str> {
    let va = VirtAddr::new(KERNEL_BASE + 0x2000);
    let pa = va.to_phys();
    check!(pa.as_u64() == 0x2000, "virt_to_phys mismatch");

    let idx = va.pml4_idx();
    check!(idx < 512, "pml4_idx should be < 512");

    let idx2 = va.pdpt_idx();
    check!(idx2 < 512, "pdpt_idx should be < 512");
    Ok(())
}

fn test_page_size() -> Result<(), &'static str> {
    let s4k = PageSize::Size4K;
    check!(s4k.size() == 4096, "4K size mismatch");
    check!(s4k.shift() == 12, "4K shift mismatch");
    check!(s4k.is_aligned(0x1000), "0x1000 should be 4K aligned");
    check!(!s4k.is_aligned(0x100), "0x100 should not be 4K aligned");

    let s2m = PageSize::Size2M;
    check!(s2m.size() == 2 * 1024 * 1024, "2M size mismatch");
    check!(s2m.is_aligned(0x200000), "0x200000 should be 2M aligned");
    Ok(())
}

fn test_page_table_entry() -> Result<(), &'static str> {
    let mut pte = PageTableEntry::new();
    check!(!pte.is_present(), "new PTE should not be present");

    pte.set_present(true);
    check!(pte.is_present(), "should be present after set");

    pte.set_writable(true);
    check!(pte.is_writable(), "should be writable after set");

    pte.set_frame(PhysAddr::new(0x1000));
    check!(pte.frame().as_u64() == 0x1000, "frame mismatch");

    let flags = pte.flags();
    check!(flags.contains(PageFlags::PRESENT), "flags should have PRESENT");
    check!(flags.contains(PageFlags::WRITABLE), "flags should have WRITABLE");
    Ok(())
}

fn test_page_flags() -> Result<(), &'static str> {
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    check!(flags.contains(PageFlags::PRESENT), "should have PRESENT");
    check!(flags.contains(PageFlags::WRITABLE), "should have WRITABLE");
    check!(flags.contains(PageFlags::USER), "should have USER");
    check!(!flags.contains(PageFlags::NX), "should not have NX");

    let no_user = flags & !PageFlags::USER;
    check!(!no_user.contains(PageFlags::USER), "should not have USER after removal");
    Ok(())
}

fn test_memory_info() -> Result<(), &'static str> {
    let info = MemoryInfo::const_default();
    check!(info.total_pages == 0, "default total should be 0");
    check!(info.free_pages == 0, "default free should be 0");
    check!(info.used_pages == 0, "default used should be 0");
    Ok(())
}

fn test_address_translation() -> Result<(), &'static str> {
    let phys: u64 = 0x1234000;
    let virt = phys_to_virt(phys);
    check!(virt == KERNEL_BASE + phys, "phys_to_virt mismatch");

    let back = virt_to_phys(virt);
    check!(back == phys, "virt_to_phys roundtrip mismatch");
    Ok(())
}

fn test_page_index_helpers() -> Result<(), &'static str> {
    let addr: u64 = 0xFFFF800000000000 | (1u64 << 39) | (2u64 << 30) | (3u64 << 21) | (4u64 << 12);
    let pml4 = pml4_index(addr);
    let pdpt = pdpt_index(addr);
    let pd = pd_index(addr);
    let pt = pt_index(addr);
    check!(pml4 < 512, "pml4 index out of range");
    check!(pdpt < 512, "pdpt index out of range");
    check!(pd < 512, "pd index out of range");
    check!(pt < 512, "pt index out of range");
    Ok(())
}

pub fn register_mm_tests() {
    let r = runner();
    r.register("mm::phys_addr", "basic", test_phys_addr);
    r.register("mm::virt_addr", "basic", test_virt_addr);
    r.register("mm::page_size", "basic", test_page_size);
    r.register("mm::pte", "basic", test_page_table_entry);
    r.register("mm::page_flags", "basic", test_page_flags);
    r.register("mm::memory_info", "default", test_memory_info);
    r.register("mm::addr", "translation", test_address_translation);
    r.register("mm::addr", "page_index", test_page_index_helpers);
}
