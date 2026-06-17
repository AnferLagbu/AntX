#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! CFS (Completely Fair Scheduler) — 调度策略 — services 层策略主体
//!
//! ## T1-1 迁移记录
//!
//! 原属 framework/proc/cfs.rs, 2026-06-16 提取到 services.
//! 纯策略代码 (权重表 + vruntime 计算 + 时间片计算 + CFS/DL 运行队列), 0 unsafe.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kernel::framework::proc::Pid;

// ============================================================================
// CFS Constants
// ============================================================================

pub use crate::kernel::framework::config::{
    CFS_NICE0_WEIGHT as NICE0_WEIGHT,
    CFS_TARGET_LATENCY as TARGET_LATENCY_TICKS,
    CFS_MIN_GRANULARITY as MIN_GRANULARITY_TICKS,
    CFS_BOOST_INTERVAL as CFS_BOOST_INTERVAL_TICKS,
    CFS_DL_MIN_RUNTIME as DL_MIN_RUNTIME_TICKS,
    CFS_DL_MIN_PERIOD as DL_MIN_PERIOD_TICKS,
    CFS_DL_MAX_UTILIZATION_PCT as DL_MAX_UTILIZATION_PCT,
};

pub const LOAD_BALANCE_THRESHOLD: u64 = 1024;

// ============================================================================
// NICE → 权重映射
// ============================================================================

pub const NICE_TO_WEIGHT: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291,
    29154, 23254, 18705, 14949, 11916,
    9548, 7620, 6100, 4904, 3906,
    3121, 2501, 1991, 1586, 1277,
    1024, 820, 655, 526, 423,
    335, 272, 215, 172, 137,
    110, 87, 70, 56, 45,
    36, 29, 23, 18, 15,
];

#[inline]
pub fn nice_to_weight(nice: i8) -> u64 {
    let clamped = nice.clamp(-20, 19);
    let idx = (clamped + 20) as usize;
    NICE_TO_WEIGHT[idx]
}

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
// Deadline 调度 (EDF + CBS)
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct DeadlineParams {
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
}

impl DeadlineParams {
    pub const fn new() -> Self {
        Self {
            runtime: 0,
            deadline: 0,
            period: 0,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.runtime >= DL_MIN_RUNTIME_TICKS
            && self.deadline >= self.runtime
            && self.period >= self.deadline
            && self.period >= DL_MIN_PERIOD_TICKS
    }

    pub fn utilization_pct(&self) -> u64 {
        if self.period == 0 {
            return 0;
        }
        (self.runtime * 100) / self.period
    }
}

// ============================================================================
// CFS Run Queue
// ============================================================================

pub struct CfsRunQueue {
    pub tree: BTreeMap<(u64, Pid), ()>,
    pub min_vruntime: AtomicU64,
    pub total_weight: AtomicU64,
    pub nr_running: u32,
    pub last_boost_tick: u64,
}

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

    pub fn enqueue(&mut self, pid: Pid, vruntime: u64, weight: u64) {
        let min_vr = self.min_vruntime.load(Ordering::Acquire);
        let start_vr = vruntime.max(min_vr);

        self.tree.insert((start_vr, pid), ());
        self.total_weight.fetch_add(weight, Ordering::Release);
        self.nr_running += 1;
    }

    pub fn dequeue(&mut self, pid: Pid, vruntime: u64, weight: u64) -> bool {
        let mut prev = self.total_weight.load(Ordering::Acquire);
        loop {
            let new = prev.saturating_sub(weight);
            match self.total_weight.compare_exchange_weak(
                prev,
                new,
                Ordering::Release,
                Ordering::Relaxed,
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

    pub fn pick_next(&mut self) -> Option<(Pid, u64)> {
        let (&(vruntime, pid), _) = self.tree.first_key_value()?;
        self.tree.remove(&(vruntime, pid));
        self.sync_min_vruntime();
        Some((pid, vruntime))
    }

    pub fn update_curr(&mut self, pid: Pid, new_vruntime: u64) {
        let min_vr = self.min_vruntime.load(Ordering::Acquire);
        let start_vr = new_vruntime.max(min_vr);
        self.tree.insert((start_vr, pid), ());
    }

    pub fn calc_time_slice(&self, weight: u64) -> u64 {
        let total_w = self.total_weight.load(Ordering::Acquire);
        if total_w == 0 || weight == 0 {
            return MIN_GRANULARITY_TICKS;
        }
        let slice = TARGET_LATENCY_TICKS.saturating_mul(weight) / total_w;
        slice.max(MIN_GRANULARITY_TICKS)
    }

    pub fn get_weighted_load(&self) -> u64 {
        self.total_weight.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.nr_running == 0
    }

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

        let entries: alloc::vec::Vec<(Pid, u64)> =
            self.tree.keys().map(|&(vr, pid)| (pid, vr)).collect();

        self.tree.clear();
        for (pid, _old_vr) in entries {
            self.tree.insert((min_vr, pid), ());
        }

        self.min_vruntime.store(min_vr, Ordering::Release);
        self.last_boost_tick = current_tick;
    }

    pub fn boost_all_vruntime(&mut self) {
        if self.tree.is_empty() {
            return;
        }

        let min_vr = self
            .tree
            .first_key_value()
            .map(|(&(vr, _), _)| vr)
            .unwrap_or(0);

        let entries: alloc::vec::Vec<(Pid, u64)> =
            self.tree.keys().map(|&(vr, pid)| (pid, vr)).collect();

        self.tree.clear();
        for (pid, _old_vr) in entries {
            self.tree.insert((min_vr, pid), ());
        }

        self.min_vruntime.store(min_vr, Ordering::Release);
    }

    fn sync_min_vruntime(&mut self) {
        if let Some((&(min_vr, _), _)) = self.tree.first_key_value() {
            self.min_vruntime.store(min_vr, Ordering::Release);
        }
    }

    pub fn steal_highest_vruntime(&mut self) -> Option<(Pid, u64)> {
        let (&(vruntime, pid), _) = self.tree.last_key_value()?;
        self.tree.remove(&(vruntime, pid));
        self.sync_min_vruntime();
        Some((pid, vruntime))
    }
}

// ============================================================================
// Deadline 运行队列 (EDF)
// ============================================================================

pub struct DlRunQueue {
    pub tree: BTreeMap<(u64, Pid), ()>,
    pub nr_running: u32,
    pub total_utilization: u64,
}

impl DlRunQueue {
    pub fn new() -> Self {
        Self {
            tree: BTreeMap::new(),
            nr_running: 0,
            total_utilization: 0,
        }
    }

    pub fn enqueue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) -> bool {
        if self.total_utilization.saturating_add(util_pct) > DL_MAX_UTILIZATION_PCT {
            return false;
        }
        self.tree.insert((deadline_abs, pid), ());
        self.nr_running += 1;
        self.total_utilization += util_pct;
        true
    }

    pub fn dequeue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) {
        if self.tree.remove(&(deadline_abs, pid)).is_some() {
            self.nr_running = self.nr_running.saturating_sub(1);
            self.total_utilization = self.total_utilization.saturating_sub(util_pct);
        }
    }

