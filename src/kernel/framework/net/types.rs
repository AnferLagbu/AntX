//! 网络子系统公共类型 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯常量与全局状态 (AtomicBool)
//! 已于 2026-06-16 迁移到 services::net::types.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::net::types::*;
