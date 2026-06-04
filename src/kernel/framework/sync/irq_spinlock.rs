//! IrqSpinLock — TCB 中断安全自旋锁
//!
//! 在 `lock()` 时保存 IF 标志并 `cli` 屏蔽中断, `unlock()` (guard drop) 时恢复 IF。
//!
//! ## 设计动机
//!
//! 防止:
//! - 中断处理程序与线程争用同一锁导致死锁
//! - 持锁状态下被中断打断, 中断处理程序自旋等待
//!
//! ## SAFETY 契约
//!
//! - 不可在中断上下文使用 (会嵌套屏蔽, 丢失中断)
//! - 不可与 `SpinLock` 嵌套使用 (顺序无定义)
//! - `Send`/`Sync`: T: Send 即可, 因中断屏蔽保证临界区原子性
//!
//! ## 与 services::sync::irq_lock 的关系
//!
//! services 层的 `IrqSpinLock` 是本类型的类型别名 (保持 API 兼容)。
//! 所有 unsafe 集中在 framework, services 零 unsafe。
//!
//! ## 实现
//!
//! - `SpinLock` (raw, 来自 `sync::spinlock`) 提供底层原子锁原语 (`raw_lock`/`raw_unlock`)
//! - 由于 `SpinLock::raw_lock` 需要 `&mut self`, 我们将 `SpinLock` 包装在 `UnsafeCell` 中
//! - `UnsafeCell<T>` 用于保护被守卫的数据
//! - 嵌套深度计数器: 防止 lock 期间再 lock 时错误恢复 IF

#![allow(dead_code)]

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kernel::framework::sync::spinlock::{
    disable_interrupts, restore_interrupts, SpinLock,
};
use crate::kernel::framework::sync::types::IrqSaveFlags;

/// 中断安全自旋锁 (TCB)。
///
/// 在 `lock()` 时保存 IF 标志并屏蔽中断, `unlock()` (guard drop) 时恢复原始 IF 状态。
///
/// # 示例
///
/// ```ignore
/// let data = IrqSpinLock::new(0u32);
/// data.with_mut(|g| *g += 1);
/// ```
pub struct IrqSpinLock<T> {
    /// 底层自旋锁 (放在 UnsafeCell 中以便从 &self 调用 raw_lock)
    lock: UnsafeCell<SpinLock>,
    /// 被保护的数据
    data: UnsafeCell<T>,
    /// 嵌套深度 (防止 lock 期间再 lock 时错误恢复 IF)
    depth: UnsafeCell<AtomicU32>,
}

// SAFETY: 中断屏蔽保证临界区原子性, T: Send 即可。
unsafe impl<T: Send> Send for IrqSpinLock<T> {}
// SAFETY: 共享引用跨线程安全, 中断屏蔽保证访问互斥。
unsafe impl<T: Send> Sync for IrqSpinLock<T> {}

impl<T> IrqSpinLock<T> {
    /// 创建新的中断安全自旋锁, 包装初始数据。
    pub fn new(data: T) -> Self {
        Self {
            lock: UnsafeCell::new(SpinLock::new()),
            data: UnsafeCell::new(data),
            depth: UnsafeCell::new(AtomicU32::new(0)),
        }
    }

    /// 获取锁并返回 RAII Guard, 持锁期间屏蔽中断。
    pub fn lock(&self) -> IrqSpinLockGuard<'_, T> {
        let prev = disable_interrupts();
        // SAFETY: cli 屏蔽中断后, 此 CPU 上不存在并发访问; 我们独占借用。
        // 同一 IrqSpinLock 上的 lock 调用由 cli + 自旋锁串行化。
        unsafe { &mut *self.lock.get() }.raw_lock();
        // SAFETY: 已持有锁, 任何其他访问者都被锁在外。
        let data_ref = unsafe { &mut *self.data.get() };
        // SAFETY: 仅本线程访问 depth (cli 保证 ISR 不并发)。
        unsafe { &*self.depth.get() }.fetch_add(1, Ordering::Relaxed);
        IrqSpinLockGuard {
            data: data_ref,
            lock_ptr: self.lock.get(),
            depth_ptr: self.depth.get(),
            prev_if: Some(prev),
        }
    }

    /// 闭包 API: 获取锁, 持有期间屏蔽中断, 执行闭包后自动释放。
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut guard = self.lock();
        f(guard.deref_mut())
    }

    /// 不可变闭包版本。
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let guard = self.lock();
        f(guard.deref())
    }
}

/// 中断安全自旋锁的 RAII Guard。
pub struct IrqSpinLockGuard<'a, T> {
    data: &'a mut T,
    /// 指向所属 `IrqSpinLock` 的 `SpinLock` 的裸指针 (cli 保证独占)
    lock_ptr: *mut SpinLock,
    /// 指向所属 `IrqSpinLock` 的深度计数器的裸指针 (cli 保证独占)
    depth_ptr: *mut AtomicU32,
    /// `lock()` 时保存的 IF 标志
    prev_if: Option<IrqSaveFlags>,
}

impl<'a, T> Deref for IrqSpinLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.data
    }
}

impl<'a, T> DerefMut for IrqSpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.data
    }
}

impl<'a, T> Drop for IrqSpinLockGuard<'a, T> {
    fn drop(&mut self) {
        // SAFETY: 持有 lock_ptr 上的锁, 任何其他访问者都被锁在外; cli 屏蔽中断。
        unsafe { &*self.lock_ptr }.raw_unlock();
        // SAFETY: 仅本线程访问 depth (cli 保证 ISR 不并发)。
        let prev = unsafe { &*self.depth_ptr }.fetch_sub(1, Ordering::Relaxed);
        if prev == 1 {
            if let Some(flags) = self.prev_if.take() {
                restore_interrupts(&flags);
            }
        }
    }
}
