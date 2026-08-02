//! KASLR 配置 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯常量与全局状态 (`AtomicU64`)
//! 已于 2026-06-16 迁移到 `services::config::kaslr`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::config::kaslr::*;
