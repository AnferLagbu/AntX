//! CFS (Completely Fair Scheduler) — framework 层 re-export
//!
//! ## T1-1 迁移记录
//!
//! 策略代码 (权重表 + vruntime 计算 + 时间片计算 + CFS/DL 运行队列)
//! 已于 2026-06-16 迁移到 services::proc::sched_policy.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::proc::sched_policy::{
    NICE0_WEIGHT, TARGET_LATENCY_TICKS, MIN_GRANULARITY_TICKS,
    CFS_BOOST_INTERVAL_TICKS, DL_MIN_RUNTIME_TICKS, DL_MIN_PERIOD_TICKS,
    DL_MAX_UTILIZATION_PCT, LOAD_BALANCE_THRESHOLD,
    NICE_TO_WEIGHT,
    nice_to_weight, weight_to_nice,
    DeadlineParams,
    CfsRunQueue, DlRunQueue,
    calc_vruntime_delta, cfs_should_preempt,
};
