#![deny(unsafe_code)]
//! 同步原语 — services 层安全代理
//!
//! ## 状态 (v2.12, 2026-06-04)
//!
//! Phase 2.5 sync 迁移 (1/N): 封装 `kernel::sync` 强类型与安全 RAII Guard:
//! - [x] types — `LockState` / `TryLockResult` / `IrqSaveFlags` / `SpinLockInner` 等
//! - [x] spinlock — 自旋锁 API (lock/unlock/trylock + irqsave/irq 版本)
//! - [x] mutex — 睡眠锁 API (lock/unlock/trylock + owner/timeout)
//! - [x] rwlock — 读写锁 API (read_lock/write_lock/try + irqsave 版本)
//! - [x] atomic — 原子操作 re-export
//! - [x] seqlock / rcu / arch — 顺序锁/RCU/arch 内存屏障 re-export
//! - [x] smp barriers — `smp_wmb` / `smp_rmb` / `smp_mb` 跨 CPU 内存屏障
//! - [x] irq 控制 — `disable_interrupts` / `restore_interrupts` / `scheduler_yield`
//!
//! ## 迁移方法
//!
//! sync 子系统本身就是 100% 安全 Rust (`unsafe` 仅出现在 FFI 包装),
//! services 层职责:
//! 1. 把所有 FFI `*mut T` / `*const T` 接口隔离在 framework 层
//! 2. 暴露类型安全 RAII 接口 (SpinLockGuard / MutexGuard / RwLockGuard) 供 services 用户使用
//! 3. 0 unsafe 出现在 services 层
//!
//! 评估日期: 2026-06-04


// ============================================================================
// 强类型 re-export
// ============================================================================

/// 锁状态枚举
pub use crate::kernel::framework::sync::types::LockState;

/// `try_lock` 结果
pub use crate::kernel::framework::sync::types::TryLockResult;

/// 自旋锁内核表示 (FFI 桥接用, 一般不直接访问)
pub use crate::kernel::framework::sync::types::{SpinLockInner, MutexInner, RwLockInner, CondVarInner};

/// 中断保存标志
pub use crate::kernel::framework::sync::types::IrqSaveFlags;

/// 锁统计信息 (仅 `lock_stats` feature 启用时可用)
#[cfg(feature = "lock_stats")]
pub use crate::kernel::framework::sync::types::LockStatistics;

// ============================================================================
// RAII Guard (类型安全, 替代裸 lock/unlock 配对)
// ============================================================================

/// 自旋锁 RAII 守卫 (`&mut T` 借用于锁内, 析构自动释放)
pub use crate::kernel::framework::sync::types::SpinLockGuard;

/// 互斥锁 RAII 守卫
pub use crate::kernel::framework::sync::types::MutexGuard;

/// 读锁 RAII 守卫
pub use crate::kernel::framework::sync::types::RwLockReadGuard;

/// 写锁 RAII 守卫
pub use crate::kernel::framework::sync::types::RwLockWriteGuard;

/// 优先级继承互斥锁 (Phase P1 #3)
pub mod pi_mutex;

pub use pi_mutex::{
    PiMutex, PiMutexError, PiMutexResult,
    lock as pi_lock, try_lock as pi_try_lock,
};

// ============================================================================
// 中断控制 (用于 irqsave 风格的锁)
// ============================================================================

/// 禁用中断并返回保存的 flags
///
/// **调用方约束**: 必须在中断上下文或单 CPU 上下文调用, 配对使用 `restore_interrupts`.
pub fn disable_interrupts() -> IrqSaveFlags {
    crate::kernel::framework::sync::spinlock::disable_interrupts()
}

/// 恢复中断到指定 flags
pub fn restore_interrupts(flags: &IrqSaveFlags) {
    crate::kernel::framework::sync::spinlock::restore_interrupts(flags)
}

/// 中断禁用 RAII 守卫 (析构时自动恢复中断)
pub struct IrqDisabled {
    flags: IrqSaveFlags,
}

impl IrqDisabled {
    /// 进入临界区 (禁用中断)
    pub fn enter() -> Self {
        Self {
            flags: disable_interrupts(),
        }
    }
}

impl Drop for IrqDisabled {
    fn drop(&mut self) {
        restore_interrupts(&self.flags);
    }
}

// ============================================================================
// 跨 CPU 内存屏障
// ============================================================================

/// 写内存屏障 (跨 CPU 顺序)
pub fn smp_wmb() {
    crate::kernel::framework::sync::spinlock::smp_wmb();
}

/// 读内存屏障
pub fn smp_rmb() {
    crate::kernel::framework::sync::spinlock::smp_rmb();
}

/// 读写全屏障
pub fn smp_mb() {
    crate::kernel::framework::sync::spinlock::smp_mb();
}

// ============================================================================
// 调度器桥接
// ============================================================================

/// 当前进程 PID (0 表示内核线程 / 启动期)
pub fn current_pid() -> u32 {
    crate::kernel::framework::proc::api::process_get_current_pid()
}

/// 主动让出 CPU
pub fn scheduler_yield() {
    crate::kernel::framework::proc::api::scheduler_yield();
}

// ============================================================================
// 错误
// ============================================================================

/// 同步原语错误 — TD-20: 收敛到 KernelError, 2 字段 sync 特有 + 1 共享包装.
///
/// 字段说明:
///   - `Deadlock`: 同线程重复 lock (POSIX EDEADLK=35 但语义不通用)
///   - `Timeout`: 加锁超时 (POSIX ETIMEDOUT=110)
///   - `Kernel(KernelError)`: 共享错误 (WouldBlock / Other) 走单一来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncError {
    /// 死锁 (同线程重复 lock)
    Deadlock,
    /// 超时
    Timeout,
    /// 共享 `KernelError` 包装
    Kernel(crate::kernel::services::error::KernelError),
}

impl SyncError {
    /// 映射为 POSIX errno
    pub fn to_errno(self) -> Errno {
        use Errno as E;
        match self {
            Self::Deadlock => E::EDEADLK,
            Self::Timeout => E::ETIMEDOUT,
            Self::Kernel(e) => e.as_errno(),
        }
    }

    pub fn from_i32(rc: i32) -> Self {
        use crate::kernel::services::error::KernelError as K;
        match rc {
            -11 => Self::Kernel(K::WouldBlock),
            -35 => Self::Deadlock,
            -110 => Self::Timeout,
            rc => Self::Kernel(K::Other(rc)),
        }
    }
}

pub type SyncResult<T> = Result<T, SyncError>;

use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// Futex — 用户态同步原语
// ============================================================================

pub mod futex;
pub mod epoll;
pub mod eventfd;
pub mod signalfd;
pub mod timerfd;
pub mod irq_lock;
pub mod once;

/// Lockdep — 运行时锁依赖检测器 (P1)
pub mod lockdep;
