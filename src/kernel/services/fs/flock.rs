//! 文件锁安全代理 — services 层
//!
//! 将 framework::fs::vfs::flock 的 TCB 接口封装为 100% safe Rust API。
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use crate::kernel::services::fs::flock;
//!
//! // flock 整文件锁
//! match flock::sys_flock(fd, LOCK_EX | LOCK_NB, pid, ino) {
//!     flock::FlockResult::Ok => { /* 获得锁 */ }
//!     flock::FlockResult::WouldBlock => { /* 锁被占用 */ }
//!     _ => { /* 其他错误 */ }
//! }
//!
//! // POSIX record lock
//! match flock::sys_posix_lock(pid, ino, F_SETLK, F_WRLCK, 0, 1024) {
//!     Ok(None) => { /* 获得锁 */ }
//!     Ok(Some(conflict)) => { /* 冲突 */ }
//!     Err(e) => { /* 错误 */ }
//! }
//! ```

#![deny(unsafe_code)]

// Re-export 类型
pub use crate::kernel::framework::fs::vfs::flock::{
    FlockResult, PosixLockConflict, PosixLockResult,
    LOCK_SH, LOCK_EX, LOCK_UN, LOCK_NB,
    F_RDLCK, F_WRLCK, F_UNLCK,
    F_SETLK, F_SETLKW, F_GETLK,
    POSIX_LOCK_TO_EOF,
};

/// flock 系统调用
pub fn sys_flock(fd: i32, operation: i32, pid: u32, ino: u32) -> FlockResult {
    crate::kernel::framework::fs::vfs::flock::sys_flock(fd, operation, pid, ino)
}

/// POSIX record lock (fcntl F_SETLK/F_GETLK)
pub fn sys_posix_lock(
    pid: u32,
    ino: u32,
    cmd: i32,
    lock_type: i32,
    start: u64,
    len: u64,
) -> Result<Option<PosixLockConflict>, PosixLockResult> {
    crate::kernel::framework::fs::vfs::flock::sys_posix_lock(pid, ino, cmd, lock_type, start, len)
}

/// 释放指定 fd 持有的所有 flock 锁 (close 时调用)
pub fn flock_release_fd(pid: u32, fd: i32) {
    crate::kernel::framework::fs::vfs::flock::flock_release_fd(pid, fd);
}

/// 释放指定进程持有的所有 flock 锁 (进程退出时调用)
pub fn flock_release_pid(pid: u32) {
    crate::kernel::framework::fs::vfs::flock::flock_release_pid(pid);
}

/// 释放指定进程持有的所有 POSIX 锁 (进程退出时调用)
pub fn posix_lock_release_pid(pid: u32) {
    crate::kernel::framework::fs::vfs::flock::posix_lock_release_pid(pid);
}

/// 释放指定 inode 上的所有 POSIX 锁 (文件删除时调用)
pub fn posix_lock_release_inode(ino: u32) {
    crate::kernel::framework::fs::vfs::flock::posix_lock_release_inode(ino);
}

/// flock 操作次数
pub fn flock_ops() -> u64 {
    crate::kernel::framework::fs::vfs::flock::flock_ops()
}

/// POSIX lock 操作次数
pub fn posix_lock_ops() -> u64 {
    crate::kernel::framework::fs::vfs::flock::posix_lock_ops()
}

/// flock 条目数
pub fn flock_count() -> usize {
    crate::kernel::framework::fs::vfs::flock::flock_count()
}

/// POSIX lock 条目数
pub fn posix_lock_count() -> usize {
    crate::kernel::framework::fs::vfs::flock::posix_lock_count()
}

/// 重置统计计数器
pub fn reset_stats() {
    crate::kernel::framework::fs::vfs::flock::reset_stats();
}
