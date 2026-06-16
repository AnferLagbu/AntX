//! 运行时数据结构 — framework 层 re-export
//!
//! ## T6-9 迁移记录
//!
//! 纯数据结构 (值栈/调用帧/线性内存)
//! 已于 2026-06-16 迁移到 services::wasm::runtime.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::wasm::runtime::*;
