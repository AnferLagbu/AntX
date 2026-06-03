//! `RacyCell<T>` — 跨线程无锁全局容器
//!
//! 与内核 `kernel/framework/racy_cell.rs` 行为等价,
//! 这里是 `std` 版用于 Miri 验证数据竞争场景。

use std::cell::UnsafeCell;

/// 无锁可变全局容器 (Miri 验证版)
///
/// # Safety
///
/// 调用方必须通过外部同步或 per-CPU 亲和性保证无数据竞争。
#[derive(Debug)]
pub struct RacyCell<T> {
    inner: UnsafeCell<T>,
}

// SAFETY: 调用方通过外部同步保证 Sync 安全。
unsafe impl<T: Send + Sync> Sync for RacyCell<T> {}
unsafe impl<T: Send> Send for RacyCell<T> {}

impl<T> RacyCell<T> {
    pub const fn new(val: T) -> Self {
        Self { inner: UnsafeCell::new(val) }
    }

    /// # Safety
    /// 调用方必须保证独占访问。
    pub unsafe fn get(&self) -> &T {
        &*self.inner.get()
    }

    /// 通过闭包只读访问 (推荐 API)
    pub fn map<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        // SAFETY: &T 是只读借用, 无并发写 (调用方保证)。
        f(unsafe { &*self.inner.get() })
    }

    /// 通过闭包修改 (推荐 API)
    ///
    /// # Safety
    /// 调用方必须保证独占写入。
    pub unsafe fn modify<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        f(&mut *self.inner.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn racy_cell_basic() {
        let cell = RacyCell::new(42u32);
        // SAFETY: 单线程测试, 无并发。
        let val = unsafe { *cell.get() };
        assert_eq!(val, 42);
    }

    #[test]
    fn racy_cell_map() {
        let cell = RacyCell::new(vec![1, 2, 3]);
        let sum = cell.map(|v| v.iter().sum::<i32>());
        assert_eq!(sum, 6);
    }

    #[test]
    fn racy_cell_modify() {
        let cell = RacyCell::new(0u32);
        // SAFETY: 单线程测试, 独占访问。
        unsafe { cell.modify(|v| *v += 10) };
        // SAFETY: 同上
        let v = unsafe { *cell.get() };
        assert_eq!(v, 10);
    }

    /// 验证 RacyCell 在静态上下文中可用
    static GLOBAL: RacyCell<u64> = RacyCell::new(0);

    #[test]
    fn racy_cell_static() {
        // SAFETY: 单线程测试。
        unsafe { GLOBAL.modify(|v| *v += 1) };
        // SAFETY: 同上
        let v = unsafe { *GLOBAL.get() };
        assert_eq!(v, 1);
    }
}
