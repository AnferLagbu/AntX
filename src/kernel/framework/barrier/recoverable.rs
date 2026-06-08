use core::sync::atomic::Ordering;

use super::fault_inject::maybe_inject_fault;
use super::undo_log::UndoLog;
use crate::kernel::framework::sync::irq_spinlock::{IrqSpinLock, IrqSpinLockGuard};

pub trait Snapshot: Copy + Sized {
    fn snapshot(&self) -> Self {
        *self
    }
    fn restore(&mut self, snapshot: &Self) {
        *self = *snapshot;
    }
}

pub trait Recoverable {
    fn domain_id(&self) -> u64 {
        0
    }
    fn capture_barrier(&self, _undo: &mut UndoLog) {}
    fn rollback(&self) -> bool {
        true
    }
    fn reset(&self) -> bool {
        true
    }
}

pub struct RecoverableMutex<T: Snapshot + 'static> {
    inner: IrqSpinLock<T>,
    domain_id: u64,
}

// SAFETY: RecoverableMutex wraps IrqSpinLock<T>, which provides mutual
// exclusion with IRQ disable. T: Send is required for cross-thread transfer;
// T: Sync is required because &T can be shared after lock acquisition. The
// domain_id is a plain u64 (Copy). No additional unsafety beyond what
// IrqSpinLock provides.
unsafe impl<T: Snapshot + Send> Send for RecoverableMutex<T> {}
unsafe impl<T: Snapshot + Sync> Sync for RecoverableMutex<T> {}

impl<T: Snapshot + 'static> RecoverableMutex<T> {
    pub const fn new(val: T, domain_id: u64) -> Self {
        Self {
            inner: IrqSpinLock::new(val),
            domain_id,
        }
    }

    pub fn lock(&self) -> IrqSpinLockGuard<'_, T> {
        let guard = self.inner.lock();
        if self.domain_id != 0 {
            if let Some(dom) = super::RECOVERY_MANAGER.lock().find(self.domain_id) {
                let mut undo = dom.undo.lock();
                undo.current_generation = dom.barrier_generation.load(Ordering::SeqCst);
                let key_ptr = &*guard as *const T as *mut T;
                let snapshot = guard.snapshot();
                undo.record(key_ptr, snapshot);
            }
        }
        maybe_inject_fault(self.domain_id);
        guard
    }

    pub fn lock_fast(&self) -> IrqSpinLockGuard<'_, T> {
        self.inner.lock()
    }
}
