//! Framework prelude — services 层可直接 import 的安全抽象

pub use super::frame::Frame;
pub use super::vmspace::VmSpace;
pub use super::usermode::enter_user_mode;
pub use super::userctx::UserContext;
pub use super::cpu_local::CpuLocal;

pub use super::sync::spinlock::SpinLock;
pub use super::sync::spinlock::SpinLockGuard;
pub use super::sync::mutex::Mutex;
pub use super::sync::mutex::MutexGuard;
pub use super::sync::rwlock::RwLock;
pub use super::sync::rwlock::RwLockReadGuard;
pub use super::sync::rwlock::RwLockWriteGuard;
pub use super::sync::rcu::{
    rcu_read_lock, rcu_read_unlock,
    rcu_dereference, rcu_assign_pointer,
    synchronize_rcu, call_rcu,
};

pub use super::alloc::frame_alloc::{FrameAlloc, BuddyFrameAlloc};
pub use super::alloc::slab_alloc::{SlabAlloc, KmallocSlabAlloc};

pub use super::iomem::IoMem;
pub use super::ioport::IoPort;
pub use super::irqline::{IrqLine, InterruptHandler};
pub use super::dma_buf::{DmaStream, DmaDirection};
pub use super::page_table::{
    check_user_boundary, check_wxorx, verify_mapping,
};

// Phase 1.4 — 调度器
pub use super::sched::sched_trait::{Scheduler, Task, QueenXScheduler};
