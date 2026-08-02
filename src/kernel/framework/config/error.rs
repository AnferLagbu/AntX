//! 配置校验结果类型 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯类型定义
//! 已于 2026-06-16 迁移到 `services::config::error`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::config::error::*;
