//! # RCU (Read-Copy-Update) 同步原语
//!
//! 读多写少场景的零开销读者锁。读者无原子操作开销，
//! 写者等待所有已有读者退出临界区后释放旧数据。
//!
//! ## 核心 API
//!
//! | 操作 | 说明 |
//! |------|------|
//! | `rcu_read_lock()` | 进入 RCU 读临界区 |
//! | `rcu_read_unlock()` | 退出 RCU 读临界区 |
//! | `rcu_dereference(p)` | 安全读取 RCU 保护指针 |
//! | `rcu_assign_pointer(p, v)` | 安全更新 RCU 保护指针 |
//! | `synchronize_rcu()` | 阻塞直到宽限期结束 |
//! | `call_rcu(head, func)` | 注册宽限期回调 |
//!
//! ## 简化实现
//!
//! QueenX 当前为单核/SMP 初期阶段, RCU 实现采用简化策略:
//! - **读锁**: per-CPU 嵌套计数 (atomic inc/dec)
//! - **宽限期**: 每次上下文切换标记为静止状态 (quiescent state)
//! - **回调**: 通过 Softirq 在宽限期结束后处理
//!
//! 后续可升级为完整 RCU (分层树, expedited, nocb 等)。

use core::ptr;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering, fence};

pub struct RcuHead {
    pub next: *mut RcuHead,
    pub func: Option<unsafe fn(*mut RcuHead)>,
}

impl RcuHead {
    pub const fn new() -> Self {
        Self { next: ptr::null_mut(), func: None }
    }
}

struct PerCpuRcu {
    nesting: AtomicU32,
    gp_state: AtomicU32,
    callbacks: UnsafeCell<*mut RcuHead>,
    callback_tail: UnsafeCell<*mut RcuHead>,
    callback_count: AtomicU32,
    need_callback_process: AtomicBool,
}

unsafe impl Sync for PerCpuRcu {}

const GP_IDLE: u32 = 0;
const GP_WAIT: u32 = 1;
const GP_DONE: u32 = 2;

static RCU_DATA: PerCpuRcu = PerCpuRcu {
    nesting: AtomicU32::new(0),
    gp_state: AtomicU32::new(GP_IDLE),
    callbacks: UnsafeCell::new(ptr::null_mut()),
    callback_tail: UnsafeCell::new(ptr::null_mut()),
    callback_count: AtomicU32::new(0),
    need_callback_process: AtomicBool::new(false),
};

static RCU_GP_COUNTER: AtomicU32 = AtomicU32::new(0);

#[inline(always)]
pub fn rcu_read_lock() {
    let nesting = RCU_DATA.nesting.fetch_add(1, Ordering::Acquire);
    // Barrier: 确保锁获取后的读写不会重排到之前
    if nesting == 0 {
        fence(Ordering::Acquire);
    }
}

#[inline(always)]
pub fn rcu_read_unlock() {
    // Barrier: 确保临界区内所有读写对后续可见
    fence(Ordering::Release);
    let nesting = RCU_DATA.nesting.fetch_sub(1, Ordering::Release);
    if nesting == 1 {
        fence(Ordering::Release);
    }
}

/// 安全读取 RCU 保护的指针
///
/// # Safety
/// 调用者必须在 RCU 读临界区内
#[inline(always)]
pub unsafe fn rcu_dereference<T>(ptr: *const T) -> *const T {
    fence(Ordering::Acquire);
    ptr::read_volatile(&ptr)
}

/// 安全更新 RCU 保护的指针
///
/// # Safety
/// 调用者确保旧值在所有 RCU 读者退出前不被释放
#[inline(always)]
pub unsafe fn rcu_assign_pointer<T>(slot: *mut *const T, new_val: *const T) {
    fence(Ordering::Release);
    ptr::write_volatile(slot, new_val);
}

/// 阻塞直到所有已有 RCU 读者退出
///
/// 内部实现:
/// 1. 标记宽限期开始
/// 2. 等待所有 CPU 通过静止状态
/// 3. 处理回调
pub fn synchronize_rcu() {
    let start_gp = RCU_GP_COUNTER.load(Ordering::Relaxed);

    // 宽限期开始
    RCU_GP_COUNTER.store(start_gp.wrapping_add(1), Ordering::Release);

    // 等待静止点 — 至少两次上下文切换确保所有读者退出
    // 简化实现: 忙等待 nesting == 0
    while RCU_DATA.nesting.load(Ordering::Acquire) > 0 {
        core::hint::spin_loop();
    }

    // 处理回调
    process_callbacks();
}

