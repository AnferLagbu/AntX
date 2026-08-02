//! 同步原语数据类型定义 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯类型定义 (锁状态/守卫/统计)
//! 已于 2026-06-16 迁移到 `services::sync::types`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::sync::types::*;
