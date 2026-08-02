//! PWM v5 能力定义 — framework 层 re-export
//!
//! ## T6-8 迁移记录
//!
//! 纯常量定义 (16 域能力位 + viable floor)
//! 已于 2026-06-16 迁移到 `services::credo::capability`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::credo::capability::*;
