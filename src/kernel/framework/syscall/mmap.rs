//! mmap/munmap/mprotect 系统调用实现 — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码已于 2026-06-17 迁移到 `services::mm::mmap`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::mm::mmap::{
    MAP_ANONYMOUS, MAP_FIXED, MAP_PRIVATE, MAP_SHARED, SYS_MMAP_FLAGS, fd_to_inode_id,
    fd_to_mount_idx, mmap_syscall, mprotect_syscall, munmap_syscall,
};
