//! WASM 二进制格式解析器 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯解析算法 (WASM 二进制格式解析)
//! 已于 2026-06-16 迁移到 `services::wasm::module`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::wasm::module::*;
