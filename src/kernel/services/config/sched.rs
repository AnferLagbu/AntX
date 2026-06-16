#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯常量定义。
//! 调度器常量 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/config/sched.rs, 2026-06-16 提取到 services.
//! 纯常量定义, 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export.

//! 调度器常量: CFS 目标延迟/粒度/Deadline/调度级别量子

// ============================================================================
// CFS (Completely Fair Scheduler) 常量
// ============================================================================

/// CFS 目标延迟 (调度器 tick 数).
pub const CFS_TARGET_LATENCY: u64 = 60;

/// CFS 最小粒度 (调度器 tick 数).
pub const CFS_MIN_GRANULARITY: u64 = 8;

/// CFS 提升检查间隔 (调度器 tick 数).
pub const CFS_BOOST_INTERVAL: u64 = 1000;

/// CFS 默认 nice=0 任务的权重.
pub const CFS_NICE0_WEIGHT: u64 = 1024;

// ============================================================================
// 截止期调度 (EDF + CBS)
// ============================================================================

/// Deadline 调度器最小运行时间 (tick 数).
pub const CFS_DL_MIN_RUNTIME: u64 = 1;

/// Deadline 调度器最小周期 (tick 数).
pub const CFS_DL_MIN_PERIOD: u64 = 10;

/// Deadline 调度器最大利用率 (百分比).
pub const CFS_DL_MAX_UTILIZATION_PCT: u64 = 95;

// ============================================================================
// 通用调度器常量
// ============================================================================

/// 调度级别 0 量子 (最高优先级, 实时).
pub const SCHED_LEVEL_0_QUANTUM: u32 = 80;

/// 调度级别 1 量子.
pub const SCHED_LEVEL_1_QUANTUM: u32 = 60;

/// 调度级别 2 量子.
pub const SCHED_LEVEL_2_QUANTUM: u32 = 40;

/// 调度级别 3 量子 (最低优先级, idle).
pub const SCHED_LEVEL_3_QUANTUM: u32 = 20;

/// 调度器提升检查间隔 (tick 数).
pub const SCHED_BOOST_INTERVAL: u64 = 1000;

/// 实时调度器看门狗超时 (tick 数).
pub const SCHED_RT_WATCHDOG_TICKS: u64 = 500;
