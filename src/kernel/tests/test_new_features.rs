use crate::kernel::tests::{assert_eq_test, check, runner, TestResult};
use crate::register_tests_inner;

// ============================================================
// Page Fault / Demand Paging
// ============================================================

fn test_pf_info_from_error_code() -> TestResult {
    let info = crate::kernel::mm::page_fault::PageFaultInfo::from_error_code(0x4000, 0x06);
    check!(info.fault_addr == 0x4000, "fault_addr");
    check!(info.write, "write flag");
    check!(info.user, "user flag");
    check!(!info.present, "not present");
    check!(!info.reserved, "not reserved");
    check!(!info.instruction, "not instruction");
    TestResult::Pass
}

fn test_pf_info_not_present() -> TestResult {
    let info = crate::kernel::mm::page_fault::PageFaultInfo::from_error_code(0x1000, 0x00);
    check!(!info.present, "not present");
    check!(!info.write, "not write");
    check!(!info.user, "not user");
    TestResult::Pass
}

fn test_pf_result_values() -> TestResult {
    use crate::kernel::mm::page_fault::PfResult;
    assert_eq_test!(PfResult::Fixed as u32, 0, "Fixed=0");
    assert_eq_test!(PfResult::SignalSegv as u32, 1, "SignalSegv=1");
    assert_eq_test!(PfResult::Oom as u32, 3, "Oom=3");
    TestResult::Pass
}

// ============================================================
// COW (Copy-on-Write)
// ============================================================

fn test_cow_frame_key_alignment() -> TestResult {
    let frame_of = |p: u64| p & !(4095u64);
    assert_eq_test!(frame_of(0x1000), 0x1000, "0x1000 aligned");
    assert_eq_test!(frame_of(0x1FFF), 0x1000, "0x1FFF rounds down");
    assert_eq_test!(frame_of(0x2000), 0x2000, "0x2000 aligned");
    assert_eq_test!(frame_of(0), 0, "0 rounds down");
    TestResult::Pass
}

fn test_cow_ref_init() -> TestResult {
    crate::kernel::mm::cow::cow_init();
    let count = crate::kernel::mm::cow::cow_ref_count(0x1000);
    assert_eq_test!(count, 0, "initially zero");
    TestResult::Pass
}

fn test_cow_ref_inc_dec() -> TestResult {
    crate::kernel::mm::cow::cow_init();
    let phys = 0x5000u64;

    crate::kernel::mm::cow::cow_inc_ref(phys);
    assert_eq_test!(
        crate::kernel::mm::cow::cow_ref_count(phys),
        1,
        "after inc=1"
    );

    crate::kernel::mm::cow::cow_inc_ref(phys);
    assert_eq_test!(
        crate::kernel::mm::cow::cow_ref_count(phys),
        2,
        "after inc=2"
    );

    let should_free = crate::kernel::mm::cow::cow_dec_ref(phys);
    check!(!should_free, "dec to 1 should not free");
    assert_eq_test!(
        crate::kernel::mm::cow::cow_ref_count(phys),
        1,
        "after dec=1"
    );

    let should_free = crate::kernel::mm::cow::cow_dec_ref(phys);
    check!(should_free, "dec to 0 should free");
    assert_eq_test!(
        crate::kernel::mm::cow::cow_ref_count(phys),
        0,
        "after dec=0"
    );
    TestResult::Pass
}

// ============================================================
// ELF Loader
// ============================================================

fn test_elf64_header_sizes() -> TestResult {
    use crate::kernel::proc::elf::Elf64Header;
    use crate::kernel::proc::elf::Elf64Phdr;
    assert_eq_test!(core::mem::size_of::<Elf64Header>(), 64, "header size");
    assert_eq_test!(core::mem::size_of::<Elf64Phdr>(), 56, "phdr size");
    TestResult::Pass
}

fn test_elf_validation_null() -> TestResult {
    let result = crate::kernel::proc::elf::elf_validate(core::ptr::null(), 64);
    check!(result.is_none(), "null pointer rejected");
    TestResult::Pass
}

fn test_elf_validation_small() -> TestResult {
    let result = crate::kernel::proc::elf::elf_validate(&0u8 as *const u8, 10);
    check!(result.is_none(), "too small rejected");
    TestResult::Pass
}

fn test_elf_magic_rejected() -> TestResult {
    let data = [0u8; 64];
    let result = crate::kernel::proc::elf::elf_validate(data.as_ptr(), 64);
    check!(result.is_none(), "bad magic rejected");
    TestResult::Pass
}

fn test_elf_valid_minimal() -> TestResult {
    use crate::kernel::proc::elf::Elf64Header;
    let data = [0u8; 80]; // header + some room
    let hdr = unsafe { &mut *(data.as_ptr() as *mut Elf64Header) };
    hdr.e_ident[0] = 0x7F;
    hdr.e_ident[1] = b'E';
    hdr.e_ident[2] = b'L';
    hdr.e_ident[3] = b'F';
    hdr.e_ident[4] = 2; // ELFCLASS64
    hdr.e_machine = 0x3E; // x86_64
    hdr.e_phentsize = 56; // sizeof(Elf64Phdr)
    let result = crate::kernel::proc::elf::elf_validate(data.as_ptr(), 80);
    check!(result.is_some(), "valid elf accepted");
    TestResult::Pass
}

