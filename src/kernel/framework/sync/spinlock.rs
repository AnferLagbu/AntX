//! SpinLock — TCB 自旋锁 (framework/sync)
//!
//! 基于底层 `xchg` 原子操作的忙等待锁,
//! 通过 `UnsafeCell` 实现内部可变性。
//!
//! ## 适用场景
//!
//! ✅ 临界区 < 1μs
//! ✅ 中断上下文
//! ❌ 不可 sleep (用 Mutex)
//!
//! ## SAFETY 不变量
//!
//! - `lock()` 返回的 `SpinLockGuard` 在 drop 时自动释放锁。
//! - 锁持有的 `T` 在 guard 生命周期内独占访问。

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{fence, Ordering};

pub use crate::kernel::sync::spinlock::{disable_interrupts, restore_interrupts};

use crate::kernel::sync::types::SpinLockInner;

/// TCB 自旋锁。
///
/// 基于 x86_64 `xchg` / aarch64 `ldaxr`+`stlxr` 指令实现。
/// 自旋循环中插入 `pause` (x86) 或 `yield` (aarch64) 优化。
pub struct SpinLock<T> {
    data: UnsafeCell<T>,
    inner: UnsafeCell<SpinLockInner>,
}

impl<T> SpinLock<T> {
    /// 创建新的自旋锁, 包装初始数据。
    pub fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            inner: UnsafeCell::new(SpinLockInner::default()),
        }
    }

    /// 获取锁并返回 RAII Guard。
    ///
    /// 自旋等待直到锁可用。Guard 在 drop 时自动释放。
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        // SAFETY: UnsafeCell gives &mut to inner. The xchg loop provides
        // mutual exclusion. We hold the lock until guard drop.
        let inner = unsafe { &mut *self.inner.get() };
        unsafe { raw_spin_lock(inner); }
        let data = unsafe { &mut *self.data.get() };
        SpinLockGuard {
            data,
            inner: inner as *const SpinLockInner,
        }
    }

    /// 尝试获取锁, 不阻塞。
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        let inner = unsafe { &mut *self.inner.get() };
        if inner.locked.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            let data = unsafe { &mut *self.data.get() };
            Some(SpinLockGuard {
                data,
                inner: inner as *const SpinLockInner,
            })
        } else {
            None
        }
    }
}

impl<T: Default> Default for SpinLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// 自旋锁 RAII Guard — drop 时自动释放锁。
pub struct SpinLockGuard<'a, T> {
    data: &'a mut T,
    inner: *const SpinLockInner,
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.data
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.data
    }
}

impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        fence(Ordering::SeqCst);
        // SAFETY: inner was locked by us, now releasing.
        unsafe {
            (*self.inner).locked.store(0, Ordering::Release);
        }
    }
}

/// 底层自旋获取锁 (x86_64 用 pause, aarch64 无特殊指令)。
unsafe fn raw_spin_lock(inner: &mut SpinLockInner) {
    loop {
        if inner.locked.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            break;
        }
        #[cfg(target_arch = "x86_64")]
        core::hint::spin_loop();
        #[cfg(not(target_arch = "x86_64"))]
        core::hint::spin_loop();
    }
}

// SAFETY: UnsafeCell + atomic 操作保证线程安全。
unsafe impl<T: Send> Send for SpinLock<T> {}
unsafe impl<T: Send> Sync for SpinLock<T> {}
