#![deny(unsafe_code)]
//! 文件模式/目录系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe,纯类型安全
//! - 委托 framework/fs/vfs::api 完成

use crate::kernel::framework::credo;
use crate::kernel::framework::fs::vfs::api as fw;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// umask
// ============================================================================

/// umask(mask) — 设置文件创建掩码,返回旧值
///
/// 简化: 0o777 范围 mask, 0 是合法参数。
pub fn umask_syscall(mask: u32) -> Result<usize, Errno> {
    // mask 范围 0..=0o777 (9 bit)
    if mask > 0o777 {
        return Err(Errno::EINVAL);
    }
    Ok(crate::kernel::framework::credo::api::umask_set(mask) as usize)
}

// ============================================================================
// mkdir
// ============================================================================

/// mkdir(path, mode) — 创建目录
pub fn mkdir_syscall(path_ptr: u64, _mode: i32) -> Result<usize, Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    let pwm = current_pwm()?;
    // SAFETY: path_ptr 由 check_user_ptr 验证
    let r = fw::vfs_mkdir(path_ptr as *const u8, pwm);
    if r < 0 { Err(Errno::from_ret(r as i64)) } else { Ok(r as usize) }
}

// ============================================================================
// rmdir
// ============================================================================

/// rmdir(path) — 删除目录
pub fn rmdir_syscall(path_ptr: u64) -> Result<usize, Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    let pwm = current_pwm()?;
    let r = fw::vfs_rmdir(path_ptr as *const u8, pwm);
    if r < 0 { Err(Errno::from_ret(r as i64)) } else { Ok(r as usize) }
}

// ============================================================================
// chmod
// ============================================================================

/// chmod(path, mode) — 改变文件权限
pub fn chmod_syscall(path_ptr: u64, mode: u32) -> Result<usize, Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    // mode 9 bit 合法
    if mode > 0o7777 {
        return Err(Errno::EINVAL);
    }
    let pwm = current_pwm()?;
    let r = fw::vfs_chmod(path_ptr as *const u8, mode as u16, pwm);
    if r < 0 { Err(Errno::from_ret(r as i64)) } else { Ok(r as usize) }
}

// ============================================================================
// fchmod
// ============================================================================

/// fchmod(fd, mode) — 按 FD 改变权限
pub fn fchmod_syscall(fd: i32, mode: u32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if mode > 0o7777 {
        return Err(Errno::EINVAL);
    }
    let r = fw::vfs_fchmod(fd as u32, mode as u16);
    if r < 0 { Err(Errno::from_ret(r as i64)) } else { Ok(r as usize) }
}

// ============================================================================
// 内部辅助
// ============================================================================

/// 取当前进程凭证,无会话时直接返回 EACCES (历史硬编码 TEST_PWM 路径已弃用)。
fn current_pwm() -> Result<u64, Errno> {
    let pwm = credo::api::pwm_get_current();
    if pwm == 0 { Err(Errno::EACCES) } else { Ok(pwm) }
}
