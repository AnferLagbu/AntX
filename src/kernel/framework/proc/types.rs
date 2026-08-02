//! 进程类型定义 — framework 层 re-export
//!
//! ## T6-2 迁移记录
//!
//! 纯数据定义 (PID/TID/ProcessState/Priority/Context)
//! 已于 2026-06-16 迁移到 `services::proc::types`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::proc::types::*;
