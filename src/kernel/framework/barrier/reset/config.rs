//! 恢复配置与类型定义 — framework 层 re-export
//!
//! ## T6-6 迁移记录
//!
//! 纯策略代码 (恢复层配置 + 原子状态 + 统计)
//! 已于 2026-06-16 迁移到 services::barrier::reset_config.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::barrier::reset_config::*;
