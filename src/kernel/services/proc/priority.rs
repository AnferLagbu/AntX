#![deny(unsafe_code)]
//! 进程优先级策略 — nice / getpriority / setpriority
//!
//! 从 framework/syscall/mod.rs 迁移的策略代码:
//! - nice_to_priority: nice 值 → ProcessPriority 映射
//! - priority_to_nice: ProcessPriority → nice 值映射
//! - nice_syscall: nice() 系统调用策略
//! - getpriority_syscall: getpriority() 系统调用策略
//! - setpriority_syscall: setpriority() 系统调用策略
//!
//! ## 框内核边界
//! - 100% safe Rust
//! - 通过 framework::proc 公开 API 访问进程表
//! - 无 unsafe, 无裸指针

use crate::kernel::framework::proc::ProcessPriority;
use crate::kernel::framework::syscall::Errno;

const PRIO_PROCESS: i32 = 0;

/// nice 值 → ProcessPriority 映射
pub fn nice_to_priority(nice: i32) -> ProcessPriority {
    let clamped = nice.clamp(-20, 19);
    if clamped < -10 {
        ProcessPriority::RealTime
    } else if clamped < 0 {
        ProcessPriority::High
    } else if clamped < 10 {
        ProcessPriority::Normal
    } else if clamped < 19 {
        ProcessPriority::Low
    } else {
        ProcessPriority::Idle
    }
}

/// ProcessPriority → nice 值映射
pub fn priority_to_nice(p: ProcessPriority) -> i32 {
    match p {
        ProcessPriority::RealTime => -20,
        ProcessPriority::High => -10,
        ProcessPriority::Normal => 0,
        ProcessPriority::Low => 10,
        ProcessPriority::Idle => 19,
    }
}

/// nice(inc) 系统调用策略
pub fn nice_syscall(inc: i32) -> i64 {
    let pid = crate::kernel::framework::proc::process_get_current_pid();
    let current_nice = getpriority_syscall(PRIO_PROCESS, pid) as i32;
    if current_nice < 0 && current_nice != -38 {
        // getpriority 失败 (非 ENOSYS)
        return current_nice as i64;
    }
    let current_nice = if current_nice == -38 { 0 } else { current_nice };
    let new_nice = (current_nice + inc).clamp(-20, 19);
    let set_ret = setpriority_syscall(PRIO_PROCESS, pid, new_nice);
    if set_ret < 0 {
        return set_ret;
    }
    new_nice as i64
}

/// getpriority(which, who) 系统调用策略
pub fn getpriority_syscall(which: i32, who: u32) -> i64 {
    if which != PRIO_PROCESS {
        return Errno::EINVAL.as_ret();
    }
    let pid = if who == 0 {
        crate::kernel::framework::proc::process_get_current_pid()
    } else {
        who
    };
    let pri = match crate::kernel::framework::proc::process_with(pid, |p| p.get_priority()) {
        Some(p) => p,
        None => return Errno::ESRCH.as_ret(),
    };
    priority_to_nice(pri) as i64
}

/// setpriority(which, who, prio) 系统调用策略
pub fn setpriority_syscall(which: i32, who: u32, prio: i32) -> i64 {
    if which != PRIO_PROCESS {
        return Errno::EINVAL.as_ret();
    }
    let clamped = prio.clamp(-20, 19);
    let pid = if who == 0 {
        crate::kernel::framework::proc::process_get_current_pid()
    } else {
        who
    };
    let new_pri = nice_to_priority(clamped);
    match crate::kernel::framework::proc::process_with_mut(pid, |p| p.set_priority(new_pri)) {
        Some(_) => 0,
        None => Errno::ESRCH.as_ret(),
    }
}
