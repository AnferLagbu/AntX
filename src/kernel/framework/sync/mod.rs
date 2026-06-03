//! 同步原语 (TCB) — SpinLock / Mutex / RwLock / RCU / OnceLock / IrqSpinLock
//!
//! 迁移自 `kernel::sync`, 附加完整 SAFETY 注释。
//! services 层通过 framework::sync 使用同步原语，
//! 而不直接接触底层 atomic / RawMutex 实现。
//!
//! - `OnceLock` / `IrqSpinLock` 是 safe 公共 API (内部 unsafe 隐藏)
//! - `OnceCellStorage` 是 unsafe 底层原语 (一般不直接使用)

pub mod spinlock;
pub mod mutex;
pub mod rwlock;
pub mod rcu;
pub mod once_lock;
pub mod once_cell;
pub mod irq_spinlock;
