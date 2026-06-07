#![deny(unsafe_code)]
//! wait4 — services 层安全代理
//!
//! 为 wait4 系统调用提供参数验证:
//! - pid 参数合法性 (POSIX pid 语义)
//! - options 标志组合
//! - wstatus 指针非零时由 framework 层 check_user_ptr 验证
//!
//! ## 安全边界
//!
//! - services 层验证 pid 范围/options 合法
//! - 进程表操作和阻塞委托给 framework 层 (TCB)

use crate::kernel::framework::syscall::types::Errno;

/// wait4 安全代理
///
/// 验证: pid 范围合法, options 仅含合法标志
pub fn wait4_syscall(pid: i32, wstatus_ptr: u64, options: i32) -> Result<usize, Errno> {
    // pid 范围: -PID_MAX_LIMIT .. PID_MAX_LIMIT
    // 简化: -32768..=32767
    const PID_MAX: i32 = 0x7FFF;
    const PID_MIN: i32 = -0x8000;
    if pid < PID_MIN || pid > PID_MAX {
        return Err(Errno::EINVAL);
    }

    // options 标志验证
    const WNOHANG: i32 = 0x1;
    const WUNTRACED: i32 = 0x2;
    const WCONTINUED: i32 = 0x8;
    let valid_opts = WNOHANG | WUNTRACED | WCONTINUED;
    if options & !valid_opts != 0 {
        return Err(Errno::EINVAL);
    }

    // wstatus 指针如果为 0, 允许 (调用方不需要状态)
    // 否则由 framework 内部 check_user_ptr 验证

    let ret = crate::kernel::framework::syscall::wait4::sys_wait4(pid, wstatus_ptr, options);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}
