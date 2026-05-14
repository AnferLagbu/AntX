//! # Barrier Stack — AntX 宏内核故障恢复子系统
//!
//! 栏栈是 AntX 在宏内核架构下实现模块级"重生"的核心基础设施。
//! 原理详见 [docs/development/barrier-stack-design.md](docs/development/barrier-stack-design.md)。
//!
//! ## 架构
//!
//! ```text
//! Rust panic!() → panic_handler → PANIC_FLAG → int 0x82
//!   → isr0x82 → exception_handler → recovery_try_recover_from_idt()
//!     → RecoveryManager::locate_domain() → cascade_rollback_bfs()
//!       → DomainState::RollingBack → undo.rollback_to(gen)
//!         → mark_recovered() → audit_log → PANIC_FLAG clear → IDT return
//! ```
//!
//! ## 模块组织
//!
//! ```text
//! barrier/
//! ├── mod.rs           入口：pub mod + 重新导出 + 全局静态变量
//! ├── types.rs         数据类型：DomainState, UndoEntry, BarrierSnapshot, RollbackEvent
//! ├── undo_log.rs      撤销日志：UndoLog + fnv1a_32
//! ├── domain.rs        恢复域：RecoveryDomain
//! ├── manager.rs       管理器：RecoveryManager + 审计日志
//! ├── recoverable.rs   可恢复原语：Snapshot trait + RecoverableMutex
//! ├── fault_inject.rs  故障注入：maybe_inject_fault (feature gate)
//! └── ffi.rs           C FFI 桥接层
//! ```

use core::sync::atomic::AtomicBool;

pub mod types;
pub mod undo_log;
pub mod domain;
pub mod manager;
pub mod recoverable;
pub mod fault_inject;
pub mod ffi;

pub use types::*;
pub use undo_log::UndoLog;
pub use domain::RecoveryDomain;
pub use manager::{RecoveryManager, ROLLBACK_LOG};
pub use recoverable::{Snapshot, Recoverable, RecoverableMutex};
pub use fault_inject::maybe_inject_fault;

pub use ffi::{
    recovery_barrier_maintenance,
    recovery_domain_register,
    recovery_domain_unregister,
    recovery_test_rollback,
    recovery_panic_flag_is_set,
    recovery_panic_flag_clear,
    recovery_try_recover_from_idt,
    recovery_trigger_panic,
    recovery_was_attempted,
    recovery_domain_set_cbs,
    recovery_undo_record,
    recovery_undo_count,
    recovery_domain_add_dep,
    recovery_domain_dep_count,
    recovery_domain_add_addr_range,
    recovery_rollback_log_count,
    recovery_domain_get_state,
    recovery_domain_get_failures,
};

#[cfg(feature = "fault_injection")]
pub use ffi::{recovery_set_fault_rate, recovery_get_fault_rate};

pub static PANIC_FLAG: AtomicBool = AtomicBool::new(false);
pub static PANIC_MSG: spin::Mutex<[u8; 128]> = spin::Mutex::new([0u8; 128]);

pub static RECOVERY_MANAGER: spin::Mutex<RecoveryManager> = spin::Mutex::new(RecoveryManager::new());
