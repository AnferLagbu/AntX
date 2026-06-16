//! WASM 类型定义 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯类型定义 (值类型/操作码/模块结构/运行时类型)
//! 已于 2026-06-16 迁移到 services::wasm::types.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::wasm::types::*;
