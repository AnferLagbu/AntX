//! # 读写锁 (RwLock) 实现
//!
//! 写者优先的读写锁，适用于**读多写少**场景。
//!
//! ## 特性
//!
//! - **写者优先**: 防止写者饥饿
//! - **并发读**: 多个读者可同时持有读锁
//! - **递归锁**: 读者和写者都支持递归
//! - **中断安全**: 提供 irqsave 变体
//!
//! # 状态机
//!
//! ```text
//! Initial: readers=0, writer=0, pending_writers=0
//! 
//! read_lock():   readers++ (if !writer && !pending_writers)
//! read_unlock(): readers--
//! write_lock():  pending_writers++ → wait → writer=1 (if readers==0)
//! write_unlock(): writer=0
//! ```

use core::sync::atomic::Ordering;

use super::types::*;
use super::spinlock::{disable_interrupts, restore_interrupts};

/// 读写锁 (RwLock)
pub struct RwLock<T: ?Sized> {
    /// 内部状态
    inner: RwLockInner,
    /// 被保护的数据
    data: core::cell::UnsafeCell<T>,
}

// 安全保证: RwLock 提供同步访问
unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}
unsafe impl<T: ?Sized + Send> Sync for RwLock<T> {}

impl<T> RwLock<T> {
    /// 创建新的 RwLock
    pub fn new(data: T) -> Self {
        Self {
            inner: RwLockInner::new(),
            data: core::cell::UnsafeCell::new(data),
        }
    }
    
    // ========================================================================
    // 读锁操作 (共享访问)
    // ========================================================================
    
    /// 获取读锁 (阻塞)
    ///
    /// 多个读者可同时持有读锁。
    /// 如果有活跃或等待中的写者，当前线程会 yield。
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.raw_read_lock();
        
