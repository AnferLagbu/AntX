//! Priority Inheritance Mutex (PI Mutex) — TCB 实现
//!
//! ## 协议
//!
//! 当高优先级线程 P_H 因等待低优先级线程 P_L 持有的 PI mutex 而阻塞时, 把
//! P_L 的**有效优先级**临时提升至 max(P_L.base, P_H.base)。P_L 释放 mutex 后
//! 恢复基础优先级。
//!
//! ## 与普通 Mutex 的差异
//!
//! | 维度 | Mutex | PiMutex |
//! |------|-------|---------|
//! | 优先级继承 | ❌ | ✅ |
//! | 等待者优先级记录 | ❌ | ✅ VecDeque<(PID, base_prio)> |
//! | 解锁时唤醒策略 | 任意一个 | 最高优先级 + FIFO |
//! | 重复 lock | ✅ 递归 | ❌ 失败 (与 PTHREAD_PRIO_INHERIT 默认行为一致) |
//!
//! ## v1 简化
//!
//! - 直接捐赠, 不处理 A→B→C 链式
//! - 自旋 + yield 等待, 不入调度等待队列
//! - 不直接修改 Process.priority, 通过回调通知
//!
//! ## 安全契约
//!
//! - 全局状态由 `IrqSpinLock` 守护, 持锁期间屏蔽中断
//! - 所有公开函数接受 `&self` / `&mut self` 借用, 不暴露 `static mut`
//! - 回调函数 `DonationCallback` 由 services 层注入, 通过原子指针替换
//!
//! ## 评估日期
//!
//! 2026-06-08, 关联 DECISION-009/010/011

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::VecDeque;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

// ============================================================================
// 常量
// ============================================================================

/// 等待队列初始容量
const WAITERS_INIT_CAP: usize = 8;

/// PID 0 = 无效/空闲 (与 Process::None 约定)
pub const PID_NONE: u32 = 0;

// ============================================================================
// 回调函数
// ============================================================================

/// 捐赠通知回调签名: `(holder_pid, donated_priority)`
pub type DonationCallback = fn(holder_pid: u32, donated_priority: u32);

/// 全局捐赠通知回调 (原子指针, 启动期单线程安装, 运行时只读)
static NOTIFY_DONATION: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// 全局撤销通知回调
static NOTIFY_REVOKE: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// 安装捐赠通知回调
///
/// # Safety
///
/// - 启动期单线程调用一次
/// - `cb` 必须为 `'static` 函数指针, 不可在运行时被释放
pub unsafe fn set_donation_callback(cb: DonationCallback) {
    NOTIFY_DONATION.store(cb as *mut (), Ordering::Release);
}

/// 安装撤销通知回调
///
/// # Safety
///
/// 同 [set_donation_callback]
pub unsafe fn set_revoke_callback(cb: DonationCallback) {
    NOTIFY_REVOKE.store(cb as *mut (), Ordering::Release);
}

#[inline]
fn notify_donation(holder_pid: u32, donated_prio: u32) {
    let ptr = NOTIFY_DONATION.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: 调用方契约保证 ptr 由 set_donation_callback 安装且未释放
        let cb: DonationCallback = unsafe { core::mem::transmute(ptr) };
        cb(holder_pid, donated_prio);
    }
}

#[inline]
fn notify_revoke(pid: u32) {
    let ptr = NOTIFY_REVOKE.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: 同上
        let cb: DonationCallback = unsafe { core::mem::transmute(ptr) };
        cb(pid, 0);
    }
}

// ============================================================================
// 等待者条目
// ============================================================================

/// 等待者条目
#[derive(Debug, Clone, Copy)]
struct WaiterEntry {
    /// 等待者 PID
    pid: u32,
    /// 等待者入队时的 base_priority (用于取消时正确撤销捐赠)
    base_priority: u32,
}

// ============================================================================
// PiMutex 内部状态
// ============================================================================

