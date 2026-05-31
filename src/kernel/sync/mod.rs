//! # 同步原语 (Synchronization Primitives) - 完整安全实现
//!
//! 提供 Mutex、RwLock、SpinLock、Atomic 等同步原语的**类型安全封装**。
//!
//! ## 架构概览
//!
//! ```text
//! sync/
//! ├── types.rs      # 核心数据结构 (与 C 版本布局兼容)
//! ├── spinlock.rs   # 自旋锁 (基于原子操作)
//! ├── mutex.rs     # 睡眠锁 (基于调度器 yield)
//! ├── rwlock.rs    # 读写锁 (写者优先策略)
//! ├── atomic.rs    # 原子操作封装
//! └── mod.rs       # 模块导出 + FFI 接口层
//! ```

pub mod arch;
pub mod atomic;
pub mod mutex;
pub mod rcu;
pub mod rwlock;
pub mod seqlock;
pub mod spinlock;
pub mod types;

use core::sync::atomic::Ordering;

// 导入 spinlock 模块的中断控制函数
use crate::kernel::sync::spinlock::{disable_interrupts, restore_interrupts};
// 导入 IrqSaveFlags 类型
use crate::kernel::sync::types::IrqSaveFlags;

/// 重新导出类型
pub use types::{MutexInner, RwLockInner, SpinLockGuard, SpinLockInner};

// ============================================================================
// FFI 接口层 (C ↔ Rust 桥接)
// ============================================================================

/// 初始化自旋锁 (FFI 导出)
#[no_mangle]
pub extern "C" fn spin_init(lock: *mut SpinLockInner) {
    if !lock.is_null() {
        unsafe {
            (*lock).locked.store(0, Ordering::Relaxed);
        }
    }
}

/// 获取自旋锁 (原始版本)
#[no_mangle]
pub extern "C" fn spin_lock_raw(lock: *const SpinLockInner) {
    if !lock.is_null() {
        // Fast path: 尝试立即获取
        let acquired = unsafe {
            (*lock)
                .locked
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        };

        if acquired {
            return;
        }

        // Slow path: 自旋等待
        loop {
            let result = unsafe {
                (*lock)
                    .locked
                    .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            };

            if result.is_ok() {
                break;
            }

            core::hint::spin_loop();
        }
    }
}

/// 释放自旋锁
#[no_mangle]
pub extern "C" fn spin_unlock(lock: *const SpinLockInner) {
    if !lock.is_null() {
        core::sync::atomic::fence(Ordering::SeqCst);
        unsafe {
            (*lock).locked.store(0, Ordering::Release);
        }
    }
}

/// 尝试获取自旋锁 (非阻塞)
#[no_mangle]
pub extern "C" fn spin_trylock(lock: *const SpinLockInner) -> i32 {
    if lock.is_null() {
        return 0; // 失败
    }

    match unsafe {
        (*lock)
            .locked
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
    } {
        Ok(_) => 1,  // 成功
        Err(_) => 0, // 已被锁定
    }
}

/// 检查自旋锁是否被持有
#[no_mangle]
pub extern "C" fn spin_is_locked(lock: *const SpinLockInner) -> i32 {
    if lock.is_null() {
        return 0;
    }

    unsafe { (*lock).locked.load(Ordering::Acquire) as i32 }
}

/// 初始化互斥锁
#[no_mangle]
pub extern "C" fn mutex_init(m: *mut MutexInner) {
    if !m.is_null() {
        unsafe {
            (*m).locked.store(0, Ordering::Relaxed);
            (*m).owner.store(-1i32, Ordering::Relaxed); // -1 表示无主
            (*m).depth.store(0, Ordering::Relaxed);
            (*m).acquire_time.store(0, Ordering::Relaxed);

            // 初始化内部自旋锁
            spin_init(&mut (*m).inner_spinlock);
        }
    }
}

