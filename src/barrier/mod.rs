//! # Barrier Stack — AntX 宏内核故障恢复子系统
//!
//! 栏栈是 AntX 在宏内核架构下实现模块级"重生"的核心基础设施。
//! 原理详见 [docs/development/barrier-stack-design.md](docs/development/barrier-stack-design.md)。
//!
//! ## 架构
//!
//! ```text
//! Rust panic!() → panic_handler → PANIC_FLAG → int 0x82
//!   → isr0x82 → exception_handler → recovery_try_recover_from_idt()
//!     → RecoveryManager::find(domain) → try_rollback()
//!       → DomainState::RollingBack → undo.rollback_to(gen)
//!         → mark_recovered() → PANIC_FLAG clear → IDT return
//! ```
//!
//! ## SMP 安全
//!
//! `recovery_try_recover_from_idt()` 使用 `try_lock()` 而非 `lock()`。
//! 若调度器正在 tick() 中持有 RECOVERY_MANAGER 锁，恢复路径会返回 -3 (busy)
//! 而非自旋死锁。调度器 tick 在下一次 tick 会自然释放锁。
//!
//! `int 0x82` 是软件陷阱 (trap gate)，不经过 IOAPIC/8259 中断控制器，
//! 因此无需 EOI 发送。与 IOAPIC/PIC 驱动完全解耦。
//!
//! ## 关键组件
//!
//! - `RecoveryDomain`: 恢复域 — 一个可独立回滚的内核模块
//! - `UndoLog`: 增量撤销日志 — 每个可变操作自动记录旧值
//! - `RecoveryManager`: 全局恢复域管理器 — 最多 32 个域
//! - `PANIC_FLAG`: 全局 panic 信号 — 由 panic_handler 设置, IDT 消费

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Set by panic handler; polled by IDT exception_handler for recovery
pub static PANIC_FLAG: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Panic message buffer for post-recovery diagnostics
pub static mut PANIC_MSG: [u8; 128] = [0u8; 128];

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
        let field_ptr = field as *mut u8;

        // Same-generation same-field dedup: keep only the oldest value
        if self.count > 0 {
            let last = &self.entries[self.count - 1];
            if last.field_ptr == field_ptr && last.generation == self.current_generation {
                return;
            }
        }

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
            field_ptr,
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

const DIRECT_MAP_SIZE: usize = 64;

