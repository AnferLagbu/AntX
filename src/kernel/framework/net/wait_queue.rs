//! Socket WaitQueue 基础设施 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯策略代码 (Socket 等待队列管理)
//! 已于 2026-06-16 迁移到 services::net::wait_queue.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::net::wait_queue::*;
