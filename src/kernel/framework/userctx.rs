//! `UserContext` — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯类型定义 (寄存器快照结构体)
//! 已于 2026-06-16 迁移到 `services::userctx`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::userctx::*;
