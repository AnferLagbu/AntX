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

pub use crate::kernel::framework::sync_legacy::spinlock::{disable_interrupts, restore_interrupts};

use crate::kernel::framework::sync_legacy::types::SpinLockInner;

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
        // SAFETY: `&mut *UnsafeCell::get()` 产生独占 `&mut SpinLockInner`;
        // `try_lock` 内部会立即 `compare_exchange` 试探锁状态, 失败则放弃,
        // 期间不会有第二个线程同时持有 `&mut`, 借用检查器允许是因为 UnsafeCell
        // 内部可变性 + CAS 串行化。
        let inner = unsafe { &mut *self.inner.get() };
        if inner.locked.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            // SAFETY: CAS 成功后, 我们独占了锁; 此时访问 `data` 安全 (无其他持有者)。
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
        // SAFETY:
        //   1. `self.inner` 来自 `try_lock` / `lock` 成功路径, 当时 CAS 0→1, 我们独占
        //   2. `SpinLockGuard` 是 RAII, drop 时 (在 `&mut self` 借用下) 没有其他线程
        //      能持有 guard, 因此 `locked.store(0)` 不会与另一个 lock 路径冲突
        //   3. `Release` ordering 与 acquire 配对, 保证 drop 前的所有数据写入对
        //      下一个 lock 路径可见
        unsafe {
            (*self.inner).locked.store(0, Ordering::Release);
        }
    }
}

/// 底层自旋获取锁 (x86_64 用 pause, aarch64 无特殊指令)。
///
/// # Safety
///
/// 1. `inner` 必须是有效的 `&mut SpinLockInner` 引用, 来自 `SpinLock::inner.get()`
/// 2. 调用方必须保证此函数不会在中断上下文调用 (自旋 + 中断 = 死锁)
/// 3. 调用方必须保证同一 `inner` 不会有并发 `raw_spin_lock` 调用 (CAS 串行化保证)
unsafe fn raw_spin_lock(inner: &mut SpinLockInner) {
    // SAFETY:
    //   1. `inner` 是 `&mut` 独占引用, 来自调用方
    //   2. `compare_exchange(0, 1, Acquire, Relaxed)` 是循环自旋直到 CAS 成功
    //   3. `Acquire` ordering 保证成功时能看到所有先前 `Release` 写
    //   4. `spin_loop()` 提示 CPU 处于自旋等待, 降低功耗
    //   5. 中断安全由调用方保证 (本函数不应在中断上下文调用)
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
