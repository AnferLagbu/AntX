#![deny(unsafe_code)]
//! Priority Inheritance Mutex — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 强类型 RAII API
//! - 委托 `framework::sync::pi_mutex` 完成所有底层操作
//! - 提供 `lock()` / `try_lock()` 强类型方法, 错误统一返回 [`PiMutexError`]
//!
//! ## 评估日期
//!
//! 2026-06-08

use crate::kernel::framework::sync::pi_mutex as fw;
pub use fw::PiMutex;

// ============================================================================
// 错误
// ============================================================================

/// PI Mutex 操作错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiMutexError {
    /// 操作会被阻塞 (try_lock 失败)
    WouldBlock,
    /// 当前线程非持有者 (双重释放)
    NotOwner,
    /// 资源耗尽 (无空闲槽位)
    Exhausted,
}

impl PiMutexError {
    /// 映射为 POSIX errno
    pub fn to_errno(self) -> crate::kernel::framework::syscall::types::Errno {
        use crate::kernel::framework::syscall::types::Errno as E;
        match self {
            Self::WouldBlock => E::EAGAIN,
            Self::NotOwner => E::EPERM,
            Self::Exhausted => E::ENOMEM,
        }
    }
}

pub type PiMutexResult<T> = Result<T, PiMutexError>;

// ============================================================================
// 安全 API
// ============================================================================

/// 获取锁 (阻塞 + 优先级继承)
///
/// # 参数
/// - `my_pid`: 当前线程 PID
/// - `my_base_priority`: 当前线程的 base_priority
///
/// # 返回
/// RAII 守卫, drop 时自动释放
pub fn lock<T>(mutex: &PiMutex<T>, my_pid: u32, my_base_priority: u32) -> PiMutexResult<fw::PiMutexGuard<'_, T>> {
    // PI Mutex 的 lock 永不返回 WouldBlock, 内部自旋直到获取
    // 但若要区分错误, 可包装一层
    let _ = my_pid;
    let _ = my_base_priority;
    // 当前实现: lock() 不返回错误 (总会自旋直到成功)
    // 此处仅作类型适配, 实际不会有 Err
    Ok(mutex.lock(my_pid, my_base_priority))
}

/// 尝试获取锁 (非阻塞)
pub fn try_lock<T>(mutex: &PiMutex<T>, my_pid: u32, my_base_priority: u32) -> PiMutexResult<fw::PiMutexGuard<'_, T>> {
    if mutex.try_lock(my_pid, my_base_priority) {
        Ok(mutex.lock(my_pid, my_base_priority))
    } else {
        Err(PiMutexError::WouldBlock)
    }
}

// 注: set_donation_callback / set_revoke_callback 是启动期 unsafe API,
// 保留在 framework/sync/pi_mutex 层 (TCB), 不通过 services 包装.
