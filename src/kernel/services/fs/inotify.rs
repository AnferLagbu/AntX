//! inotify — 文件系统事件通知 services 层安全封装
//!
//! 100% safe Rust, 0 unsafe.
//! 封装 framework 层 inotify 接口, 提供类型安全的 API.

#![deny(unsafe_code)]

pub use crate::kernel::framework::fs::vfs::inotify::{
    InotifyEvent, INOTIFY_FD_BASE,
    IN_ACCESS, IN_MODIFY, IN_ATTRIB, IN_CLOSE_WRITE, IN_CLOSE_NOWRITE,
    IN_OPEN, IN_MOVED_FROM, IN_MOVED_TO, IN_CREATE, IN_DELETE, IN_DELETE_SELF,
    IN_MOVE_SELF, IN_ISDIR, IN_Q_OVERFLOW, IN_IGNORED, IN_NONBLOCK, IN_CLOEXEC,
    IN_ALL_EVENTS,
};

/// inotify_init1 系统调用
pub fn sys_inotify_init1(flags: i32) -> i64 {
    crate::kernel::framework::fs::vfs::inotify::sys_inotify_init1(flags)
}

/// inotify_add_watch 系统调用
pub fn sys_inotify_add_watch(fd: i64, ino: u32, mask: u32) -> i64 {
    crate::kernel::framework::fs::vfs::inotify::sys_inotify_add_watch(fd, ino, mask)
}

/// inotify_rm_watch 系统调用
pub fn sys_inotify_rm_watch(fd: i64, wd: i32) -> i64 {
    crate::kernel::framework::fs::vfs::inotify::sys_inotify_rm_watch(fd, wd)
}

/// inotify_read — 从 inotify fd 读取事件
pub fn sys_inotify_read(fd: i64, buf: *mut u8, count: usize) -> i64 {
    crate::kernel::framework::fs::vfs::inotify::sys_inotify_read(fd, buf, count)
}

/// 通知所有监控指定 inode 的 inotify 实例
pub fn inotify_notify(ino: u32, mask: u32, name: &str, is_dir: bool) {
    crate::kernel::framework::fs::vfs::inotify::inotify_notify(ino, mask, name, is_dir)
}

/// 释放指定 inotify 实例
pub fn inotify_release(fd: i64) {
    crate::kernel::framework::fs::vfs::inotify::inotify_release(fd)
}

/// 检查 inotify fd 是否可读 (epoll 集成)
pub fn inotify_fd_readable(fd: i64) -> bool {
    crate::kernel::framework::fs::vfs::inotify::inotify_fd_readable(fd)
}

/// 获取 inotify 统计信息
pub fn inotify_stats() -> (u64, u64) {
    crate::kernel::framework::fs::vfs::inotify::inotify_stats()
}
