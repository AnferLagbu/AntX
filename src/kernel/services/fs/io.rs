#![deny(unsafe_code)]
//! IO 系统调用 — services 层安全代理
//!
//! ## 范围
//!
//! - pipe: 匿名管道创建
//! - dup/dup2: 文件描述符复制
//! - fcntl: 文件控制
//!
//! ## 安全边界
//!
//! - services 层: 验证参数类型/范围,委托 framework 实现
//! - framework 层: 实际访问 VFS / 创建内核对象

use crate::kernel::framework::syscall::types::Errno;

/// pipe 系统调用安全代理
///
/// `fds` 指向用户空间 i32[2] 数组 (8 字节)
pub fn pipe_syscall(fds: u64) -> Result<usize, Errno> {
    if fds == 0 {
        return Err(Errno::EFAULT);
    }
    let ret = crate::kernel::framework::syscall::io::sys_pipe(fds);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}

/// dup 安全代理
pub fn dup_syscall(oldfd: i32) -> Result<usize, Errno> {
    if oldfd < 0 {
        return Err(Errno::EBADF);
    }
    let ret = crate::kernel::framework::syscall::io::sys_dup(oldfd);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}

/// dup2 安全代理
pub fn dup2_syscall(oldfd: i32, newfd: i32) -> Result<usize, Errno> {
    if oldfd < 0 || newfd < 0 {
        return Err(Errno::EBADF);
    }
    let ret = crate::kernel::framework::syscall::io::sys_dup2(oldfd, newfd);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}

/// fcntl 安全代理
pub fn fcntl_syscall(fd: i32, cmd: i32, arg: u64) -> Result<usize, Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let ret = crate::kernel::framework::syscall::io::sys_fcntl(fd, cmd, arg);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}
