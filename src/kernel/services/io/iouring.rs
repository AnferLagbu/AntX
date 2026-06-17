#![deny(unsafe_code)]
//! io_uring 安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::io::iouring` 的安全 API.

// 重导出强类型
pub use crate::kernel::framework::io::iouring::{
    IoOpCode, Sqe, Cqe, IoUring, RingBuffer,
    MAX_URING_INSTANCES, DEFAULT_RING_SIZE,
};

use crate::kernel::framework::io::iouring::{
    io_uring_setup, io_uring_destroy, io_uring_submit, io_uring_enter, io_uring_reap,
};
use crate::kernel::framework::syscall::Errno;

/// 创建 io_uring 实例 (安全封装)
pub fn setup(entries: u32, owner_pid: u32) -> Result<u32, Errno> {
    io_uring_setup(entries, owner_pid)
}

/// 销毁 io_uring 实例 (安全封装)
pub fn destroy(id: u32) -> Result<(), Errno> {
    io_uring_destroy(id)
}

/// 提交 SQE (安全封装)
pub fn submit(id: u32, sqe: Sqe) -> Result<(), Errno> {
    io_uring_submit(id, sqe)
}

/// 进入 io_uring (安全封装)
pub fn enter(id: u32, to_submit: u32, min_complete: u32) -> Result<u32, Errno> {
    io_uring_enter(id, to_submit, min_complete)
}

/// 收割 CQE (安全封装)
pub fn reap(id: u32) -> Option<Cqe> {
    io_uring_reap(id)
}
