//! # Framework 进程子系统 (TCB)
//!
//! 唯一允许 `unsafe` 的进程管理底层实现。对应 `services/proc/` 安全代理。
//!
//! ## 依赖声明
//!
//! framework 内部依赖: mm, sync, syscall, config, idt, sched, barrier, tests
//! services 依赖: services::proc (安全代理)
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
/// D2: cgroup 资源控制器
/// TD-02: 全局统一 FD 分配器与基址规划
pub mod fd_alloc;
pub mod cgroup;
pub mod canary;
pub mod coredump;
pub mod cpu_queue;
pub mod elf;
pub mod api;
pub mod madvise_mlock;
/// D1: Linux 兼容 Namespace 框架
pub mod namespace;
pub mod oomd;
pub mod posix_timer;
pub mod process;
pub mod rlimit;
pub mod sched_trait;
pub mod scheduler;
pub mod scheduler_ex;
pub mod seccomp;
pub mod session;
pub mod signal;
pub mod thread;
pub mod types;
pub mod user_proc;

// LATER(polish): 用显式导入替代 glob re-export 消除歧义
// USER_STACK_SIZE: types(usize) 对比 user_proc(u64)
// init: scheduler vs user_proc
pub use crate::kernel::framework::barrier::*;
pub use canary::*;
pub use posix_timer::*;
pub use process::*;
pub use scheduler::*;
pub use scheduler_ex::*;
pub use session::*;
pub use signal::*;
pub use thread::*;
pub use types::*;
pub use user_proc::*;

// cpu_queue 公共接口 re-export — 避免跨子系统直接访问 proc::cpu_queue 内部
pub use cpu_queue::init_cpu_queue;

// api 公共接口 re-export — 避免跨子系统直接访问 proc::api 内部
pub use api::*;

// api::raw 公共接口 re-export — 避免跨子系统直接访问 proc::api::raw 内部
pub use api::raw;

// fd_alloc 公共接口 re-export — 避免跨子系统直接访问 proc::fd_alloc 内部
pub use fd_alloc::{FdPlan, FdSubsystem, fd_at, idx_of};

// madvise_mlock 公共接口 re-export — 避免跨子系统直接访问 proc::madvise_mlock 内部
pub use madvise_mlock::{sys_madvise, sys_mlock, sys_munlock, sys_mlockall, sys_munlockall, sys_mincore};

// elf 公共接口 re-export — 避免跨子系统直接访问 proc::elf 内部
pub use elf::{Elf64Header, Elf64Phdr, ElfLoadResult, elf_validate, elf_load};

// rlimit 公共接口显式 re-export — glob re-export 可能被遮蔽
pub use rlimit::{sys_getrlimit, sys_setrlimit, RLIMIT_CORE, RLIM_INFINITY, get_memlock_limit};

// seccomp 公共接口 re-export — 避免跨子系统直接访问 proc::seccomp 内部
pub use seccomp::{seccomp_check, sys_seccomp, sys_prctl_prctl, SeccompMode, SeccompState};

// namespace 公共接口 re-export — 避免跨子系统直接访问 proc::namespace 内部
pub use namespace::{sys_unshare, sys_setns, NamespaceSet};

// cgroup 公共接口 re-export — 避免跨子系统直接访问 proc::cgroup 内部
pub use cgroup::{sys_cgroup_create, sys_cgroup_destroy, sys_cgroup_attach, sys_cgroup_set_limit, sys_cgroup_get_stat, cgroup_is_initialized, cgroup_subsystem};

// sched_trait 公共接口 re-export — T-01 策略-机制分离
pub use sched_trait::{SchedDecision, FallbackMlfqPolicy, register_sched_decision, current_sched_decision};
