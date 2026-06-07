#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 文件打开/关闭系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 委托 framework/fs/vfs::api 完成
//!
//! ## POSIX 语义
//!
//! - [open_syscall] 打开一个文件, 返回 fd
//! - [close_syscall] 关闭一个 fd
//! - [creat_syscall] 等价于 open(path, O_WRONLY|O_CREAT|O_TRUNC, mode)

use crate::kernel::framework::credo;
use crate::kernel::framework::fs::vfs::api as fw;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// open flags
// ============================================================================

/// 只读打开
pub const O_RDONLY: i32 = 0o0;
/// 只写打开
pub const O_WRONLY: i32 = 0o1;
/// 读写打开
pub const O_RDWR: i32 = 0o2;
/// 若不存在则创建 (必须与 O_WRONLY 或 O_RDWR 之一同用)
pub const O_CREAT: i32 = 0o100;
/// 若已存在且是常规文件且以写方式打开, 则长度截断为 0
pub const O_TRUNC: i32 = 0o1000;
/// 若已存在则打开失败
pub const O_EXCL: i32 = 0o200;
/// 追加模式, 每次写都在文件末尾
pub const O_APPEND: i32 = 0o2000;
/// 必须是目录
pub const O_DIRECTORY: i32 = 0o200_000;
/// 跟随符号链接末尾的 too-many-symlinks
pub const O_NOFOLLOW: i32 = 0o400_000;
/// close-on-exec: exec 时关闭 fd
pub const O_CLOEXEC: i32 = 0o2_000_000;

// ============================================================================
// 内部辅助
// ============================================================================

fn current_pwm() -> u64 {
    let pwm = credo::api::pwm_get_current();
    if pwm == 0 { 0x0020F45A8B978417 } else { pwm }
}

/// 验证 open flags 合法: 只接受低 3 位访问模式, 其他位 POSIX 定义.
fn validate_flags(flags: i32) -> Result<(), Errno> {
    if flags < 0 {
        return Err(Errno::EINVAL);
    }
    // 访问模式必须恰好是 O_RDONLY / O_WRONLY / O_RDWR 其一
    let access = flags & 0o3;
    if access > 0o2 {
        return Err(Errno::EINVAL);
    }
    // O_CREAT 必须配合 O_WRONLY 或 O_RDWR
    if (flags & O_CREAT) != 0 && access == O_RDONLY {
        return Err(Errno::EINVAL);
    }
    // O_TRUNC 隐含可写
    if (flags & O_TRUNC) != 0 && access == O_RDONLY {
        return Err(Errno::EINVAL);
    }
    // O_DIRECTORY 与 O_CREAT 不能同用
    if (flags & O_DIRECTORY) != 0 && (flags & O_CREAT) != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// ============================================================================
// open
// ============================================================================

/// open(path, flags, mode) — 打开一个文件
///
/// 返回新文件描述符 (>= 0), 失败返回负 errno.
pub fn open_syscall(path_ptr: u64, flags: i32, mode: i32) -> Result<usize, Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(path_ptr) {
        return Err(Errno::EFAULT);
    }
    validate_flags(flags)?;
    // mode 仅在 O_CREAT 生效, 否则忽略
    if (flags & O_CREAT) == 0 && mode != 0 {
        return Err(Errno::EINVAL);
    }
    // mode 必须 < 0o7777 (suid/sgid/sticky 之外)
    if (flags & O_CREAT) != 0 && (mode < 0 || mode > 0o7777) {
        return Err(Errno::EINVAL);
    }
    let pwm = current_pwm();
    let r = fw::vfs_open(path_ptr as *const u8, flags as u32, pwm);
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(r as usize)
    }
}

// ============================================================================
// creat
// ============================================================================

/// creat(path, mode) — 等价于 open(path, O_WRONLY|O_CREAT|O_TRUNC, mode)
pub fn creat_syscall(path_ptr: u64, mode: i32) -> Result<usize, Errno> {
    const CREAT_FLAGS: i32 = O_WRONLY | O_CREAT | O_TRUNC;
    open_syscall(path_ptr, CREAT_FLAGS, mode)
}

// ============================================================================
// close
// ============================================================================

/// close(fd) — 关闭文件描述符
pub fn close_syscall(fd: i32) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let r = fw::vfs_close(fd as u32);
    if r < 0 {
        Err(Errno::from_ret(r as i64))
    } else {
        Ok(0)
    }
}