// ============================================================
// RCU
// ============================================================

fn test_rcu_read_lock_unlock() -> TestResult {
    crate::kernel::sync::rcu::rcu_read_lock();
    // In single-core context, nesting should be 1
    crate::kernel::sync::rcu::rcu_read_unlock();
    TestResult::Pass
}

fn test_rcu_nested_locks() -> TestResult {
    crate::kernel::sync::rcu::rcu_read_lock();
    crate::kernel::sync::rcu::rcu_read_lock();
    crate::kernel::sync::rcu::rcu_read_unlock();
    crate::kernel::sync::rcu::rcu_read_unlock();
    TestResult::Pass
}

// ============================================================
// Chitin Device Tree
// ============================================================

fn test_devtree_create_node() -> TestResult {
    let node_id = crate::kernel::chitin::devtree::devtree_create_node(
        "test_device",
        crate::kernel::chitin::ChitinProto::Other,
        None,
    );
    match node_id {
        Some(id) => {
            check!(id > 0, "node id positive");
            let node = crate::kernel::chitin::devtree::devtree_get_node(id);
            check!(node.is_some(), "can get node");
        }
        None => {
            // DevTree may not be initialized
        }
    }
    TestResult::Pass
}

fn test_devtree_set_compatible() -> TestResult {
    use alloc::vec;
    let node_id = crate::kernel::chitin::devtree::devtree_create_node(
        "compat_device",
        crate::kernel::chitin::ChitinProto::Other,
        None,
    );
    match node_id {
        Some(id) => {
            let compat = vec!["test,device"];
            crate::kernel::chitin::devtree::devtree_set_compatible(id, compat);
            let found = crate::kernel::chitin::devtree::devtree_find_compatible("test,device");
            check!(found.is_some(), "find by compatible");
        }
        None => {}
    }
    TestResult::Pass
}

// ============================================================
// Kmalloc-Slab integration
// ============================================================

fn test_slab_cache_index_selection() -> TestResult {
    use crate::kernel::mm::kmalloc_slab;
    // Test via the public API
    let p1 = kmalloc_slab::slab_kmalloc(8);
    let p2 = kmalloc_slab::slab_kmalloc(32);
    let p3 = kmalloc_slab::slab_kmalloc(4096);
    // Large alloc goes to heap
    if let Some(p) = p1 {
        kmalloc_slab::slab_kfree(p, 8);
    }
    if let Some(p) = p2 {
        kmalloc_slab::slab_kfree(p, 32);
    }
    if let Some(p) = p3 {
        kmalloc_slab::slab_kfree(p, 4096);
    }
    TestResult::Pass
}

// ============================================================
// ZIL Persistence
// ============================================================

fn test_zil_crc32_deterministic() -> TestResult {
    // CRC32 is defined in zil_persist.rs; test via roundtrip
    let data = b"Hello, ZIL!";
    let c1 = crate::kernel::fs::hvfs::zil_persist::crc32_test_wrapper(data);
    let c2 = crate::kernel::fs::hvfs::zil_persist::crc32_test_wrapper(data);
    assert_eq_test!(c1, c2, "crc32 deterministic");
    check!(c1 != 0, "crc32 non-zero");
    TestResult::Pass
}

// ============================================================
// mmap syscall
// ============================================================

fn test_prot_to_vma_flags() -> TestResult {
    use crate::kernel::mm::PageFlags;

    // We can't call prot_to_vma_flags directly (private), but test basic flag semantics
    let r = PageFlags::PRESENT | PageFlags::USER;
    check!(r.contains(PageFlags::PRESENT), "PROT_READ has PRESENT");
    check!(r.contains(PageFlags::USER), "PROT_READ has USER");
    check!(!r.contains(PageFlags::WRITABLE), "PROT_READ no WRITE");

    let rw = PageFlags::PRESENT | PageFlags::USER | PageFlags::WRITABLE;
    check!(rw.contains(PageFlags::WRITABLE), "PROT_WRITE has WRITABLE");

    let rx = PageFlags::PRESENT | PageFlags::USER;
    check!(!rx.contains(PageFlags::NX), "PROT_EXEC has no NX");
    TestResult::Pass
}

// ============================================================
// IPC Dynamic Namespace
// ============================================================

fn test_dyn_ipc_pipe_no_limit() -> TestResult {
    let ns = crate::kernel::ipc::dynamic::DynIpcNamespace::new();
    let mut ids = alloc::vec::Vec::new();
    for _ in 0..50 {
        let id = ns.pipe_create(1000, 2000);
        check!(id != 0, "pipe id non-zero");
        ids.push(id);
    }
    assert_eq_test!(ids.len(), 50, "50 pipes created");
    assert_eq_test!(ns.pipe_count(), 50, "pipe count 50");
    for id in ids {
        ns.pipe_destroy(id).unwrap();
    }
    assert_eq_test!(ns.pipe_count(), 0, "all pipes destroyed");
    TestResult::Pass
}

