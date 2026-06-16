//! POSIX Errno 统一 re-export
//!
//! 消除 proc/mm/fs 对 syscall 子系统的 Errno 类型依赖.
//! Errno 是通用错误类型, 不应属于 syscall 子系统.
//! 实际定义在 services::syscall::types, 此处仅 re-export.

pub use crate::kernel::services::syscall::types::Errno;
