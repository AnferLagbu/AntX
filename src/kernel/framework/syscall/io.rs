//! IO 系统调用 (TCB)
//!
//! POSIX 标准的 IO/管道/文件控制:
//! - pipe / pipe2: 匿名管道创建
//! - dup / dup2 / dup3: 文件描述符复制
//! - fcntl: 文件控制

use crate::kernel::framework::fs::vfs::api;
use crate::kernel::framework::ipc::pipe as ipc_pipe;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 管道
// ============================================================================

/// pipe 系统调用
///
/// `fds` 指向 i32[2] 数组, 返回读端和写端文件描述符.
pub fn sys_pipe(fds: u64) -> i64 {
    if fds == 0 || !raw::check_user_buf(fds, 8) {
        return Errno::EFAULT.as_ret();
    }

    let mut pipefd: [i32; 2] = [0; 2];
    // SAFETY: pipefd 是栈上有效数组
    let result = unsafe { ipc_pipe::ipc_pipe_create(pipefd.as_mut_ptr()) };
    if result < 0 {
        return Errno::EBUSY.as_ret();
    }
    if pipefd[0] < 0 || pipefd[1] < 0 {
        return Errno::EBUSY.as_ret();
    }
    // SAFETY: fds 由 check_user_buf 验证为可写, 大小 8 = 2 × i32
    unsafe {
        raw::write_u32(fds as *mut u32, pipefd[0] as u32);
        raw::write_u32((fds + 4) as *mut u32, pipefd[1] as u32);
    }
    0
}

// ============================================================================
// dup
// ============================================================================

/// dup — 复制文件描述符 (返回新 fd, 取最小可用值)
pub fn sys_dup(oldfd: i32) -> i64 {
    if oldfd < 0 {
        return Errno::EBADF.as_ret();
    }
    api::vfs_dup(oldfd as u32) as i64
}

/// dup2 — 复制文件描述符到 newfd
///
/// 若 newfd 已打开则先关闭. 若 oldfd == newfd 则不关闭直接返回.
pub fn sys_dup2(oldfd: i32, newfd: i32) -> i64 {
    if oldfd < 0 || newfd < 0 {
        return Errno::EBADF.as_ret();
    }
    if oldfd == newfd {
        return newfd as i64;
    }
    let result = api::vfs_dup2(oldfd as u32, newfd as u32);
    if result < 0 {
        return Errno::EBADF.as_ret();
    }
    result as i64
}

/// dup3 — dup2 扩展版, 支持 flags
///
/// flags: O_CLOEXEC 等. 当前简化: 等同 dup2.
pub fn sys_dup3(oldfd: i32, newfd: i32, flags: i32) -> i64 {
    if oldfd < 0 || newfd < 0 {
        return Errno::EBADF.as_ret();
    }
    if oldfd == newfd {
        return Errno::EINVAL.as_ret(); // dup3 要求 oldfd != newfd
    }
    // flags 当前被忽略 (未实现 O_CLOEXEC 处理)
    let _ = flags;
    let result = api::vfs_dup2(oldfd as u32, newfd as u32);
    if result < 0 {
        return Errno::EBADF.as_ret();
    }
    result as i64
}

// ============================================================================
// fcntl
// ============================================================================

const F_DUPFD: i32 = 0;
const F_GETFD: i32 = 1;
const F_SETFD: i32 = 2;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;

/// fcntl 系统调用
pub fn sys_fcntl(fd: i32, cmd: i32, arg: u64) -> i64 {
    if fd < 0 {
        return Errno::EBADF.as_ret();
    }
    match cmd {
        F_GETFD => 0,
        F_SETFD => 0,
        F_GETFL => {
            let fd_table = crate::kernel::framework::fs::vfs::vfs::VFS_MANAGER.fd_table.lock();
            if (fd as usize) < 256 && fd_table[fd as usize].used {
                fd_table[fd as usize].flags as i64
            } else {
                Errno::EBADF.as_ret()
            }
        }
        F_SETFL => 0,
        F_DUPFD => sys_dup2(fd, arg as i32),
        _ => Errno::EINVAL.as_ret(),
    }
}
