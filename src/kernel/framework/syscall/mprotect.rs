//! mprotect — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码已于 2026-06-17 迁移到 `services::mm::mprotect`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::mm::mprotect::{
    mprotect_syscall as sys_mprotect,
    prot_to_page_flags,
    PROT_NONE, PROT_READ, PROT_WRITE, PROT_EXEC,
};
