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
// 内部辅助
// ============================================================================

fn current_pwm() -> u64 {
    let pwm = credo::api::pwm_get_current();
    if pwm == 0 { 0x0020F45A8B978417 } else { pwm }
}
