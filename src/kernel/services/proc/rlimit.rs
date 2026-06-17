#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! Per-process 资源限制 (rlimit) — services 层策略主体
//!
//! ## T1-8 迁移记录
//!
//! 原属 framework/proc/rlimit.rs, 2026-06-16 提取到 services.
//! 纯策略代码 (数据结构 + 限制检查逻辑), 无 unsafe 操作.
//! syscall 入口 (含用户指针操作) 仍留在 framework.
//!
//! ## 设计
//!
//! - `RlimitTable`: 17 个资源限制条目 (RLIMIT_CPU..RLIMIT_NLIMITS=16)
//! - 每个条目 `{ cur: u64, max: u64 }` (soft/hard limit)
//! - fork 时继承父进程 rlimit
//! - `setrlimit`: 只允许降低 cur; 降低 max 需特权 (当前简化为允许 pid=1)
//! - 关键限制检查: NOFILE (open), AS (mmap), STACK (mmap), NPROC (fork)
//!
//! ## 默认值
//!
//! - 大部分资源默认 RLIM_INFINITY
//! - RLIMIT_NOFILE 默认 MAX_OPEN_FILES (32)
//! - RLIMIT_NPROC 默认 MAX_PROCESSES (256)
//! - RLIMIT_STACK 默认 8MB

use crate::kernel::framework::config::{MAX_OPEN_FILES, MAX_PROCESSES};
use crate::kernel::framework::proc::process_get_current_pid;
use crate::kernel::framework::proc::PROCESS_TABLE;
use crate::kernel::framework::syscall::Errno;

// ============================================================================
// POSIX 资源类型常量
// ============================================================================

pub const RLIMIT_CPU: usize = 0;
pub const RLIMIT_FSIZE: usize = 1;
pub const RLIMIT_DATA: usize = 2;
pub const RLIMIT_STACK: usize = 3;
pub const RLIMIT_CORE: usize = 4;
pub const RLIMIT_RSS: usize = 5;
pub const RLIMIT_NPROC: usize = 6;
pub const RLIMIT_NOFILE: usize = 7;
pub const RLIMIT_MEMLOCK: usize = 8;
pub const RLIMIT_AS: usize = 9;
pub const RLIMIT_LOCKS: usize = 10;
pub const RLIMIT_SIGPENDING: usize = 11;
pub const RLIMIT_MSGQUEUE: usize = 12;
pub const RLIMIT_NICE: usize = 13;
pub const RLIMIT_RTPRIO: usize = 14;
pub const RLIMIT_RTTIME: usize = 15;
pub const RLIMIT_NLIMITS: usize = 16;

/// POSIX RLIM_INFINITY
pub const RLIM_INFINITY: u64 = u64::MAX;

/// 单个资源限制条目
#[derive(Debug, Clone, Copy)]
pub struct Rlimit {
    /// 软限制 (当前限制)
    pub cur: u64,
    /// 硬限制 (最大可设置值)
    pub max: u64,
}

impl Rlimit {
    pub const fn new(cur: u64, max: u64) -> Self {
        Self { cur, max }
    }

    pub const fn infinity() -> Self {
        Self::new(RLIM_INFINITY, RLIM_INFINITY)
    }
}

/// Per-process 资源限制表
#[derive(Debug, Clone)]
pub struct RlimitTable {
    limits: [Rlimit; RLIMIT_NLIMITS],
}

impl RlimitTable {
    /// 创建默认 rlimit 表
    pub fn new() -> Self {
        Self {
            limits: [
                Rlimit::infinity(),                          // 0  CPU
                Rlimit::infinity(),                          // 1  FSIZE
                Rlimit::infinity(),                          // 2  DATA
                Rlimit::new(8 * 1024 * 1024, RLIM_INFINITY), // 3  STACK (8MB soft)
                Rlimit::infinity(),                          // 4  CORE
                Rlimit::infinity(),                          // 5  RSS
                Rlimit::new(MAX_PROCESSES as u64, RLIM_INFINITY), // 6  NPROC
                Rlimit::new(MAX_OPEN_FILES as u64, RLIM_INFINITY), // 7  NOFILE
                Rlimit::infinity(),                          // 8  MEMLOCK
                Rlimit::infinity(),                          // 9  AS
                Rlimit::infinity(),                          // 10 LOCKS
                Rlimit::infinity(),                          // 11 SIGPENDING
                Rlimit::infinity(),                          // 12 MSGQUEUE
                Rlimit::infinity(),                          // 13 NICE
                Rlimit::infinity(),                          // 14 RTPRIO
                Rlimit::infinity(),                          // 15 RTTIME
            ],
        }
    }

    /// 获取指定资源的限制
    pub fn get(&self, resource: usize) -> Option<Rlimit> {
        if resource < RLIMIT_NLIMITS {
            Some(self.limits[resource])
        } else {
            None
        }
    }

