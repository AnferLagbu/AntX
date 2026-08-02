//! 调度器常量 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯常量定义
//! 已于 2026-06-16 迁移到 `services::config::sched`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::config::sched::*;
