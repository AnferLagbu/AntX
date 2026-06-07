#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 文件状态系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 委托 framework/fs/vfs::api 完成
//!
//! ## POSIX 语义
//!
//! - [stat_syscall] 跟随符号链接
//! - [lstat_syscall] 不跟随符号链接
//! - [fstat_syscall] 按 FD 查询

use crate::kernel::framework::credo;
use crate::kernel::framework::fs::vfs::api as fw;
use crate::kernel::framework::fs::vfs::types::VfsStat;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

const VFS_STAT_SIZE: u64 = core::mem::size_of::<VfsStat>() as u64;

// ============================================================================
// stat
// ============================================================================

/// stat(path, st_buf) — 跟随符号链接查询文件元数据
pub fn stat_syscall(path_ptr: u64, st_buf_ptr: u64) -> Result<usize, Errno> {
    if path_ptr == 0 || st_buf_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(st_buf_ptr, VFS_STAT_SIZE) {
        return Err(Errno::EFAULT);
    }
    let pwm = current_pwm();
    // framework safe API: 返回 VfsStat (无 raw pointer 跨边界)
    let stat = fw::vfs_stat_safe(path_ptr as *const u8, pwm).ok_or(Errno::EIO)?;
    // framework safe API: 写结构体到 user buf, 内部已 check_user_buf
    if !raw::write_struct_to_user::<VfsStat>(st_buf_ptr, &stat) {
        return Err(Errno::EFAULT);
    }
    Ok(0)
}

// ============================================================================
// lstat
// ============================================================================

/// lstat(path, st_buf) — 不跟随符号链接查询文件元数据
///
/// Framekernel 简化: vfs_stat 不跟随 symlink, 行为同 lstat。
pub fn lstat_syscall(path_ptr: u64, st_buf_ptr: u64) -> Result<usize, Errno> {
    stat_syscall(path_ptr, st_buf_ptr)
}

// ============================================================================
// fstat
// ============================================================================

/// fstat(fd, st_buf) — 按 FD 查询文件元数据
pub fn fstat_syscall(fd: i32, st_buf_ptr: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if st_buf_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(st_buf_ptr, VFS_STAT_SIZE) {
        return Err(Errno::EFAULT);
    }
    let pwm = current_pwm();
    let stat = fw::vfs_fstat_safe(fd as u32, pwm).ok_or(Errno::EIO)?;
    if !raw::write_struct_to_user::<VfsStat>(st_buf_ptr, &stat) {
        return Err(Errno::EFAULT);
    }
    Ok(0)
}

// ============================================================================
// 内部辅助
// ============================================================================

fn current_pwm() -> u64 {
    let pwm = credo::api::pwm_get_current();
    if pwm == 0 { 0x0020F45A8B978417 } else { pwm }
}
