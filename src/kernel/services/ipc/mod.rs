//! IPC — 管道/共享内存/消息队列/信号 (services 层占位)
//!
//! ## 当前状态: ⏳ 未迁移
//!
//! 实际实现仍在 `kernel/ipc/` 老位置:
//! - [kernel/ipc/pipe.rs](file:///home/anfer/Code/AntX/src/kernel/ipc/pipe.rs) — 管道
//! - [kernel/ipc/shm.rs](file:///home/anfer/Code/AntX/src/kernel/ipc/shm.rs) — 共享内存
//! - [kernel/ipc/msgq.rs](file:///home/anfer/Code/AntX/src/kernel/ipc/msgq.rs) — 消息队列
//! - [kernel/ipc/sem.rs](file:///home/anfer/Code/AntX/src/kernel/ipc/sem.rs) — 信号量
//! - [kernel/ipc/signal.rs](file:///home/anfer/Code/AntX/src/kernel/ipc/signal.rs) — 信号
//!
//! ## 迁移路径
//!
//! 1. 管道/共享内存/消息队列的 buffer 走 `framework::Frame` + `framework::VmSpace`
//! 2. 信号注册表走 `framework::sync::SpinLock` (不再用 kernel::sync)
//! 3. 在 services/ipc/ 暴露 `pub fn pipe_create` 等纯 safe API
//!
//! ## 估算: 1 人月
//!
//! 评估日期: 2026-06-03
