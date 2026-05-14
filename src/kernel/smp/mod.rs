//! SMP (Symmetric Multiprocessing) Stub Module
//!
//! Provides stub implementations for SMP functionality.
//! When the `smp` feature is enabled, these will be replaced with
//! real implementations using IPI (Inter-Processor Interrupts).

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static SMP_ENABLED: AtomicBool = AtomicBool::new(false);
static CPU_COUNT: AtomicU32 = AtomicU32::new(1);

pub fn is_smp_enabled() -> bool {
    SMP_ENABLED.load(Ordering::Acquire)
}

pub fn get_cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Acquire)
}

pub fn get_current_cpu() -> u32 {
    0
}

pub fn send_tlb_invalidate_ipi(_addr: u64) {
}

pub fn send_broadcast_ipi(_vector: u8) {
}

pub fn init_smp() {
    SMP_ENABLED.store(false, Ordering::Release);
    CPU_COUNT.store(1, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn smp_init() {
    init_smp();
}

#[no_mangle]
pub extern "C" fn smp_is_enabled() -> bool {
    is_smp_enabled()
}

#[no_mangle]
pub extern "C" fn smp_get_cpu_count() -> u32 {
    get_cpu_count()
}