    /// 设置指定资源的限制
    ///
    /// 返回 Ok(()) 或 Err(Errno)
    /// - EPERM: 非特权进程试图提高 hard limit
    /// - EINVAL: cur > max
    pub fn set(&mut self, resource: usize, cur: u64, max: u64, is_privileged: bool) -> Result<(), Errno> {
        if resource >= RLIMIT_NLIMITS {
            return Err(Errno::EINVAL);
        }
        if cur > max {
            return Err(Errno::EINVAL);
        }
        let old = self.limits[resource];
        // 提高 hard limit 需要特权
        if max > old.max && !is_privileged {
            return Err(Errno::EPERM);
        }
        self.limits[resource] = Rlimit::new(cur, max);
        Ok(())
    }
}

impl Default for RlimitTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 限制检查辅助函数 (供其他子系统调用)
// ============================================================================

/// 检查当前进程的 NOFILE 限制
///
/// 返回 true 表示已超出限制
pub fn check_nofile_exceeded(fd_count: usize) -> bool {
    let pid = process_get_current_pid();
    let table = &PROCESS_TABLE;
    table.with_process(pid, |proc| {
        let rlimit_table = proc.rlimit_table.lock();
        if let Some(rlim) = rlimit_table.get(RLIMIT_NOFILE) {
            fd_count as u64 >= rlim.cur
        } else {
            false
        }
    }).unwrap_or(false)
}

/// 检查当前进程的 AS (地址空间) 限制
///
/// `current_usage`: 当前已映射的地址空间大小
/// `additional_bytes`: 即将额外映射的大小
/// 返回 true 表示已超出限制
pub fn check_as_exceeded(current_usage: u64, additional_bytes: u64) -> bool {
    let pid = process_get_current_pid();
    let table = &PROCESS_TABLE;
    table.with_process(pid, |proc| {
        let rlimit_table = proc.rlimit_table.lock();
        if let Some(rlim) = rlimit_table.get(RLIMIT_AS) {
            if rlim.cur == RLIM_INFINITY {
                return false;
            }
            current_usage.saturating_add(additional_bytes) > rlim.cur
        } else {
            false
        }
    }).unwrap_or(false)
}

/// 检查当前进程的 NPROC 限制
///
/// 返回 true 表示已超出限制
pub fn check_nproc_exceeded(child_count: usize) -> bool {
    let pid = process_get_current_pid();
    let table = &PROCESS_TABLE;
    table.with_process(pid, |proc| {
        let rlimit_table = proc.rlimit_table.lock();
        if let Some(rlim) = rlimit_table.get(RLIMIT_NPROC) {
            child_count as u64 >= rlim.cur
        } else {
            false
        }
    }).unwrap_or(false)
}

/// 获取当前进程的 STACK 限制
pub fn get_stack_limit() -> u64 {
    let pid = process_get_current_pid();
    let table = &PROCESS_TABLE;
    table.with_process(pid, |proc| {
        let rlimit_table = proc.rlimit_table.lock();
        rlimit_table.get(RLIMIT_STACK).map(|r| r.cur).unwrap_or(8 * 1024 * 1024)
    }).unwrap_or(8 * 1024 * 1024)
}

/// 获取当前进程的 NOFILE 限制
pub fn get_nofile_limit() -> u64 {
    let pid = process_get_current_pid();
    let table = &PROCESS_TABLE;
    table.with_process(pid, |proc| {
        let rlimit_table = proc.rlimit_table.lock();
        rlimit_table.get(RLIMIT_NOFILE).map(|r| r.cur).unwrap_or(MAX_OPEN_FILES as u64)
    }).unwrap_or(MAX_OPEN_FILES as u64)
}

/// 获取当前进程的 RLIMIT_MEMLOCK (字节)
pub fn get_memlock_limit() -> u64 {
    let pid = process_get_current_pid();
    let table = &PROCESS_TABLE;
    table.with_process(pid, |proc| {
        let rlimit_table = proc.rlimit_table.lock();
        rlimit_table.get(RLIMIT_MEMLOCK).map(|r| r.cur).unwrap_or(64 * 1024)
    }).unwrap_or(64 * 1024)
}

/// 检查 mlock 锁定字节数是否超 RLIMIT_MEMLOCK
///
/// 返回 true 表示超额, mlock 应失败.
pub fn check_memlock_exceeded(current_locked: u64, additional_bytes: u64) -> bool {
    let limit = get_memlock_limit();
    if limit == RLIM_INFINITY {
        return false;
    }
    current_locked.saturating_add(additional_bytes) > limit
}

/// getrlimit — 获取资源限制 (委托 framework syscall 入口)
pub fn getrlimit_syscall(resource: i32, rlim_ptr: u64) -> Result<usize, Errno> {
    let ret = crate::kernel::framework::proc::rlimit::sys_getrlimit(resource, rlim_ptr);
    if ret >= 0 {
        Ok(0)
    } else {
        Err(Errno::from_ret(ret))
    }
}

/// setrlimit — 设置资源限制 (委托 framework syscall 入口)
pub fn setrlimit_syscall(resource: i32, rlim_ptr: u64) -> Result<usize, Errno> {
    let ret = crate::kernel::framework::proc::rlimit::sys_setrlimit(resource, rlim_ptr);
    if ret >= 0 {
        Ok(0)
    } else {
        Err(Errno::from_ret(ret))
    }
}
