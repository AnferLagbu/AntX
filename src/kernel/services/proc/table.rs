#![deny(unsafe_code)]
//! 进程表 CRUD 安全代理
//!
//! 把 `kernel::process::ProcessTable` 的 `*mut Process` 裸指针接口
//! 收敛到 `services::proc::table`, 用闭包 API (`with`/`with_mut`) 替代裸指针暴露。
//!
//! ## 状态 (v2.13, 2026-06-04)
//!
//! Phase 2.5 进程迁移 2/4 (进程表 CRUD):
//! - [x] 强类型查询 (state / priority / policy / rt_priority / pwm)
//! - [x] 状态变更 (set_state_safe / set_priority / set_sched_policy)
//! - [x] PID 分配 (allocate_pid)
//! - [x] 引用计数 (try_inc_ref / dec_ref_and_maybe_free)
//! - [x] 全表遍历 (for_each 闭包形式)
//! - [x] 移除 (remove_and_free)
//! - [ ] 进程创建/析构 — 留待 Phase 2.5.3 (依赖 ELF 加载与 VmSpace)
//!
//! ## 迁移方法
//!
//! 所有需要 `*mut Process` 的操作都收敛到 framework 层,
//! services 层使用闭包 `with`/`with_mut` 访问, 借用检查器保证生命周期安全。
//!
//! 评估日期: 2026-06-04

use crate::kernel::framework::proc::process::{self, PROCESS_TABLE};
use crate::kernel::framework::proc::types::ProcessState;
use crate::kernel::framework::proc::types::ProcessPriority;

// ============================================================================
// 强类型 re-export
// ============================================================================

/// 进程调度策略 (从 `proc::scheduler` 透传)
pub use crate::kernel::framework::proc::scheduler::SchedPolicy;

/// 进程句柄 (新类型包装, 表示对表项的活跃引用)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessHandle {
    /// 进程 PID
    pub pid: crate::kernel::framework::proc::types::Pid,
}

impl ProcessHandle {
    /// 构造新句柄
    #[inline]
    pub const fn new(pid: crate::kernel::framework::proc::types::Pid) -> Self {
        Self { pid }
    }

    /// 获取 PID
    #[inline]
    pub const fn pid(self) -> crate::kernel::framework::proc::types::Pid {
        self.pid
    }
}

// ============================================================================
// 错误
// ============================================================================

/// 进程表错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableError {
    /// 进程不存在
    NotFound,
    /// 表已满 (无法分配新 PID)
    TableFull,
    /// 引用计数已为 0 (双重释放风险)
    RefCountUnderflow,
    /// 状态转换非法
    InvalidStateTransition,
    /// 其他
    Other(i32),
}

pub type TableResult<T> = Result<T, TableError>;

// ============================================================================
// PID 分配
// ============================================================================

/// 分配一个新 PID
///
/// **返回**: 成功返回 PID, 表已满返回 `TableError::TableFull`.
pub fn allocate_pid() -> TableResult<crate::kernel::framework::proc::types::Pid> {
    PROCESS_TABLE
        .allocate_pid()
        .ok_or(TableError::TableFull)
}

// ============================================================================
// 引用计数
// ============================================================================

/// 增加进程引用计数
///
/// **返回**: 成功返回 (), 进程不存在返回 `NotFound`.
pub fn try_inc_ref(pid: crate::kernel::framework::proc::types::Pid) -> TableResult<()> {
    if PROCESS_TABLE.try_inc_ref(pid) {
        Ok(())
    } else {
        Err(TableError::NotFound)
    }
}

/// 减少引用计数, 归零时回收 PCB
pub fn dec_ref_and_maybe_free(pid: crate::kernel::framework::proc::types::Pid) {
    PROCESS_TABLE.dec_ref_and_maybe_free(pid);
}

// ============================================================================
// 访问器 (闭包风格, 避免暴露裸指针)
// ============================================================================

/// 在持有表锁的情况下, 对进程执行只读闭包
///
/// **示例**:
/// ```ignore
/// let state = with(handle.pid(), |p| p.get_state());
/// ```
pub fn with<F, R>(pid: crate::kernel::framework::proc::types::Pid, f: F) -> Option<R>
where
    F: FnOnce(&process::Process) -> R,
{
    PROCESS_TABLE.with_process(pid, f)
}

/// 在持有表锁的情况下, 对进程执行可变闭包
pub fn with_mut<F, R>(pid: crate::kernel::framework::proc::types::Pid, f: F) -> Option<R>
where
    F: FnOnce(&mut process::Process) -> R,
{
    PROCESS_TABLE.with_process_mut(pid, f)
}

// ============================================================================
// 便利状态查询 (基于 with 闭包)
// ============================================================================