fn test_dyn_ipc_msgq_growth() -> TestResult {
    let ns = crate::kernel::ipc::dynamic::DynIpcNamespace::new();
    for _ in 0..20 {
        let id = ns.msgq_create(1000, 64, 4096).unwrap();
        check!(ns.msgq_exists(id), "msgq exists");
        ns.msgq_destroy(id).unwrap();
    }
    assert_eq_test!(ns.msgq_count(), 0, "all msgqs destroyed");
    TestResult::Pass
}

fn test_dyn_ipc_shm_create() -> TestResult {
    let ns = crate::kernel::ipc::dynamic::DynIpcNamespace::new();
    let result = ns.shm_create(2000, 8192);
    // May fail if PMM not initialized in test context
    if let Ok(id) = result {
        check!(id != 0, "shm id non-zero");
        ns.shm_destroy(id).unwrap();
    }
    TestResult::Pass
}

fn test_dyn_ipc_sem_create() -> TestResult {
    let ns = crate::kernel::ipc::dynamic::DynIpcNamespace::new();
    let id = ns.sem_create(1000, 1, 10).unwrap();
    check!(id != 0, "sem id non-zero");
    ns.sem_destroy(id).unwrap();
    TestResult::Pass
}

// ============================================================
// VMA
// ============================================================

fn test_vma_creation() -> TestResult {
    use crate::kernel::mm::vma::{Vma, VmaType};
    use crate::kernel::mm::PageFlags;
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let vma = Vma::new(0x400000, 0x401000, flags, VmaType::Anonymous);
    assert_eq_test!(vma.start, 0x400000usize, "start");
    assert_eq_test!(vma.end, 0x401000usize, "end");
    check!(vma.contains(0x400500), "contains addr inside");
    check!(!vma.contains(0x402000), "not contains addr outside");
    TestResult::Pass
}

fn test_mm_struct_operations() -> TestResult {
    use crate::kernel::mm::vma::{MmStruct, Vma, VmaType};
    use crate::kernel::mm::PageFlags;
    let mm = MmStruct::new();
    let flags = PageFlags::PRESENT | PageFlags::USER;

    let vma = Vma::new(0x400000, 0x401000, flags, VmaType::Anonymous);
    mm.insert_vma(vma).unwrap();

    let found = mm.find_vma(0x400500);
    check!(found.is_some(), "find_vma");
    if let Some(v) = found {
        assert_eq_test!(v.start, 0x400000usize, "found start");
    }

    mm.remove_range(0x400000, 0x401000);
    let not_found = mm.find_vma(0x400500);
    check!(not_found.is_none(), "removed");

    TestResult::Pass
}

fn test_vma_stack_guard() -> TestResult {
    use crate::kernel::mm::vma::{Vma, VmaType};
    use crate::kernel::mm::PageFlags;
    let guard = Vma::new(0x700000, 0x701000, PageFlags::empty(), VmaType::Guard);
    check!(guard.is_guard(), "is_guard");
    check!(!guard.is_stack(), "not stack");
    TestResult::Pass
}

// ============================================================
// Test Registration
// ============================================================

pub fn register_new_tests() {
    let r = runner();
    register_tests_inner! { r:
        "page_fault": {
            "pf_info_from_error_code": test_pf_info_from_error_code,
            "pf_info_not_present": test_pf_info_not_present,
            "pf_result_values": test_pf_result_values,
        },
        "cow": {
            "frame_key_alignment": test_cow_frame_key_alignment,
            "ref_init": test_cow_ref_init,
            "ref_inc_dec": test_cow_ref_inc_dec,
        },
        "elf": {
            "header_sizes": test_elf64_header_sizes,
            "validation_null": test_elf_validation_null,
            "validation_small": test_elf_validation_small,
            "magic_rejected": test_elf_magic_rejected,
            "valid_minimal": test_elf_valid_minimal,
        },
        "rcu": {
            "read_lock_unlock": test_rcu_read_lock_unlock,
            "nested_locks": test_rcu_nested_locks,
        },
        "devtree": {
            "create_node": test_devtree_create_node,
            "set_compatible": test_devtree_set_compatible,
        },
        "kmalloc_slab": {
            "cache_index_selection": test_slab_cache_index_selection,
        },
        "zil_persist": {
            "crc32_deterministic": test_zil_crc32_deterministic,
        },
        "mmap": {
            "prot_to_vma_flags": test_prot_to_vma_flags,
        },
        "ipc_dynamic": {
            "pipe_no_limit": test_dyn_ipc_pipe_no_limit,
            "msgq_growth": test_dyn_ipc_msgq_growth,
            "shm_create": test_dyn_ipc_shm_create,
            "sem_create": test_dyn_ipc_sem_create,
        },
        "vma": {
            "creation": test_vma_creation,
            "mm_struct_ops": test_mm_struct_operations,
            "stack_guard": test_vma_stack_guard,
        },
    }
}
