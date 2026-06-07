#![deny(unsafe_code)]
//! Socket 系统调用 — services 层入口代理
//!
//! ## 职责
//!
//! - 0 unsafe,纯类型安全
//! - 委托 framework/net/syscall.rs 完成用户空间数据搬运 + smoltcp 协议栈调用
//!
//! ## 与 [super::socket] 区别
//!
//! - [super::socket] 强类型 API (Domain, SockType, SockAddrIn, &[u8])
//! - 本模块 syscall 入口 API (i32, u64 用户指针, u32 长度)

use crate::kernel::framework::net::syscall as fw;
use crate::kernel::framework::syscall::types::Errno;
use crate::kernel::framework::syscall::raw;

// ============================================================================
// 12 个 Socket Syscall 安全代理
// ============================================================================

/// socket(domain, type, protocol) — 返回新 FD
pub fn socket_syscall(domain: i32, sock_type: i32, protocol: i32) -> Result<usize, Errno> {
    let r = fw::socket_syscall(domain, sock_type, protocol);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// bind(fd, addr, addrlen)
pub fn bind_syscall(fd: i32, addr_ptr: u64, addrlen: u32) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    let r = fw::bind_syscall(fd, addr_ptr, addrlen);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// listen(fd, backlog)
pub fn listen_syscall(fd: i32, backlog: i32) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    if backlog < 0 { return Err(Errno::EINVAL); }
    let r = fw::listen_syscall(fd, backlog);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// accept(fd, addr, addrlen)
pub fn accept_syscall(fd: i32, addr_ptr: u64, addrlen_ptr: u64) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    let r = fw::accept_syscall(fd, addr_ptr, addrlen_ptr);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// connect(fd, addr, addrlen)
pub fn connect_syscall(fd: i32, addr_ptr: u64, addrlen: u32) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    let r = fw::connect_syscall(fd, addr_ptr, addrlen);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// sendto(fd, buf, len, flags, dest_addr, addrlen)
pub fn sendto_syscall(
    fd: i32,
    buf_ptr: u64,
    len: u32,
    flags: i32,
    dest_ptr: u64,
    dest_len: u32,
) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    let r = fw::sendto_syscall(fd, buf_ptr, len, flags, dest_ptr, dest_len);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// recvfrom(fd, buf, len, flags, src_addr, addrlen)
pub fn recvfrom_syscall(
    fd: i32,
    buf_ptr: u64,
    len: u32,
    flags: i32,
    src_ptr: u64,
    src_len_ptr: u64,
) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    if buf_ptr == 0 || len == 0 { return Err(Errno::EFAULT); }
    let r = fw::recvfrom_syscall(fd, buf_ptr, len, flags, src_ptr, src_len_ptr);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// setsockopt(fd, level, optname, val, valen)
pub fn setsockopt_syscall(
    fd: i32,
    level: i32,
    optname: i32,
    val_ptr: u64,
    valen: u32,
) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    let r = fw::setsockopt_syscall(fd, level, optname, val_ptr, valen);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// getsockopt(fd, level, optname, val, valen)
pub fn getsockopt_syscall(
    fd: i32,
    level: i32,
    optname: i32,
    val_ptr: u64,
    valen_ptr: u64,
) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    let r = fw::getsockopt_syscall(fd, level, optname, val_ptr, valen_ptr);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// shutdown(fd, how)
pub fn shutdown_syscall(fd: i32, how: i32) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    let r = fw::shutdown_syscall(fd, how);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// sendmsg(fd, msg, flags)
/// 真实实现: 校验 msg 指针 (Msghdr 56B 可读) + iov 范围; 委托 framework。
pub fn sendmsg_syscall(fd: i32, msg_ptr: u64, flags: i32) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    if msg_ptr == 0 { return Err(Errno::EFAULT); }
    if !raw::check_user_buf(msg_ptr, 56) {
        return Err(Errno::EFAULT);
    }
    let iov_ptr = raw::read_u64_from_user(msg_ptr + 16).ok_or(Errno::EFAULT)?;
    let iovlen = raw::read_u64_from_user(msg_ptr + 24).ok_or(Errno::EFAULT)?;
    if iovlen == 0 || iovlen > 1024 {
        return Err(Errno::EINVAL);
    }
    if iov_ptr == 0 {
        return Err(Errno::EINVAL);
    }
    let iov_bytes = iovlen.checked_mul(16).ok_or(Errno::EINVAL)?;
    if !raw::check_user_buf(iov_ptr, iov_bytes) {
        return Err(Errno::EFAULT);
    }
    let r = fw::sendmsg_syscall(fd, msg_ptr, flags);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}

/// recvmsg(fd, msg, flags)
pub fn recvmsg_syscall(fd: i32, msg_ptr: u64, flags: i32) -> Result<usize, Errno> {
    if fd < 0 { return Err(Errno::EBADF); }
    if msg_ptr == 0 { return Err(Errno::EFAULT); }
    if !raw::check_user_buf(msg_ptr, 56) {
        return Err(Errno::EFAULT);
    }
    let iov_ptr = raw::read_u64_from_user(msg_ptr + 16).ok_or(Errno::EFAULT)?;
    let iovlen = raw::read_u64_from_user(msg_ptr + 24).ok_or(Errno::EFAULT)?;
    if iovlen == 0 || iovlen > 1024 {
        return Err(Errno::EINVAL);
    }
    if iov_ptr == 0 {
        return Err(Errno::EINVAL);
    }
    let iov_bytes = iovlen.checked_mul(16).ok_or(Errno::EINVAL)?;
    if !raw::check_user_buf(iov_ptr, iov_bytes) {
        return Err(Errno::EFAULT);
    }
    let r = fw::recvmsg_syscall(fd, msg_ptr, flags);
    if r < 0 { Err(Errno::from_ret(r)) } else { Ok(r as usize) }
}
