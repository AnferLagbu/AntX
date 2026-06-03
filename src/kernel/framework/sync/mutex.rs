//! Mutex — TCB 睡眠互斥锁 (framework/sync)
//!
//! 基于 `kernel::sync::mutex::Mutex` 的 TCB 安全包装。
//! 锁竞争时让出 CPU 而非忙等待, 适用于长临界区。
//!
//! ## 适用场景
//!
//! ✅ 临界区可能较长 (> 10μs)
//! ✅ 持有锁期间可能 sleep / 阻塞
//! ❌ 中断上下文 (必须可用 SpinLock)
//!
//! ## SAFETY 不变量
//!
//! - `lock()` 返回 `MutexGuard`, drop 时自动释放。
//! - Mutex 可递归: 同一线程可多次 lock。
//! - 不可在中断上下文使用。

use core::ops::{Deref, DerefMut};

use crate::kernel::sync::mutex::Mutex as InnerMutex;

/// TCB 睡眠互斥锁。
pub struct Mutex<T> {
    inner: InnerMutex<T>,
}

impl<T> Mutex<T> {
    /// 创建新的互斥锁, 包装初始数据。
    pub fn new(data: T) -> Self {
        Self {
            inner: InnerMutex::new(data),
        }
    }

    /// 获取锁并返回 RAII Guard。
    ///
    /// 若锁被其他线程持有, 当前线程让出 CPU。
    /// 若锁已被当前线程持有, 计数器递增 (递归)。
    ///
    /// # 安全约束
    /// 不可在中断上下文调用。
    pub fn lock(&self) -> MutexGuard<'_, T> {
        MutexGuard {
            guard: self.inner.lock(),
        }
    }

    /// 尝试获取锁, 不阻塞。
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.inner.try_lock().map(|guard| MutexGuard { guard })
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// 互斥锁 RAII Guard — drop 时自动释放。
pub struct MutexGuard<'a, T> {
    guard: crate::kernel::sync::types::MutexGuard<'a, T>,
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard.data
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.data
    }
}

// SAFETY: Mutex 内部 Mutex 已实现 Send + Sync。
unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}
