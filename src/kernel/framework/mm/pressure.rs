//! 内存压力策略 — framework re-export shim
//!
//! ## 框架责任分离
//!
//! - 机制 (framework): AtomicU8/AtomicU64 原语
//! - 策略 (services): 阈值表 / 4 级状态机 / 转换判定 → [`services::mm::memory_pressure`]
//!
//! P1-I-01 D9 提取 (2026-06-11).
//! 详见 [docs/plan/maintenance-2026-06-11.md] I-01 D9.

pub use crate::kernel::services::mm::memory_pressure::*;
