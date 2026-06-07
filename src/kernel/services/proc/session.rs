#![deny(unsafe_code)]
//! 会话/进程组系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe,纯类型安全
//! - 委托 framework/proc/session 实际完成 setsid/getsid/setpgid
//!
//! ## POSIX 语义
//!
//! - [setsid] 创建新会话,返回新 SID (= 当前 PID)
//! - [getsid] 取会话 ID (0 表示当前)
//! - [setpgid] 设置进程组 ID

use crate::kernel::framework::proc::session as fw;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// setsid
// ============================================================================

/// setsid() — 创建新会话
///
/// 成功返回新会话 ID (≥ 1),失败返回 errno。
pub fn setsid_syscall() -> Result<usize, Errno> {
    let r = fw::proc_setsid();
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

// ============================================================================
// getsid
// ============================================================================

/// getsid(pid) — 取会话 ID
///
/// - `pid == 0`: 当前进程会话
/// - `pid > 0`:  目标进程会话 (简化实现 = pid)
/// - `pid < 0`:  EINVAL
pub fn getsid_syscall(pid: i32) -> Result<usize, Errno> {
    if pid < 0 {
        return Err(Errno::EINVAL);
    }
    let r = fw::proc_getsid(pid);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

// ============================================================================
// setpgid
// ============================================================================

/// setpgid(pid, pgid) — 设置进程组
pub fn setpgid_syscall(pid: i32, pgid: i32) -> Result<usize, Errno> {
    if pid < 0 || pgid < 0 {
        return Err(Errno::EINVAL);
    }
    let r = fw::proc_setpgid(pid, pgid);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}
