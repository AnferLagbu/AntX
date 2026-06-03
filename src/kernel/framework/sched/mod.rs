//! 调度器 (TCB) — Scheduler trait / Task 抽象
//!
//! 策略注入点: services 层通过 Scheduler trait 管理任务，
//! 通过 Task 句柄安全访问进程属性。

pub mod sched_trait;
