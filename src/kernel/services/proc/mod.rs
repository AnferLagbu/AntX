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
/// TD-02: 全局统一 FD 分配器 (范围规划 + 分配/释放/反查)
pub mod fd_alloc;
pub mod info;
pub mod madvise_mlock;
/// D1: Namespace 安全封装
pub mod namespace;
/// D2: cgroup 安全封装
pub mod cgroup;
pub mod rlimit;
pub mod sched;
/// D3: CFS 调度策略 (权重表 + vruntime + 时间片 + CFS/DL 运行队列)
pub mod sched_policy;
pub mod seccomp;
pub mod session;
pub mod signal;
pub mod sleep;
pub mod table;
pub mod types;
pub mod wait4;
pub mod posix_timer;
/// D7: Shadow Stack (CET) 安全封装
pub mod shadow_stack;
/// OOMD — 内存不足守护进程策略
pub mod oomd;
/// 进程优先级策略 — nice / getpriority / setpriority
pub mod priority;
/// CPU 亲和性策略 — sched_setaffinity / sched_getaffinity
pub mod affinity;
/// 系统信息策略 — getrusage / sysinfo / getrlimit / hostname / boot_check
pub mod sysinfo;
/// 进程管理策略 — proc_list / proc_setpri / credo_proc_cputime
pub mod proc_mgmt;
/// 进程生命周期策略 — fork / exit / sched_yield
pub mod lifecycle;
/// pidfd 系统调用 — pidfd_open / pidfd_send_signal / pidfd_getfd
pub mod pidfd;
/// memfd_create — 匿名内存文件
pub mod memfd;

// ============================================================================
// ID 与状态 (直接 re-export 本地 types 模块)
// ============================================================================

/// 进程 ID (新类型, 替代裸 `u32`)
pub use types::{Pid, Tid, ProcessId, ThreadId};

/// 进程状态 (七状态模型)
pub use types::ProcessState;

/// 进程优先级
pub use types::ProcessPriority;

// sched_policy 公共接口 re-export — 策略-机制分离
pub use sched_policy::{DefaultPolicy, register_default_policy};

// namespace 公共接口 re-export — 避免跨层直接访问 services::proc::namespace 内部
pub use namespace::NamespaceSet;

use crate::kernel::framework::syscall::Errno;

// ============================================================================
// 错误
// ============================================================================

/// 进程操作错误 — TD-19: 收敛到 KernelError, 1 字段 proc 特有 + 1 共享包装.
///
/// 字段说明:
///   - `Exited`: 进程已退出, 走 ESRCH (POSIX 中 ESRCH=3 兼含 "no such process" 语义)
///   - `Kernel(KernelError)`: 共享错误 (NotFound → NoSuchProcess / PermissionDenied /
///     NoResources → WouldBlock / InvalidArgument) 全部走单一来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcError {
    /// 进程已退出
    Exited,
    /// 共享 `KernelError` 包装
    Kernel(crate::kernel::services::error::KernelError),
}

impl ProcError {
    /// 映射为 POSIX errno
    pub fn to_errno(self) -> Errno {
        use Errno as E;
        match self {
            Self::Exited => E::ESRCH,
            Self::Kernel(e) => e.as_errno(),
        }
    }

    pub fn from_i32(rc: i32) -> Self {
        use crate::kernel::services::error::KernelError as K;
        match rc {
            -1 => Self::Kernel(K::NoSuchProcess),
            -2 => Self::Kernel(K::PermissionDenied),
            -3 => Self::Kernel(K::WouldBlock),
            -4 => Self::Exited,
            -22 => Self::Kernel(K::InvalidArgument),
            _ => Self::Kernel(K::Other(rc)),
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
    // 注册 services 层调度策略 (在 framework 调度器初始化之前)
    let _ = sched_policy::register_default_policy();
    // REVAL-1: 注册 services 层信号策略
    let _ = signal::register_standard_signal_policy();

    crate::kernel::framework::proc::thread::init();
    crate::kernel::framework::proc::scheduler::init();
    crate::kernel::framework::proc::scheduler_ex::init();
    crate::kernel::framework::proc::session::init();
}

/// 初始化指定 CPU 的每 CPU 调度队列
///
/// 由 SMP 启动代码在每个 CPU 上调用一次。
pub fn init_per_cpu(cpu_id: u32) {
    crate::kernel::framework::proc::init_per_cpu_sched(cpu_id);
}

// ============================================================================
// 调度器状态
// ============================================================================

/// 调度器是否已就绪
pub fn scheduler_ready() -> bool {
    crate::kernel::framework::proc::SCHEDULER_READY.load(core::sync::atomic::Ordering::Acquire)
}

/// 触发调度 (在 timer tick 或阻塞唤醒后调用)
///
/// 由架构中断处理代码调用。
pub fn schedule() {
    crate::kernel::framework::proc::SCHEDULER.schedule();
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
