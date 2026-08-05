//! `io_uring` 异步 I/O 框架 — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码 (数据结构 + 环形缓冲区 + 实例管理 + syscall 分发)
//! 于 2026-06-18 迁移到 `services::io::iouring`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::io::iouring::{
    Cqe, DEFAULT_RING_SIZE, IoOpCode, IoUring, MAX_URING_INSTANCES, RingBuffer, Sqe,
    io_uring_destroy, io_uring_enter, io_uring_reap, io_uring_setup, io_uring_submit,
    sys_io_uring_enter, sys_io_uring_register, sys_io_uring_setup, sys_io_uring_submit_sqe,
};
