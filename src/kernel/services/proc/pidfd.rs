//! pidfd 系统调用实现
//!
//! pidfd_open: 为进程打开一个 pidfd
//! pidfd_send_signal: 通过 pidfd 发送信号
//! pidfd_getfd: 通过 pidfd 获取文件描述符

use crate::kernel::framework::syscall::types::Errno;

/// pidfd 标志位
const PIDFD_NONBLOCK: u32 = 1;

/// pidfd_open — 为进程打开一个 pidfd
pub fn pidfd_open(pid: u32, flags: u32) -> Result<usize, Errno> {
    if flags & !PIDFD_NONBLOCK != 0 {
        return Err(Errno::EINVAL);
    }

    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if table.get(pid).is_none() {
        return Err(Errno::ESRCH);
    }

    // 简化实现: 使用进程 PID 作为 fd 值
    Ok(pid as usize)
}

/// pidfd_send_signal — 通过 pidfd 发送信号
pub fn pidfd_send_signal(pidfd: u32, sig: i32, _siginfo: u64, _flags: u32) -> Result<usize, Errno> {
    if sig < 1 || sig > 64 {
        return Err(Errno::EINVAL);
    }

    let pid = pidfd;
    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if table.get(pid).is_none() {
        return Err(Errno::ESRCH);
    }

    crate::kernel::services::proc::signal::kill(pid)
        .map_err(|_| Errno::EINVAL)?;

    Ok(0)
}

/// pidfd_getfd — 通过 pidfd 获取文件描述符
pub fn pidfd_getfd(pidfd: u32, targetfd: u32, flags: u32) -> Result<usize, Errno> {
    if flags != 0 {
        return Err(Errno::EINVAL);
    }

    let target_pid = pidfd;
    let table = &crate::kernel::framework::proc::PROCESS_TABLE;

    // 使用 with_process 安全访问目标进程
    let global_fd = table.with_process(target_pid, |proc| {
        proc.fd_table.get_global_fd(targetfd as usize)
    }).ok_or(Errno::ESRCH)?;

    let global_fd = global_fd.ok_or(Errno::EBADF)?;

    // 在当前进程分配新 fd
    let current_pid = crate::kernel::framework::proc::process_get_current_pid();
    let new_local_fd = table.with_process_mut(current_pid, |proc| {
        proc.fd_table.alloc_fd(global_fd)
    }).ok_or(Errno::ESRCH)?;

    let new_local_fd = new_local_fd.ok_or(Errno::EMFILE)?;

    Ok(new_local_fd)
}
