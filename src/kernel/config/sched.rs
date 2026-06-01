//! 调度器常量: CFS 目标延迟/粒度/Deadline/调度级别量子

// ============================================================================
// CFS (Completely Fair Scheduler) 常量
// ============================================================================

/// CFS target latency in scheduler ticks.
pub const CFS_TARGET_LATENCY: u64 = 60;

/// CFS minimum granularity in scheduler ticks.
pub const CFS_MIN_GRANULARITY: u64 = 8;

/// CFS boost interval in scheduler ticks.
pub const CFS_BOOST_INTERVAL: u64 = 1000;

/// Default nice-0 weight in CFS.
pub const CFS_NICE0_WEIGHT: u64 = 1024;

// ============================================================================
// Deadline Scheduling (EDF + CBS)
// ============================================================================

/// Deadline scheduler minimum runtime in ticks.
pub const CFS_DL_MIN_RUNTIME: u64 = 1;

/// Deadline scheduler minimum period in ticks.
pub const CFS_DL_MIN_PERIOD: u64 = 10;

/// Deadline scheduler max utilization percent.
pub const CFS_DL_MAX_UTILIZATION_PCT: u64 = 95;

// ============================================================================
// 通用调度器常量
// ============================================================================

/// Scheduler level 0 quantum (highest priority, real-time).
pub const SCHED_LEVEL_0_QUANTUM: u32 = 80;

/// Scheduler level 1 quantum.
pub const SCHED_LEVEL_1_QUANTUM: u32 = 60;

/// Scheduler level 2 quantum.
pub const SCHED_LEVEL_2_QUANTUM: u32 = 40;

/// Scheduler level 3 quantum (lowest priority, idle).
pub const SCHED_LEVEL_3_QUANTUM: u32 = 20;

/// Scheduler boost check interval (ticks).
pub const SCHED_BOOST_INTERVAL: u64 = 1000;

/// Real-time scheduler watchdog timeout (ticks).
pub const SCHED_RT_WATCHDOG_TICKS: u64 = 500;