/// 获取进程状态
pub fn get_state(pid: crate::kernel::framework::proc::types::Pid) -> Option<ProcessState> {
    with(pid, |p| p.get_state())
}

/// 设置进程状态 (安全版本, 自动检查状态转换合法性)
pub fn set_state(pid: crate::kernel::framework::proc::types::Pid, state: ProcessState) -> TableResult<()> {
    with_mut(pid, |p| p.set_state_safe(state))
        .ok_or(TableError::NotFound)?
        .map_err(|_| TableError::InvalidStateTransition)
}

/// 获取进程优先级
pub fn get_priority(pid: crate::kernel::framework::proc::types::Pid) -> Option<ProcessPriority> {
    with(pid, |p| p.get_priority())
}

/// 设置进程优先级
pub fn set_priority(pid: crate::kernel::framework::proc::types::Pid, priority: ProcessPriority) -> TableResult<()> {
    with_mut(pid, |p| p.set_priority(priority));
    Ok(())
}

/// 是否内核进程
pub fn is_kernel(pid: crate::kernel::framework::proc::types::Pid) -> Option<bool> {
    with(pid, |p| p.is_kernel())
}

/// 设置内核/用户标志
pub fn set_kernel(pid: crate::kernel::framework::proc::types::Pid, is_kernel: bool) -> TableResult<()> {
    with_mut(pid, |p| p.set_kernel(is_kernel));
    Ok(())
}

/// 获取调度策略
pub fn get_sched_policy(pid: crate::kernel::framework::proc::types::Pid) -> Option<SchedPolicy> {
    with(pid, |p| p.get_sched_policy())
}

/// 设置调度策略
pub fn set_sched_policy(pid: crate::kernel::framework::proc::types::Pid, policy: SchedPolicy) -> TableResult<()> {
    with_mut(pid, |p| p.set_sched_policy(policy));
    Ok(())
}

/// 获取 RT 优先级 (0-99)
pub fn get_rt_priority(pid: crate::kernel::framework::proc::types::Pid) -> Option<u8> {
    with(pid, |p| p.get_rt_priority())
}

/// 设置 RT 优先级
pub fn set_rt_priority(pid: crate::kernel::framework::proc::types::Pid, priority: u8) -> TableResult<()> {
    with_mut(pid, |p| p.set_rt_priority(priority));
    Ok(())
}

/// 获取进程 PMM (Per-Memory Mapping) 字节数
pub fn get_pwm(pid: crate::kernel::framework::proc::types::Pid) -> Option<u64> {
    with(pid, |p| p.get_pwm())
}

/// 设置 PMM
pub fn set_pwm(pid: crate::kernel::framework::proc::types::Pid, pwm: u64) -> TableResult<()> {
    with_mut(pid, |p| p.set_pwm(pwm));
    Ok(())
}

// ============================================================================
// 信号
// ============================================================================

/// 设置待处理信号位
pub fn signal_set(pid: crate::kernel::framework::proc::types::Pid, sig: u32) -> TableResult<()> {
    with_mut(pid, |p| p.signal_pending_set(sig));
    Ok(())
}

/// 获取待处理信号位图
pub fn signal_get(pid: crate::kernel::framework::proc::types::Pid) -> Option<u64> {
    with(pid, |p| p.signal_pending_get())
}

/// 清除待处理信号位
pub fn signal_clear(pid: crate::kernel::framework::proc::types::Pid, mask: u64) -> TableResult<()> {
    with_mut(pid, |p| p.signal_pending_clear(mask));
    Ok(())
}

// ============================================================================
// 遍历
// ============================================================================

/// 遍历所有进程 (只读闭包)
///
/// **示例**:
/// ```ignore
/// let count = for_each(|p| {
///     if p.get_state() == ProcessState::Running {
///         true  // 继续
///     } else {
///         true  // 跳过
///     }
/// });
/// ```
pub fn for_each<F>(mut f: F) -> u32
where
    F: FnMut(&process::Process) -> bool,
{
    let mut count = 0u32;
    PROCESS_TABLE.for_each(|p| {
        if f(p) {
            count += 1;
            true
        } else {
            false
        }
    });
    count
}

// ============================================================================
// 进程移除
// ============================================================================

/// 移除并释放进程 PCB
///
/// **安全保证**: 内部引用计数归零后才真正释放内存.
pub fn remove_and_free(pid: crate::kernel::framework::proc::types::Pid) {
    PROCESS_TABLE.remove_and_free(pid);
}

/// 强制分配 PID (内核线程专用, 0/1/2 等保留 PID)
pub fn allocate_reserved_pid() -> TableResult<crate::kernel::framework::proc::types::Pid> {
    // 启动期特殊 PID (0=kthread, 1=init) 由 thread 模块单独分配
    // 普通进程用 allocate_pid
    allocate_pid()
}