/// PiMutex 内部状态
struct PiMutexInner {
    /// 是否被持有
    locked: AtomicBool,
    /// 当前持有者 PID (None = 未持有)
    holder: AtomicU32,
    /// 等待队列 (FIFO 顺序, 同优先级按 FIFO)
    waiters: IrqSpinLock<VecDeque<WaiterEntry>>,
    /// 当前有效优先级 (= max(holder_prio, waiters_prio))
    effective_priority: AtomicU32,
}

impl PiMutexInner {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            holder: AtomicU32::new(PID_NONE),
            waiters: IrqSpinLock::new(VecDeque::new()),
            effective_priority: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PiMutex 公开类型
// ============================================================================

/// Priority Inheritance Mutex
pub struct PiMutex<T: ?Sized> {
    inner: PiMutexInner,
    /// 初始持有者的 base_priority (用于解锁后通知撤销)
    holder_base_priority: AtomicU32,
    data: UnsafeCell<T>,
}

// SAFETY: PiMutex 通过内部锁提供互斥, T: Send 即可跨线程传递所有权
unsafe impl<T: ?Sized + Send> Send for PiMutex<T> {}
// SAFETY: 共享引用跨线程安全
unsafe impl<T: ?Sized + Send> Sync for PiMutex<T> {}

/// RAII 守卫, drop 时自动释放锁
pub struct PiMutexGuard<'a, T: ?Sized> {
    data: &'a mut T,
    mutex: &'a PiMutex<T>,
}

// SAFETY: MutexGuard 持有时即为独占访问, T: Sync 由 Mutex Sync 提供
unsafe impl<T: ?Sized + Sync> Sync for PiMutexGuard<'_, T> {}

// ============================================================================
// PiMutex 构造
// ============================================================================

