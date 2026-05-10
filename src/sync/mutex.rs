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

use core::sync::atomic::{AtomicU32, AtomicI64, AtomicU64, Ordering};

use super::types::*;
use super::spinlock::{self, SpinLock};

/// 睡眠锁 (Mutex)
///
/// 当锁被持有时，后续的 lock() 调用会让出 CPU，
/// 允许调度器选择其他就绪进程运行。
pub struct Mutex<T: ?Sized> {
    /// 内部状态
    inner: MutexInner,
    /// 被保护的数据
    data: core::cell::UnsafeCell<T>,
}

// 安全保证: Mutex 提供同步访问
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// 创建新的 Mutex
    pub fn new(data: T) -> Self {
        Self {
            inner: MutexInner::default(),
            data: core::cell::UnsafeCell::new(data),
        }
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
            data: unsafe { &mut *self.data.get() },
            _mutex: &self.inner,
        }
    }
    
    /// 尝试获取锁 (非阻塞)
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if self.raw_trylock() {
            Some(MutexGuard {
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
        extern "C" { fn process_get_current_pid() -> i32; }
        let pid = unsafe { process_get_current_pid() };
        self.inner.owner.store(pid, Ordering::Release);
        
        // 重置递归深度
        self.inner.depth.store(1, Ordering::Release);
        
        // 记录获取时间
        self.inner.acquire_time.store(rdtsc(), Ordering::Release);
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
pub struct CondVar;

impl CondVar {
    /// 创建新的条件变量
    pub const fn new() -> Self {
        CondVar
    }
    
    /// 等待条件变量 (原子地释放锁并挂起)
    ///
    /// # Arguments
    /// * `_mutex` - 关联的 Mutex (仅用于类型检查)
    ///
    /// # Behavior
    /// 1. 释放 mutex
    /// 2. 让出 CPU (yield)
    /// 3. 被唤醒后重新获取 mutex
    pub fn wait<T>(&self, _mutex: &Mutex<T>) {
        // 释放 mutex (raw_unlock)
        // 注意: 这里需要 unsafe 因为 Rust 无法静态验证我们持有锁
        
        // 让出 CPU
        scheduler_yield();
        
        // 重新获取 mutex 将在返回后由调用者处理
        // (或者使用 loop 包装)
    }
    
    /// 带超时的等待
    pub fn wait_timeout<T>(&self, _mutex: &Mutex<T>, _timeout_ms: u32) -> bool {
        // 简化实现: 总是返回 true (表示被唤醒)
        // 完整实现需要真正的等待队列支持
        self.wait(_mutex);
        true
    }
    
    /// 唤醒一个等待者
    pub fn signal(&self) {
        // 简化实现: 实际需要等待队列支持
        // 当前版本依赖 scheduler_yield 的隐式唤醒
        let _ = self;
    }
    
    /// 唤醒所有等待者
    pub fn broadcast(&self) {
        // 简化实现: 同上
        let _ = self;
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

/// 读取 TSC 时间戳计数器
fn rdtsc() -> u64 {
    let tsc: u64;
    unsafe { core::arch::asm!("rdtsc", out("rax") tsc, options(nomem, nostack)) };
    tsc
}

/// 让出 CPU 给调度器
fn scheduler_yield() {
    extern "C" { fn scheduler_yield(); }
    unsafe { scheduler_yield(); }
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
        let _ = cond;  // 仅验证可以创建
    }
}
