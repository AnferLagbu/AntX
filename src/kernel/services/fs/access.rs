#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 文件访问系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 委托 framework/fs/vfs::api 完成
//!
//! ## POSIX 语义
//!
//! - [access_syscall] 检查可访问性 (R_OK/W_OK/X_OK/F_OK)
//! - [unlink_syscall] 解除链接 (删除文件)

use crate::kernel::framework::credo;
use crate::kernel::framework::fs::vfs::api as fw;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 权限位
// ============================================================================

/// F_OK - 文件存在性检查
pub const F_OK: i32 = 0;
/// R_OK - 可读
pub const R_OK: i32 = 4;
/// W_OK - 可写
pub const W_OK: i32 = 2;
/// X_OK - 可执行
pub const X_OK: i32 = 1;

// ============================================================================
// access
// ============================================================================

/// access(path, mode) — 检查当前用户对路径的访问权
///
/// mode 是 R_OK/W_OK/X_OK 的位或, F_OK 表示存在性检查.
/// Framekernel 简化: PWM 即身份, 不深入 rwx 位, 仅检查存在性.
pub fn access_syscall(path_ptr: u64, mode: i32) -> Result<usize, Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    // mode 仅取低 6 位 (POSIX R|W|X|F)
    if !(0..=0o7).contains(&mode) {
        return Err(Errno::EINVAL);
    }
    let pwm = current_pwm()?;
    // 调用 vfs_stat_safe 验证存在性; 不需要 VfsStat 内容.
    let _stat = fw::vfs_stat_safe(path_ptr as *const u8, pwm).ok_or(Errno::EACCES)?;
    Ok(0)
}

// ============================================================================
// faccessat (简化: 同 access)
// ============================================================================

/// faccessat(dirfd, path, mode, flags) — 相对目录 fd 的 access.
///
/// Framekernel 简化: 不支持 AT_EACCESS/AT_SYMLINK_NOFOLLOW, 行为同 access.
pub fn faccessat_syscall(_dirfd: i32, path_ptr: u64, mode: i32, _flags: i32) -> Result<usize, Errno> {
    access_syscall(path_ptr, mode)
}

// ============================================================================
// unlink
// ============================================================================

/// unlink(path) — 删除一个名称到 inode 的链接
///
/// 若为最后链接且无进程打开该文件, 则删除文件.
pub fn unlink_syscall(path_ptr: u64) -> Result<usize, Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    let pwm = current_pwm()?;
    let r = fw::vfs_unlink(path_ptr as *const u8, pwm);
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}

// ============================================================================
// 内部辅助
// ============================================================================

/// 取当前进程凭证,无会话时直接返回 EACCES (历史硬编码 TEST_PWM 路径已弃用)。
fn current_pwm() -> Result<u64, Errno> {
    let pwm = credo::api::pwm_get_current();
    if pwm == 0 { Err(Errno::EACCES) } else { Ok(pwm) }
}