    pub fn pick_next(&mut self) -> Option<(Pid, u64)> {
        let (&(dl_abs, pid), _) = self.tree.first_key_value()?;
        self.tree.remove(&(dl_abs, pid));
        Some((pid, dl_abs))
    }

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
// Tick 计数辅助
// ============================================================================

#[inline]
pub fn calc_vruntime_delta(weight: u64) -> u64 {
    if weight == 0 {
        return NICE0_WEIGHT;
    }
    (NICE0_WEIGHT / weight).max(1)
}

#[inline]
pub fn cfs_should_preempt(curr_vruntime: u64, min_vruntime: u64, weight: u64) -> bool {
    let threshold = if weight == 0 {
        MIN_GRANULARITY_TICKS.saturating_mul(NICE0_WEIGHT)
    } else {
        MIN_GRANULARITY_TICKS.saturating_mul(NICE0_WEIGHT) / weight
    };
    curr_vruntime.saturating_sub(min_vruntime) > threshold
}

// ============================================================================
// T-01: MLFQ 调度策略实现 — services 层策略主体
// ============================================================================

use crate::kernel::framework::proc::sched_trait::SchedDecision;
use crate::kernel::framework::proc::types::ThreadPriority;

/// MLFQ 调度策略 — services 层安全实现
///
/// 策略决策 (优先级选择、boost 触发、时间片计算) 全部在此.
/// framework 层的 SchedulerEx 仅保留 RunQueue 操作和上下文切换机制.
///
/// ## 设计
///
/// - 默认行为与原 FallbackMlfqPolicy 一致 (高→低扫描)
/// - 可通过替换此 struct 自定义调度行为 (如 CFS 集成、实时增强)
/// - 在 `services::proc::init()` 中通过 `register_sched_policy()` 注册
pub struct MlfqPolicy;

impl SchedDecision for MlfqPolicy {
    fn pick_next_priority(&self, queue_lengths: [u32; 5]) -> Option<usize> {
        // 从高到低优先级扫描, 与原 scheduler_ex 行为一致
        for prio in (0..5).rev() {
            if queue_lengths[prio] > 0 {
                return Some(prio);
            }
        }
        None
    }

    fn should_boost(&self, tick_count: u64, last_boost: u64) -> bool {
        tick_count.saturating_sub(last_boost) >= CFS_BOOST_INTERVAL_TICKS
    }

    fn boost_target(&self) -> ThreadPriority {
        ThreadPriority::High
    }

    fn time_slice_for(&self, priority: ThreadPriority) -> u32 {
        use crate::kernel::framework::config::*;
        match priority {
            ThreadPriority::Realtime => SCHED_LEVEL_0_QUANTUM,
            ThreadPriority::High => SCHED_LEVEL_1_QUANTUM,
            ThreadPriority::Normal => SCHED_LEVEL_2_QUANTUM,
            ThreadPriority::Low => SCHED_LEVEL_3_QUANTUM,
            ThreadPriority::Idle => u32::MAX,
        }
    }

    fn should_reschedule(&self, time_slice_remaining: u32) -> bool {
        time_slice_remaining <= 1
    }
}

/// 注册 MLFQ 调度策略到 framework
///
/// 由 `services::proc::init()` 调用. 只能注册一次.
pub fn register_mlfq_policy() -> Result<(), ()> {
    static POLICY: MlfqPolicy = MlfqPolicy;
    crate::kernel::framework::proc::register_sched_decision(&POLICY).map_err(|_| ())
}
