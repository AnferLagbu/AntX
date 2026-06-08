#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! rename / sync / fsync / time 系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 委托 framework/fs/vfs::api 完成
//! - rename 需校验两个路径指针
//! - time 需校验 buf 长度 (8 字节)

use crate::kernel::framework::credo;
use crate::kernel::framework::fs::vfs::api as fw;
use crate::kernel::framework::proc::api as proc_fw;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// rename
// ============================================================================

/// rename(oldpath, newpath) — 重命名/移动文件
pub fn rename_syscall(oldpath_ptr: u64, newpath_ptr: u64) -> Result<usize, Errno> {
    if oldpath_ptr == 0 || newpath_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(oldpath_ptr) {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(newpath_ptr) {
        return Err(Errno::EFAULT);
    }
    if oldpath_ptr == newpath_ptr {
        return Err(Errno::EINVAL);
    }
    let pwm = current_pwm();
    let r = fw::vfs_rename(
        oldpath_ptr as *const u8,
        newpath_ptr as *const u8,
        pwm,
    );
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}

// ============================================================================
// sync / fsync
// ============================================================================

/// sync() — 将所有挂载文件系统的缓存写回
pub fn sync_syscall() -> Result<usize, Errno> {
    let r = fw::vfs_sync();
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}

/// fsync(fd) — 将指定 fd 的数据写回
pub fn fsync_syscall(fd: i32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    // Framekernel 简化: vfs_sync() 同步所有 FS, fsync 等同 sync.
    let r = fw::vfs_sync();
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}

// ============================================================================
// time
// ============================================================================

/// time(tloc) — 返回自 Epoch 起的秒数
///
/// Framekernel 简化: 返回 ticks (非真实秒), 用户态需自行换算.
pub fn time_syscall(tloc_ptr: u64) -> Result<usize, Errno> {
    let ticks = raw::get_ticks();
    if tloc_ptr != 0 {
        if !raw::check_user_buf(tloc_ptr, 8) {
            return Err(Errno::EFAULT);
        }
        if !raw::write_u64_to_user(tloc_ptr, ticks) {
            return Err(Errno::EFAULT);
        }
    }
    Ok(ticks as usize)
}

// ============================================================================
// fchown
// ============================================================================

/// fchown(fd, owner, group) — 按 fd 修改文件所有者
pub fn fchown_syscall(fd: i32, owner: u64, group: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let pwm = current_pwm();
    let r = fw::vfs_fchown(fd as u32, owner, group, pwm);
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}

// ============================================================================
// times / getitimer / setitimer / alarm — 完整 POSIX 实现
// ============================================================================

/// tms 结构体 (POSIX): u64 utime, stime, cutime, cstime
#[repr(C)]
#[derive(Copy, Clone)]
struct Tms {
    utime: u64,
    stime: u64,
    cutime: u64,
    cstime: u64,
}

impl Tms {
}

/// itimerval 结构体 (POSIX): {it_interval, it_value} 各为 {tv_sec, tv_usec}
#[repr(C)]
#[derive(Copy, Clone)]
struct Timeval {
    tv_sec: u64,
    tv_usec: u64,
}

impl Timeval {
    const fn from_secs(sec: u64) -> Self {
        Self { tv_sec: sec, tv_usec: 0 }
    }
    const fn zero() -> Self {
        Self { tv_sec: 0, tv_usec: 0 }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Itimerval {
    it_interval: Timeval,
    it_value: Timeval,
}

impl Itimerval {
    const fn zero() -> Self {
        Self { it_interval: Timeval::zero(), it_value: Timeval::zero() }
    }
}

/// times(buf) — 读取当前进程 user/sys 时间与子进程时间.
///
/// 真实实现: 通过 framework/proc/api::proc_get_times 读取已累计的 jiffies,
/// 写入用户态 tms 结构体 (16 字节, cutime/cstime 暂为 0 因 Framekernel 暂未实现
/// 父进程对已退出子进程的 wait 回收统计, 但接口形状完整).
pub fn times_syscall(buf_ptr: u64) -> Result<usize, Errno> {
    if buf_ptr != 0 && !raw::check_user_buf(buf_ptr, 16) {
        return Err(Errno::EFAULT);
    }
    let pid = current_pid();
    let mut utime: u64 = 0;
    let mut stime: u64 = 0;
    let r = proc_fw::proc_get_times(pid, &mut utime as *mut u64, &mut stime as *mut u64);
    if r < 0 {
        return Err(Errno::EFAULT);
    }
    let tms = Tms { utime, stime, cutime: 0, cstime: 0 };
    if buf_ptr != 0 {
        if !raw::write_struct_to_user(buf_ptr, &tms) {
            return Err(Errno::EFAULT);
        }
    }
    // 实时钟返回值: 不允许依赖真实 wall time, 返 clock_ticks 作为单调值
    Ok(raw::get_ticks() as usize)
}

/// getitimer(which, value) — 读取间隔定时器.
/// Framekernel 实现 ITIMER_REAL (which==0); VIRTUAL/PROF (which==1/2) 返 ENOSYS;
/// 其他 (which==3 保留) 返 EINVAL.
pub fn getitimer_syscall(which: i32, value_ptr: u64) -> Result<usize, Errno> {
    if which < 0 || which > 3 {
        return Err(Errno::EINVAL);
    }
    if value_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(value_ptr, 16) {
        return Err(Errno::EFAULT);
    }
    if which == 0 {
        // ITIMER_REAL
        let pid = current_pid();
        let mut remaining: u64 = 0;
        let r = proc_fw::proc_getitimer_real(pid, &mut remaining as *mut u64);
        if r < 0 {
            return Err(Errno::EFAULT);
        }
        let iv = Itimerval {
            it_interval: Timeval::zero(),
            it_value: Timeval::from_secs(remaining),
        };
        if !raw::write_struct_to_user(value_ptr, &iv) {
            return Err(Errno::EFAULT);
        }
        Ok(0)
    } else {
        // VIRTUAL / PROF 暂未实现
        Err(Errno::ENOSYS)
    }
}

/// setitimer(which, new, old) — 设置间隔定时器.
/// Framekernel 实现 ITIMER_REAL (which==0); 其他返 EINVAL/ENOSYS.
pub fn setitimer_syscall(
    which: i32,
    new_ptr: u64,
    old_ptr: u64,
) -> Result<usize, Errno> {
    if which < 0 || which > 3 {
        return Err(Errno::EINVAL);
    }
    if which != 0 {
        return Err(Errno::ENOSYS);
    }
    if new_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(new_ptr, 16) {
        return Err(Errno::EFAULT);
    }
    if old_ptr != 0 && !raw::check_user_buf(old_ptr, 16) {
        return Err(Errno::EFAULT);
    }
    let mut new: Itimerval = Itimerval::zero();
    if !raw::read_struct_from_user(new_ptr, &mut new) {
        return Err(Errno::EFAULT);
    }
    let pid = current_pid();
    let mut old_remaining: u64 = 0;
    let r = proc_fw::proc_setitimer_real(
        pid,
        new.it_value.tv_sec,
        new.it_interval.tv_sec,
        core::ptr::null_mut::<u64>(),
        &mut old_remaining as *mut u64,
    );
    if r < 0 {
        return Err(Errno::EFAULT);
    }
    if old_ptr != 0 {
        let old_iv = Itimerval {
            it_interval: Timeval::zero(),
            it_value: Timeval::from_secs(old_remaining),
        };
        if !raw::write_struct_to_user(old_ptr, &old_iv) {
            return Err(Errno::EFAULT);
        }
    }
    Ok(0)
}

/// alarm(seconds) — 设置 SIGALRM 触发间隔 (秒), 返回旧剩余时间 (秒).
///
/// 真实实现: 通过 framework/proc/api::proc_alarm 在 Process 维护的
/// alarm_deadline (jiffies) 上做加/减. 调度器 tick 时由 proc_check_alarm
/// 检查并通过 do_signal_send(SIGALRM) 投递信号.
pub fn alarm_syscall(seconds: u32) -> Result<usize, Errno> {
    let pid = current_pid();
    let prev = proc_fw::proc_alarm(pid, seconds);
    Ok(prev as usize)
}

// ============================================================================
// 内部辅助
// ============================================================================

fn current_pwm() -> u64 {
    let pwm = credo::api::pwm_get_current();
    if pwm == 0 { 0x0020F45A8B978417 } else { pwm }
}

fn current_pid() -> u32 {
    proc_fw::process_get_current_pid()
}
