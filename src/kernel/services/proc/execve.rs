#![deny(unsafe_code)]
//! execve — services 层安全代理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::syscall。
//!
//! ## 职责
//!
//! - 提供类型安全的 execve API
//! - 参数验证与类型转换
//! - 错误码封装
//!
//! ## 注意
//!
//! execve 的核心逻辑 (用户指针验证、SUID 处理、进程替换)
//! 必须在 framework TCB 中执行, 因为涉及:
//! - 原始指针操作 (read_volatile 用户空间)
//! - 进程地址空间替换 (页表切换)
//! - Credo PWM 权限提升 (SUID)
//!
//! services 层仅做参数类型转换和错误码封装.

use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// execve 安全 API
// ============================================================================

/// execve 结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecveResult {
    /// 成功 (execve 成功时不返回, 此值仅用于类型完整性)
    Success,
    /// 失败, errno
    Err(Errno),
}

impl ExecveResult {
    /// 从 syscall 返回值解析
    pub fn from_ret(ret: i64) -> Self {
        if ret >= 0 {
            ExecveResult::Success
        } else {
            let errno = match -ret as i32 {
                2 => Errno::ENOENT,
                14 => Errno::EFAULT,
                13 => Errno::EACCES,
                8 => Errno::ENOEXEC,
                _ => Errno::EINVAL,
            };
            ExecveResult::Err(errno)
        }
    }

    /// 转换为 syscall 返回值
    pub fn as_ret(&self) -> i64 {
        match self {
            ExecveResult::Success => 0,
            ExecveResult::Err(e) => -(*e as i64),
        }
    }
}