/// 注册 RCU 回调, 在宽限期结束后调用
///
/// `func` 在 `synchronize_rcu()` 或 `process_callbacks()` 时被调用。
///
/// # Safety
/// `head` 必须是从分配器分配的有效指针, `func` 必须正确处理 `head` 指向的内存
pub unsafe fn call_rcu(head: *mut RcuHead, func: unsafe fn(*mut RcuHead)) {
    if head.is_null() {
        return;
    }

    unsafe {
        (*head).next = ptr::null_mut();
        (*head).func = Some(func);
    }

    let flags = crate::kernel::sync::spinlock::disable_interrupts();

    let tail = unsafe { *RCU_DATA.callback_tail.get() };
    if !tail.is_null() {
        unsafe { (*tail).next = head; }
    } else {
        unsafe { *RCU_DATA.callbacks.get() = head; }
    }
    unsafe { *RCU_DATA.callback_tail.get() = head; }

    RCU_DATA.callback_count.fetch_add(1, Ordering::Relaxed);
    RCU_DATA.need_callback_process.store(true, Ordering::Release);

    crate::kernel::sync::spinlock::restore_interrupts(&flags);

    // 提升 Softirq 尽快处理回调
    crate::kernel::irq::raise_softirq(crate::kernel::irq::SoftirqVec::High);
}

/// 检查当前上下文是否在 RCU 读临界区内
pub fn rcu_read_lock_held() -> bool {
    RCU_DATA.nesting.load(Ordering::Acquire) > 0
}

/// 处理所有挂起的 RCU 回调
pub fn process_callbacks() {
    if !RCU_DATA.need_callback_process.load(Ordering::Acquire) {
        return;
    }

    let flags = crate::kernel::sync::spinlock::disable_interrupts();

    let head = unsafe { *RCU_DATA.callbacks.get() };
    unsafe {
        *RCU_DATA.callbacks.get() = ptr::null_mut();
        *RCU_DATA.callback_tail.get() = ptr::null_mut();
    }
    RCU_DATA.callback_count.store(0, Ordering::Relaxed);
    RCU_DATA.need_callback_process.store(false, Ordering::Release);

    crate::kernel::sync::spinlock::restore_interrupts(&flags);

    let mut cur = head;
    while !cur.is_null() {
        let next = unsafe { (*cur).next };
        let func = unsafe { (*cur).func };

        if let Some(f) = func {
            unsafe { f(cur); }
        }

        cur = next;
    }
}

/// 标记静止状态 (由调度器在上下文切换时调用)
pub fn rcu_note_quiescent_state() {
    if RCU_DATA.nesting.load(Ordering::Acquire) > 0 {
        return;
    }

    let state = RCU_DATA.gp_state.load(Ordering::Acquire);
    if state == GP_WAIT {
        RCU_DATA.gp_state.store(GP_DONE, Ordering::Release);
    }

    if RCU_DATA.need_callback_process.load(Ordering::Acquire)
        && RCU_DATA.nesting.load(Ordering::Acquire) == 0
    {
        process_callbacks();
    }
}

pub fn rcu_gp_count() -> u32 {
    RCU_GP_COUNTER.load(Ordering::Relaxed)
}

pub fn rcu_callback_count() -> u32 {
    RCU_DATA.callback_count.load(Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn rcu_read_lock_c() {
    rcu_read_lock();
}

#[no_mangle]
pub extern "C" fn rcu_read_unlock_c() {
    rcu_read_unlock();
}

#[no_mangle]
pub extern "C" fn synchronize_rcu_c() {
    synchronize_rcu();
}

#[no_mangle]
pub extern "C" fn rcu_init() {
    // 宽限期计数器从 1 开始
    RCU_GP_COUNTER.store(1, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rcu_read_lock_unlock() {
        rcu_read_lock();
        assert!(rcu_read_lock_held());
        rcu_read_lock();
        rcu_read_unlock();
        assert!(rcu_read_lock_held());
        rcu_read_unlock();
        assert!(!rcu_read_lock_held());
    }

    #[test]
    fn test_rcu_gp_counter() {
        let before = rcu_gp_count();
        synchronize_rcu();
        let after = rcu_gp_count();
        assert!(after > before);
    }

    #[test]
    fn test_call_rcu() {
        static CALLED: AtomicBool = AtomicBool::new(false);

        unsafe extern "C" fn callback(head: *mut RcuHead) {
            CALLED.store(true, Ordering::Release);
        }

        let mut head = RcuHead::new();
        unsafe {
            call_rcu(&mut head as *mut RcuHead, callback);
        }

        assert!(rcu_callback_count() == 1);

        synchronize_rcu();

        assert!(rcu_callback_count() == 0);
        assert!(CALLED.load(Ordering::Acquire));
    }
}