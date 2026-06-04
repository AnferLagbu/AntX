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
    // RCU 读临界区由 rcu_read_lock/unlock 配对; 嵌套由 per-CPU 计数器维护。
    // 无内存操作, 仅写 per-CPU 变量 (cli 上下文或单线程启动路径保证独占)。
    unsafe {
        crate::kernel::framework::sync_legacy::rcu::rcu_read_lock();
    }
}

/// 标记 RCU 读临界区结束。
pub fn rcu_read_unlock() {
    // SAFETY: 同 `rcu_read_lock` 的契约; 减少 per-CPU 嵌套计数, 0 时退出临界区。
    unsafe {
        crate::kernel::framework::sync_legacy::rcu::rcu_read_unlock();
    }
}

/// 安全读取 RCU 保护的指针。
///
/// # SAFETY
/// 必须在 `rcu_read_lock()` / `rcu_read_unlock()` 之间调用。
/// `ptr` 必须来自 `rcu_assign_pointer()` 的有效写入。
pub unsafe fn rcu_dereference<'a, T>(ptr: *const T) -> &'a T {
    fence(Ordering::Acquire);
    // SAFETY: 调用方必须保证 (按 doc comment):
    //   1. 在 RCU 读临界区内 (rcu_read_lock/unlock 配对)
    //   2. `ptr` 来自之前 `rcu_assign_pointer` 的写入
    //   3. `ptr` 非空, 指向对齐到 `align_of::<T>()` 的有效内存
    // `Acquire` fence 与 `rcu_assign_pointer` 的 `Release` 配对, 保证
    // 能看到指针指向对象的最新写入。
    unsafe { &*ptr }
}

/// 安全更新 RCU 保护的指针。
///
/// # SAFETY
/// 旧数据不可立即释放; 必须通过 `synchronize_rcu()` 或 `call_rcu()` 释放。
pub unsafe fn rcu_assign_pointer<T>(ptr: *mut *const T, new_val: *const T) {
    // SAFETY: 调用方必须保证:
    //   1. `ptr` 是有效 `*mut *const T`, 指向已发布的 RCU 指针槽
    //   2. `new_val` 指向的对象生命周期 >= synchronize_rcu 后的释放
    // `Release` fence 之后, 所有先前写对后续 rcu_dereference 可见。
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
    // SAFETY: 本函数本身不在 RCU 读临界区内 (调用契约), 不可在中断上下文
    // (会自旋等待所有 CPU, 嵌套屏蔽会死锁)。由 `synchronize_rcu` 内部
    // 通过 per-CPU 计数器自旋等待所有 CPU 退出临界区。
    unsafe {
        crate::kernel::framework::sync_legacy::rcu::synchronize_rcu();
    }
}

/// 注册宽限期回调。
///
/// 当所有 CPU 宽限期结束时调用 `func(head)`,
/// 用于延迟释放旧数据。
///
/// # Safety
///
/// 1. `head` 必须指向一个有效的 `RcuHead` 实例, 且在 `func` 被调用前不被释放
/// 2. `func` 必须是合法的 `unsafe fn(*mut RcuHead)`, 实现须满足 RCU 回调契约
///    (不睡眠, 不持有锁, 不递归注册)
/// 3. `head` 指向的对象 (通常是包含 RcuHead 字段的外层结构) 的释放时机
///    必须严格通过此 call_rcu 注册, 不可直接 deallocate
pub fn call_rcu(head: &crate::kernel::framework::sync_legacy::rcu::RcuHead, func: unsafe fn(*mut crate::kernel::framework::sync_legacy::rcu::RcuHead)) {
    // SAFETY:
    //   1. `head` 是有效 `RcuHead` 引用, 通过 `&` 借用保证存活
    //   2. `func` 是合法的 `unsafe fn(*mut RcuHead)`, 由调用方实现
    //   3. `as *const _ as *mut RcuHead` 转换是 ABI 兼容的 (RcuHead 是 repr(C)
    //      结构, 转换不丢字段)
    //   4. 内部 call_rcu 会在宽限期结束时调用 func(head_ptr), 由 head 的实际
    //      类型保证 layout 兼容
    unsafe {
        crate::kernel::framework::sync_legacy::rcu::call_rcu(
            head as *const _ as *mut crate::kernel::framework::sync_legacy::rcu::RcuHead,
            func,
        );
    }
}
