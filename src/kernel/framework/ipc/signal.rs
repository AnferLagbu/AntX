//! 信号 (Signal) 机制实现 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯策略代码 (信号发送/注册/屏蔽/分发)
//! 已于 2026-06-16 迁移到 services::ipc::signal.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::ipc::signal::*;
