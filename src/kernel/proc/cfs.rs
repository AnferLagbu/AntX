//! CFS (Completely Fair Scheduler) — vruntime-based proportional-share scheduling
//!
//! Replaces the legacy MLFQ (Multi-Level Feedback Queue) for SCHED_NORMAL tasks
//! with O(log n) red-black tree selection (implemented via BTreeMap for rapid
//! validation; intrusive RBTree planned for Phase 11 follow-up).
//!
//! ## Architecture
//!
//! ```text
//! schedule()
//!   ├── 1. pick_deadline_task()   ← SCHED_DEADLINE (EDF, highest priority)
//!   ├── 2. pick_rt_task()         ← SCHED_FIFO / SCHED_RR (preserved)
//!   └── 3. cfs.pick_next()        ← SCHED_NORMAL (vruntime minimum)
//! ```
//!
//! ## Core Formulas
//!
//! ```text
//! vruntime_delta = exec_delta_ticks × NICE0_WEIGHT(1024) / entity.weight
//! time_slice     = TARGET_LATENCY × weight / total_weight
//! time_slice     = max(time_slice, MIN_GRANULARITY)
//! ```
//!
//! ## Weight Table
//!
//! NICE_TO_WEIGHT maps nice values [-20, 19] to geometric weights.
//! Each step is ~10% change (≈1.25× per level in reverse).
//! NICE0_WEIGHT = 1024 is the reference.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use super::types::Pid;

// ============================================================================
// CFS Constants
// ============================================================================

pub const NICE0_WEIGHT: u64 = 1024;

/// Target scheduling latency in ticks.
/// 60 ticks ≈ 6ms at 100Hz — desktop-class responsiveness.
pub const TARGET_LATENCY_TICKS: u64 = 60;

/// Minimum time slice granularity in ticks.
/// Prevents excessive context switches under high load.
/// 8 ticks ≈ 0.75ms at 100Hz.
pub const MIN_GRANULARITY_TICKS: u64 = 8;

/// Load-balance threshold: minimum weight difference to trigger migration.
/// ≈1× NICE0 weight — prevents thrashing.
pub const LOAD_BALANCE_THRESHOLD: u64 = 1024;

/// Priority boost interval: every 1000 ticks, all tasks move to min_vruntime
/// to prevent indefinite starvation of low-nice (CPU-bound) tasks.
pub const CFS_BOOST_INTERVAL_TICKS: u64 = 1000;

// ============================================================================
// NICE → Weight Mapping
// ============================================================================

/// Maps nice [-20, 19] to geometric weights.
///
/// Weights decrease by ~10% per nice level (≈1.25× per level in reverse).
/// Index = nice + 20.
pub const NICE_TO_WEIGHT: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, // nice -20 .. -16
    29154, 23254, 18705, 14949, 11916, // nice -15 .. -11
     9548,  7620,  6100,  4904,  3906, // nice -10 ..  -6
     3121,  2501,  1991,  1586,  1277, // nice  -5 ..  -1
     1024,   820,   655,   526,   423, // nice   0 ..   4
      335,   272,   215,   172,   137, // nice   5 ..   9
      110,    87,    70,    56,    45, // nice  10 ..  14
       36,    29,    23,    18,    15, // nice  15 ..  19
];

/// Convert nice value to scheduling weight.
#[inline]
pub fn nice_to_weight(nice: i8) -> u64 {
    let clamped = nice.clamp(-20, 19);
    let idx = (clamped + 20) as usize;
    NICE_TO_WEIGHT[idx]
}

/// Approximate inverse: weight → nice. Returns the closest nice level.
/// Used for diagnostics and backward compatibility (add_with_priority → nice).
#[inline]
pub fn weight_to_nice(weight: u64) -> i8 {
    if weight >= NICE_TO_WEIGHT[0] {
        return -20;
    }
    if weight <= NICE_TO_WEIGHT[39] {
        return 19;
    }
    let mut best = 0i8;
    let mut best_diff = u64::MAX;
    for (i, &w) in NICE_TO_WEIGHT.iter().enumerate() {
        let diff = w.abs_diff(weight);
        if diff < best_diff {
            best_diff = diff;
            best = (i as i32 - 20) as i8;
        }
    }
    best
}

