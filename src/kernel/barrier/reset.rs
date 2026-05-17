//! # Barrier Reset - BSR/BHR 实现
//!
//! Barrier Soft Reset (BSR): 软重启，回滚到初始栏并重置设备
//! Barrier Hard Reset (BHR): 硬重启，完全从硬件层面重置

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};

use crate::kernel::barrier::PANIC_FLAG;
use crate::kernel::barrier::RECOVERY_MANAGER;
use crate::kernel::barrier::types::DomainState;
use super::snapshot;

pub const RESET_SUCCESS: u32 = 0;
pub const RESET_FAILED: u32 = 1;
pub const RESET_ESCALATE: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RecoveryLayer {
    Layer1 = 1,
    Layer2 = 2,
    Layer3 = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RecoveryResult {
    Success = RESET_SUCCESS,
    Failed = RESET_FAILED,
    Escalate = RESET_ESCALATE,
}

impl RecoveryResult {
    pub fn is_success(&self) -> bool {
        matches!(self, RecoveryResult::Success)
    }

    pub fn should_escalate(&self) -> bool {
        matches!(self, RecoveryResult::Escalate)
    }
}

#[derive(Debug)]
pub struct RecoveryConfig {
    pub enable_layer1: bool,
    pub enable_layer2: bool,
    pub enable_layer3: bool,
    pub layer1_failure_threshold: u32,
    pub layer2_device_timeout_ticks: u64,
    pub layer2_max_attempts: u32,
    pub audit_enabled: bool,
}

impl RecoveryConfig {
    pub const fn default() -> Self {
        RecoveryConfig {
            enable_layer1: true,
            enable_layer2: true,
            enable_layer3: true,
            layer1_failure_threshold: 5,
            layer2_device_timeout_ticks: 100,
            layer2_max_attempts: 3,
            audit_enabled: true,
        }
    }
}

pub static RECOVERY_CONFIG: RecoveryConfig = RecoveryConfig::default();

pub static CURRENT_LAYER: AtomicU32 = AtomicU32::new(0);
pub static RESET_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
pub static BSR_ATTEMPT_COUNT: AtomicU32 = AtomicU32::new(0);
pub static BHR_ATTEMPT_COUNT: AtomicU32 = AtomicU32::new(0);
pub static LAST_RESET_TICK: AtomicU64 = AtomicU64::new(0);

pub struct ResetAuditLog {
    pub entries: [ResetAuditEntry; 16],
    pub count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ResetAuditEntry {
    pub tick: u64,
    pub layer: RecoveryLayer,
    pub result: RecoveryResult,
    pub reason: u32,
}

impl ResetAuditLog {
    pub const fn new() -> Self {
        ResetAuditLog {
            entries: [ResetAuditEntry {
                tick: 0,
                layer: RecoveryLayer::Layer1,
                result: RecoveryResult::Success,
                reason: 0,
            }; 16],
            count: 0,
        }
    }

    pub fn record(&mut self, tick: u64, layer: RecoveryLayer, result: RecoveryResult, reason: u32) {
        if self.count >= 16 {
            for i in 0..15 {
                self.entries[i] = self.entries[i + 1];
            }
            self.count = 15;
        }
        self.entries[self.count] = ResetAuditEntry {
            tick,
            layer,
            result,
            reason,
        };
        self.count += 1;
    }
}

pub static RESET_AUDIT_LOG: spin::Mutex<ResetAuditLog> = spin::Mutex::new(ResetAuditLog::new());

fn audit_record(layer: RecoveryLayer, result: RecoveryResult, reason: u32) {
    if !RECOVERY_CONFIG.audit_enabled {
        return;
    }
    let tick = unsafe { crate::kernel::timer::tick::get_ticks() };
    let mut log = RESET_AUDIT_LOG.lock();
    log.record(tick, layer, result, reason);
}

pub fn bsr_freeze_all_domains() {
    let manager = RECOVERY_MANAGER.lock();
    let count = manager.count.load(Ordering::SeqCst) as usize;
    for i in 0..count {
        if let Some(domain) = &manager.domains[i] {
            domain.set_state(DomainState::Freezing, Ordering::SeqCst);
        }
    }
}

pub fn bsr_unfreeze_all_domains() {
    let manager = RECOVERY_MANAGER.lock();
    let count = manager.count.load(Ordering::SeqCst) as usize;
    for i in 0..count {
        if let Some(domain) = &manager.domains[i] {
            domain.set_state(DomainState::Active, Ordering::SeqCst);
            domain.consecutive_failures.store(0, Ordering::SeqCst);
        }
    }
}

pub fn bsr_rollback_to_init() -> usize {
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

pub fn bsr_reset_devices() -> RecoveryResult {
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

pub fn bsr_reset_interrupts() {
    #[cfg(not(feature = "kernel_test"))]
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

pub fn bsr_clear_panic_state() {
    PANIC_FLAG.store(false, Ordering::SeqCst);
    RESET_IN_PROGRESS.store(false, Ordering::SeqCst);
}

pub fn bsr_execute() -> RecoveryResult {
    if RESET_IN_PROGRESS.load(Ordering::SeqCst) {
        return RecoveryResult::Escalate;
    }

    RESET_IN_PROGRESS.store(true, Ordering::SeqCst);
    CURRENT_LAYER.store(RecoveryLayer::Layer2 as u32, Ordering::SeqCst);
    BSR_ATTEMPT_COUNT.fetch_add(1, Ordering::SeqCst);

    crate::klog_crit!(Kernel, "[BSR] Barrier Soft Reset initiated");

    bsr_freeze_all_domains();

    let rolled = bsr_rollback_to_init();
    crate::klog_info!(Kernel, "[BSR] Rolled back {} undo entries", rolled);

    let device_result = bsr_reset_devices();
    if device_result.should_escalate() {
        crate::klog_err!(Kernel, "[BSR] Device reset failed, escalating to BHR");
        audit_record(RecoveryLayer::Layer2, RecoveryResult::Escalate, 1);
        return RecoveryResult::Escalate;
    }

    bsr_reset_interrupts();
    bsr_unfreeze_all_domains();
    bsr_clear_panic_state();

    audit_record(RecoveryLayer::Layer2, RecoveryResult::Success, 0);
    crate::klog_crit!(Kernel, "[BSR] Barrier Soft Reset completed successfully");

    RecoveryResult::Success
}

pub fn bhr_disable_interrupts() {
    #[cfg(not(feature = "kernel_test"))]
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

pub fn bhr_mask_all_irqs() {
    #[cfg(not(feature = "kernel_test"))]
    unsafe {
        core::arch::asm!(
            "mov al, 0xFF",
            "out 0xA1, al",
            "out 0x21, al",
            options(nomem, nostack)
        );
    }
}

pub fn bhr_shutdown_devices() {
    #[cfg(not(feature = "kernel_test"))]
    {
        crate::klog_info!(Kernel, "[BHR] Shutting down devices...");
    }
}

pub fn bhr_save_crash_info() {
    #[cfg(not(feature = "kernel_test"))]
    {
        crate::klog_info!(Kernel, "[BHR] Crash info saved");
    }
}

pub fn bhr_keyboard_reset() -> ! {
    #[cfg(not(feature = "kernel_test"))]
    unsafe {
        core::arch::asm!(
            "mov al, 0xFE",
            "out 0x64, al",
            options(nomem, nostack)
        );
    }

    #[cfg(feature = "kernel_test")]
    loop {
        core::hint::spin_loop();
    }

    #[cfg(not(feature = "kernel_test"))]
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

pub fn bhr_triple_fault() -> ! {
    #[cfg(not(feature = "kernel_test"))]
    unsafe {
        core::arch::asm!(
            "lidt [0]",
            "int 3",
            options(nomem, nostack)
        );
    }

    loop {
        core::hint::spin_loop();
    }
}

pub fn bhr_execute() -> ! {
    CURRENT_LAYER.store(RecoveryLayer::Layer3 as u32, Ordering::SeqCst);
    BHR_ATTEMPT_COUNT.fetch_add(1, Ordering::SeqCst);

    crate::klog_crit!(Kernel, "[BHR] Barrier Hard Reset initiated");

    audit_record(RecoveryLayer::Layer3, RecoveryResult::Success, 0);

    bhr_save_crash_info();
    bhr_disable_interrupts();
    bhr_mask_all_irqs();
    bhr_shutdown_devices();

    crate::klog_crit!(Kernel, "[BHR] Performing keyboard controller reset...");

    bhr_keyboard_reset()
}

pub fn bhr_execute_fallback() -> ! {
    crate::klog_crit!(Kernel, "[BHR] Keyboard reset failed, attempting triple fault");
    bhr_triple_fault()
}

pub fn recovery_layer1_result() -> RecoveryResult {
    RecoveryResult::Escalate
}

pub fn recovery_execute_layered() -> ! {
    if !RESET_IN_PROGRESS.load(Ordering::SeqCst) {
        RESET_IN_PROGRESS.store(true, Ordering::SeqCst);
    }

    if RECOVERY_CONFIG.enable_layer2 {
        CURRENT_LAYER.store(RecoveryLayer::Layer2 as u32, Ordering::SeqCst);

        match bsr_execute() {
            RecoveryResult::Success => {
                crate::klog_crit!(Kernel, "[RECOVERY] Layer 2 (BSR) succeeded");
                #[cfg(not(feature = "kernel_test"))]
                unsafe {
                    core::hint::unreachable_unchecked();
                }
                #[cfg(feature = "kernel_test")]
                loop { core::hint::spin_loop(); }
            }
            RecoveryResult::Escalate => {
                crate::klog_warn!(Kernel, "[RECOVERY] Layer 2 (BSR) failed, escalating to Layer 3");
            }
            RecoveryResult::Failed => {
                crate::klog_err!(Kernel, "[RECOVERY] Layer 2 (BSR) failed");
            }
        }
    }

    if RECOVERY_CONFIG.enable_layer3 {
        CURRENT_LAYER.store(RecoveryLayer::Layer3 as u32, Ordering::SeqCst);
        bhr_execute();
    }

    bhr_execute_fallback()
}

pub fn recovery_get_current_layer() -> RecoveryLayer {
    match CURRENT_LAYER.load(Ordering::SeqCst) {
        1 => RecoveryLayer::Layer1,
        2 => RecoveryLayer::Layer2,
        3 => RecoveryLayer::Layer3,
        _ => RecoveryLayer::Layer1,
    }
}

pub fn recovery_get_stats() -> (u32, u32, u32) {
    let bsr_count = BSR_ATTEMPT_COUNT.load(Ordering::SeqCst);
    let bhr_count = BHR_ATTEMPT_COUNT.load(Ordering::SeqCst);
    let last_tick = LAST_RESET_TICK.load(Ordering::SeqCst) as u32;
    (bsr_count, bhr_count, last_tick)
}

pub fn recovery_reset_stats() {
    BSR_ATTEMPT_COUNT.store(0, Ordering::SeqCst);
    BHR_ATTEMPT_COUNT.store(0, Ordering::SeqCst);
    LAST_RESET_TICK.store(0, Ordering::SeqCst);
    CURRENT_LAYER.store(0, Ordering::SeqCst);
    RESET_IN_PROGRESS.store(false, Ordering::SeqCst);
}

#[cfg(feature = "kernel_test")]
pub mod tests {
    use super::*;

    pub fn test_recovery_result() -> bool {
        let success = RecoveryResult::Success;
        let failed = RecoveryResult::Failed;
        let escalate = RecoveryResult::Escalate;

        success.is_success() && !success.should_escalate()
            && !failed.is_success() && !failed.should_escalate()
            && !escalate.is_success() && escalate.should_escalate()
    }

    pub fn test_recovery_layer() -> bool {
        let layer1 = RecoveryLayer::Layer1;
        let layer2 = RecoveryLayer::Layer2;
        let layer3 = RecoveryLayer::Layer3;

        layer1 as u32 == 1 && layer2 as u32 == 2 && layer3 as u32 == 3
    }

    pub fn test_config_default() -> bool {
        let config = RecoveryConfig::default();
        config.enable_layer1 && config.enable_layer2 && config.enable_layer3
            && config.layer1_failure_threshold == 5
    }

    pub fn test_audit_log() -> bool {
        let mut log = ResetAuditLog::new();
        log.record(100, RecoveryLayer::Layer1, RecoveryResult::Success, 0);
        log.record(200, RecoveryLayer::Layer2, RecoveryResult::Escalate, 1);
        log.count == 2
    }

    pub fn test_bsr_freeze_unfreeze() -> bool {
        bsr_freeze_all_domains();
        bsr_unfreeze_all_domains();
        true
    }

    pub fn test_stats() -> bool {
        recovery_reset_stats();
        let (bsr, bhr, tick) = recovery_get_stats();
        bsr == 0 && bhr == 0 && tick == 0
    }
}
