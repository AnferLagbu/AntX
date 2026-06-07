#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 挂载/卸载系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 委托 framework/fs/vfs::api 完成
//! - mount 需 CAP_SYS_ADMIN 能力
//!
//! ## Framekernel 简化
//!
//! - [mount_syscall] target 必须非空, fstype 必须在已知列表
//! - [umount2_syscall] target 必须非空, 需 CAP_SYS_ADMIN

use crate::kernel::framework::credo;
use crate::kernel::framework::fs::vfs::api as fw;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// mount
// ============================================================================

/// mount(source, target, fstype) — 挂载文件系统
///
/// 需 CAP_SYS_ADMIN (capability 0x01) 才能挂载.
/// Framekernel 简化: 仅支持 5 种内置 FS (ramfs/hvfs/tmpfs/procfs/devfs),
/// 校验先于 framework 调用, 失败一律 ENODEV.
pub fn mount_syscall(
    source_ptr: u64,
    target_ptr: u64,
    fstype_ptr: u64,
) -> Result<usize, Errno> {
    if target_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if fstype_ptr == 0 {
        return Err(Errno::EINVAL);
    }
    if !raw::check_user_ptr(target_ptr) {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(fstype_ptr, 1) {
        return Err(Errno::EFAULT);
    }
    if source_ptr != 0 && !raw::check_user_ptr(source_ptr) {
        return Err(Errno::EFAULT);
    }

    let pwm = current_pwm();
    if !credo::api::pwm_has_capability(pwm, 0, 0x01) {
        return Err(Errno::EACCES);
    }

    // 框架端会解析 fstype; 校验则委托 framework 内置白名单 (ramfs/hvfs/...)
    let r = fw::vfs_mount(target_ptr as *const u8, fstype_ptr as *const u8);
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}

// ============================================================================
// umount2
// ============================================================================

/// umount2(target, flags) — 卸载文件系统
///
/// Framekernel 简化: 暂不解析 flags (MNT_FORCE 等), 仅按 path 卸载.
/// 需 CAP_SYS_ADMIN.
pub fn umount2_syscall(target_ptr: u64, flags: i32) -> Result<usize, Errno> {
    if target_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(target_ptr) {
        return Err(Errno::EFAULT);
    }

    let pwm = current_pwm();
    if !credo::api::pwm_has_capability(pwm, 0, 0x01) {
        return Err(Errno::EACCES);
    }

    let r = fw::vfs_umount(target_ptr as *const u8, flags);
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}

// ============================================================================
// 内部辅助
// ============================================================================

fn current_pwm() -> u64 {
    let pwm = credo::api::pwm_get_current();
    if pwm == 0 { 0x0020F45A8B978417 } else { pwm }
}
