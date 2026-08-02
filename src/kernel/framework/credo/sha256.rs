//! SHA-256 哈希实现 — framework 层 re-export
//!
//! ## T6-8 迁移记录
//!
//! 纯算法实现 (SHA-256 哈希)
//! 已于 2026-06-16 迁移到 `services::credo::sha256`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::credo::sha256::*;
