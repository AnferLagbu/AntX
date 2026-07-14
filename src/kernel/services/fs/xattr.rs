//! 扩展属性 (xattr) 系统调用处理器
//!
//! 提供 setxattr/getxattr/listxattr/removexattr 系统调用的实现。
//! 调用 framework 层的 vfs_*_internal 函数处理指针转换。

use crate::kernel::services::syscall::types::Errno;

/// 设置扩展属性
pub fn setxattr_syscall(path_ptr: u64, name_ptr: u64, value_ptr: u64, value_len: usize, pwm: u64) -> Result<usize, Errno> {
    if path_ptr == 0 || name_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let r = crate::kernel::framework::fs::vfs::api::vfs_setxattr_internal(
        path_ptr as *const u8,
        name_ptr as *const u8,
        value_ptr as *const u8,
        value_len as u32,
        pwm,
    );

    if r >= 0 { Ok(r as usize) } else { Err(Errno::from_ret(r as i64)) }
}

/// 获取扩展属性
pub fn getxattr_syscall(path_ptr: u64, name_ptr: u64, buf_ptr: u64, buf_len: usize, pwm: u64) -> Result<usize, Errno> {
    if path_ptr == 0 || name_ptr == 0 || buf_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let r = crate::kernel::framework::fs::vfs::api::vfs_getxattr_internal(
        path_ptr as *const u8,
        name_ptr as *const u8,
        buf_ptr as *mut u8,
        buf_len as u32,
        pwm,
    );

    if r >= 0 { Ok(r as usize) } else { Err(Errno::from_ret(r as i64)) }
}

/// 列出扩展属性
pub fn listxattr_syscall(path_ptr: u64, buf_ptr: u64, buf_len: usize, pwm: u64) -> Result<usize, Errno> {
    if path_ptr == 0 || buf_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let r = crate::kernel::framework::fs::vfs::api::vfs_listxattr_internal(
        path_ptr as *const u8,
        buf_ptr as *mut u8,
        buf_len as u32,
        pwm,
    );

    if r >= 0 { Ok(r as usize) } else { Err(Errno::from_ret(r as i64)) }
}

/// 删除扩展属性
pub fn removexattr_syscall(path_ptr: u64, name_ptr: u64, pwm: u64) -> Result<usize, Errno> {
    if path_ptr == 0 || name_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    let r = crate::kernel::framework::fs::vfs::api::vfs_removexattr_internal(
        path_ptr as *const u8,
        name_ptr as *const u8,
        pwm,
    );

    if r >= 0 { Ok(r as usize) } else { Err(Errno::from_ret(r as i64)) }
}
