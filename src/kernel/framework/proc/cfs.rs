//! CFS (Completely Fair Scheduler) — 基于 vruntime 的比例份额调度
//!
//! 取代旧版 MLFQ (Multi-Level Feedback Queue) 调度 SCHED_NORMAL 任务,
//! 采用 O(log n) 红黑树选取 (用 BTreeMap 实现以加速验证;
//! 侵入式 RBTree 计划在 Phase 11 跟进).
//!
//! ## 架构
//!
//! ```text
//! schedule()
//!   ├── 1. pick_deadline_task()   ← SCHED_DEADLINE (EDF, 最高优先级)
//!   ├── 2. pick_rt_task()         ← SCHED_FIFO / SCHED_RR (保留)
//!   └── 3. cfs.pick_next()        ← SCHED_NORMAL (vruntime 最小者)
//! ```
//!
//! ## 核心公式
//!
//! ```text
//! vruntime_delta = exec_delta_ticks × NICE0_WEIGHT(1024) / entity.weight
//! time_slice     = TARGET_LATENCY × weight / total_weight
//! time_slice     = max(time_slice, MIN_GRANULARITY)
//! ```
//!
//! ## 权重表
//!
//! NICE_TO_WEIGHT 把 nice 值 [-20, 19] 映射为几何权重.
//! 每级变化 ~10% (反向 ≈1.25×/级).
//! NICE0_WEIGHT = 1024 作为参考基准.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use super::types::Pid;

// ============================================================================
// CFS Constants (统一从 config.rs 引用)
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

/// 负载均衡阈值: 触发迁移所需的最小权重差.
/// ≈1× NICE0 权重 —— 防止抖动.
pub const LOAD_BALANCE_THRESHOLD: u64 = 1024;

// ============================================================================
// NICE → 权重映射
// ============================================================================

/// 把 nice [-20, 19] 映射为几何权重.
///
/// 权重随 nice 每级降低约 10% (反向 ≈1.25×/级).
/// 索引 = nice + 20.
pub const NICE_TO_WEIGHT: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, // nice -20 .. -16
    29154, 23254, 18705, 14949, 11916, // nice -15 .. -11
    9548, 7620, 6100, 4904, 3906, // nice -10 ..  -6
    3121, 2501, 1991, 1586, 1277, // nice  -5 ..  -1
    1024, 820, 655, 526, 423, // nice   0 ..   4
    335, 272, 215, 172, 137, // nice   5 ..   9
    110, 87, 70, 56, 45, // nice  10 ..  14
    36, 29, 23, 18, 15, // nice  15 ..  19
];

/// 把 nice 值转换为调度权重.
#[inline]
pub fn nice_to_weight(nice: i8) -> u64 {
    let clamped = nice.clamp(-20, 19);
    let idx = (clamped + 20) as usize;
    NICE_TO_WEIGHT[idx]
}

/// 近似反函数: 权重 → nice. 返回最接近的 nice 级.
/// 用于诊断与向后兼容 (add_with_priority → nice).
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

/// 将旧版 MLFQ 等级映射到 nice 值, 用于向后兼容过渡.
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
//
// 三个常量 (DL_MIN_RUNTIME_TICKS, DL_MIN_PERIOD_TICKS,
// DL_MAX_UTILIZATION_PCT) 已在文件顶部 pub use config::* 引入,
// 此处仅保留文档说明.

/// SCHED_DEADLINE 任务的 CBS (Constant Bandwidth Server) 参数.
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

/// 每 CPU CFS 运行队列.
///
/// 用 BTreeMap<(vruntime, pid), ()> 作为红黑树的替代.
/// (vruntime, pid) 元组键即使在两个任务共享同一 vruntime 时
/// 也能保证唯一性 (pid 用作决胜者 —— PID 小者优先).
pub struct CfsRunQueue {
    tree: BTreeMap<(u64, Pid), ()>,
    pub min_vruntime: AtomicU64,
    pub total_weight: AtomicU64,
    pub nr_running: u32,
    pub last_boost_tick: u64,
}

