//! pidfd 系统调用实现
//!
//! pidfd_open: 为进程打开一个 pidfd
//! pidfd_send_signal: 通过 pidfd 发送信号
//! pidfd_getfd: 通过 pidfd 获取文件描述符 (需要 OpenFile 系统, 暂存根)

use crate::kernel::framework::syscall::types::Errno;

/// pidfd 标志位
const PIDFD_NONBLOCK: u32 = 1;

/// `pidfd_open` — 为进程打开一个 pidfd
///
/// # Errors
///
/// - flags 含非法位 → `EINVAL`
/// - 进程不存在 → `ESRCH`
pub fn pidfd_open(pid: u32, flags: u32) -> Result<usize, Errno> {
    if flags & !PIDFD_NONBLOCK != 0 {
        return Err(Errno::EINVAL);
    }

    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if table.get(pid).is_none() {
        return Err(Errno::ESRCH);
    }

    Ok(pid as usize)
}

/// `pidfd_send_signal` — 通过 pidfd 发送信号
///
/// # Errors
///
/// - 信号编号不在 1..=64 范围或发送失败 → `EINVAL`
/// - 进程不存在 → `ESRCH`
pub fn pidfd_send_signal(pidfd: u32, sig: i32, _siginfo: u64, _flags: u32) -> Result<usize, Errno> {
    if !(1..=64).contains(&sig) {
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

/// `pidfd_getfd` — 通过 pidfd 获取文件描述符
///
/// 需要完整的 `OpenFile` 系统支持, 暂存根.
///
/// # Errors
///
/// 该接口尚未实现, 始终返回 `ENOSYS`.
pub fn pidfd_getfd(_pidfd: u32, _targetfd: u32, _flags: u32) -> Result<usize, Errno> {
    // TODO: 需要 Task 4 (OpenFile 系统) 完成后实现
    Err(Errno::ENOSYS)
}
