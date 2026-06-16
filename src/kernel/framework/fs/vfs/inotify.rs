//! inotify — 文件系统事件通知机制 — framework re-export 层
//!
//! 实现已迁移到 services::fs::inotify, 本文件 re-export 公共 API
//! 并保留需要 unsafe 用户指针写入的 sys_inotify_read。
//!
//! ## 迁移记录
//!
//! - 原实现: framework/fs/vfs/inotify.rs (603 行, 2 unsafe)
//! - 迁移到: services/fs/inotify.rs (策略逻辑, 0 unsafe)
//! - 保留在 framework: sys_inotify_read (用户缓冲区写入, 需要 unsafe)

use crate::kernel::framework::errno::Errno;

// Re-export 所有公共类型与函数
pub use crate::kernel::services::fs::inotify::{
    InotifyEvent, INOTIFY_FD_BASE,
    IN_ACCESS, IN_MODIFY, IN_ATTRIB, IN_CLOSE_WRITE, IN_CLOSE_NOWRITE,
    IN_OPEN, IN_MOVED_FROM, IN_MOVED_TO, IN_CREATE, IN_DELETE, IN_DELETE_SELF,
    IN_MOVE_SELF, IN_ISDIR, IN_Q_OVERFLOW, IN_IGNORED, IN_NONBLOCK, IN_CLOEXEC,
    IN_ALL_EVENTS,
    sys_inotify_init1, sys_inotify_add_watch, sys_inotify_rm_watch,
    is_inotify_fd, inotify_notify, inotify_release, inotify_fd_readable, inotify_stats,
};

/// inotify_read — 从 inotify fd 读取事件
///
/// 保留在 framework 层因为需要 unsafe 写入用户缓冲区。
/// 策略逻辑 (事件出队) 委托到 services::fs::inotify::inotify_read_events。
pub fn sys_inotify_read(fd: i64, buf: *mut u8, count: usize) -> i64 {
    if buf.is_null() || count < InotifyEvent::FULL_SIZE {
        return Errno::EINVAL.as_ret();
    }

    let (events, _written) = match crate::kernel::services::fs::inotify::inotify_read_events(fd, count) {
        Some(r) => r,
        None => return Errno::EAGAIN.as_ret(),
    };

    let mut written = 0usize;
    for event in &events {
        // SAFETY: buf 非空且 count 已验证, written + FULL_SIZE <= count
        let dst = unsafe { buf.add(written) as *mut InotifyEvent };
        // SAFETY: dst 对齐且在 [buf, buf+count) 范围内
        unsafe { core::ptr::write(dst, *event) };
        written += InotifyEvent::FULL_SIZE;
    }

    written as i64
}
