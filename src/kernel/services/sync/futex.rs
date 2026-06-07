#![deny(unsafe_code)]
//! Futex — services 层安全代理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::syscall::futex。
//!
//! ## 职责
//!
//! - 提供类型安全的 futex API (强类型参数, Errno 错误处理)
//! - 参数验证 (uaddr 有效性, 操作码范围)
//! - 委托 framework 层执行底层操作

use crate::kernel::framework::syscall::futex;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 强类型常量 re-export
// ============================================================================

/// 等待: 若 *uaddr == val, 阻塞当前线程
pub const FUTEX_WAIT: i32 = futex::FUTEX_WAIT;
/// 唤醒: 唤醒最多 val 个等待者
pub const FUTEX_WAKE: i32 = futex::FUTEX_WAKE;
/// 迁移等待者
pub const FUTEX_REQUEUE: i32 = futex::FUTEX_REQUEUE;
/// 私有标志
pub const FUTEX_PRIVATE_FLAG: i32 = futex::FUTEX_PRIVATE_FLAG;

// ============================================================================
// Futex 操作结果
// ============================================================================

/// Futex 操作结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutexResult {
    /// 成功, 附加返回值
    Ok(i64),
    /// 失败, errno
    Err(Errno),
}

impl FutexResult {
    /// 从 syscall 返回值解析
    pub fn from_ret(ret: i64) -> Self {
        if ret >= 0 {
            FutexResult::Ok(ret)
        } else {
            // 将 errno 数值映射到 Errno 枚举
            let errno = match -ret as i32 {
                14 => Errno::EFAULT,
                11 => Errno::EAGAIN,
                22 => Errno::EINVAL,
                _ => Errno::EINVAL,
            };
            FutexResult::Err(errno)
        }
    }

    /// 是否成功
    pub fn is_ok(&self) -> bool {
        matches!(self, FutexResult::Ok(_))
    }

    /// 获取返回值 (仅成功时有效)
    pub fn value(&self) -> Option<i64> {
        match self {
            FutexResult::Ok(v) => Some(*v),
            _ => None,
        }
    }

    /// 转换为 syscall 返回值 (POSIX: 正数=成功, 负数=-errno)
    pub fn as_ret(&self) -> i64 {
        match self {
            FutexResult::Ok(v) => *v,
            FutexResult::Err(e) => -(*e as i64),
        }
    }
}

// ============================================================================
// 安全 API
// ============================================================================

/// FUTEX_WAIT: 原子比较并阻塞
///
/// 若 `*uaddr == val`, 阻塞当前线程直到被唤醒.
/// 否则立即返回 EAGAIN.
///
/// # 参数验证
///
/// - `uaddr` 必须非零 (syscall 入口已通过 check_user_ptr 验证)
/// - `val` 任意值
pub fn futex_wait(uaddr: u64, val: i32, timeout: u64) -> FutexResult {
    if uaddr == 0 {
        return FutexResult::Err(Errno::EFAULT);
    }
    let ret = futex::sys_futex(uaddr, FUTEX_WAIT, val, timeout, 0);
    FutexResult::from_ret(ret)
}

/// FUTEX_WAKE: 唤醒等待者
///
/// 唤醒最多 `max_count` 个等待在 `uaddr` 上的线程.
///
/// # 返回
///
/// 实际唤醒的线程数.
pub fn futex_wake(uaddr: u64, max_count: u32) -> FutexResult {
    if uaddr == 0 {
        return FutexResult::Err(Errno::EFAULT);
    }
    let ret = futex::sys_futex(uaddr, FUTEX_WAKE, max_count as i32, 0, 0);
    FutexResult::from_ret(ret)
}

/// FUTEX_REQUEUE: 迁移等待者
///
/// 唤醒最多 `max_wake` 个等待者, 将最多 `max_requeue` 个
/// 等待者从 `uaddr` 迁移到 `uaddr2`.
pub fn futex_requeue(uaddr: u64, max_wake: u32, uaddr2: u64, max_requeue: u32) -> FutexResult {
    if uaddr == 0 || uaddr2 == 0 {
        return FutexResult::Err(Errno::EFAULT);
    }
    let ret = futex::sys_futex(uaddr, FUTEX_REQUEUE, max_wake as i32, uaddr2, max_requeue);
    FutexResult::from_ret(ret)
}

/// 通用 futex 系统调用代理
///
/// 支持任意操作码, 内部委托 framework 层.
pub fn futex_syscall(uaddr: u64, op: i32, val: i32, timeout_or_uaddr2: u64, val2: u32) -> FutexResult {
    if uaddr == 0 {
        return FutexResult::Err(Errno::EFAULT);
    }
    let ret = futex::sys_futex(uaddr, op, val, timeout_or_uaddr2, val2);
    FutexResult::from_ret(ret)
}