pub struct RecoveryManager {
    pub domains: [Option<&'static RecoveryDomain>; MAX_RECOVERY_DOMAINS],
    pub direct_map: [Option<&'static RecoveryDomain>; DIRECT_MAP_SIZE],
    pub count: AtomicU32,
}

impl RecoveryManager {
    pub const fn new() -> Self {
        const NONE: Option<&'static RecoveryDomain> = None;
        Self {
            domains: [NONE; MAX_RECOVERY_DOMAINS],
            direct_map: [NONE; DIRECT_MAP_SIZE],
            count: AtomicU32::new(0),
        }
    }

    pub fn register(&mut self, domain: &'static RecoveryDomain) -> Option<u64> {
        let idx = self.count.load(Ordering::SeqCst) as usize;
        if idx >= MAX_RECOVERY_DOMAINS { return None; }
        let id = domain.id as usize;
        if id < DIRECT_MAP_SIZE {
            self.direct_map[id] = Some(domain);
        }
        self.domains[idx] = Some(domain);
        self.count.fetch_add(1, Ordering::SeqCst);
        Some(domain.id)
    }

    pub fn tick(&self, current_tick: u64) {
        if self.count.load(Ordering::Relaxed) == 0 { return; }
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
        let idx = id as usize;
        if idx < DIRECT_MAP_SIZE {
            self.direct_map[idx]
        } else {
            (0..self.count.load(Ordering::SeqCst) as usize)
                .find_map(|i| self.domains[i].filter(|d| d.id == id))
        }
    }
}

pub trait Recoverable {
    fn domain_id(&self) -> u64 { 0 }
    fn capture_barrier(&self, _undo: &mut UndoLog) {}
    fn rollback(&self) -> bool { true }
    fn reset(&self) -> bool { true }
}

pub static RECOVERY_MANAGER: spin::Mutex<RecoveryManager> = spin::Mutex::new(RecoveryManager::new());

// ── C FFI ──

/// C FFI: advance barrier generations
#[no_mangle]
pub extern "C" fn recovery_barrier_maintenance() {
    use crate::proc::scheduler::TICK_COUNT;
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

/// C FFI: unregister a recovery domain (for test cleanup)
#[no_mangle]
pub extern "C" fn recovery_domain_unregister(domain_id: u64) -> i32 {
    let mut mgr = RECOVERY_MANAGER.lock();
    let count = mgr.count.load(Ordering::SeqCst) as usize;
    for i in 0..count {
        if let Some(dom) = mgr.domains[i] {
            if dom.id == domain_id {
                mgr.domains[i] = None;
                let id_idx = domain_id as usize;
                if id_idx < DIRECT_MAP_SIZE {
                    mgr.direct_map[id_idx] = None;
                }
                return 0;
            }
        }
    }
    -1
}

/// C FFI: test rollback trigger
#[no_mangle]
pub extern "C" fn recovery_test_rollback(domain_id: u64, crash_fingerprint: u64) -> i32 {
    use crate::proc::scheduler::TICK_COUNT;
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

/// Internal: prevent IDT recovery loop
static RECOVERY_ATTEMPTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// C FFI: check if panic flag is set (polled by IDT exception_handler)
#[no_mangle]
pub extern "C" fn recovery_panic_flag_is_set() -> bool {
    PANIC_FLAG.load(Ordering::SeqCst)
}

/// C FFI: clear panic flag after recovery attempt
#[no_mangle]
pub extern "C" fn recovery_panic_flag_clear() {
    PANIC_FLAG.store(false, Ordering::SeqCst)
}

/// C FFI: attempt recovery for the first registered domain (called from IDT on fatal)
/// Returns 0 on success, -1 if no domains to recover, -2 if already attempted, -3 if lock busy
#[no_mangle]
pub extern "C" fn recovery_try_recover_from_idt() -> i32 {
    use crate::proc::scheduler::TICK_COUNT;
    let tick = TICK_COUNT.load(Ordering::SeqCst);

    if RECOVERY_ATTEMPTED.swap(true, Ordering::SeqCst) {
        return -2;
    }

    let Some(mgr) = RECOVERY_MANAGER.try_lock() else {
        // SMP safety: scheduler tick is in progress holding the lock;
        // return -3 (busy/retry) instead of deadlocking
        RECOVERY_ATTEMPTED.store(false, Ordering::SeqCst);
        return -3;
    };

    let count = mgr.count.load(Ordering::SeqCst) as usize;
    if count == 0 {
        return -1;
    }

    // Compute crash fingerprint: djb2 hash of PANIC_MSG
    let fingerprint = {
        let mut h: u64 = 5381;
        unsafe {
            for &b in PANIC_MSG.iter().take(128) {
                if b == 0 { break; }
                h = h.wrapping_mul(33).wrapping_add(b as u64);
            }
        }
        h
    };

    // Try all registered domains — not just domains[0]
    for i in 0..count {
        if let Some(dom) = mgr.domains[i] {
            if dom.try_rollback(tick, fingerprint) {
                dom.state.store(DomainState::RollingBack as u32, Ordering::SeqCst);
                let target_gen = dom.barrier_generation.load(Ordering::SeqCst);
                let mut undo = dom.undo.lock();
                undo.rollback_to(target_gen);
                dom.state.store(DomainState::Active as u32, Ordering::SeqCst);
                dom.mark_recovered();
                recovery_panic_flag_clear();
                RECOVERY_ATTEMPTED.store(false, Ordering::SeqCst);
                return 0;
            }
        }
    }
    -1
}

/// C FFI: deliberately trigger a panic for end-to-end recovery testing
#[no_mangle]
pub extern "C" fn recovery_trigger_panic() -> ! {
    PANIC_FLAG.store(true, Ordering::SeqCst);
    panic!("[RECOVERY-TEST] Deliberate panic for barrier-stack E2E test");
}

/// C FFI: check if recovery was attempted (for test verification)
#[no_mangle]
pub extern "C" fn recovery_was_attempted() -> i32 {
    if RECOVERY_ATTEMPTED.load(Ordering::SeqCst) { 1 } else { 0 }
}

/// C FFI: record a test value into a domain's UndoLog
#[no_mangle]
pub extern "C" fn recovery_undo_record(domain_id: u64, field_ptr: *mut u8, old_val: u64) -> i32 {
    let mgr = RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        let mut undo = dom.undo.lock();
        undo.record(field_ptr as *mut u64, old_val);
        0
    } else {
        -1
    }
}

/// C FFI: return current UndoLog entry count for a domain
#[no_mangle]
pub extern "C" fn recovery_undo_count(domain_id: u64) -> i32 {
    let mgr = RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        dom.undo.lock().count as i32
    } else {
        -1
    }
}
