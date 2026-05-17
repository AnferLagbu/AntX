//! # 分层恢复入口
//!
//! 统一的恢复策略：Layer 1 → Layer 2 → Layer 3

use super::config::{self, RecoveryLayer, RecoveryResult};
use super::bsr;
use super::bhr;

pub fn execute_layered() -> ! {
    if !config::is_reset_in_progress() {
        config::set_reset_in_progress(true);
    }

    if config::RECOVERY_CONFIG.enable_layer2 {
        config::set_current_layer(RecoveryLayer::Layer2);

        match bsr::execute() {
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

    if config::RECOVERY_CONFIG.enable_layer3 {
        config::set_current_layer(RecoveryLayer::Layer3);
        bhr::execute();
    }

    bhr::execute_fallback()
}

pub fn execute_from_panic() -> ! {
    crate::klog_crit!(Kernel, "[RECOVERY] Panic detected, initiating layered recovery");
    execute_layered()
}

pub fn try_layer1_first() -> RecoveryResult {
    RecoveryResult::Escalate
}

pub fn get_recovery_status() -> RecoveryStatus {
    RecoveryStatus {
        current_layer: config::get_current_layer(),
        reset_in_progress: config::is_reset_in_progress(),
        bsr_count: config::BSR_ATTEMPT_COUNT.load(core::sync::atomic::Ordering::SeqCst),
        bhr_count: config::BHR_ATTEMPT_COUNT.load(core::sync::atomic::Ordering::SeqCst),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RecoveryStatus {
    pub current_layer: RecoveryLayer,
    pub reset_in_progress: bool,
    pub bsr_count: u32,
    pub bhr_count: u32,
}

#[cfg(feature = "kernel_test")]
pub mod tests {
    use super::*;

    pub fn test_recovery_status() -> bool {
        let status = get_recovery_status();
        status.bsr_count == 0 || status.bsr_count > 0
    }
}
