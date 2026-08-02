//! LEB128 编解码器 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯算法 (变长整数编解码)
//! 已于 2026-06-16 迁移到 `services::wasm::leb128`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::wasm::leb128::*;
