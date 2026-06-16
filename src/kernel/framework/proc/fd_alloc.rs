//! 全局统一 FD 分配器 — framework 层 re-export
//!
//! ## T6-5 迁移记录
//!
//! 纯策略代码 (FD 范围规划 + 分配/释放/反查)
//! 已于 2026-06-16 迁移到 services::proc::fd_alloc.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::proc::fd_alloc::*;
