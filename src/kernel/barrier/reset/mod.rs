//! # Barrier Reset 模块
//!
//! 分层恢复策略：BBR → BSR → BHR
//!
//! ```text
//! Layer 1: BBR (Barrier Base Recovery)  ~1μs   >95%成功率
//! Layer 2: BSR (Barrier Soft Reset)     ~50ms  >80%成功率
//! Layer 3: BHR (Barrier Hard Reset)     ~120ms ~100%成功率
//! ```

pub mod config;
pub mod audit;
pub mod bbr;
pub mod bsr;
pub mod bhr;
pub mod parallel;
pub mod layered;

pub use config::{
    RecoveryLayer,
    RecoveryResult,
    RecoveryConfig,
    RollbackMode,
    RECOVERY_CONFIG,
    CURRENT_LAYER,
    RESET_IN_PROGRESS,
    BBR_ATTEMPT_COUNT,
    BSR_ATTEMPT_COUNT,
    BHR_ATTEMPT_COUNT,
    is_reset_in_progress,
    set_reset_in_progress,
    get_current_layer,
    set_current_layer,
    get_stats,
    reset_stats,
};

pub use audit::{
    ResetAuditLog,
    ResetAuditEntry,
    RESET_AUDIT_LOG,
    audit_record,
    audit_record_domain,
    audit_get_last,
    audit_clear,
};

pub use bbr::{
    execute as bbr_execute,
    locate_domain_from_panic,
    try_rollback_single,
    cascade_rollback,
    compute_fingerprint,
    mark_recovered,
    should_attempt_recovery,
};

pub use bsr::{
    execute as bsr_execute,
    freeze_all_domains as bsr_freeze_all_domains,
    unfreeze_all_domains as bsr_unfreeze_all_domains,
    rollback_to_init as bsr_rollback_to_init,
    reset_devices as bsr_reset_devices,
    reset_interrupts as bsr_reset_interrupts,
    clear_panic_state as bsr_clear_panic_state,
};

pub use bhr::{
    execute as bhr_execute,
    execute_fallback as bhr_execute_fallback,
    disable_interrupts as bhr_disable_interrupts,
    mask_all_irqs as bhr_mask_all_irqs,
    shutdown_devices as bhr_shutdown_devices,
    save_crash_info as bhr_save_crash_info,
    keyboard_reset as bhr_keyboard_reset,
    triple_fault as bhr_triple_fault,
};

pub use parallel::{
    DependencyLayer,
    DependencyLayers,
    compute_dependency_layers,
    rollback_layer_serial,
    rollback_layer_parallel,
    rollback_all_parallel,
    rollback_all,
    get_parallel_stats,
};

pub use layered::{
    execute_layered as recovery_execute_layered,
    execute_from_panic,
    try_bbr_first,
    get_recovery_status,
    RecoveryStatus,
};
