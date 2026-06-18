//! 启动自检 — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码于 2026-06-17 从 framework::config::validate 迁移至此。
//! framework 层仅 re-export 保持调用方兼容.

pub use crate::kernel::services::config::validate::{
    validate_cpu_config, validate_memory_config, validate_interrupt_config,
    validate_cross_module_consistency, validate_pci_subsystem,
    validate_network_subsystem, validate_drivers, validate_system_config,
};
