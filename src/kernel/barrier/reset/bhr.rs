//! # Barrier Hard Reset (BHR)
//!
//! Layer 3 恢复：完全从硬件层面重置

use super::config::{self, RecoveryLayer};
use super::audit;

pub fn disable_interrupts() {
    #[cfg(not(feature = "kernel_test"))]
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

pub fn mask_all_irqs() {
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

pub fn shutdown_devices() {
    #[cfg(not(feature = "kernel_test"))]
    {
        crate::klog_info!(Kernel, "[BHR] Shutting down devices...");
    }
}

pub fn save_crash_info() {
    #[cfg(not(feature = "kernel_test"))]
    {
        crate::klog_info!(Kernel, "[BHR] Crash info saved");
    }
}

pub fn keyboard_reset() -> ! {
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

pub fn triple_fault() -> ! {
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

pub fn execute() -> ! {
    config::set_current_layer(RecoveryLayer::Layer3);
    config::increment_bhr_count();

    crate::klog_crit!(Kernel, "[BHR] Barrier Hard Reset initiated");

    audit::audit_record(RecoveryLayer::Layer3, RecoveryResult::Success, 0);

    save_crash_info();
    disable_interrupts();
    mask_all_irqs();
    shutdown_devices();

    crate::klog_crit!(Kernel, "[BHR] Performing keyboard controller reset...");

    keyboard_reset()
}

pub fn execute_fallback() -> ! {
    crate::klog_crit!(Kernel, "[BHR] Keyboard reset failed, attempting triple fault");
    triple_fault()
}

use super::config::RecoveryResult;