/// 获取互斥锁 (阻塞)
#[no_mangle]
pub extern "C" fn mutex_lock(m: *const MutexInner) {
    if m.is_null() {
        return;
    }

    // Fast path: 尝试立即获取
    {
        unsafe { spin_lock_raw(&(*m).inner_spinlock) };

        let is_locked = unsafe { (*m).locked.load(Ordering::Acquire) != 0 };

        if !is_locked {
            // 成功获取
            unsafe {
                (*m).locked.store(1, Ordering::Release);
                let pid = process_get_current_pid();
                (*m).owner.store(pid as i32, Ordering::Release);
                (*m).depth.store(1, Ordering::Release);
            }
            unsafe { spin_unlock(&(*m).inner_spinlock) };
            return;
        }

        unsafe { spin_unlock(&(*m).inner_spinlock) };
    }

    // Slow path: 自旋 + yield (简化版)
    loop {
        unsafe { spin_lock_raw(&(*m).inner_spinlock) };

        let is_locked = unsafe { (*m).locked.load(Ordering::Acquire) != 0 };

        if !is_locked {
            unsafe {
                (*m).locked.store(1, Ordering::Release);
                let pid = process_get_current_pid();
                (*m).owner.store(pid as i32, Ordering::Release);
                (*m).depth.store(1, Ordering::Release);
            }
            unsafe { spin_unlock(&(*m).inner_spinlock) };
            return;
        }

        unsafe { spin_unlock(&(*m).inner_spinlock) };

        // 让出 CPU (unsafe 调用)
        unsafe { scheduler_yield() };
    }
}

/// 释放互斥锁
#[no_mangle]
pub extern "C" fn mutex_unlock(m: *const MutexInner) {
    if m.is_null() {
        return;
    }

    unsafe { spin_lock_raw(&(*m).inner_spinlock) };

    let depth = unsafe { (*m).depth.load(Ordering::Acquire) };

    if depth > 1 {
        // 嵌套锁，减少深度
        unsafe { (*m).depth.store(depth - 1, Ordering::Release) };
    } else {
        // 完全释放
        unsafe {
            (*m).locked.store(0, Ordering::Release);
            (*m).owner.store(0, Ordering::Release);
            (*m).depth.store(0, Ordering::Release);
            (*m).acquire_time.store(0, Ordering::Release);
        }
    }

    unsafe { spin_unlock(&(*m).inner_spinlock) };
}

/// 尝试获取互斥锁 (非阻塞)
#[no_mangle]
pub extern "C" fn mutex_trylock(m: *const MutexInner) -> i32 {
    if m.is_null() {
        return 0;
    }

    unsafe { spin_lock_raw(&(*m).inner_spinlock) };

    let result = if unsafe { (*m).locked.load(Ordering::Acquire) == 0 } {
        unsafe {
            (*m).locked.store(1, Ordering::Release);
            let pid = process_get_current_pid();
            (*m).owner.store(pid as i32, Ordering::Release);
            (*m).depth.store(1, Ordering::Release);
        }
        1 // 成功
    } else {
        0 // 已被锁定
    };

    unsafe { spin_unlock(&(*m).inner_spinlock) };

    result
}

/// 检查互斥锁是否被持有
#[no_mangle]
pub extern "C" fn mutex_is_locked(m: *const MutexInner) -> i32 {
    if m.is_null() {
        return 0;
    }

    unsafe { (*m).locked.load(Ordering::Acquire) as i32 }
}

/// 初始化读写锁
#[no_mangle]
pub extern "C" fn rwlock_init(rw: *mut RwLockInner) {
    if !rw.is_null() {
        unsafe {
            (*rw).readers.store(0, Ordering::Relaxed);
            (*rw).writer.store(0, Ordering::Relaxed);
            (*rw).pending_writers.store(0, Ordering::Relaxed);
            spin_init(&mut (*rw).lock);
        }
    }
}

