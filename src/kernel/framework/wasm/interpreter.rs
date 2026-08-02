//! WASM 解释器 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯解释器 (栈式虚拟机核心)
//! 已于 2026-06-16 迁移到 `services::wasm::interpreter`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::wasm::interpreter::*;