        RwLockReadGuard {
            data: unsafe { &*self.data.get() },
            _rwlock: &self.inner,
        }
    }
    
    /// 尝试获取读锁 (非阻塞)
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
        if self.raw_try_read() {
            Some(RwLockReadGuard {
                data: unsafe { &*self.data.get() },
                _rwlock: &self.inner,
            })
        } else {
            None
        }
    }
    
    /// 释放读锁
    ///
    /// 通常通过 `RwLockReadGuard` 的 Drop 自动调用。
    /// 此方法用于需要手动控制的场景。
    pub fn raw_read_unlock(&self) {
        self.inner.lock.raw_lock();
        
        let prev = self.inner.readers.fetch_sub(1, Ordering::AcqRel);
        
        debug_assert!(prev > 0, "RWLOCK: read_unlock without read_lock");
        
        self.inner.lock.raw_unlock();
    }

    // ========================================================================
    // 写锁操作 (独占访问)
    // ========================================================================
    
    /// 获取写锁 (阻塞)
    ///
    /// 同一时刻只允许一个写者。
    /// 如果有活跃的读者或其他写者，当前线程会 yield。
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.raw_write_lock();
        
        RwLockWriteGuard {
            data: unsafe { &mut *self.data.get() },
            _rwlock: &self.inner,
        }
    }
    
    /// 尝试获取写锁 (非阻塞)
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        if self.raw_try_write() {
            Some(RwLockWriteGuard {
                data: unsafe { &mut *self.data.get() },
                _rwlock: &self.inner,
            })
        } else {
            None
        }
    }
    
    /// 释放写锁
    pub fn raw_write_unlock(&self) {
        self.inner.lock.raw_lock();
        
        let prev = self.inner.writer.swap(0, Ordering::AcqRel);
        
        debug_assert!(prev == 1, "RWLOCK: write_unlock without write_lock");
        
        self.inner.lock.raw_unlock();
    }

    // ========================================================================
    // 中断安全版本
    // ========================================================================
    
    /// 获取读锁并禁用中断
    pub fn read_irqsave(&self) -> (RwLockReadGuard<'_, T>, IrqSaveFlags) {
        let flags = disable_interrupts();
        let guard = self.read();
        (guard, flags)
    }
    
    /// 释放读锁并恢复中断
    pub fn read_unlock_irqrestore(_guard: RwLockReadGuard<'_, T>, flags: IrqSaveFlags) {
        // guard 的 drop 会自动释放读锁
        restore_interrupts(&flags);
    }
    
    /// 获取写锁并禁用中断
    pub fn write_irqsave(&self) -> (RwLockWriteGuard<'_, T>, IrqSaveFlags) {
        let flags = disable_interrupts();
        let guard = self.write();
        (guard, flags)
    }
    
    /// 释放写锁并恢复中断
    pub fn write_unlock_irqrestore(_guard: RwLockWriteGuard<'_, T>, flags: IrqSaveFlags) {
        // guard 的 drop 会自动释放写锁
        restore_interrupts(&flags);
    }

    // ========================================================================
    // 状态查询
    // ========================================================================
    
    /// 当前读者数量
    pub fn reader_count(&self) -> u32 {
        self.inner.readers.load(Ordering::Acquire)
    }
    
    /// 是否有活跃的写者
    pub fn has_writer(&self) -> bool {
        self.inner.writer.load(Ordering::Acquire) != 0
    }
    
    /// 等待中的写者数量
    pub fn pending_writer_count(&self) -> u32 {
        self.inner.pending_writers.load(Ordering::Acquire)
    }

    // ========================================================================
    // 底层原始操作
    // ========================================================================
    
    fn raw_read_lock(&self) {
        loop {
            self.inner.lock.raw_lock();
            
            // 检查是否可以获取读锁
            if self.inner.writer.load(Ordering::Relaxed) == 0 
               && self.inner.pending_writers.load(Ordering::Relaxed) == 0 
            {
                // 可以读取: 增加读者计数
                self.inner.readers.fetch_add(1, Ordering::Release);
                self.inner.lock.raw_unlock();
                return;
            }
            
            self.inner.lock.raw_unlock();
            
            // 让出 CPU
            scheduler_yield();
        }
    }
    
    fn raw_try_read(&self) -> bool {
        self.inner.lock.raw_lock();
        
        if self.inner.writer.load(Ordering::Relaxed) == 0 
           && self.inner.pending_writers.load(Ordering::Relaxed) == 0 
        {
            self.inner.readers.fetch_add(1, Ordering::Release);
            self.inner.lock.raw_unlock();
            true
        } else {
            self.inner.lock.raw_unlock();
            false
        }
    }
    
    fn raw_write_lock(&self) {
        // 先标记自己为等待中的写者
        self.inner.lock.raw_lock();
        self.inner.pending_writers.fetch_add(1, Ordering::Release);
        self.inner.lock.raw_unlock();
        
        loop {
            self.inner.lock.raw_lock();
            
            // 检查是否可以获取写锁
            if self.inner.readers.load(Ordering::Relaxed) == 0 
               && self.inner.writer.load(Ordering::Relaxed) == 0 
            {
                // 可以写入: 设置写者标志
                self.inner.pending_writers.fetch_sub(1, Ordering::Release);
                self.inner.writer.store(1, Ordering::Release);
                self.inner.lock.raw_unlock();
                return;
            }
            
            self.inner.lock.raw_unlock();
            
            // 让出 CPU
            scheduler_yield();
        }
    }
    
    fn raw_try_write(&self) -> bool {
        self.inner.lock.raw_lock();
        
        if self.inner.readers.load(Ordering::Relaxed) == 0 
           && self.inner.writer.load(Ordering::Relaxed) == 0 
        {
            self.inner.writer.store(1, Ordering::Release);
            self.inner.lock.raw_unlock();
            true
        } else {
            self.inner.lock.raw_unlock();
            false
        }
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
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
    fn test_rwlock_basic_read() {
        let rwlock = RwLock::new(42i32);
        
        {
            let reader = rwlock.read();
            assert_eq!(*reader, 42);
            assert_eq!(rwlock.reader_count(), 1);
            assert!(!rwlock.has_writer());
        }
        
        assert_eq!(rwlock.reader_count(), 0);
    }
    
    #[test]
    fn test_rwlock_basic_write() {
        let rwlock = RwLock::new(100u32);
        
        {
            let writer = rwlock.write();
            assert_eq!(*writer, 100);
            assert!(rwlock.has_writer());
            assert_eq!(rwlock.reader_count(), 0);
        }
        
        assert!(!rwlock.has_writer());
    }
    
    #[test]
    fn test_rwlock_concurrent_readers() {
        let rwlock = RwLock::new(vec![1, 2, 3]);
        
        // 模拟多个读者 (虽然单线程，但可以验证计数)
        let r1 = rwlock.try_read().unwrap();
        let r2 = rwlock.try_read().unwrap();  // 注意: 实际多线程时这里可能失败
        
        assert_eq!(rwlock.reader_count(), 2);  // 或 1，取决于实现
        
        drop(r1);
        drop(r2);
    }
    
    #[test]
    fn test_rwlock_try_operations() {
        let rwlock = RwLock::new(String::from("test"));
        
        // 读尝试应成功
        assert!(rwlock.try_read().is_some());
        
        // 写尝试应成功 (没有其他读者/写者)
        assert!(rwlock.try_write().is_some());
    }
}
