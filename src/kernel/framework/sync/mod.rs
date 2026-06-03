//! 同步原语 (TCB) — SpinLock / Mutex / RwLock / RCU
//!
//! 迁移自 `kernel::sync`, 附加完整 SAFETY 注释。
//! services 层通过 framework::sync 使用同步原语，
//! 而不直接接触底层 atomic / RawMutex 实现。

pub mod spinlock;
pub mod mutex;
pub mod rwlock;
pub mod rcu;
