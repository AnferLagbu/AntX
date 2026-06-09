//! # Framework 进程子系统 (TCB)
//!
//! 唯一允许 `unsafe` 的进程管理底层实现。对应 `services/proc/` 安全代理。
//!
//! ## 架构
//!
//! ```text
//! framework/proc/  (TCB 进程管理)
//! ├── types.rs          核心数据结构 (Process/Thread/Task/Pid/Tid 等)
//! ├── process.rs        进程表 (PCB/进程 CRUD/状态机)
//! ├── thread.rs         线程 (内核线程/用户线程/上下文)
//! ├── session.rs        会话/进程组/凭证
//! ├── elf.rs            ELF 二进制解析 (TCB 原始)
//! ├── api.rs            C ↔ Rust 桥接 (兼容层)
//! ├── scheduler.rs      主调度器 (CFS + 实时 + 批处理)
//! ├── scheduler_ex.rs   调度器扩展 (SMP 负载均衡/亲和性)
//! ├── cfs.rs            CFS 公平调度算法
//! ├── cpu_queue.rs      每 CPU 运行队列
//! ├── oomd.rs           OOM killer
//! ├── switch.asm        上下文切换汇编
//! └── mod.rs            模块导出
//! ```
//!
//! ## SAFETY 契约
//!
//! - 唯一允许 unsafe 的进程管理实现层
//! - 进程表/线程表的裸指针解引用集中在本模块
//! - services/proc/ 通过安全 API (`with(pid, |p| ...)`) 调用本模块
//!
//! ## 与 services/proc/ 的关系
//!
//! - `services/proc/` 是 100% safe 业务封装, 顶部 `#![deny(unsafe_code)]`
//! - `services/proc/` 通过 `pub use framework::proc::*` 暴露底层能力给 syscall
//! - 任何 unsafe 改动必须在 `framework/proc/` 完成, 业务层禁止穿透

#![allow(ambiguous_glob_reexports)]

pub mod cfs;
pub mod coredump;
pub mod cpu_queue;
pub mod elf;
pub mod api;
pub mod oomd;
pub mod posix_timer;
pub mod process;
pub mod rlimit;
pub mod scheduler;
pub mod scheduler_ex;
pub mod session;
pub mod signal;
pub mod thread;
pub mod types;
pub mod user_proc;

// LATER(polish): 用显式导入替代 glob re-export 消除歧义
// USER_STACK_SIZE: types(usize) vs user_proc(u64)
// init: scheduler vs user_proc
pub use crate::kernel::framework::barrier::*;
pub use posix_timer::*;
pub use process::*;
pub use scheduler::*;
pub use scheduler_ex::*;
pub use session::*;
pub use thread::*;
pub use types::*;
pub use user_proc::*;
