//! Syscall 类型定义和常量 — framework 层 re-export
//!
//! ## T5-4 迁移记录
//!
//! 纯数据定义 (syscall 编号 + Errno + SyscallRegs + 辅助类型)
//! 已于 2026-06-16 迁移到 services::syscall::types.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::syscall::types::*;
