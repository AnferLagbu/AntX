//! Per-process 资源限制 (rlimit) — framework 层
//!
//! ## T1-8 迁移记录
//!
//! 策略代码 (`RlimitTable`, Rlimit, 常量, check_*/get_* 辅助函数)
//! 已于 2026-06-16 迁移到 `services::proc::rlimit`.
//! 本文件仅保留:
//! 1. re-export services 层的公共 API (保持调用方兼容)
//! 2. syscall 入口 (含 unsafe 用户指针操作, 必须留在 framework)

// Re-export services 层的策略主体 — 保持调用方路径兼容
pub use crate::kernel::services::proc::rlimit::{
    Rlimit, RlimitTable,
    RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK, RLIMIT_CORE,
    RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE, RLIMIT_MEMLOCK, RLIMIT_AS,
    RLIMIT_LOCKS, RLIMIT_SIGPENDING, RLIMIT_MSGQUEUE, RLIMIT_NICE,
    RLIMIT_RTPRIO, RLIMIT_RTTIME, RLIMIT_NLIMITS,
    RLIM_INFINITY,
    check_nofile_exceeded, check_as_exceeded, check_nproc_exceeded,
    get_stack_limit, get_nofile_limit,
    get_memlock_limit, check_memlock_exceeded,
};

use crate::kernel::framework::proc::{process_get_current_pid, process_with};
use crate::kernel::framework::userptr;
use crate::kernel::framework::errno::Errno;

// ============================================================================
// 系统调用实现 (含 unsafe 用户指针操作, 必须留在 framework)
// ============================================================================

/// `sys_getrlimit` — 获取资源限制
///
/// `resource`: POSIX 资源类型 (0..=16)
/// `rlim_ptr`: 用户空间指针, 指向 `struct rlimit { rlim_cur: u64, rlim_max: u64 }`
pub fn sys_getrlimit(resource: i32, rlim_ptr: u64) -> i64 {
    if rlim_ptr == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !(0..RLIMIT_NLIMITS as i32).contains(&resource) {
        return Errno::EINVAL.as_ret();
    }

    let pid = process_get_current_pid();
    let rlim = match process_with(pid, |proc| {
        let rlimit_table = proc.rlimit_table.lock();
        rlimit_table.get(resource as usize)
    }) {
        Some(Some(r)) => r,
        Some(None) => return Errno::EINVAL.as_ret(),
        None => return Errno::ESRCH.as_ret(),
    };

    if !userptr::validate_user_buf(rlim_ptr, 16) {
        return Errno::EFAULT.as_ret();
    }
    // SAFETY: rlim_ptr 已验证 16 字节可写
    unsafe {
        core::ptr::write_volatile(rlim_ptr as *mut u64, rlim.cur);
        core::ptr::write_volatile((rlim_ptr as *mut u64).add(1), rlim.max);
    }
    0
}

/// `sys_setrlimit` — 设置资源限制
///
/// `resource`: POSIX 资源类型 (0..=16)
/// `rlim_ptr`: 用户空间指针, 指向 `struct rlimit { rlim_cur: u64, rlim_max: u64 }`
///
/// # Panics
/// 仅在两处 `expect` 调用 (`bytes[0..8].try_into()` 与 `bytes[8..16].try_into()`) 处
/// 存在潜在 panic; 由于切片长度恒为 8 字节, 实际上不会触发.
pub fn sys_setrlimit(resource: i32, rlim_ptr: u64) -> i64 {
    if rlim_ptr == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !(0..RLIMIT_NLIMITS as i32).contains(&resource) {
        return Errno::EINVAL.as_ret();
    }

    // 从用户空间读取 rlim_cur 和 rlim_max
    if !userptr::validate_user_buf(rlim_ptr, 16) {
        return Errno::EFAULT.as_ret();
    }
    // SAFETY: rlim_ptr 已验证可读, 16 字节
    let bytes: [u8; 16] = unsafe { core::ptr::read(rlim_ptr as *const [u8; 16]) };
    // SAFETY: bytes[0..8] 和 bytes[8..16] 各为 8 字节, try_into 不可能失败
    let cur = u64::from_ne_bytes(bytes[0..8].try_into().expect("rlimit: 长度不为 8"));
    let max = u64::from_ne_bytes(bytes[8..16].try_into().expect("rlimit: 长度不为 8"));

    // 判断特权: pid=1 (init) 视为特权进程
    let pid = process_get_current_pid();
    let is_privileged = pid == 1;

    match process_with(pid, |proc| {
        let mut rlimit_table = proc.rlimit_table.lock();
        rlimit_table.set(resource as usize, cur, max, is_privileged)
    }) {
        Some(Ok(())) => 0,
        Some(Err(e)) => e.as_ret(),
        None => Errno::ESRCH.as_ret(),
    }
}
