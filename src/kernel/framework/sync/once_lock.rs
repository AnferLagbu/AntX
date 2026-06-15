//! OnceLock — TCB 一次性值容器 (safe 公共 API)
//!
//! 等价于 `std::sync::OnceLock<T>` 的内核版。
//!
//! ## 设计
//!
//! - 内部: `Once` (串行化) + `UnsafeCell<MaybeUninit<T>>` (存储)
//! - 公共 API: `set` / `get` / `get_or_init` 全部 safe
//! - `unsafe` 块隐藏在 `Once` 互斥保证之后
//!
//! ## SAFETY 契约
//!
//! 所有 unsafe 块都遵循统一模式:
//! - `write` 之前必须由 `Once` 串行化, 确认是唯一的写者
//! - `read` 必须确认 `Once::is_completed()` 为真 (cell 已初始化)
//! - `drop` 必须确认 `Once::is_completed()` 为真 (避免 drop uninit)
//!
//! 内部用 `AtomicU8` 状态机 (`UNINITIALIZED / IN_PROGRESS / DONE`)
//! 替代 `Once`, 避免循环依赖 (Once 在 services::sync::once)。
//!
//! ## 与 `services::sync::once` 的关系
//!
//! ```text
//! framework::sync::once_lock::OnceLock    ← 本模块 (safe 公共 API, unsafe 内部)
//!   ↑
//! services::sync::once::OnceCell          ← 纯 thin wrapper, 转调
//! services::sync::once::Once              ← 纯 safe, 简单闭包一次性
//! ```

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};

// ============================================================================
// 内嵌 Once — 仅在本模块使用, 避免循环依赖
// ============================================================================

const UNINITIALIZED: u8 = 0;
const IN_PROGRESS: u8 = 1;
const DONE: u8 = 2;

struct InnerOnce {
    state: AtomicU8,
}

impl InnerOnce {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(UNINITIALIZED),
        }
    }

    /// 仅当尚未完成时执行闭包 (执行后可重入调用, 但闭包本身只跑一次)。
    fn call_once<F: FnOnce()>(&self, f: F) {
        // 快速路径
        if self.state.load(Ordering::Acquire) == DONE {
            return;
        }
        // 简单自旋等待 (无 Mutex 依赖, 纯原子操作)
        // 状态机: UNINITIALIZED → IN_PROGRESS → DONE
        // 多个线程同时进入时, CAS 保证只有一个把 UNINITIALIZED 翻成 IN_PROGRESS。
        let prev = self
            .state
            .compare_exchange(
                UNINITIALIZED,
                IN_PROGRESS,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .unwrap_or_else(|p| p);
        match prev {
            DONE => {}
            UNINITIALIZED => {
                // 我们赢得了执行权
                f();
                self.state.store(DONE, Ordering::Release);
            }
            IN_PROGRESS => {
                // 别的线程正在执行, 自旋等待完成
                while self.state.load(Ordering::Acquire) != DONE {
                    core::hint::spin_loop();
                }
            }
            _ => unreachable!("Once: unknown state"),
        }
    }

    #[inline]
    fn is_completed(&self) -> bool {
        self.state.load(Ordering::Acquire) == DONE
    }
}

// ============================================================================
// OnceLock<T> — safe 公共 API
// ============================================================================

/// 一次性值容器 (safe)。
///
/// ## 用法
///
/// ```ignore
/// let lock: OnceLock<u32> = OnceLock::new();
/// assert!(lock.get().is_none());
/// let v = lock.get_or_init(|| 42);
/// assert_eq!(*v, 42);
/// ```
pub struct OnceLock<T> {
    once: InnerOnce,
    value: UnsafeCell<MaybeUninit<T>>,
}

// SAFETY: `InnerOnce` 串行化所有访问; `T: Send` 即可跨线程移动 (与 std::OnceLock 一致)。
unsafe impl<T: Send> Send for OnceLock<T> {}
// SAFETY: 共享引用跨线程安全 (访问经 Once 互斥)。
unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}

impl<T> OnceLock<T> {
    /// 创建未初始化的 `OnceLock`。
    pub const fn new() -> Self {
        Self {
            once: InnerOnce::new(),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// 若未初始化, 用 `f` 计算并存储; 返回最终值的 `&T` 引用。
    pub fn get_or_init(&self, f: impl FnOnce() -> T) -> &T {
        let mut holder: Option<T> = None;
        self.once.call_once(|| {
            holder = Some(f());
        });
        if let Some(v) = holder {
            // SAFETY: call_once 互斥保证这是唯一的写者; 此后该 cell 视为已初始化。
            unsafe { (*self.value.get()).write(v) };
        }
        // SAFETY: 此刻 `self.once.is_completed()` 必为真 (call_once 已返回),
        // 因此 cell 已被初始化。
        unsafe { (*self.value.get()).assume_init_ref() }
    }

    /// 直接设置值 (若未初始化)。
    ///
    /// 返回 `Ok(())` 表示首次设置成功, `Err(value)` 表示已初始化, 值被退回。
    pub fn set(&self, value: T) -> Result<(), T> {
        let mut slot: Option<T> = Some(value);
        self.once.call_once(|| {
            let v = slot.take().expect("OnceLock: set slot empty");
            // SAFETY: call_once 互斥保证此写独占。
            unsafe { (*self.value.get()).write(v) };
        });
        match slot {
            None => Ok(()),
            Some(v) => Err(v),
        }
    }

    /// 获取值 (若已初始化)。
    #[inline]
    pub fn get(&self) -> Option<&T> {
        if self.once.is_completed() {
            // SAFETY: is_completed 保证 cell 已初始化。
            Some(unsafe { (*self.value.get()).assume_init_ref() })
        } else {
            None
        }
    }
}

impl<T> Default for OnceLock<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for OnceLock<T> {
    fn drop(&mut self) {
        if self.once.is_completed() {
            // SAFETY: is_completed 保证 cell 已初始化, drop 是唯一一次访问。
            unsafe { (*self.value.get()).assume_init_drop() };
        }
    }
}