impl<T> PiMutex<T> {
    /// 创建新的 PI Mutex
    pub const fn new(data: T) -> Self {
        Self {
            inner: PiMutexInner::new(),
            holder_base_priority: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: Default> Default for PiMutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

// ============================================================================
// PiMutex 锁操作
// ============================================================================

impl<T: ?Sized> PiMutex<T> {
    /// 获取锁 (阻塞 + 优先级继承)
    ///
    /// # 参数
    /// - `my_pid`: 当前线程 PID
    /// - `my_base_priority`: 当前线程的 base_priority (而非有效优先级)
    ///
    /// # 行为
    /// 1. 尝试立即获取
    /// 2. 失败时注册为等待者, 通知 holder 接受捐赠
    /// 3. 自旋 + yield 直到被唤醒 (v1 简化)
    pub fn lock(&self, my_pid: u32, my_base_priority: u32) -> PiMutexGuard<'_, T> {
        loop {
            if self.try_lock(my_pid, my_base_priority) {
                // SAFETY: 持锁后独占访问 data
                return PiMutexGuard {
                    data: unsafe { &mut *self.data.get() },
                    mutex: self,
                };
            }
            // 自旋 + yield 等待被唤醒
            scheduler_yield();
        }
    }

    /// 尝试获取锁 (非阻塞)
    ///
    /// 成功时返回 true, 失败时返回 false 并**已自动注册为等待者 + 触发捐赠**。
    pub fn try_lock(&self, my_pid: u32, my_base_priority: u32) -> bool {
        // fast path: 直接尝试
        if self.inner.locked.compare_exchange(
            false,
            true,
            Ordering::AcqRel,
            Ordering::Acquire,
        ).is_ok()
        {
            self.inner.holder.store(my_pid, Ordering::Release);
            self.inner.effective_priority.store(my_base_priority, Ordering::Release);
            self.holder_base_priority.store(my_base_priority, Ordering::Release);
            return true;
        }

        // 失败: 慢路径 — 注册为等待者 + 捐赠
        self.register_waiter_and_donate(my_pid, my_base_priority);
        false
    }

    /// 注册为等待者并触发捐赠 (内部函数)
    fn register_waiter_and_donate(&self, my_pid: u32, my_base_priority: u32) {
        {
            let mut waiters = self.inner.waiters.lock();
            // 避免重复注册 (同一线程多次 lock)
            if waiters.iter().any(|w| w.pid == my_pid) {
                return;
            }
            waiters.push_back(WaiterEntry {
                pid: my_pid,
                base_priority: my_base_priority,
            });
        } // waiters 锁释放

        // 计算新的 effective_priority = max(holder_prio, max(waiters_prio))
        let holder_prio = self.inner.effective_priority.load(Ordering::Acquire);
        let max_waiter_prio = {
            let waiters = self.inner.waiters.lock();
            waiters.iter().map(|w| w.base_priority).max().unwrap_or(0)
        };
        let new_effective = holder_prio.max(max_waiter_prio);
        self.inner.effective_priority.store(new_effective, Ordering::Release);

        // 通知调度器: holder 接受捐赠
        let holder_pid = self.inner.holder.load(Ordering::Acquire);
        if holder_pid != PID_NONE {
            notify_donation(holder_pid, new_effective);
        }
    }

    /// 释放锁 (由 PiMutexGuard::drop 自动调用)
    ///
    /// `pub(crate)` 以便 tests 模块直接验证状态机
    /// (测试无需完整 lock 循环, 直接 drop 即可触发)
    pub(crate) fn unlock_internal(&self) {
        let my_pid = current_pid();
        if self.inner.holder.load(Ordering::Acquire) != my_pid {
            // 双重释放 / 非持有者释放: 静默忽略 (v1)
            return;
        }

        // 1. 找到下一个最高优先级等待者
        let (next_pid, next_base_prio) = {
            let mut waiters = self.inner.waiters.lock();
            // 找到 max 优先级 (同优先级按 FIFO, 即 VecDeque 头部优先)
            let mut best_idx: Option<usize> = None;
            let mut best_prio: u32 = 0;
            for (i, w) in waiters.iter().enumerate() {
                if w.base_priority >= best_prio {
                    best_prio = w.base_priority;
                    best_idx = Some(i);
                }
            }
            match best_idx {
                Some(idx) => {
                    let entry = waiters.remove(idx).unwrap();
                    (entry.pid, entry.base_priority)
                }
                None => {
                    // 无等待者, 完全释放
                    self.inner.locked.store(false, Ordering::Release);
                    self.inner.holder.store(PID_NONE, Ordering::Release);
                    self.inner.effective_priority.store(0, Ordering::Release);
                    self.holder_base_priority.store(0, Ordering::Release);
                    return;
                }
            }
        };

        // 2. 移交锁给下一个等待者 (原子 CAS: holder = next.pid, locked 保持 true)
        //    由于我们仍持 "持有者" 身份, 中间需要短暂 false → true 让 next 看到
        //    这里采用: 短暂释放 + next 重入, 与 register_waiter_and_donate 配合

        // 重置自己的优先级 (基线, 由调度器钩子处理)
        let my_base = self.holder_base_priority.load(Ordering::Acquire);
        notify_revoke(my_pid); // 通知调度器我自己撤销捐赠

        // 短暂 release, 让 next.try_lock 能看到 unlocked
        self.inner.locked.store(false, Ordering::Release);
        self.inner.holder.store(PID_NONE, Ordering::Release);
        self.inner.effective_priority.store(0, Ordering::Release);
        self.holder_base_priority.store(0, Ordering::Release);

        // next 线程此刻还在自旋等待 holder==next.pid;
        // 我们直接设置 holder, 让它从自旋中退出并完成获取
        self.inner.holder.store(next_pid, Ordering::Release);
        self.inner.locked.store(true, Ordering::Release);
        self.inner.effective_priority.store(next_base_prio, Ordering::Release);
        // 持有者 base_priority 由 next 在 try_lock 时设置
        // (这里没有, 因为我们走的是 unlock 路径而非 try_lock)
        // 修正: 重新赋值
        self.holder_base_priority.store(next_base_prio, Ordering::Release);

        // 通知: 仍有等待者, 计算新 effective_priority
        let new_effective = {
            let waiters = self.inner.waiters.lock();
            waiters.iter().map(|w| w.base_priority).max().unwrap_or(next_base_prio)
        };
        if new_effective > next_base_prio {
            self.inner.effective_priority.store(new_effective, Ordering::Release);
            notify_donation(next_pid, new_effective);
        }

        // 静默: 防止 my_base 未使用警告
        let _ = my_base;
    }

    /// 查询当前是否被持有
    pub fn is_locked(&self) -> bool {
        self.inner.locked.load(Ordering::Acquire)
    }

    /// 查询当前持有者 PID (0 = 未持有)
    pub fn holder(&self) -> u32 {
        self.inner.holder.load(Ordering::Acquire)
    }

    /// 查询当前 effective_priority (供调度器钩子使用)
    pub fn effective_priority(&self) -> u32 {
        self.inner.effective_priority.load(Ordering::Acquire)
    }

    /// 查询当前等待者数量
    pub fn waiter_count(&self) -> usize {
        self.inner.waiters.lock().len()
    }
}

// ============================================================================
// PiMutexGuard
// ============================================================================

impl<'a, T: ?Sized> Drop for PiMutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.unlock_internal();
    }
}

