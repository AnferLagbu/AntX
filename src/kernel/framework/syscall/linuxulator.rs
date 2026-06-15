//! Linux ABI 兼容层 (linuxulator) — framework 层 re-export
//!
//! ## T5-2 迁移记录
//!
//! 策略代码 (编号翻译表 + 参数转换) 已于 2026-06-16 迁移到
//! services::syscall::linuxulator. 本文件仅 re-export 保持调用方兼容.

// Re-export services 层的策略主体 — 保持调用方路径兼容
pub use crate::kernel::services::syscall::linuxulator::{
    is_rt_sigreturn, translate_syscall, translate_args, LinuxArgs,
};