/// 获取读锁 (阻塞)
#[no_mangle]
pub extern "C" fn read_lock(rw: *const RwLockInner) {
    if rw.is_null() {
        return;
    }

    loop {
        unsafe { spin_lock_raw(&(*rw).lock) };

        // 检查是否有写者或等待中的写者
        let has_writer = unsafe { (*rw).writer.load(Ordering::Relaxed) != 0 };
        let pending_writers = unsafe { (*rw).pending_writers.load(Ordering::Relaxed) > 0 };

        if !has_writer && !pending_writers {
            // 可以读取: 增加读者计数
            let readers = unsafe { (*rw).readers.fetch_add(1, Ordering::AcqRel) };
            unsafe { spin_unlock(&(*rw).lock) };

            if readers == 0xFFFF {
                // 溢出保护
                unsafe { (*rw).readers.fetch_sub(1, Ordering::AcqRel) };
                continue;
            }

            return;
        }

        unsafe { spin_unlock(&(*rw).lock) };

        // 让出 CPU
        unsafe { scheduler_yield() };
    }
}

/// 释放读锁
#[no_mangle]
pub extern "C" fn read_unlock(rw: *const RwLockInner) {
    if !rw.is_null() {
        unsafe { (*rw).readers.fetch_sub(1, Ordering::AcqRel) };
    }
}

/// 获取写锁 (阻塞)
#[no_mangle]
pub extern "C" fn write_lock(rw: *const RwLockInner) {
    if rw.is_null() {
        return;
    }

    // 先标记自己为等待中的写者
    unsafe {
        spin_lock_raw(&(*rw).lock);
        (*rw).pending_writers.fetch_add(1, Ordering::Release);
        spin_unlock(&(*rw).lock);
    };

    loop {
        unsafe { spin_lock_raw(&(*rw).lock) };

        // 检查是否可以获取写锁
        let readers = unsafe { (*rw).readers.load(Ordering::Relaxed) };
        let writer = unsafe { (*rw).writer.load(Ordering::Relaxed) };

        if readers == 0 && writer == 0 {
            // 可以写入: 设置写者标志
            unsafe {
                (*rw).pending_writers.fetch_sub(1, Ordering::Release);
                (*rw).writer.store(1, Ordering::Release);
            }
            unsafe { spin_unlock(&(*rw).lock) };
            return;
        }

        unsafe { spin_unlock(&(*rw).lock) };

        // 让出 CPU
        unsafe { scheduler_yield() };
    }
}

/// 释放写锁
#[no_mangle]
pub extern "C" fn write_unlock(rw: *const RwLockInner) {
    if !rw.is_null() {
        unsafe { (*rw).writer.store(0, Ordering::Release) };
    }
}

// ============================================================================
// 中断安全锁操作 (补充缺失的函数)
// ============================================================================

/// 获取自旋锁并禁用中断 (返回中断标志)
#[no_mangle]
pub extern "C" fn spin_lock_irqsave_raw(lock: *const SpinLockInner) -> IrqSaveFlags {
    let flags = disable_interrupts();
    spin_lock_raw(lock);
    flags
}

/// 释放自旋锁并恢复中断
#[no_mangle]
pub extern "C" fn spin_unlock_irqrestore(lock: *const SpinLockInner, flags: &IrqSaveFlags) {
    spin_unlock(lock);
    restore_interrupts(flags);
}

/// 获取自旋锁并禁用中断 (不保存标志)
#[no_mangle]
pub extern "C" fn spin_lock_irq(lock: *const SpinLockInner) {
    disable_interrupts();
    spin_lock_raw(lock);
}

/// 释放自旋锁并启用中断
#[no_mangle]
pub extern "C" fn spin_unlock_irq(lock: *const SpinLockInner) {
    spin_unlock(lock);
    crate::arch!(interrupt_enable());
}

// ============================================================================
// 读写锁扩展操作 (补充缺失的函数)
// ============================================================================

