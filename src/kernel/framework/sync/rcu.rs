//! RCU — TCB 读-复制-更新 (framework/sync)
//!
//! 读多写少场景的零开销读者锁。读者无原子操作,
//! 写者等待宽限期结束后释放旧数据。
//!
//! ## 适用场景
//!
//! ✅ 极高读频率 (> 99% reads)
//! ✅ 写者罕见到达
//! ❌ 写者频繁 (宽限期开销大)
//!
//! ## SAFETY 不变量
//!
//! - `rcu_read_lock()` / `rcu_read_unlock()` 必须嵌套配对。
//! - `rcu_dereference()` 必须在读临界区内调用。
//! - `synchronize_rcu()` 阻塞直到所有 CPU 宽限期结束。
//! - 旧数据释放必须推迟到宽限期后。

use core::sync::atomic::{fence, Ordering};

/// 标记 RCU 读临界区开始。
///
/// # SAFETY
/// 必须配对 `rcu_read_unlock()`。嵌套深度由 per-CPU 计数器追踪。
pub fn rcu_read_lock() {
    // SAFETY: Existing per-CPU RCU nesting counters in sync::rcu.
    unsafe {
        crate::kernel::sync::rcu::rcu_read_lock();
    }
}

/// 标记 RCU 读临界区结束。
pub fn rcu_read_unlock() {
    unsafe {
        crate::kernel::sync::rcu::rcu_read_unlock();
    }
}

/// 安全读取 RCU 保护的指针。
///
/// # SAFETY
/// 必须在 `rcu_read_lock()` / `rcu_read_unlock()` 之间调用。
/// `ptr` 必须来自 `rcu_assign_pointer()` 的有效写入。
pub unsafe fn rcu_dereference<'a, T>(ptr: *const T) -> &'a T {
    fence(Ordering::Acquire);
    unsafe { &*ptr }
}

/// 安全更新 RCU 保护的指针。
///
/// # SAFETY
/// 旧数据不可立即释放; 必须通过 `synchronize_rcu()` 或 `call_rcu()` 释放。
pub unsafe fn rcu_assign_pointer<T>(ptr: *mut *const T, new_val: *const T) {
    unsafe {
        *ptr = new_val;
    }
    fence(Ordering::Release);
}

/// 阻塞直到宽限期结束。
///
/// 等待所有 CPU 完成至少一个 RCU 读临界区后返回。
///
/// # 安全约束
/// 不可在 RCU 读临界区内调用 (会死锁)。
/// 不可在中断上下文调用。
pub fn synchronize_rcu() {
    unsafe {
        crate::kernel::sync::rcu::synchronize_rcu();
    }
}

/// 注册宽限期回调。
///
/// 当所有 CPU 宽限期结束时调用 `func(head)`,
/// 用于延迟释放旧数据。
pub fn call_rcu(head: &crate::kernel::sync::rcu::RcuHead, func: unsafe fn(*mut crate::kernel::sync::rcu::RcuHead)) {
    unsafe {
        crate::kernel::sync::rcu::call_rcu(
            head as *const _ as *mut crate::kernel::sync::rcu::RcuHead,
            func,
        );
    }
}
