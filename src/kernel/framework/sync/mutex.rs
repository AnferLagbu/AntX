//! # 睡眠锁 (Mutex) 实现
//!
//! 基于等待队列和调度器的睡眠锁。
//!
//! ## 与 SpinLock 的区别
//!
//! | 特性 | SpinLock | Mutex |
//! |------|----------|--------|
//! | 等待方式 | 忙等待 (CPU) | 让出 CPU (scheduler_yield) |
//! | 适用场景 | 极短临界区 | 长时间持有锁 |
//! | 中断上下文 | ✅ 可用 | ❌ 不可用 (会 sleep) |
//! | 开销 | 低 (无系统调用) | 高 (涉及调度) |
//!
//! # 设计特性
//!
//! - **递归锁定支持**: 同一线程可多次 lock
//! - **公平性**: 竞争时让出 CPU，避免饥饿
//! - **调试信息**: 记录持有者 PID 和获取时间

use core::sync::atomic::Ordering;

use super::types::*;
#[cfg(debug_assertions)]
use super::lockdep::{self, LockClassId, LockClassDesc, LockKind};

/// 睡眠锁 (Mutex)
///
/// 当锁被持有时，后续的 lock() 调用会让出 CPU，
/// 允许调度器选择其他就绪进程运行。
pub struct Mutex<T: ?Sized> {
    /// 内部状态
    inner: MutexInner,
    /// Lockdep 锁类 ID (debug 模式下使用)
    #[cfg(debug_assertions)]
    lockdep_class: LockClassId,
    /// 被保护的数据 (必须为最后一项, 以支持 ?Sized)
    data: core::cell::UnsafeCell<T>,
}

