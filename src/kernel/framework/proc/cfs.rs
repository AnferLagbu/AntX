//! CFS (Completely Fair Scheduler) — framework 层 re-export
//!
//! ## T1-1 迁移记录
//!
//! 策略代码 (权重表 + vruntime 计算 + 时间片计算 + CFS/DL 运行队列)
//! 已于 2026-06-16 迁移到 `services::proc::sched_policy`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::proc::sched_policy::{
    CFS_BOOST_INTERVAL_TICKS, CfsRunQueue, DL_MAX_UTILIZATION_PCT, DL_MIN_PERIOD_TICKS,
    DL_MIN_RUNTIME_TICKS, DeadlineParams, DlRunQueue, LOAD_BALANCE_THRESHOLD,
    MIN_GRANULARITY_TICKS, NICE_TO_WEIGHT, NICE0_WEIGHT, TARGET_LATENCY_TICKS, calc_vruntime_delta,
    cfs_should_preempt, nice_to_weight, weight_to_nice,
};