/// Map legacy MLFQ level to nice value for backward-compatible transition.
#[inline]
pub fn mlfq_level_to_nice(level: usize) -> i8 {
    match level {
        0 => -10,
        1 => -4,
        2 => 0,
        3 => 8,
        _ => 0,
    }
}

// ============================================================================
// Deadline Scheduling (EDF + CBS)
// ============================================================================

pub const DL_MIN_RUNTIME_TICKS: u64 = 1;
pub const DL_MIN_PERIOD_TICKS: u64 = 10;
/// Maximum accepted deadline task utilization: 95%.
pub const DL_MAX_UTILIZATION_PCT: u64 = 95;

/// CBS (Constant Bandwidth Server) parameters for a SCHED_DEADLINE task.
#[derive(Debug, Clone, Copy)]
pub struct DeadlineParams {
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
}

impl DeadlineParams {
    pub const fn new() -> Self {
        Self { runtime: 0, deadline: 0, period: 0 }
    }

    pub fn is_valid(&self) -> bool {
        self.runtime >= DL_MIN_RUNTIME_TICKS
            && self.deadline >= self.runtime
            && self.period >= self.deadline
            && self.period >= DL_MIN_PERIOD_TICKS
    }

    pub fn utilization_pct(&self) -> u64 {
        if self.period == 0 { return 0; }
        (self.runtime * 100) / self.period
    }
}

// ============================================================================
// CFS Run Queue
// ============================================================================

/// Per-CPU CFS run queue.
///
/// Uses BTreeMap<(vruntime, pid), ()> as a red-black tree substitute.
/// The (vruntime, pid) tuple key ensures uniqueness even when two tasks
/// share the same vruntime (pid acts as tiebreaker — lower PID first).
pub struct CfsRunQueue {
    tree: BTreeMap<(u64, Pid), ()>,
    pub min_vruntime: AtomicU64,
    pub total_weight: AtomicU64,
    pub nr_running: u32,
    pub last_boost_tick: u64,
}

// SAFETY: CfsRunQueue is per-CPU data accessed only from the owning CPU's
// schedule()/tick() paths. tree is mutated under implicit per-CPU
// serialization. Atomic fields are lock-free.
unsafe impl Send for CfsRunQueue {}
unsafe impl Sync for CfsRunQueue {}

impl CfsRunQueue {
    pub fn new() -> Self {
        Self {
            tree: BTreeMap::new(),
            min_vruntime: AtomicU64::new(0),
            total_weight: AtomicU64::new(0),
            nr_running: 0,
            last_boost_tick: 0,
        }
    }

    /// Enqueue a task. New tasks start at max(min_vruntime, own_vruntime)
    /// to prevent starvation of existing long-running tasks.
    pub fn enqueue(&mut self, pid: Pid, vruntime: u64, weight: u64) {
        let min_vr = self.min_vruntime.load(Ordering::Acquire);
        let start_vr = vruntime.max(min_vr);

        self.tree.insert((start_vr, pid), ());
        self.total_weight.fetch_add(weight, Ordering::Release);
        self.nr_running += 1;
    }

    /// Dequeue a task from the CFS runqueue.
    ///
    /// Always adjusts total_weight and nr_running — even when the task is not
    /// in the tree (e.g. currently running).  This is intentional:
    /// `pick_next() → dequeue()` together form the complete removal sequence
    /// (tree removal + accounting finalisation), used by load_balance.
    ///
    /// total_weight is decremented via a CAS loop that saturates at zero,
    /// preventing underflow to u64::MAX that would corrupt time-slice
    /// calculations.
    pub fn dequeue(&mut self, pid: Pid, vruntime: u64, weight: u64) -> bool {
        let mut prev = self.total_weight.load(Ordering::Acquire);
        loop {
            let new = prev.saturating_sub(weight);
            match self.total_weight.compare_exchange_weak(
                prev, new, Ordering::Release, Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => prev = actual,
            }
        }
        self.nr_running = self.nr_running.saturating_sub(1);
        if self.tree.remove(&(vruntime, pid)).is_some() {
            self.sync_min_vruntime();
            true
        } else {
            false
        }
    }

