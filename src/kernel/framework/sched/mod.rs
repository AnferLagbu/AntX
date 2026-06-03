//! 调度器 (TCB) — Scheduler trait / Task 抽象
//!
//! 策略注入点: services 层通过 Scheduler trait 管理任务，
//! 通过 Task 句柄安全访问进程属性。

pub mod sched_trait;

// 注: task 抽象在 Phase 1.4.2 计划中但尚未实现, 见 services/proc/mod.rs 占位说明。
// 任务书估时 5d, 实际未开工。阻塞 services/proc 迁移。
