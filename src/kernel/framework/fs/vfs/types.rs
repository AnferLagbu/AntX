//! VFS 公共类型 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯类型定义 (常量/枚举/结构体/FileSystem trait)
//! 已于 2026-06-16 迁移到 `services::fs::vfs_types`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::fs::vfs_types::*;
