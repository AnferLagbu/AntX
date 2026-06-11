#![deny(unsafe_code)]
//! 进程管理子系统 — services 层安全代理
//!
//! ## 状态 (v2.11, 2026-06-04)
//!
//! Phase 2.5 进程迁移 (1/4): 封装 `kernel::crate::kernel::framework::proc::types` 强类型与状态/ID API:
//! - [x] types — `Pid` / `Tid` / `ProcessId` / `ThreadId` / `ProcessState` / `ProcessPriority`
//! - [x] session — 初始化入口
//! - [x] scheduler / scheduler_ex — 调度器初始化入口
//! - [x] process / thread — 进程/线程初始化入口
//! - [ ] process table — 完整 CRUD (后续 Phase 2.5.x)
//! - [ ] ELF loader — 完整加载器 (后续 Phase 2.5.x)
//! - [ ] signal — 信号系统 (后续 Phase 2.5.x)
//!
//! ## 迁移方法
//!
//! 1. `pub fn proc_init_*` 集合 → 单一 `services::proc::init()` 入口
//! 2. `ProcessState` / `ProcessPriority` 直接 re-export (已是强类型)
//! 3. 0 unsafe 出现在 services 层
//!
//! 评估日期: 2026-06-04


pub mod canary;
pub mod elf;
pub mod execve;
pub mod clone;
pub mod coredump;
/// D8: FD Table 分配策略 (first-fit, 上限 64)
pub mod fd_table;
pub mod info;
pub mod madvise_mlock;
/// D1: Namespace 安全封装
pub mod namespace;
/// D2: cgroup 安全封装
pub mod cgroup;
pub mod rlimit;
pub mod sched;
pub mod seccomp;
pub mod session;
pub mod signal;
pub mod sleep;
pub mod table;
pub mod wait4;
pub mod posix_timer;
/// D7: Shadow Stack (CET) 安全封装
pub mod shadow_stack;

// ============================================================================
// ID 与状态 (直接 re-export 内核强类型)
// ============================================================================

/// 进程 ID (新类型, 替代裸 `u32`)
pub use crate::kernel::framework::proc::types::{Pid, Tid, ProcessId, ThreadId};

/// 进程状态 (七状态模型)
pub use crate::kernel::framework::proc::types::ProcessState;

/// 进程优先级
pub use crate::kernel::framework::proc::types::ProcessPriority;

// ============================================================================
// 错误
// ============================================================================

/// 进程操作错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcError {
    /// 进程不存在
    NotFound,
    /// 权限不足
    PermissionDenied,
    /// 资源耗尽 (PID 表满)
    NoResources,
    /// 进程已退出
    Exited,
    /// 无效参数
    InvalidArgument,
    /// 其他
    Other(i32),
}

impl ProcError {
    pub fn from_i32(rc: i32) -> Self {
        match rc {
            -1 => Self::NotFound,
            -2 => Self::PermissionDenied,
            -3 => Self::NoResources,
            -4 => Self::Exited,
            -22 => Self::InvalidArgument,
            _ => Self::Other(rc),
        }
    }
}

pub type ProcResult<T> = Result<T, ProcError>;

// ============================================================================
// 初始化入口
// ============================================================================

/// 初始化进程子系统 (按依赖顺序调用各 init)
///
/// **调用顺序** (依赖链):
/// 1. 进程表 (`PROCESS_TABLE` 常量初始化自动完成, 无需显式 init)
/// 2. 线程表 (`THREAD_MANAGER.init` → `THREAD_MANAGER` 通过 `pub fn init()`)
/// 3. 主调度器 (`SCHEDULER.init` + `SCHEDULER_EX.init`)
/// 4. Session 管理器 (`SESSION_MANAGER.init` → `pub fn init()`)
///
/// 由启动期 `kernel::init` 调用一次。
pub fn init() {
    crate::kernel::framework::proc::thread::init();
    crate::kernel::framework::proc::scheduler::init();
    crate::kernel::framework::proc::scheduler_ex::init();
    crate::kernel::framework::proc::session::init();
}

/// 初始化指定 CPU 的每 CPU 调度队列
///
/// 由 SMP 启动代码在每个 CPU 上调用一次。
pub fn init_per_cpu(cpu_id: u32) {
    crate::kernel::framework::proc::scheduler::init_per_cpu_sched(cpu_id);
}

// ============================================================================
// 调度器状态
// ============================================================================

/// 调度器是否已就绪
pub fn scheduler_ready() -> bool {
    crate::kernel::framework::proc::scheduler::SCHEDULER_READY.load(core::sync::atomic::Ordering::Acquire)
}

/// 触发调度 (在 timer tick 或阻塞唤醒后调用)
///
/// 由架构中断处理代码调用。
pub fn schedule() {
    crate::kernel::framework::proc::scheduler::SCHEDULER.schedule();
}

// ============================================================================
// 进程状态查询
// ============================================================================

/// 从 u8 值构造 ProcessState (安全转换)
pub fn state_from_u8(v: u8) -> ProcessState {
    ProcessState::from_u8(v)
}

/// 从 u32 值构造 ProcessState (兼容 AtomicU32 存储)
pub fn state_from_u32(v: u32) -> ProcessState {
    ProcessState::from_u32(v)
}

/// 从优先级数值构造 ProcessPriority
pub fn priority_from_u32(v: u32) -> ProcessPriority {
    ProcessPriority::from_u32(v)
}

// ============================================================================
// 进程 ID 转换
// ============================================================================

/// `u32` → `ProcessId` (零成本包装)
#[inline]
pub const fn pid_new(raw: u32) -> ProcessId {
    ProcessId(raw)
}

/// `ProcessId` → `u32`
#[inline]
pub const fn pid_raw(id: ProcessId) -> u32 {
    id.0
}

/// `u32` → `ThreadId`
#[inline]
pub const fn tid_new(raw: u32) -> ThreadId {
    ThreadId(raw)
}

/// `ThreadId` → `u32`
#[inline]
pub const fn tid_raw(id: ThreadId) -> u32 {
    id.0
}
