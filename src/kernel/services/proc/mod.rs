//! 进程管理 — 调度 / 进程表 / ELF 加载 (services 层占位)
//!
//! ## 当前状态: ⏳ 未迁移
//!
//! 实际实现仍在 `kernel/proc/` 老位置:
//! - [kernel/proc/scheduler.rs](file:///home/anfer/Code/AntX/src/kernel/proc/scheduler.rs) — 主调度器
//! - [kernel/proc/process.rs](file:///home/anfer/Code/AntX/src/kernel/proc/process.rs) — 进程控制块
//! - [kernel/proc/cfs.rs](file:///home/anfer/Code/AntX/src/kernel/proc/cfs.rs) — CFS 公平调度
//! - [kernel/proc/scheduler_ex.rs](file:///home/anfer/Code/AntX/src/kernel/proc/scheduler_ex.rs) — 实时调度 + OOMD
//! - [kernel/proc/elf.rs](file:///home/anfer/Code/AntX/src/kernel/proc/elf.rs) — ELF64 加载器
//! - [kernel/proc/user_proc.rs](file:///home/anfer/Code/AntX/src/kernel/proc/user_proc.rs) — 用户进程管理
//!
//! ## 迁移路径
//!
//! 1. 引入 `framework::sched::Scheduler` trait (已实现) + `framework::vm::VmSpace` (已实现)
//! 2. 重新组织进程表, 把 raw pointer 操作 (`*mut Process`) 下沉到 framework
//! 3. 在 services/proc/ 暴露 `pub fn spawn`, `pub fn schedule` 等纯 safe API
//! 4. 在 `kernel_main` 中调用 `services::proc::init()`, 删除 `kernel::proc::init`
//!
//! ## 估算: 1-2 人月
//!
//! 评估日期: 2026-06-03
//! 阻塞点: scheduler_ex.rs 仍有 74 处 unsafe (Phase 2.5 声称完成但未集成)