    /// Pick the task with minimum vruntime (leftmost in tree).
    /// Returns (pid, vruntime) on success. Does NOT adjust total_weight
    /// or nr_running — the task transitions from "waiting" to "running"
    /// and is still part of the runqueue load.
    pub fn pick_next(&mut self) -> Option<(Pid, u64)> {
        let (&(vruntime, pid), _) = self.tree.first_key_value()?;
        self.tree.remove(&(vruntime, pid));
        self.sync_min_vruntime();
        Some((pid, vruntime))
    }

    /// Update the currently-running task's vruntime after execution
    /// and re-enqueue it. Does NOT change total_weight or nr_running
    /// because the task was already counted (pick_next preserves both).
    pub fn update_curr(&mut self, pid: Pid, new_vruntime: u64) {
        let min_vr = self.min_vruntime.load(Ordering::Acquire);
        let start_vr = new_vruntime.max(min_vr);
        self.tree.insert((start_vr, pid), ());
    }

    /// Calculate the dynamic time slice for a task based on its weight
    /// and the total weight of all runnable tasks.
    pub fn calc_time_slice(&self, weight: u64) -> u64 {
        let total_w = self.total_weight.load(Ordering::Acquire);
        if total_w == 0 || weight == 0 {
            return MIN_GRANULARITY_TICKS;
        }
        let slice = TARGET_LATENCY_TICKS.saturating_mul(weight) / total_w;
        slice.max(MIN_GRANULARITY_TICKS)
    }

    /// Get the weighted load of this run queue for SMP load balancing.
    pub fn get_weighted_load(&self) -> u64 {
        self.total_weight.load(Ordering::Acquire)
    }

    /// Returns true when no tasks are runnable in this queue
    /// (nr_running counts all tasks, both waiting in tree and currently running).
    pub fn is_empty(&self) -> bool {
        self.nr_running == 0
    }

    /// Boost all tasks to min_vruntime to prevent indefinite starvation
    /// of low-weight (high-nice) CPU-bound tasks.
    pub fn boost_priority(&mut self, current_tick: u64) {
        if self.tree.is_empty() {
            self.last_boost_tick = current_tick;
            return;
        }

        let min_vr = self
            .tree
            .first_key_value()
            .map(|(&(vr, _), _)| vr)
            .unwrap_or(0);

        let entries: alloc::vec::Vec<(Pid, u64)> = self
            .tree
            .keys()
            .map(|&(vr, pid)| (pid, vr))
            .collect();

        self.tree.clear();
        for (pid, _old_vr) in entries {
            self.tree.insert((min_vr, pid), ());
        }

        self.min_vruntime.store(min_vr, Ordering::Release);
        self.last_boost_tick = current_tick;
    }

    /// Convenience wrapper: boost without tracking the tick.
    pub fn boost_all_vruntime(&mut self) {
        if self.tree.is_empty() {
            return;
        }

        let min_vr = self
            .tree
            .first_key_value()
            .map(|(&(vr, _), _)| vr)
            .unwrap_or(0);

        let entries: alloc::vec::Vec<(Pid, u64)> = self
            .tree
            .keys()
            .map(|&(vr, pid)| (pid, vr))
            .collect();

        self.tree.clear();
        for (pid, _old_vr) in entries {
            self.tree.insert((min_vr, pid), ());
        }

        self.min_vruntime.store(min_vr, Ordering::Release);
    }

    /// Sync min_vruntime to the leftmost key in the tree.
    fn sync_min_vruntime(&mut self) {
        if let Some((&(min_vr, _), _)) = self.tree.first_key_value() {
            self.min_vruntime.store(min_vr, Ordering::Release);
        }
    }

    /// Find and return the pid with the maximum vruntime for load-balance stealing.
    /// Does NOT adjust total_weight or nr_running — the caller will dequeue
    /// the task on the source CPU and enqueue on the destination.
    pub fn steal_highest_vruntime(&mut self) -> Option<(Pid, u64)> {
        let (&(vruntime, pid), _) = self.tree.last_key_value()?;
        self.tree.remove(&(vruntime, pid));
        self.sync_min_vruntime();
        Some((pid, vruntime))
    }
}

