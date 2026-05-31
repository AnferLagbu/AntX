#![allow(dead_code)]
//! C FFI Interface for DMA Engine
//!
//! Matches the declarations in src/include/dma.h exactly.
//! All functions use `#[no_mangle]` and `extern "C"` for ABI compatibility.

use super::engine::dma as engine;
use crate::kernel::dma::DmaPoolStats;
use crate::kernel::mm::{PhysAddr, VirtAddr};
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

// ============================================================
// Internal helper: serial output for debug
// ============================================================

extern "C" {
    fn klog_ffi_info(msg: *const u8);
}

macro_rules! klog_dma_info {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

fn print(s: &str) {
    unsafe {
        klog_ffi_info(s.as_ptr());
    }
}

// ============================================================
// C-compatible mapping struct (matches dma.h exactly)
// ============================================================

#[repr(C)]
pub struct c_dma_mapping {
    pub cpu_addr: *mut c_void,
    pub dma_addr: u64,
    pub size: usize,
    pub direction: u32,
    pub cache: u32,
    pub is_coherent: i32,
    pub is_mapped: i32,
    pub next: *mut c_dma_mapping,
    pub prev: *mut c_dma_mapping,
    // Internal tracking data (not in C struct)
    _internal_id: u64,
}

static NEXT_MAPPING_ID: AtomicU64 = AtomicU64::new(0);

unsafe fn alloc_mapping_struct() -> *mut c_dma_mapping {
    extern "C" {
        fn kmalloc(size: u64) -> *mut c_void;
        fn kfree(ptr: *mut c_void);
    }

    let ptr = kmalloc(core::mem::size_of::<c_dma_mapping>() as u64);
    if ptr.is_null() {
        return ptr::null_mut();
    }
    ptr::write_bytes(ptr, 0, core::mem::size_of::<c_dma_mapping>());

    let mapping = ptr as *mut c_dma_mapping;
    let id = NEXT_MAPPING_ID.fetch_add(1, Ordering::Relaxed);
    (*mapping)._internal_id = id;
    mapping
}

// ============================================================
// Lifecycle
// ============================================================

#[no_mangle]
pub extern "C" fn dma_init() -> i32 {
    print("\n[DMA-RS] Initializing DMA engine...\n");
    engine().init();
    print("[DMA-RS] Engine initialized successfully\n");
    0
}

#[no_mangle]
pub extern "C" fn dma_shutdown() {
    engine().shutdown();
    print("[DMA-RS] Engine shutdown complete\n");
}

// ============================================================
// Coherent DMA Memory
// ============================================================

#[no_mangle]
pub extern "C" fn dma_alloc_coherent(size: usize, _align: usize) -> *mut c_void {
    match engine().alloc_coherent(size) {
        Some((virt, _phys)) => virt.0 as *mut c_void,
        None => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn dma_free_coherent(addr: *mut c_void, size: usize) {
    if addr.is_null() {
        return;
    }
    engine().free_coherent(VirtAddr(addr as u64), size);
}

#[no_mangle]
pub extern "C" fn dma_get_device_address(cpu_addr: *mut c_void) -> u64 {
    if cpu_addr.is_null() {
        return 0;
    }
    engine()
        .device_address(VirtAddr(cpu_addr as u64))
        .map(|p| p.0)
        .unwrap_or(0)
}

// ============================================================
// ioremap / iounmap
// ============================================================

#[no_mangle]
pub extern "C" fn ioremap(phys_addr: u64, size: usize) -> *mut c_void {
    match engine().ioremap(PhysAddr(phys_addr), size) {
        Some(virt) => virt.0 as *mut c_void,
        None => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn iounmap(virt_addr: *mut c_void, size: usize) {
    if virt_addr.is_null() {
        return;
    }
    engine().iounmap(VirtAddr(virt_addr as u64), size);
}

// ============================================================
// Streaming DMA Mappings
// ============================================================

#[no_mangle]
pub extern "C" fn dma_map_single(
    buffer: *mut c_void,
    size: usize,
    direction: u32,
) -> *mut c_dma_mapping {
    if buffer.is_null() || size == 0 {
        return ptr::null_mut();
    }

    let dma_dir = match direction {
        0 => super::DmaDirection::ToDevice,
        1 => super::DmaDirection::FromDevice,
        _ => super::DmaDirection::Bidirectional,
    };

    let cpu_addr = VirtAddr(buffer as u64);

    match engine().map_single(cpu_addr, size, dma_dir) {
        Some(_internal_mapping) => {
            let m = unsafe { alloc_mapping_struct() };
            if m.is_null() {
                return ptr::null_mut();
            }

            let dma_addr = engine().device_address(cpu_addr).map(|p| p.0).unwrap_or(0);

            unsafe {
                (*m).cpu_addr = buffer;
                (*m).dma_addr = dma_addr;
                (*m).size = size;
                (*m).direction = direction;
                (*m).cache = 1;
                (*m).is_coherent = 0;
                (*m).is_mapped = 1;
            }
            m
        }
        None => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn dma_unmap_single(mapping: *mut c_dma_mapping) {
    if mapping.is_null() {
        return;
    }

    unsafe {
        if (*mapping).is_mapped == 0 {
            return;
        }

        let cpu_addr = VirtAddr((*mapping).cpu_addr as u64);
        let dma_addr = PhysAddr((*mapping).dma_addr);
        let mapping_inner = super::DmaMapping {
            cpu_addr,
            dma_addr,
            size: (*mapping).size,
            direction: super::DmaDirection::Bidirectional,
            cache: super::DmaCachePolicy::Writeback,
            is_coherent: false,
            is_mapped: true,
        };

        engine().unmap_single(&mapping_inner);
        (*mapping).is_mapped = 0;

        extern "C" {
            fn kfree(ptr: *mut c_void);
        }
        kfree(mapping as *mut c_void);
    }
}

#[no_mangle]
pub extern "C" fn dma_map_sg(
    sglist: *mut super::DmaScatterList,
    direction: u32,
) -> *mut c_dma_mapping {
    if sglist.is_null() {
        return ptr::null_mut();
    }

    let sg = unsafe { &*sglist };
    if sg.entry_count == 0 {
        return ptr::null_mut();
    }

    let m = unsafe { alloc_mapping_struct() };
    if m.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*m).cpu_addr = sg.entries[0].page_addr as *mut c_void;
        (*m).dma_addr = sg.entries[0].phys_addr;
        (*m).size = sg.total_length;
        (*m).direction = direction;
        (*m).cache = 1;
        (*m).is_coherent = 0;
        (*m).is_mapped = 1;

        // Memory fence for all entries
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    m
}

#[no_mangle]
pub extern "C" fn dma_unmap_sg(mapping: *mut c_dma_mapping) {
    dma_unmap_single(mapping);
}

// ============================================================
// Cache Synchronization
// ============================================================

#[no_mangle]
pub extern "C" fn dma_sync_for_device(_mapping: *mut c_dma_mapping, _offset: usize, _size: usize) {
    // sfence + compiler barrier
    crate::arch!(fence_w());
    core::sync::atomic::fence(Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn dma_sync_for_cpu(_mapping: *mut c_dma_mapping, _offset: usize, _size: usize) {
    // lfence + compiler barrier
    crate::arch!(fence_r());
    core::sync::atomic::fence(Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn dma_sync_both(mapping: *mut c_dma_mapping, offset: usize, size: usize) {
    dma_sync_for_device(mapping, offset, size);
    dma_sync_for_cpu(mapping, offset, size);
}

// ============================================================
// DMA Transfer Control
// ============================================================

#[repr(C)]
pub struct c_dma_transfer {
    src_addr: u64,
    dst_addr: u64,
    length: usize,
    direction: u32,
    synchronous: i32,
    completed: i32,
    result: i32,
    callback: Option<extern "C" fn(*mut c_void, i32)>,
    private_data: *mut c_void,
}

#[no_mangle]
pub extern "C" fn dma_memcpy(dest: u64, source: u64, length: usize, _direction: u32) -> i32 {
    if length == 0 {
        return 0;
    }

    let src_virt = if source != 0 {
        (source + 0xFFFF800000000000u64) as *const u8
    } else {
        return -1;
    };
    let dst_virt = if dest != 0 {
        (dest + 0xFFFF800000000000u64) as *mut u8
    } else {
        return -1;
    };

    unsafe {
        ptr::copy_nonoverlapping(src_virt, dst_virt, length);
    }

    core::sync::atomic::fence(Ordering::SeqCst);
    0
}

#[no_mangle]
pub extern "C" fn dma_async_memcpy(transfer: *mut c_dma_transfer) -> i32 {
    if transfer.is_null() {
        return -1;
    }

    unsafe {
        (*transfer).completed = 0;
        (*transfer).result = 0;

        let result = dma_memcpy(
            (*transfer).dst_addr,
            (*transfer).src_addr,
            (*transfer).length,
            (*transfer).direction,
        );

        (*transfer).completed = 1;
        (*transfer).result = result;

        if let Some(cb) = (*transfer).callback {
            cb((*transfer).private_data, result);
        }
    }

    0
}

#[no_mangle]
pub extern "C" fn dma_wait_for_completion(transfer: *mut c_dma_transfer, timeout_ms: u32) -> i32 {
    if transfer.is_null() {
        return -1;
    }

    let start = crate::arch!(timestamp());

    loop {
        let completed = unsafe { (*transfer).completed != 0 };
        if completed {
            return unsafe { (*transfer).result };
        }

        if timeout_ms > 0 {
            let current = crate::arch!(timestamp());
            if current.wrapping_sub(start) > timeout_ms as u64 * 2400000u64 {
                return -1;
            }
        }

        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "C" fn dma_cancel_transfer(transfer: *mut c_dma_transfer) -> i32 {
    if transfer.is_null() {
        return -1;
    }

    unsafe {
        if (*transfer).completed != 0 {
            return -1;
        }
        (*transfer).completed = 1;
        (*transfer).result = -1;
    }
    0
}

#[no_mangle]
pub extern "C" fn dma_create_transfer(
    src: u64,
    dst: u64,
    length: usize,
    direction: u32,
    callback: Option<extern "C" fn(*mut c_void, i32)>,
    private_data: *mut c_void,
) -> *mut c_dma_transfer {
    if length == 0 {
        return ptr::null_mut();
    }

    extern "C" {
        fn kmalloc(size: u64) -> *mut c_void;
    }

    let ptr = unsafe { kmalloc(core::mem::size_of::<c_dma_transfer>() as u64) };
    if ptr.is_null() {
        return ptr::null_mut();
    }

    let t = ptr as *mut c_dma_transfer;
    unsafe {
        (*t).src_addr = src;
        (*t).dst_addr = dst;
        (*t).length = length;
        (*t).direction = direction;
        (*t).synchronous = if callback.is_none() { 1 } else { 0 };
        (*t).completed = 0;
        (*t).result = 0;
        (*t).callback = callback;
        (*t).private_data = private_data;
    }
    t
}

#[no_mangle]
pub extern "C" fn dma_destroy_transfer(transfer: *mut c_dma_transfer) {
    if transfer.is_null() {
        return;
    }

    unsafe {
        if (*transfer).completed == 0 {
            dma_cancel_transfer(transfer);
        }
    }

    extern "C" {
        fn kfree(ptr: *mut c_void);
    }
    unsafe {
        kfree(transfer as *mut c_void);
    }
}

// ============================================================
// Scatter-Gather Helpers
// ============================================================

#[no_mangle]
pub extern "C" fn dma_sg_init(sglist: *mut super::DmaScatterList) {
    if sglist.is_null() {
        return;
    }
    unsafe {
        (*sglist).entry_count = 0;
        (*sglist).total_length = 0;
    }
}

#[no_mangle]
pub extern "C" fn dma_sg_add_entry(
    sglist: *mut super::DmaScatterList,
    addr: *mut c_void,
    length: usize,
) -> i32 {
    if sglist.is_null() || addr.is_null() || length == 0 {
        return -1;
    }

    unsafe {
        if (*sglist).entry_count as usize >= super::DMA_MAX_SCATTER_ENTRIES {
            return -1;
        }

        let idx = (*sglist).entry_count as usize;

        let phys = crate::kernel::mm::vmm::get_vmm()
            .get_physical(VirtAddr(addr as u64))
            .map(|p| p.0)
            .unwrap_or(0);

        (*sglist).entries[idx].phys_addr = phys;
        (*sglist).entries[idx].length = length;
        (*sglist).entries[idx].page_addr = addr as usize;

        (*sglist).entry_count += 1;
        (*sglist).total_length += length;
    }
    0
}

#[no_mangle]
pub extern "C" fn dma_sg_total_length(sglist: *mut super::DmaScatterList) -> usize {
    if sglist.is_null() {
        return 0;
    }
    unsafe { (*sglist).total_length }
}

// ============================================================
// Statistics & Debugging
// ============================================================

#[no_mangle]
pub extern "C" fn dma_get_stats(stats_out: *mut DmaPoolStats) {
    if stats_out.is_null() {
        return;
    }
    unsafe {
        *stats_out = engine().get_stats();
    }
}

#[no_mangle]
pub extern "C" fn dma_dump_stats() {
    let s = engine().get_stats();
    klog_dma_info!("=== DMA Engine Statistics ===");
    klog_dma_info!("  Total Allocations: {}", s.total_allocations);
    klog_dma_info!("  Total Frees: {}", s.total_frees);
    klog_dma_info!("  Total Mappings: {}", s.total_mappings);
    klog_dma_info!("  Total Unmappings: {}", s.total_unmappings);
    klog_dma_info!("  Current Active: {}", s.current_in_use);
    klog_dma_info!("  Max Concurrent: {}", s.max_concurrent);
    klog_dma_info!("  Coherence Fails: {}", s.coherence_fails);
    klog_dma_info!(
        "  Total Bytes Allocated: {} KB",
        s.total_bytes_allocated / 1024
    );
    klog_dma_info!("  Current Bytes Used: {} KB", s.current_bytes_used / 1024);
}

#[no_mangle]
pub extern "C" fn dump_active_mappings() {
    let s = engine().get_stats();
    klog_dma_info!("--- Active DMA Mappings (Rust managed) ---");
    klog_dma_info!("  Total: {} mappings", s.current_in_use);
}

#[no_mangle]
pub extern "C" fn dma_reset_stats() {
    engine().reset_stats();
    print("[DMA-RS] Statistics reset\n");
}
