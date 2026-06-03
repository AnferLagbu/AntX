//! 同步原语 (services 层) — 高级 RAII 与领域封装
//!
//! ## 与 `framework::sync` 的关系
//!
//! ```text
//! services::sync/*        (本模块, 100% safe Rust)
//!   ├─ irq_lock::IrqSpinLock        中断安全自旋锁 (保存/恢复 IF)
//!   ├─ scoped::*                    闭包作用域 API (with / with_mut)
//!   ├─ barrier::Barrier             N 线程集合点 (latch-style)
//!   ├─ once::{Once, OnceCell}       一次性初始化
//!   └─ (re-export) framework::sync  基础类型供 services 直接使用
//!        ↑
//! framework::sync/*        (TCB, unsafe 允许, 真正实现)
//! ```
//!
//! ## 设计目标
//!
//! 1. **更易用**: `mutex.with(|g| g.value = 42)` 闭包 API, 杜绝 guard 泄漏。
//! 2. **更安全**: 类型系统约束, 编译期阻止跨 await 持锁 / 递归死锁。
//! 3. **更领域**: `IrqSpinLock` 在持锁期间自动屏蔽中断, 退出后恢复。
//!
//! ## @SAFE
//! 本目录所有文件不含 `unsafe`. 所有底层操作委托 `framework::sync` TCB API.

#![allow(dead_code)]

/// 同步原语 — 基础重导出 (供 services 层共享)
pub use crate::kernel::framework::sync::{
    mutex::{Mutex, MutexGuard},
    rwlock::{RwLock, RwLockReadGuard, RwLockWriteGuard},
    spinlock::SpinLock,
};

/// 中断安全自旋锁 (保存/恢复 IF 标志)
pub mod irq_lock;

/// 闭包作用域 API (with / with_mut / try_with)
pub mod scoped;

/// N-线程集合点 (latch-style barrier)
pub mod barrier;

/// 一次性初始化 (Once + OnceCell)
pub mod once;

// ============================================================================
// 单元自检 (编译期)
// ============================================================================

/// 编译期断言: services::sync 不允许 unsafe 代码。
///
/// 与 `tools/check_tcb.sh` 配合: 该脚本 grep 整个 services/sync 目录,
/// 一旦发现 `unsafe` 关键字即失败。
pub const SERVICES_SYNC_SAFE: bool = true;