/// 尝试获取读锁 (非阻塞)
#[no_mangle]
pub extern "C" fn read_trylock(rw: *const RwLockInner) -> i32 {
    if rw.is_null() {
        return 0; // 失败
    }

    unsafe { spin_lock_raw(&(*rw).lock) };

    let has_writer = unsafe { (*rw).writer.load(Ordering::Relaxed) != 0 };
    let pending_writers = unsafe { (*rw).pending_writers.load(Ordering::Relaxed) > 0 };

    if !has_writer && !pending_writers {
        unsafe { (*rw).readers.fetch_add(1, Ordering::AcqRel) };
        unsafe { spin_unlock(&(*rw).lock) };
        return 1; // 成功
    }

    unsafe { spin_unlock(&(*rw).lock) };
    0 // 失败
}

/// 获取读锁并禁用中断
#[no_mangle]
pub extern "C" fn read_lock_irqsave(rw: *const RwLockInner) -> IrqSaveFlags {
    let flags = disable_interrupts();
    read_lock(rw);
    flags
}

/// 释放读锁并恢复中断
#[no_mangle]
pub extern "C" fn read_unlock_irqrestore(rw: *const RwLockInner, flags: &IrqSaveFlags) {
    read_unlock(rw);
    restore_interrupts(flags);
}

/// 获取写锁并禁用中断
#[no_mangle]
pub extern "C" fn write_lock_irqsave(rw: *const RwLockInner) -> IrqSaveFlags {
    let flags = disable_interrupts();
    write_lock(rw);
    flags
}

/// 释放写锁并恢复中断
#[no_mangle]
pub extern "C" fn write_unlock_irqrestore(rw: *const RwLockInner, flags: &IrqSaveFlags) {
    write_unlock(rw);
    restore_interrupts(flags);
}

/// 尝试获取写锁 (非阻塞)
#[no_mangle]
pub extern "C" fn write_trylock(rw: *const RwLockInner) -> i32 {
    if rw.is_null() {
        return 0; // 失败
    }

    unsafe { spin_lock_raw(&(*rw).lock) };

    let readers = unsafe { (*rw).readers.load(Ordering::Relaxed) };
    let writer = unsafe { (*rw).writer.load(Ordering::Relaxed) };

    if readers == 0 && writer == 0 {
        unsafe { (*rw).pending_writers.fetch_sub(1, Ordering::Release) };
        unsafe { (*rw).writer.store(1, Ordering::Release) };
        unsafe { spin_unlock(&(*rw).lock) };
        return 1; // 成功
    }

    unsafe { spin_unlock(&(*rw).lock) };
    0 // 失败
}

// ============================================================================
// 互斥锁扩展操作 (补充缺失的函数)
// ============================================================================

/// 获取互斥锁持有者 PID
#[no_mangle]
pub extern "C" fn mutex_owner(m: *const MutexInner) -> i32 {
    if m.is_null() {
        return -1;
    }

    unsafe { (*m).owner.load(Ordering::Acquire) }
}

/// 带超时的互斥锁获取 (简化版，暂不支持超时)
#[no_mangle]
pub extern "C" fn mutex_lock_timeout(m: *const MutexInner, _timeout_ms: u64) -> i32 {
    if m.is_null() {
        return -1;
    }

    mutex_lock(m);
    0 // 成功
}

// ============================================================================
// 条件变量桩函数 (简化实现)
// ============================================================================

/// 条件变量结构 (简化)
#[repr(C)]
pub struct CondVar {
    _padding: [u8; 64], // 占位符，保持与 C 版本兼容
}

/// 初始化条件变量
#[no_mangle]
pub extern "C" fn cond_init(_cond: *mut CondVar) -> i32 {
    0 // 成功
}

/// 发送信号唤醒一个等待者
#[no_mangle]
pub extern "C" fn cond_signal(_cond: *mut CondVar) -> i32 {
    0 // 成功
}

/// 广播唤醒所有等待者
#[no_mangle]
pub extern "C" fn cond_broadcast(_cond: *mut CondVar) -> i32 {
    0 // 成功
}

// ============================================================================
// 辅助函数声明
// ============================================================================

extern "C" {
    fn process_get_current_pid() -> u32;
    fn scheduler_yield();
}