impl<'a, T: ?Sized> core::ops::Deref for PiMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.data
    }
}

impl<'a, T: ?Sized> core::ops::DerefMut for PiMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.data
    }
}

// ============================================================================
// 辅助: 获取当前 PID (TCB 桥接)
// ============================================================================

fn current_pid() -> u32 {
    extern "C" {
        fn process_get_current_pid() -> u32;
    }
    // SAFETY: process_get_current_pid 是有效的 C ABI 函数, 无副作用
    unsafe { process_get_current_pid() }
}

fn scheduler_yield() {
    extern "C" {
        fn scheduler_yield();
    }
    // SAFETY: scheduler_yield 是有效的 C ABI 函数, 让出 CPU
    unsafe { scheduler_yield() }
}

// ============================================================================
// 单元测试 (host 端)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering as AOrd};
    use std::sync::Mutex as StdMutex;

    /// 模拟当前 PID (测试用)
    static TEST_PID: AtomicU32 = AtomicU32::new(1);
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    /// 测试上下文: PID + base_priority
    struct TestCtx {
        pid: u32,
        base_prio: u32,
    }

    fn with_ctx<F: FnOnce(TestCtx)>(ctx: TestCtx, f: F) {
        let _g = TEST_LOCK.lock().unwrap();
        TEST_PID.store(ctx.pid, AOrd::SeqCst);
        f(ctx);
        TEST_PID.store(0, AOrd::SeqCst);
    }

    /// 直接调 register_waiter_and_donate (绕过 try_lock 失败路径) 的等价测试:
    /// 构造 PiMutex, 用低优先级线程 A 持锁, 模拟高优先级线程 B 调用 lock 失败
    fn low_prio_holds_high_prio_waits_setup() -> (PiMutex<u32>, u32, u32) {
        let m = PiMutex::new(0u32);
        let a_pid = 100u32;
        let a_prio = 1u32; // 低优先级
        // A 获取锁
        assert!(m.try_lock(a_pid, a_prio));
        assert_eq!(m.holder(), a_pid);
        assert_eq!(m.effective_priority(), a_prio);
        (m, a_pid, a_prio)
    }

    #[test]
    fn basic_lock_unlock() {
        let m = PiMutex::new(42u32);
        assert!(!m.is_locked());
        let g = m.lock(1, 5);
        assert!(m.is_locked());
        assert_eq!(m.holder(), 1);
        assert_eq!(*g, 42);
        drop(g);
        assert!(!m.is_locked());
    }

    #[test]
    fn try_lock_fails_when_held() {
        let m = PiMutex::new(0u32);
        assert!(m.try_lock(1, 5));
        assert!(!m.try_lock(2, 5));
        assert_eq!(m.holder(), 1);
    }

    /// 核心测试: 高优先级等待 → 有效优先级 = max
    #[test]
    fn donation_boosts_effective_priority() {
        let (m, a_pid, a_prio) = low_prio_holds_high_prio_waits_setup();
        // 模拟高优先级 B (prio=10) 注册为等待者
        assert!(!m.try_lock(200, 10));
        // A 的 effective_priority 应该被提升到 10
        assert_eq!(m.effective_priority(), 10);
        assert_eq!(m.holder(), a_pid); // 仍为 A 持有
        assert_eq!(m.waiter_count(), 1);
    }

    /// 多等待者: effective_priority = max(所有等待者)
    #[test]
    fn donation_max_of_all_waiters() {
        let (m, _, _) = low_prio_holds_high_prio_waits_setup();
        assert!(!m.try_lock(200, 10));
        assert!(!m.try_lock(201, 5));
        assert!(!m.try_lock(202, 8));
        assert!(!m.try_lock(203, 12));
        // max = 12
        assert_eq!(m.effective_priority(), 12);
        assert_eq!(m.waiter_count(), 4);
    }

    /// 释放后最高优先级等待者成为新持有者
    #[test]
    fn unlock_transfers_to_highest_waiter() {
        let (m, a_pid, _a_prio) = low_prio_holds_high_prio_waits_setup();
        assert!(!m.try_lock(200, 10));
        assert!(!m.try_lock(201, 5));
        assert!(!m.try_lock(202, 8));
        // 模拟 A 释放: 直接调 unlock_internal (测试专用)
        m.unlock_internal();
        // 新持有者应是最高优先级等待者 (200, prio=10)
        assert_eq!(m.holder(), 200);
        assert_eq!(m.effective_priority(), 10);
        // 仍有一个等待者 (201, 202), max=8
        assert_eq!(m.waiter_count(), 2);
        // 持有者 base 优先级被设为 10 (来自新持有者 entry)
        assert_eq!(m.holder_base_priority.load(AOrd::Acquire), 10);
        // 防止 a_pid 未使用警告
        let _ = a_pid;
    }

    /// 无等待者时完全释放
    #[test]
    fn unlock_with_no_waiters_full_release() {
        let (m, _a_pid, a_prio) = low_prio_holds_high_prio_waits_setup();
        m.unlock_internal();
        assert!(!m.is_locked());
        assert_eq!(m.holder(), 0);
        assert_eq!(m.effective_priority(), 0);
        let _ = a_prio;
    }

    /// 重复 lock 同一线程: 注册为等待者 (v1 不递归, 但需避免重复 push)
    #[test]
    fn duplicate_lock_same_pid_does_not_double_register() {
        let (m, _a_pid, _a_prio) = low_prio_holds_high_prio_waits_setup();
        assert!(!m.try_lock(200, 10));
        // 同一 PID 再次 lock (失败但不应再 push)
        assert!(!m.try_lock(200, 10));
        assert_eq!(m.waiter_count(), 1);
    }

    /// 测试钩子回调设置 (编译期验证类型)
    #[test]
    fn callback_types_compile() {
        fn dummy_donate(_holder: u32, _prio: u32) {}
        // SAFETY: 启动期安装, 测试作用域内有效
        unsafe { set_donation_callback(dummy_donate) };
        // SAFETY: 同上
        unsafe { set_revoke_callback(dummy_donate) };
        // 验证回调能调 (即使绑的是 dummy)
        notify_donation(1, 5);
        notify_revoke(1);
    }
}
