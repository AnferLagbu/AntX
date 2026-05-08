use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const MAX_RECOVERY_DOMAINS: usize = 32;
pub const MAX_DOMAIN_DEPENDENCIES: usize = 8;
pub const MAX_UNDO_ENTRIES: usize = 256;
pub const DEFAULT_BARRIER_INTERVAL: u64 = 100;
pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;
pub const BACKOFF_BASE_TICKS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainState {
    Active = 0,
    Freezing = 1,
    RollingBack = 2,
    Recovering = 3,
    Quarantined = 4,
}

impl DomainState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Active, 1 => Self::Freezing,
            2 => Self::RollingBack, 3 => Self::Recovering,
            4 => Self::Quarantined, _ => Self::Active,
        }
    }
}

#[derive(Clone, Copy)]
pub struct UndoEntry {
    pub generation: u64,
    pub field_ptr: *mut u8,
    pub old_value: [u8; 8],
}

unsafe impl Send for UndoEntry {}
unsafe impl Sync for UndoEntry {}

pub struct UndoLog {
    pub entries: [UndoEntry; MAX_UNDO_ENTRIES],
    pub count: usize,
    pub current_generation: u64,
}

unsafe impl Send for UndoLog {}
unsafe impl Sync for UndoLog {}

impl UndoLog {
    pub fn new() -> Self {
        Self {
            entries: [UndoEntry {
                generation: 0, field_ptr: core::ptr::null_mut(), old_value: [0u8; 8],
            }; MAX_UNDO_ENTRIES],
            count: 0,
            current_generation: 0,
        }
    }

    pub fn record<T: Copy>(&mut self, field: *mut T, old_value: T) {
        if self.count >= MAX_UNDO_ENTRIES {
            self.emergency_compact(self.current_generation.saturating_sub(1));
        }
        let raw = unsafe {
            core::slice::from_raw_parts(
                &old_value as *const T as *const u8,
                core::mem::size_of::<T>().min(8),
            )
        };
        let mut old_bytes = [0u8; 8];
        let len = raw.len().min(8);
        old_bytes[..len].copy_from_slice(&raw[..len]);

        self.entries[self.count] = UndoEntry {
            generation: self.current_generation,
            field_ptr: field as *mut u8,
            old_value: old_bytes,
        };
        self.count += 1;
    }

    pub fn rollback_to(&mut self, target_generation: u64) -> usize {
        let mut rolled_back = 0;
        while self.count > 0 {
            let entry = &self.entries[self.count - 1];
            if entry.generation < target_generation { break; }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    entry.old_value.as_ptr(), entry.field_ptr, 8,
                );
            }
            self.count -= 1;
            rolled_back += 1;
        }
        if self.count > MAX_UNDO_ENTRIES / 2 { self.compact(); }
        rolled_back
    }

    fn emergency_compact(&mut self, keep_gen: u64) {
        let mut write = 0;
        for i in 0..self.count {
            if self.entries[i].generation >= keep_gen {
                self.entries[write] = self.entries[i];
                write += 1;
            }
        }
        self.count = write;
    }

    fn compact(&mut self) {
        if self.count == 0 { return; }
        let oldest_keep = self.entries[self.count / 4].generation;
        self.emergency_compact(oldest_keep);
    }
}

pub struct RecoveryDomain {
    pub id: u64,
    pub state: AtomicU32,
    pub barrier_generation: AtomicU64,
    pub barrier_interval_ticks: u64,
    pub next_barrier_tick: AtomicU64,
    pub rollback_count: AtomicU32,
    pub consecutive_failures: AtomicU32,
    pub last_crash_fingerprint: AtomicU64,
    pub last_rollback_time: AtomicU64,
    pub backoff_until: AtomicU64,
    pub depends_on: [Option<u64>; MAX_DOMAIN_DEPENDENCIES],
    pub dom_cap_mask: AtomicU64,
    pub cpu_quota_max: u64,
    pub cpu_quota_period: u64,
    pub cpu_quota_consumed: AtomicU64,
    pub proc_limit_max: u32,
    pub proc_limit_current: AtomicU32,
    pub undo: spin::Mutex<UndoLog>,
}

unsafe impl Send for RecoveryDomain {}
unsafe impl Sync for RecoveryDomain {}

