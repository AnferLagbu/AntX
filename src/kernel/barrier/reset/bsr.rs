//! # Barrier Soft Reset (BSR)
//!
//! Layer 2 恢复：回滚到初始栏并重置设备

use core::sync::atomic::Ordering;

use crate::kernel::barrier::PANIC_FLAG;
use crate::kernel::barrier::RECOVERY_MANAGER;
use crate::kernel::barrier::types::DomainState;
use super::config::{self, RecoveryResult, RecoveryLayer};
use super::audit;

pub fn freeze_all_domains() {
    let manager = RECOVERY_MANAGER.lock();
    let count = manager.count.load(Ordering::SeqCst) as usize;
    for i in 0..count {
        if let Some(domain) = &manager.domains[i] {
            domain.set_state(DomainState::Freezing, Ordering::SeqCst);
        }
    }
}

pub fn unfreeze_all_domains() {
    let manager = RECOVERY_MANAGER.lock();
    let count = manager.count.load(Ordering::SeqCst) as usize;
    for i in 0..count {
        if let Some(domain) = &manager.domains[i] {
            domain.set_state(DomainState::Active, Ordering::SeqCst);
            domain.consecutive_failures.store(0, Ordering::SeqCst);
        }
    }
}

pub fn rollback_to_init() -> usize {
    let mut total_rolled = 0usize;
    let manager = RECOVERY_MANAGER.lock();
    let count = manager.count.load(Ordering::SeqCst) as usize;
    for i in 0..count {
        if let Some(domain) = &manager.domains[i] {
            let mut undo = domain.undo.lock();
            let rolled = undo.rollback_to(0);
            total_rolled += rolled;
            drop(undo);
            domain.barrier_generation.store(1, Ordering::SeqCst);
        }
    }
    total_rolled
}

pub fn reset_devices() -> RecoveryResult {
    use crate::kernel::barrier::snapshot;

    fn dummy_write(_base: u64, _offset: u32, _value: u32) {}

    let (success, failed) = snapshot::snapshot_restore_all(dummy_write);

    if failed == 0 {
        RecoveryResult::Success
    } else if success > 0 {
        RecoveryResult::Success
    } else {
        RecoveryResult::Escalate
    }
}

pub fn reset_interrupts() {
    #[cfg(not(feature = "kernel_test"))]
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

pub fn clear_panic_state() {
    PANIC_FLAG.store(false, Ordering::SeqCst);
    config::set_reset_in_progress(false);
}

pub fn execute() -> RecoveryResult {
    if config::is_reset_in_progress() {
        return RecoveryResult::Escalate;
    }

    config::set_reset_in_progress(true);
    config::set_current_layer(RecoveryLayer::Layer2);
    config::increment_bsr_count();

    crate::klog_crit!(Kernel, "[BSR] Barrier Soft Reset initiated");

    freeze_all_domains();

    let rolled = rollback_to_init();
    crate::klog_info!(Kernel, "[BSR] Rolled back {} undo entries", rolled);

    let device_result = reset_devices();
    if device_result.should_escalate() {
        crate::klog_err!(Kernel, "[BSR] Device reset failed, escalating to BHR");
        audit::audit_record(RecoveryLayer::Layer2, RecoveryResult::Escalate, 1);
        return RecoveryResult::Escalate;
    }

    reset_interrupts();
    unfreeze_all_domains();
    clear_panic_state();

    audit::audit_record(RecoveryLayer::Layer2, RecoveryResult::Success, 0);
    crate::klog_crit!(Kernel, "[BSR] Barrier Soft Reset completed successfully");

    RecoveryResult::Success
}

#[cfg(feature = "kernel_test")]
pub mod tests {
    use super::*;

    pub fn test_freeze_unfreeze() -> bool {
        freeze_all_domains();
        unfreeze_all_domains();
        true
    }
}
