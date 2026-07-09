//! pidfd 系统调用实现
//!
//! pidfd_open: 为进程打开一个 pidfd
//! pidfd_send_signal: 通过 pidfd 发送信号
//! pidfd_getfd: 通过 pidfd 获取文件描述符 (暂存根)

use crate::kernel::framework::syscall::types::Errno;

/// pidfd 标志位
const PIDFD_NONBLOCK: u32 = 1;

/// pidfd_open — 为进程打开一个 pidfd
///
/// # Arguments
/// * `pid` - 目标进程 PID
/// * `flags` - 标志位 (PIDFD_NONBLOCK 等)
///
/// # Returns
/// 成功返回 pidfd 文件描述符，失败返回 Errno
pub fn pidfd_open(pid: u32, flags: u32) -> Result<usize, Errno> {
    // 检查 flags 是否支持
    if flags & !PIDFD_NONBLOCK != 0 {
        return Err(Errno::EINVAL);
    }

    // 检查进程是否存在
    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if table.get(pid).is_none() {
        return Err(Errno::ESRCH);
    }

    // 分配一个文件描述符
    // pidfd 本质上是一个指向进程的文件描述符
    // 简化实现: 使用进程 PID 作为 fd 值 (实际应分配真正的 fd)
    // TODO: 实现真正的 pidfd fd 分配和生命周期管理
    let fd = pid as usize;

    Ok(fd)
}

/// pidfd_send_signal — 通过 pidfd 发送信号
///
/// # Arguments
/// * `pidfd` - pidfd 文件描述符
/// * `sig` - 信号编号
/// * `siginfo` - 信号信息 (暂不使用)
/// * `flags` - 标志位
///
/// # Returns
/// 成功返回 0，失败返回 Errno
pub fn pidfd_send_signal(pidfd: u32, sig: i32, _siginfo: u64, _flags: u32) -> Result<usize, Errno> {
    // 检查信号编号有效性
    if sig < 1 || sig > 64 {
        return Err(Errno::EINVAL);
    }

    // 从 pidfd 获取进程 PID
    // 简化实现: pidfd 值就是 PID
    let pid = pidfd;

    // 检查进程是否存在
    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if table.get(pid).is_none() {
        return Err(Errno::ESRCH);
    }

    // 发送信号 (使用 kill 函数)
    crate::kernel::services::proc::signal::kill(pid)
        .map_err(|_| Errno::EINVAL)?;

    Ok(0)
}

/// pidfd_getfd — 通过 pidfd 获取文件描述符
///
/// # Arguments
/// * `pidfd` - pidfd 文件描述符
/// * `targetfd` - 目标进程中的 fd
/// * `flags` - 标志位
///
/// # Returns
/// 暂存根，返回 ENOSYS
pub fn pidfd_getfd(_pidfd: u32, _targetfd: u32, _flags: u32) -> Result<usize, Errno> {
    // TODO: 实现真正的 pidfd_getfd
    // 需要:
    // 1. 从 pidfd 获取目标进程
    // 2. 检查目标进程是否有 targetfd
    // 3. 在当前进程分配新 fd
    // 4. 复制文件描述符引用
    Err(Errno::ENOSYS)
}
