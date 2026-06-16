//! IPC 数据类型定义 — framework 层 re-export
//!
//! ## T6-3 迁移记录
//!
//! 纯数据定义 (IPC 类型/信号/管道/共享内存/消息队列/信号量)
//! 已于 2026-06-16 迁移到 services::ipc::types.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::ipc::types::*;