// ============================================================================
// Deadline Run Queue (EDF)
// ============================================================================

/// Per-CPU Deadline run queue.
///
/// EDF (Earliest Deadline First): task with the smallest absolute deadline
/// runs first. CBS (Constant Bandwidth Server) enforces bandwidth isolation.
pub struct DlRunQueue {
    tree: BTreeMap<(u64, Pid), ()>,
    pub nr_running: u32,
    pub total_utilization: u64,
}

// SAFETY: DlRunQueue is per-CPU data with the same serialization guarantees
// as CfsRunQueue. tree is only mutated from schedule()/tick() on the owning CPU.
unsafe impl Send for DlRunQueue {}
unsafe impl Sync for DlRunQueue {}

impl DlRunQueue {
    pub fn new() -> Self {
        Self {
            tree: BTreeMap::new(),
            nr_running: 0,
            total_utilization: 0,
        }
    }

    /// Try to enqueue a deadline task. Fails if bandwidth admission control
    /// would exceed 95% total utilization.
    pub fn enqueue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) -> bool {
        if self.total_utilization.saturating_add(util_pct) > DL_MAX_UTILIZATION_PCT {
            return false;
        }
        self.tree.insert((deadline_abs, pid), ());
        self.nr_running += 1;
        self.total_utilization += util_pct;
        true
    }

    /// Dequeue a deadline task.
    pub fn dequeue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) {
        if self.tree.remove(&(deadline_abs, pid)).is_some() {
            self.nr_running = self.nr_running.saturating_sub(1);
            self.total_utilization = self.total_utilization.saturating_sub(util_pct);
        }
    }

    /// Pick the task with earliest absolute deadline.
    /// Does NOT change nr_running or total_utilization — the task was
    /// already counted at initial enqueue and is still part of the
    /// runqueue load（in flight）.
    pub fn pick_next(&mut self) -> Option<(Pid, u64)> {
        let (&(dl_abs, pid), _) = self.tree.first_key_value()?;
        self.tree.remove(&(dl_abs, pid));
        Some((pid, dl_abs))
    }

    /// Re-insert a task into the tree after it was picked or after a
    /// cancelled pick. Does NOT change nr_running or total_utilization
    /// — the task was already counted at initial enqueue.
    pub fn reinsert(&mut self, pid: Pid, dl_abs: u64) {
        self.tree.insert((dl_abs, pid), ());
    }

    pub fn is_empty(&self) -> bool {
        self.nr_running == 0
    }

    pub fn get_load(&self) -> u32 {
        self.nr_running
    }
}

// ============================================================================
// Tick Accounting Helpers
// ============================================================================

/// Update vruntime for one tick of execution.
///
/// vruntime_delta = 1 tick × NICE0_WEIGHT / weight
///
/// Lower-nice (higher-weight) tasks accumulate vruntime slower → get more CPU.
/// For weight > NICE0_WEIGHT (nice < 0), integer division yields 0, which
/// would stall vruntime.  `max(1)` guarantees at least 1 unit per tick.
#[inline]
pub fn calc_vruntime_delta(weight: u64) -> u64 {
    if weight == 0 {
        return NICE0_WEIGHT;
    }
    (NICE0_WEIGHT / weight).max(1)
}

/// Check if the current task should be preempted by a CFS task.
///
/// Preempt when curr.vruntime > min_vruntime + threshold,
/// where threshold = MIN_GRANULARITY × NICE0_WEIGHT / curr.weight.
#[inline]
pub fn cfs_should_preempt(curr_vruntime: u64, min_vruntime: u64, weight: u64) -> bool {
    let threshold = if weight == 0 {
        MIN_GRANULARITY_TICKS.saturating_mul(NICE0_WEIGHT)
    } else {
        MIN_GRANULARITY_TICKS.saturating_mul(NICE0_WEIGHT) / weight
    };
    curr_vruntime.saturating_sub(min_vruntime) > threshold
}