// SAFETY: Mutex provides mutual exclusion through its internal spinlock.
// UnsafeCell provides interior mutability; access to T is gated by lock
// acquisition. T: Send is required because ownership can transfer between
// threads via lock/unlock. T: Sync is guaranteed because only one thread
// can hold the lock at a time, providing exclusive access.
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// 创建新的 Mutex
    pub fn new(data: T) -> Self {
        Self {
            inner: MutexInner::default(),
            data: core::cell::UnsafeCell::new(data),
            #[cfg(debug_assertions)]
            lockdep_class: LockClassId::INVALID,
        }
    }

    /// 创建命名 Mutex (用于调试 + lockdep)
    #[cfg(debug_assertions)]
    pub fn named(name: &'static str, data: T) -> Self {
        let class_id = lockdep::register_class(LockClassDesc {
            name,
            kind: LockKind::Mutex,
        });
        Self {
            inner: MutexInner::default(),
            data: core::cell::UnsafeCell::new(data),
            lockdep_class: class_id,
        }
    }

    /// 创建命名 Mutex (release 模式: 忽略名称)
    #[cfg(not(debug_assertions))]
    pub fn named(_name: &'static str, data: T) -> Self {
        Self::new(data)
    }

    /// 获取内部数据的不可变引用
    ///
    /// # Safety
    /// 调用者必须确保已获取锁或通过其他方式保证安全
    pub unsafe fn get(&self) -> &T {
        unsafe { &*self.data.get() }
    }

    /// 获取内部数据的可变引用
    ///
    /// # Safety
    /// 调用者必须已持有该锁
    pub unsafe fn get_mut(&self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }

    /// 获取锁 (阻塞)
    ///
    /// 如果锁已被持有，当前线程会**让出 CPU**，
    /// 直到锁被释放后重新尝试获取。
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.raw_lock();

        // 创建守卫 (RAII)
        MutexGuard {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            data: unsafe { &mut *self.data.get() },
            _mutex: &self.inner,
        }
    }

    /// 尝试获取锁 (非阻塞)
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if self.raw_trylock() {
            Some(MutexGuard {
                // SAFETY: `self` 由调用方保证为有效指针; 只读访问
                data: unsafe { &mut *self.data.get() },
                _mutex: &self.inner,
            })
        } else {
            None
        }
    }

    /// 带超时的锁获取
    ///
    /// # Arguments
    /// * `timeout_ms` - 超时时间 (毫秒), 0 = 无限等待
    ///
    /// # Returns
    /// - `Some(guard)`: 成功获取锁
    /// - `None`: 超时
    pub fn lock_timeout(&self, timeout_ms: u32) -> Option<MutexGuard<'_, T>> {
        if timeout_ms == 0 {
            return Some(self.lock());
        }

        let start = rdtsc();
        let timeout_cycles = (timeout_ms as u64) * 2400000; // 近似转换

        loop {
            if self.raw_trylock() {
                return Some(MutexGuard {
                    // SAFETY: `self` 由调用方保证为有效指针; 只读访问
                    data: unsafe { &mut *self.data.get() },
                    _mutex: &self.inner,
                });
            }

            // 检查超时
            let now = rdtsc();
            if now.saturating_sub(start) > timeout_cycles {
                return None;
            }

            // 让出 CPU
            scheduler_yield();
        }
    }

    // ========================================================================
    // 底层原始操作 (供 FFI 使用)
    // ========================================================================

    /// 原始锁获取 (不返回 Guard)
    fn raw_lock(&self) {
        // Fast path: 尝试立即获取
        {
            // 先获取内部自旋锁
            self.inner.inner_spinlock.raw_lock();

            if !self.is_locked_internal() {
                // 成功获取
                self.acquire_lock_internal();
                self.inner.inner_spinlock.raw_unlock();
                return;
            }

            #[cfg(feature = "debug_mutex")]
            log::warn!("MUTEX: lock contention detected");

            self.inner.inner_spinlock.raw_unlock();
        }

        // Slow path: 自旋 + yield
        loop {
            // 检查是否可用
            self.inner.inner_spinlock.raw_lock();

            if !self.is_locked_internal() {
                self.acquire_lock_internal();
                self.inner.inner_spinlock.raw_unlock();
                return;
            }

            self.inner.inner_spinlock.raw_unlock();

            // 让出 CPU 给其他进程
            scheduler_yield();
        }
    }

    /// 原始尝试获取锁
    fn raw_trylock(&self) -> bool {
        self.inner.inner_spinlock.raw_lock();

        if !self.is_locked_internal() {
            self.acquire_lock_internal();
            self.inner.inner_spinlock.raw_unlock();
            true
        } else {
            self.inner.inner_spinlock.raw_unlock();
            false
        }
    }

    /// 原始释放锁
    fn raw_unlock(&self) {
        // Lockdep: 通知锁释放
        #[cfg(debug_assertions)]
        lockdep::release(self.lockdep_class);

        self.inner.inner_spinlock.raw_lock();

        let depth = self.inner.depth.fetch_sub(1, Ordering::AcqRel);

        if depth <= 1 {
            // 完全释放
            self.inner.locked.store(0, Ordering::Release);
            self.inner.owner.store(-1, Ordering::Release);
            self.inner.acquire_time.store(0, Ordering::Release);
        }

        self.inner.inner_spinlock.raw_unlock();
    }

    // ========================================================================
    // 内部辅助函数
    // ========================================================================

    fn is_locked_internal(&self) -> bool {
        self.inner.locked.load(Ordering::Acquire) != 0
    }

    fn acquire_lock_internal(&self) {
        self.inner.locked.store(1, Ordering::Release);

        // 设置持有者 PID (从 C 函数获取)
        extern "C" {
            fn process_get_current_pid() -> u32;
        }
        // SAFETY: `process_get_current_pid` 是有效的 C ABI 函数指针; 参数列表与声明一致
        let pid = unsafe { process_get_current_pid() };
        self.inner.owner.store(pid as i32, Ordering::Release);

        // 重置递归深度
        self.inner.depth.store(1, Ordering::Release);

        // 记录获取时间
        self.inner.acquire_time.store(rdtsc(), Ordering::Release);

        // Lockdep: 通知锁获取
        #[cfg(debug_assertions)]
        lockdep::acquire(self.lockdep_class, lockdep::in_irq_context());
    }

    /// 检查锁是否被持有
    pub fn is_locked(&self) -> bool {
        self.inner.locked.load(Ordering::Acquire) != 0
    }

    /// 获取当前持有者 PID (-1 = 未持有)
    pub fn owner(&self) -> i32 {
        self.inner.owner.load(Ordering::Acquire)
    }

    /// 获取递归深度
    pub fn depth(&self) -> u32 {
        self.inner.depth.load(Ordering::Acquire)
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

// ============================================================================
// 条件变量 (CondVar)
// ============================================================================

/// 条件变量
///
/// 用于线程间通知机制，通常配合 Mutex 使用。
///
/// # Example
/// ```rust,ignore
/// let mutex = Mutex::new(false);
/// let cond = CondVar::new();
///
/// // Producer:
/// mutex.lock();
/// *mutex.get_mut() = true;
/// cond.signal(&mutex);  // 通知一个等待者
///
/// // Consumer:
/// mutex.lock();
/// while !*unsafe { mutex.get() } {
///     cond.wait(&mutex);  // 释放锁并等待
/// }
/// ```
pub struct CondVar {
    waiters: core::sync::atomic::AtomicU32,
}

impl CondVar {
    pub const fn new() -> Self {
        CondVar {
            waiters: core::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn wait<T>(&self, mutex: &Mutex<T>) {
        self.waiters.fetch_add(1, Ordering::AcqRel);
        mutex.raw_unlock();
        scheduler_yield();
        mutex.lock();
        self.waiters.fetch_sub(1, Ordering::AcqRel);
    }

    pub fn wait_timeout<T>(&self, mutex: &Mutex<T>, timeout_ms: u32) -> bool {
        self.waiters.fetch_add(1, Ordering::AcqRel);
        mutex.raw_unlock();

        extern "C" {
            fn timer_sleep_busy(ms: u64);
        }
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            timer_sleep_busy(timeout_ms as u64);
        }

        mutex.lock();
        self.waiters.fetch_sub(1, Ordering::AcqRel);
        true
    }

    pub fn signal(&self) {
        if self.waiters.load(Ordering::Acquire) > 0 {
            scheduler_yield();
        }
    }

    pub fn broadcast(&self) {
        let count = self.waiters.load(Ordering::Acquire);
        if count > 0 {
            for _ in 0..count {
                scheduler_yield();
            }
        }
    }
}

impl Default for CondVar {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 读取 TSC 时间戳计数器 (架构无关封装)
fn rdtsc() -> u64 {
    crate::arch!(timestamp())
}

/// 让出 CPU 给调度器
fn scheduler_yield() {
    extern "C" {
        fn scheduler_yield();
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        scheduler_yield();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutex_basic() {
        let mutex = Mutex::new(42i32);
        assert!(!mutex.is_locked());

        {
            let guard = mutex.lock();
            assert!(mutex.is_locked());
            assert_eq!(*guard, 42);
        } // ← 自动 unlock

        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_mutex_trylock() {
        let mutex = Mutex::new(100u32);

        let guard1 = mutex.try_lock().expect("first try should succeed");
        assert!(mutex.is_locked());
        assert_eq!(*guard1, 100);

        // 第二次尝试应失败
        assert!(mutex.trylock().is_none());

        drop(guard1);
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_condvar_creation() {
        let cond = CondVar::new();
        let _ = cond; // 仅验证可以创建
    }
}

#[cfg(feature = "kernel_test")]
pub fn register_mutex_tests() {
    crate::kernel::framework::tests::sync::register_mutex_tests();
}