impl RecoveryDomain {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            state: AtomicU32::new(DomainState::Active as u32),
            barrier_generation: AtomicU64::new(0),
            barrier_interval_ticks: DEFAULT_BARRIER_INTERVAL,
            next_barrier_tick: AtomicU64::new(DEFAULT_BARRIER_INTERVAL),
            rollback_count: AtomicU32::new(0),
            consecutive_failures: AtomicU32::new(0),
            last_crash_fingerprint: AtomicU64::new(0),
            last_rollback_time: AtomicU64::new(0),
            backoff_until: AtomicU64::new(0),
            depends_on: [None; MAX_DOMAIN_DEPENDENCIES],
            dom_cap_mask: AtomicU64::new(0),
            cpu_quota_max: 0,
            cpu_quota_period: 0,
            cpu_quota_consumed: AtomicU64::new(0),
            proc_limit_max: 0,
            proc_limit_current: AtomicU32::new(0),
            undo: spin::Mutex::new(UndoLog::new()),
        }
    }

    pub fn get_state(&self) -> DomainState {
        DomainState::from_u32(self.state.load(Ordering::SeqCst))
    }

    pub fn is_active(&self) -> bool {
        self.get_state() == DomainState::Active
    }

    pub fn try_rollback(&self, current_tick: u64, crash_fingerprint: u64) -> bool {
        let failures = self.consecutive_failures.load(Ordering::SeqCst);
        if failures >= MAX_CONSECUTIVE_FAILURES {
            self.state.store(DomainState::Quarantined as u32, Ordering::SeqCst);
            return false;
        }
        let prev_fp = self.last_crash_fingerprint.swap(crash_fingerprint, Ordering::SeqCst);
        if crash_fingerprint != 0 && prev_fp == crash_fingerprint {
            self.consecutive_failures.store(MAX_CONSECUTIVE_FAILURES, Ordering::SeqCst);
            self.state.store(DomainState::Quarantined as u32, Ordering::SeqCst);
            return false;
        }
        let backoff = (1u64 << failures.min(8)) * BACKOFF_BASE_TICKS;
        let bu = self.backoff_until.load(Ordering::SeqCst);
        if current_tick < bu { return false; }
        self.backoff_until.store(current_tick + backoff, Ordering::SeqCst);
        self.consecutive_failures.fetch_add(1, Ordering::SeqCst);
        self.rollback_count.fetch_add(1, Ordering::SeqCst);
        self.last_rollback_time.store(current_tick, Ordering::SeqCst);
        true
    }

    pub fn mark_recovered(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
    }

    pub fn consume_quota_tick(&self) -> bool {
        if self.cpu_quota_max == 0 { return false; }
        let c = self.cpu_quota_consumed.fetch_add(1, Ordering::SeqCst) + 1;
        c >= self.cpu_quota_max
    }
}

pub struct RecoveryManager {
    pub domains: [Option<&'static RecoveryDomain>; MAX_RECOVERY_DOMAINS],
    pub count: AtomicU32,
}

impl RecoveryManager {
    pub const fn new() -> Self {
        const NONE: Option<&'static RecoveryDomain> = None;
        Self { domains: [NONE; MAX_RECOVERY_DOMAINS], count: AtomicU32::new(0) }
    }

    pub fn register(&mut self, domain: &'static RecoveryDomain) -> Option<u64> {
        let idx = self.count.load(Ordering::SeqCst) as usize;
        if idx >= MAX_RECOVERY_DOMAINS { return None; }
        self.domains[idx] = Some(domain);
        self.count.fetch_add(1, Ordering::SeqCst);
        Some(domain.id)
    }

    pub fn tick(&self, current_tick: u64) {
        for i in 0..self.count.load(Ordering::SeqCst) as usize {
            if let Some(dom) = self.domains[i] {
                if dom.is_active() && current_tick >= dom.next_barrier_tick.load(Ordering::SeqCst) {
                    dom.barrier_generation.fetch_add(1, Ordering::SeqCst);
                    dom.next_barrier_tick.store(
                        current_tick + dom.barrier_interval_ticks, Ordering::SeqCst,
                    );
                }
            }
        }
    }

    pub fn find(&self, id: u64) -> Option<&'static RecoveryDomain> {
        (0..self.count.load(Ordering::SeqCst) as usize)
            .find_map(|i| self.domains[i].filter(|d| d.id == id))
    }
}

pub trait Recoverable {
    fn domain_id(&self) -> u64 { 0 }
    fn capture_barrier(&self, _undo: &mut UndoLog) {}
    fn rollback(&self) -> bool { true }
    fn reset(&self) -> bool { true }
}

pub static RECOVERY_MANAGER: spin::Mutex<RecoveryManager> = spin::Mutex::new(RecoveryManager::new());

/// C FFI: advance barrier generations
#[no_mangle]
pub extern "C" fn recovery_barrier_maintenance() {
    use super::scheduler::TICK_COUNT;
    let tick = TICK_COUNT.load(Ordering::SeqCst);
    RECOVERY_MANAGER.lock().tick(tick);
}

/// C FFI: register a recovery domain. Returns 0 on success, -1 on failure.
/// The domain is heap-allocated and leaked (lifetime = kernel lifetime).
#[no_mangle]
pub extern "C" fn recovery_domain_register(domain_id: u64) -> i32 {
    let domain: &'static RecoveryDomain = {
        let bx = alloc::boxed::Box::new(RecoveryDomain::new(domain_id));
        alloc::boxed::Box::leak(bx)
    };
    match RECOVERY_MANAGER.lock().register(domain) {
        Some(_) => 0,
        None => -1,
    }
}

/// C FFI: test rollback trigger
#[no_mangle]
pub extern "C" fn recovery_test_rollback(domain_id: u64, crash_fingerprint: u64) -> i32 {
    use super::scheduler::TICK_COUNT;
    let tick = TICK_COUNT.load(Ordering::SeqCst);
    let mgr = RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        if dom.try_rollback(tick, crash_fingerprint) {
            dom.state.store(DomainState::RollingBack as u32, Ordering::SeqCst);
            let target_gen = dom.barrier_generation.load(Ordering::SeqCst);
            let mut undo = dom.undo.lock();
            undo.rollback_to(target_gen);
            dom.state.store(DomainState::Active as u32, Ordering::SeqCst);
            dom.mark_recovered();
            0
        } else {
            -1
        }
    } else {
        -1
    }
}