// CfsRunQueue 始终存放在调度器持有的 Mutex 内.
// 所有字段自动实现 Send; Sync 由外层 Mutex 提供.

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

    /// 入队一个任务. 新任务起点为 max(min_vruntime, own_vruntime),
    /// 防止长时间运行的任务被饿死.
    pub fn enqueue(&mut self, pid: Pid, vruntime: u64, weight: u64) {
        let min_vr = self.min_vruntime.load(Ordering::Acquire);
        let start_vr = vruntime.max(min_vr);

        self.tree.insert((start_vr, pid), ());
        self.total_weight.fetch_add(weight, Ordering::Release);
        self.nr_running += 1;
    }

    /// 从 CFS 运行队列出队一个任务.
    ///
    /// 始终会调整 total_weight 与 nr_running —— 即便任务不在树中
    /// (例如正在运行). 这是有意为之:
    /// `pick_next() → dequeue()` 共同构成完整的移除流程
    /// (树移除 + 计数终结), 供 load_balance 使用.
    ///
    /// total_weight 通过 CAS 循环递减, 在零处饱和,
    /// 防止回绕到 u64::MAX 破坏时间片计算.
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

    /// 选取 vruntime 最小的任务 (树最左).
    /// 成功时返回 (pid, vruntime). 不修改 total_weight
    /// 或 nr_running —— 任务状态从 "等待" 切换到 "运行",
    /// 仍计入运行队列负载.
    pub fn pick_next(&mut self) -> Option<(Pid, u64)> {
        let (&(vruntime, pid), _) = self.tree.first_key_value()?;
        self.tree.remove(&(vruntime, pid));
        self.sync_min_vruntime();
        Some((pid, vruntime))
    }

    /// 在执行后更新当前运行任务的 vruntime 并重新入队.
    /// 不修改 total_weight 或 nr_running,
    /// 因为任务此前已被计入 (pick_next 已保留这两项).
    pub fn update_curr(&mut self, pid: Pid, new_vruntime: u64) {
        let min_vr = self.min_vruntime.load(Ordering::Acquire);
        let start_vr = new_vruntime.max(min_vr);
        self.tree.insert((start_vr, pid), ());
    }

    /// 根据任务权重与所有可运行任务的总权重计算动态时间片.
    pub fn calc_time_slice(&self, weight: u64) -> u64 {
        let total_w = self.total_weight.load(Ordering::Acquire);
        if total_w == 0 || weight == 0 {
            return MIN_GRANULARITY_TICKS;
        }
        let slice = TARGET_LATENCY_TICKS.saturating_mul(weight) / total_w;
        slice.max(MIN_GRANULARITY_TICKS)
    }

    /// 获取本运行队列的加权负载, 用于 SMP 负载均衡.
    pub fn get_weighted_load(&self) -> u64 {
        self.total_weight.load(Ordering::Acquire)
    }

    /// 当本队列中没有可运行任务时返回 true
    /// (nr_running 统计所有任务, 包括树中等待的与正在运行的).
    pub fn is_empty(&self) -> bool {
        self.nr_running == 0
    }

    /// 将所有任务提升到 min_vruntime, 防止低权重 (高 nice) CPU 密集型任务
    /// 长期被饿死.
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

    /// 便捷包装: 提升 vruntime 而不记录 tick.
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

    /// 将 min_vruntime 同步为树最左键.
    fn sync_min_vruntime(&mut self) {
        if let Some((&(min_vr, _), _)) = self.tree.first_key_value() {
            self.min_vruntime.store(min_vr, Ordering::Release);
        }
    }

    /// 查找并返回 vruntime 最大的 pid, 用于负载均衡偷取.
    /// 不修改 total_weight 或 nr_running —— 调用方会在源 CPU 上 dequeue,
    /// 在目标 CPU 上 enqueue.
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

/// 每 CPU Deadline 运行队列.
///
/// EDF (Earliest Deadline First): 绝对 deadline 最早的任务先运行.
/// CBS (Constant Bandwidth Server) 强制带宽隔离.
pub struct DlRunQueue {
    tree: BTreeMap<(u64, Pid), ()>,
    pub nr_running: u32,
    pub total_utilization: u64,
}

// DlRunQueue 始终存放在调度器持有的 Mutex 内.
// 所有字段自动实现 Send; Sync 由外层 Mutex 提供.

impl DlRunQueue {
    pub fn new() -> Self {
        Self {
            tree: BTreeMap::new(),
            nr_running: 0,
            total_utilization: 0,
        }
    }

    /// 尝试入队一个 deadline 任务. 若带宽接纳控制会令
    /// 总利用率超过 95% 时返回 false.
    pub fn enqueue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) -> bool {
        if self.total_utilization.saturating_add(util_pct) > DL_MAX_UTILIZATION_PCT {
            return false;
        }
        self.tree.insert((deadline_abs, pid), ());
        self.nr_running += 1;
        self.total_utilization += util_pct;
        true
    }

    /// 出队一个 deadline 任务.
    pub fn dequeue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) {
        if self.tree.remove(&(deadline_abs, pid)).is_some() {
            self.nr_running = self.nr_running.saturating_sub(1);
            self.total_utilization = self.total_utilization.saturating_sub(util_pct);
        }
    }

    /// 选取绝对 deadline 最早的任务.
    /// 不修改 nr_running 或 total_utilization —— 任务已在最初
    /// 入队时计入, 仍属于运行队列负载 (in flight).
    pub fn pick_next(&mut self) -> Option<(Pid, u64)> {
        let (&(dl_abs, pid), _) = self.tree.first_key_value()?;
        self.tree.remove(&(dl_abs, pid));
        Some((pid, dl_abs))
    }

    /// 在选取之后或取消选取时把任务重新插入树.
    /// 不修改 nr_running 或 total_utilization
    /// —— 任务在最初入队时已计入.
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

/// 对一个 tick 的执行更新 vruntime.
///
/// 公式: `vruntime_delta = 1 tick × NICE0_WEIGHT / weight`
///
/// nice 较低 (权重高) 的任务 vruntime 增长更慢 → 获得更多 CPU.
/// 当 weight > NICE0_WEIGHT (nice < 0) 时整数除法得 0,
/// 会让 vruntime 停滞. `max(1)` 保证每 tick 至少 1 单位.
#[inline]
pub fn calc_vruntime_delta(weight: u64) -> u64 {
    if weight == 0 {
        return NICE0_WEIGHT;
    }
    (NICE0_WEIGHT / weight).max(1)
}

/// 检查当前任务是否应被一个 CFS 任务抢占.
///
/// 当 curr.vruntime > min_vruntime + threshold 时抢占,
/// 其中 threshold = MIN_GRANULARITY × NICE0_WEIGHT / curr.weight.
#[inline]
pub fn cfs_should_preempt(curr_vruntime: u64, min_vruntime: u64, weight: u64) -> bool {
    let threshold = if weight == 0 {
        MIN_GRANULARITY_TICKS.saturating_mul(NICE0_WEIGHT)
    } else {
        MIN_GRANULARITY_TICKS.saturating_mul(NICE0_WEIGHT) / weight
    };
    curr_vruntime.saturating_sub(min_vruntime) > threshold
}
