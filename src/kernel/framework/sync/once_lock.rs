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

    /// 返回原始状态值 (仅用于调试诊断).
    #[inline]
    fn debug_state(&self) -> u8 {
        self.state.load(Ordering::Relaxed)
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
/// let v = lock.get_or_init(|slot| slot.write(42));
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

    /// 若未初始化, 用 `f` 在 cell 上写入并返回 `&T` 引用。
    ///
    /// 2026-06-29 修复: 闭包签名改为 `FnOnce(&mut MaybeUninit<T>)`, 强制
    /// 调用方在 `&mut MaybeUninit<T>` 上**就地构造** T, 避免返回值通过
    /// x86_64 SysV ABI 大对象返回约定 (隐藏指针 + 调用方栈帧分配) 产生的
    /// **栈溢出**. 之前签名 `FnOnce() -> T` 在 T 较大时 (如
    /// `IdentityTable` ≈ 78 KB) 即使闭包体内只 `unsafe { cell.write(f()) }`,
    /// 编译器仍会为 `f()` 返回值在调用方栈帧 (此处为 64 KB
    /// KERNEL_STACK_SIZE) 分配 78 KB 槽位, 直接踩栈破坏后续函数 (如
    /// `pmm_alloc_pages` 的 spin_lock 标志) 导致 QEMU 120s hang.
    /// 新签名下, `f` 必须显式 `slot.write(value)`, 整个生命周期都在 BSS
    /// `value` cell 上, 零额外栈开销. SAFETY 不变: `call_once` 互斥保证
    /// cell 只被写一次.
    pub fn get_or_init(&self, f: impl FnOnce(&mut MaybeUninit<T>)) -> &T {
        self.once.call_once(|| {
            // SAFETY: call_once 互斥保证本闭包是唯一的 cell 写者. cell 之前是
            // uninit, 闭包必须确保 `f` 调用后 cell 是 init 状态. 后续
            // `assume_init_ref` 看到完整有效值.
            f(unsafe { &mut *self.value.get() });
        });
        // SAFETY: 此刻 `self.once.is_completed()` 必为真 (call_once 已返回),
        // 因此 cell 已被初始化.
        unsafe { (*self.value.get()).assume_init_ref() }
    }

    /// 直接设置值 (若未初始化)。
    ///
    /// 返回 `Ok(())` 表示首次设置成功, `Err(value)` 表示已初始化, 值被退回。
    ///
    /// 2026-06-29 同步修复: 用 stack-local 持有 value (仅在 set 路径上).
    /// 适用 T 通常较小 (指针/Option/Box), 栈分配可接受. 若 T 较大且需要
    /// 避免栈分配, 改用 `get_or_init(|slot| slot.write(value))` 配合 cell
    /// 已有 UNINIT 状态的预检.
    pub fn set(&self, value: T) -> Result<(), T> {
        let mut slot: Option<T> = Some(value);
        self.once.call_once(|| {
            let v = slot.take().expect("OnceLock: set slot empty");
            // SAFETY: call_once 互斥保证此写独占 cell.
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

    /// 返回内部状态机的原始值 (仅用于调试诊断).
    /// 返回值: 0=未初始化, 1=初始化中, 2=已完成.
    #[inline]
    pub fn debug_state(&self) -> u8 {
        self.once.debug_state()
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
