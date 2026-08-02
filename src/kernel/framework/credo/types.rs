//! Credo v1 类型定义 — framework 层 re-export
//!
//! ## T6-7 迁移记录
//!
//! 纯数据定义 (PWM 类型/能力矩阵/身份条目/审计类型)
//! 已于 2026-06-16 迁移到 `services::credo::types`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::credo::types::*;
