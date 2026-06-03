//! RwLock — TCB 读写锁 (framework/sync)
//!
//! 写者优先的读写锁, 适用于读多写少场景。
//!
//! ## 适用场景
//!
//! ✅ 多读者并发读
//! ✅ 写者优先 (防止写饥饿)
//! ❌ 中断上下文 (必须可用 SpinLock)
//!
//! ## SAFETY 不变量
//!
//! - `read()` 返回 `RwLockReadGuard`, 允许多个读者同时持有。
//! - `write()` 返回 `RwLockWriteGuard`, 独占访问。
//! - 写者等待时, 新读者阻塞 (写者优先策略)。

use core::ops::{Deref, DerefMut};

use crate::kernel::sync::rwlock::RwLock as InnerRwLock;

/// TCB 读写锁。
pub struct RwLock<T> {
    inner: InnerRwLock<T>,
}

impl<T> RwLock<T> {
    /// 创建新的读写锁, 包装初始数据。
    pub fn new(data: T) -> Self {
        Self {
            inner: InnerRwLock::new(data),
        }
    }

    /// 获取读锁。
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        RwLockReadGuard {
            guard: self.inner.read(),
        }
    }

    /// 获取写锁 (独占)。
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        RwLockWriteGuard {
            guard: self.inner.write(),
        }
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// 读锁 RAII Guard — drop 时释放读锁。
pub struct RwLockReadGuard<'a, T> {
    guard: crate::kernel::sync::types::RwLockReadGuard<'a, T>,
}

impl<'a, T> Deref for RwLockReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // 必须显式取址: `self.guard.data` 是 T 拷贝 (若 T: Copy),
        // 而 trait 需要 &T 引用。
        #[allow(clippy::needless_borrow)]
        &self.guard.data
    }
}

/// 写锁 RAII Guard — drop 时释放写锁。
pub struct RwLockWriteGuard<'a, T> {
    guard: crate::kernel::sync::types::RwLockWriteGuard<'a, T>,
}

impl<'a, T> Deref for RwLockWriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // 必须显式取址: `self.guard.data` 是 T 拷贝 (若 T: Copy),
        // 而 trait 需要 &T 引用。
        #[allow(clippy::needless_borrow)]
        &self.guard.data
    }
}

impl<'a, T> DerefMut for RwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.data
    }
}

// SAFETY: RwLock 内部 RwLock 已实现 Send + Sync。
unsafe impl<T: Send> Send for RwLock<T> {}
unsafe impl<T: Send> Sync for RwLock<T> {}
