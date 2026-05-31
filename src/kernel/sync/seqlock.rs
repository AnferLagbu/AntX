//! SeqLock — Sequence Lock
//!
//! A lock primitive optimized for read-mostly-write-rarely scenarios.
//! Readers never block writers; writers mutually exclude each other.
//!
//! # When to use
//! - Interrupt statistics (read every timer tick, write on IRQ)
//! - Performance counters (read frequently, write rarely)
//! - Any data where stale reads are acceptable but torn reads are not
//!
//! # Algorithm
//! ```text
//! Writer:  sequence++ → write data → sequence++
//! Reader:  do {
//!            s1 = sequence
//!            if s1 is odd: continue  (write in progress)
//!            read data
//!            s2 = sequence
//!          } while (s1 != s2)
//! ```
//!
//! # Safety
//! `UnsafeCell<T>` provides interior mutability. Writer-side safety is
//! ensured by `AtomicUsize sequence` (odd = writing). `unsafe impl Sync`
//! is sound because all writes occur under the sequence lock.

use core::cell::UnsafeCell;
use core::sync::atomic::{compiler_fence, AtomicUsize, Ordering};

pub struct SeqLock<T> {
    pub(crate) sequence: AtomicUsize,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SeqLock<T> {}

impl<T> SeqLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            sequence: AtomicUsize::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn current_sequence(&self) -> usize {
        self.sequence.load(Ordering::Relaxed)
    }

    pub fn read(&self) -> SeqLockReadGuard<'_, T> {
        loop {
            let seq1 = self.sequence.load(Ordering::Acquire);

            if seq1 & 1 == 1 {
                core::hint::spin_loop();
                continue;
            }

            compiler_fence(Ordering::Acquire);

            return SeqLockReadGuard { lock: self, seq1 };
        }
    }

    pub fn write(&self) -> SeqLockWriteGuard<'_, T> {
        loop {
            let current = self.sequence.load(Ordering::Relaxed);
            if current & 1 == 1 {
                core::hint::spin_loop();
                continue;
            }
            if self
                .sequence
                .compare_exchange_weak(current, current + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                compiler_fence(Ordering::Acquire);
                return SeqLockWriteGuard { lock: self };
            }
        }
    }

    pub fn try_write(&self) -> Option<SeqLockWriteGuard<'_, T>> {
        let current = self.sequence.load(Ordering::Relaxed);
        if current & 1 == 1 {
            return None;
        }

        if self
            .sequence
            .compare_exchange(current, current | 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            compiler_fence(Ordering::Acquire);
            Some(SeqLockWriteGuard { lock: self })
        } else {
            None
        }
    }
}

pub struct SeqLockReadGuard<'a, T> {
    lock: &'a SeqLock<T>,
    seq1: usize,
}

impl<'a, T> SeqLockReadGuard<'a, T> {
    pub fn is_valid(&self) -> bool {
        compiler_fence(Ordering::Release);
        let seq2 = self.lock.sequence.load(Ordering::Acquire);
        seq2 == self.seq1
    }

    pub fn get(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }

    pub fn get_valid(&self) -> Option<&T> {
        if self.is_valid() {
            Some(self.get())
        } else {
            None
        }
    }
}

impl<'a, T> core::ops::Deref for SeqLockReadGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

pub struct SeqLockWriteGuard<'a, T> {
    lock: &'a SeqLock<T>,
}

impl<'a, T> core::ops::Deref for SeqLockWriteGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> core::ops::DerefMut for SeqLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T> Drop for SeqLockWriteGuard<'a, T> {
    fn drop(&mut self) {
        compiler_fence(Ordering::Release);
        self.lock.sequence.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seqlock_basic() {
        let lock = SeqLock::new(42u32);
        {
            let guard = lock.read();
            assert!(guard.is_valid());
            assert_eq!(*guard, 42);
        }
    }

    #[test]
    fn test_seqlock_write() {
        let lock = SeqLock::new(0u32);
        {
            let mut w = lock.write();
            *w = 100;
        }
        let r = lock.read();
        assert!(r.is_valid());
        assert_eq!(*r, 100);
    }

    #[test]
    fn test_seqlock_sequence_increments() {
        let lock = SeqLock::new(0u32);
        assert_eq!(lock.sequence.load(Ordering::Relaxed), 0);
        {
            let _w = lock.write();
            assert_eq!(lock.sequence.load(Ordering::Relaxed), 1);
        }
        assert_eq!(lock.sequence.load(Ordering::Relaxed), 2);
    }
}

#[cfg(feature = "kernel_test")]
pub fn register_seqlock_tests() {
    crate::kernel::tests::sync::register_seqlock_tests();
}
