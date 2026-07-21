//! Socket 系统调用 — framework TCB 入口
//!
//! 服务于 services::net::syscall 调用的 raw unsafe 桥接。
//! 实际完成:
//! - 用户空间数据 copy-in/copy-out (依赖 userptr::validate_user_buf)
//! - 调 smoltcp 协议栈 (framework::net::init::sm_*)
//!
//! services 层通过 framework 层接口访问,本模块是 services/net/syscall.rs 的 TCB 后端.

use crate::kernel::framework::net_socket;
use crate::kernel::framework::userptr;
use crate::kernel::framework::errno::Errno;
use crate::kernel::framework::mm::{copy_from_user as safe_copy_from_user, copy_to_user as safe_copy_to_user};

use crate::kernel::services::net::socket::{SockAddrIn, SockType, Domain};

// ============================================================================
// 用户空间数据搬运 (TCB)
// ============================================================================

/// 从用户空间读 8 字节 sockaddr_in,返回 (SockAddrIn, Errno)
pub fn raw_read_sockaddr_in(ptr: u64) -> Result<SockAddrIn, Errno> {
    if ptr == 0 || !userptr::validate_user_buf(ptr, 8) {
        return Err(Errno::EFAULT);
    }
    let mut buf = [0u8; 8];
    // P0-I-37 修复: 走异常表保护版 copy_from_user, 用户 munmap 缓冲区时
    // 返回 EFAULT 而非 panic.
    if safe_copy_from_user(&mut buf, ptr, 8).is_err() {
        return Err(Errno::EFAULT);
    }
    let family = u16::from_be_bytes([buf[0], buf[1]]);
    if family != 2 {
        return Err(Errno::EAFNOSUPPORT);
    }
    let port = u16::from_be_bytes([buf[2], buf[3]]);
    let mut ip = [0u8; 4];
    ip.copy_from_slice(&buf[4..8]);
    Ok(SockAddrIn { port, ip })
}

/// 向用户空间写 8 字节 sockaddr_in
pub fn raw_write_sockaddr_in(ptr: u64, addr: &SockAddrIn) -> Result<(), Errno> {
    if ptr == 0 || !userptr::validate_user_buf(ptr, 8) {
        return Err(Errno::EFAULT);
    }
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&(2u16).to_be_bytes());
    out[2..4].copy_from_slice(&addr.port.to_be_bytes());
    out[4..8].copy_from_slice(&addr.ip);
    // P0-I-37 修复: 走异常表保护版 copy_to_user
    if safe_copy_to_user(ptr, &out, 8).is_err() {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// copy-in 用户空间数据到 alloc::vec::Vec
pub fn raw_copy_in(ptr: u64, len: u32) -> Result<alloc::vec::Vec<u8>, Errno> {
    if len == 0 {
        return Ok(alloc::vec::Vec::new());
    }
    if ptr == 0 || !userptr::validate_user_buf(ptr, len as u64) {
        return Err(Errno::EFAULT);
    }
    let mut buf = alloc::vec![0u8; len as usize];
    // P0-I-37 修复: 走异常表保护版
    if safe_copy_from_user(&mut buf, ptr, len as usize).is_err() {
        return Err(Errno::EFAULT);
    }
    Ok(buf)
}

/// copy-out 内核数据到用户空间,返回实际写入字节数
pub fn raw_copy_out(ptr: u64, len: u32, data: &[u8]) -> Result<u32, Errno> {
    if len == 0 {
        return Ok(0);
    }
    if ptr == 0 || !userptr::validate_user_buf(ptr, len as u64) {
        return Err(Errno::EFAULT);
    }
    let n = data.len().min(len as usize);
    // P0-I-37 修复: 走异常表保护版
    if safe_copy_to_user(ptr, &data[..n], n).is_err() {
        return Err(Errno::EFAULT);
    }
    Ok(n as u32)
}

/// 读取 4 字节 u32
pub fn raw_read_u32(ptr: u64) -> Result<u32, Errno> {
    if ptr == 0 || !userptr::validate_user_buf(ptr, 4) {
        return Err(Errno::EFAULT);
    }
    // SAFETY: ptr 由 check_user_buf 验证为可读 4 字节
    let v = unsafe { core::ptr::read_unaligned(ptr as *const u32) };
    Ok(v)
}

/// 写入 4 字节 u32
pub fn raw_write_u32(ptr: u64, v: u32) -> Result<(), Errno> {
    if ptr == 0 || !userptr::validate_user_buf(ptr, 4) {
        return Err(Errno::EFAULT);
    }
    // SAFETY: ptr 由 check_user_buf 验证为可写
    unsafe { core::ptr::write_unaligned(ptr as *mut u32, v); }
    Ok(())
}

// ============================================================================
// sockaddr_un 解析 (Phase C.3 UDS)
// ============================================================================

/// 从用户指针读 2 字节 sun_family (大端 u16)
pub fn raw_read_sun_family(ptr: u64) -> Result<u16, Errno> {
    if ptr == 0 || !userptr::validate_user_buf(ptr, 2) {
        return Err(Errno::EFAULT);
    }
    // SAFETY: ptr 由 check_user_buf 验证为可读 2 字节
    let lo = unsafe { core::ptr::read_volatile(ptr as *const u8) };
    let hi = unsafe { core::ptr::read_volatile((ptr + 1) as *const u8) };
    Ok(u16::from_be_bytes([hi, lo]))
}

/// 从用户指针读 sockaddr_un (110 字节布局: family u16 + path[108])
///
/// 返回 (path_bytes, path_len)。若用户提供的 addrlen 不足 2 字节返回 EFAULT。
pub fn raw_read_sockaddr_un(ptr: u64, addrlen: u32) -> Result<crate::kernel::services::net::unix::SockAddrUn, Errno> {
    if ptr == 0 || addrlen < 2 {
        return Err(Errno::EFAULT);
    }
    if !userptr::validate_user_buf(ptr, addrlen as u64) {
        return Err(Errno::EFAULT);
    }
    let mut buf = [0u8; 110];
    let copy_len = (addrlen as usize).min(110);
    // P0-I-37 修复: 走异常表保护版
    if safe_copy_from_user(&mut buf[..copy_len], ptr, copy_len).is_err() {
        return Err(Errno::EFAULT);
    }
    // 校验 family = AF_UNIX = 1
    let family = u16::from_be_bytes([buf[0], buf[1]]);
    if family != 1 {
        return Err(Errno::EAFNOSUPPORT);
    }
    // 路径从 offset 2 开始, 到第一个 NUL 或末尾
    let path_bytes = &buf[2..copy_len];
    let nul_pos = path_bytes.iter().position(|&b| b == 0).unwrap_or(path_bytes.len());
    if nul_pos == 0 {
        return Err(Errno::EINVAL);
    }
    let mut path = [0u8; 108];
    path[..nul_pos].copy_from_slice(&path_bytes[..nul_pos]);
    Ok(crate::kernel::services::net::unix::SockAddrUn {
        path,
        path_len: nul_pos as u16,
    })
}

/// 向用户指针写 sockaddr_un (110 字节布局)
pub fn raw_write_sockaddr_un(
    ptr: u64,
    addrlen_ptr: u64,
    addr: &crate::kernel::services::net::unix::SockAddrUn,
) -> Result<(), Errno> {
    if ptr == 0 || addrlen_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !userptr::validate_user_buf(ptr, 110) {
        return Err(Errno::EFAULT);
    }
    if !userptr::validate_user_buf(addrlen_ptr, 4) {
        return Err(Errno::EFAULT);
    }
    let mut buf = [0u8; 110];
    buf[0..2].copy_from_slice(&(1u16).to_be_bytes()); // AF_UNIX
    let n = addr.path_len as usize;
    buf[2..2 + n].copy_from_slice(&addr.path[..n]);
    // P0-I-37 修复: 走异常表保护版
    if safe_copy_to_user(ptr, &buf, 110).is_err() {
        return Err(Errno::EFAULT);
    }
    // 写回 addrlen = 110
    let total = (n + 2) as u32;
    let total_bytes = total.to_ne_bytes();
    if safe_copy_to_user(addrlen_ptr, &total_bytes, 4).is_err() {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

// ============================================================================
// Socket 12 Syscall TCB 实现
// ============================================================================

/// socket — 创建 socket
pub fn socket_syscall(domain: i32, sock_type: i32, _protocol: i32) -> i64 {
    let d = match Domain::from_i32(domain) {
        Some(x) => x,
        None => return Errno::EAFNOSUPPORT.as_ret(),
    };
    let t = match SockType::from_i32(sock_type) {
        Some(x) => x,
        None => return Errno::EINVAL.as_ret(),
    };
    let fd = net_socket::sm_socket(d as i32, t as i32, 0);
    if fd < 0 { Errno::EINVAL.as_ret() } else { fd as i64 }
}

/// bind
pub fn bind_syscall(fd: i32, addr_ptr: u64, _addrlen: u32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let addr = match raw_read_sockaddr_in(addr_ptr) {
        Ok(a) => a,
        Err(e) => return e.as_ret(),
    };
    let bytes = [
        0u8, 2, // AF_INET
        (addr.port >> 8) as u8, (addr.port & 0xFF) as u8,
        addr.ip[0], addr.ip[1], addr.ip[2], addr.ip[3],
    ];
    let rc = net_socket::sm_bind(fd, bytes.as_ptr(), 8);
    if rc == 0 { 0 } else { Errno::EINVAL.as_ret() }
}

/// listen
pub fn listen_syscall(fd: i32, backlog: i32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    if backlog < 0 { return Errno::EINVAL.as_ret(); }
    let rc = net_socket::sm_listen(fd, backlog);
    if rc == 0 { 0 } else { Errno::EINVAL.as_ret() }
}

/// accept — 当前简化:不写对端地址
pub fn accept_syscall(fd: i32, _addr_ptr: u64, _addrlen_ptr: u64) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let new_fd = net_socket::sm_accept(fd, core::ptr::null_mut(), core::ptr::null_mut());
    if new_fd < 0 { Errno::EBADF.as_ret() } else { new_fd as i64 }
}

/// connect
pub fn connect_syscall(fd: i32, addr_ptr: u64, _addrlen: u32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let addr = match raw_read_sockaddr_in(addr_ptr) {
        Ok(a) => a,
        Err(e) => return e.as_ret(),
    };
    let bytes = [
        0u8, 2,
        (addr.port >> 8) as u8, (addr.port & 0xFF) as u8,
        addr.ip[0], addr.ip[1], addr.ip[2], addr.ip[3],
    ];
    let rc = net_socket::sm_connect(fd, bytes.as_ptr(), 8);
    if rc == 0 { 0 } else { Errno::ECONNREFUSED.as_ret() }
}

/// sendto / send
pub fn sendto_syscall(
    fd: i32,
    buf_ptr: u64,
    len: u32,
    _flags: i32,
    dest_ptr: u64,
    _dest_len: u32,
) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let data = match raw_copy_in(buf_ptr, len) {
        Ok(v) => v,
        Err(e) => return e.as_ret(),
    };
    let rc = if dest_ptr == 0 {
        net_socket::sm_send(fd, data.as_ptr(), data.len() as u32, 0)
    } else {
        let dest = match raw_read_sockaddr_in(dest_ptr) {
            Ok(a) => a,
            Err(e) => return e.as_ret(),
        };
        let bytes = [
            0u8, 2,
            (dest.port >> 8) as u8, (dest.port & 0xFF) as u8,
            dest.ip[0], dest.ip[1], dest.ip[2], dest.ip[3],
        ];
        net_socket::sm_sendto(fd, data.as_ptr(), data.len() as u32, 0, bytes.as_ptr(), 8)
    };
    if rc < 0 { Errno::EINVAL.as_ret() } else { rc as i64 }
}

/// recvfrom / recv
pub fn recvfrom_syscall(
    fd: i32,
    buf_ptr: u64,
    len: u32,
    _flags: i32,
    _src_ptr: u64,
    _src_len_ptr: u64,
) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    if buf_ptr == 0 || len == 0 { return Errno::EFAULT.as_ret(); }
    if !userptr::validate_user_buf(buf_ptr, len as u64) { return Errno::EFAULT.as_ret(); }
    // 在栈上准备临时缓冲
    const MAX: usize = 4096;
    let want = (len as usize).min(MAX);
    let mut stack_buf = [0u8; MAX];
    let n = net_socket::sm_recv(fd, stack_buf.as_mut_ptr(), want as u32, 0);
    if n < 0 {
        return Errno::EAGAIN.as_ret();
    }
    // P0-I-37 修复: 走异常表保护版
    if safe_copy_to_user(buf_ptr, &stack_buf[..n as usize], n as usize).is_err() {
        return Errno::EFAULT.as_ret();
    }
    n as i64
}

/// setsockopt
pub fn setsockopt_syscall(
    fd: i32,
    level: i32,
    optname: i32,
    val_ptr: u64,
    _valen: u32,
) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let v = match raw_read_u32(val_ptr) {
        Ok(x) => x,
        Err(e) => return e.as_ret(),
    };
    let val_bytes = v.to_ne_bytes();
    let rc = net_socket::sm_setsockopt(fd, level, optname, val_bytes.as_ptr(), val_bytes.len() as u32);
    if rc == 0 { 0 } else { Errno::ENOSYS.as_ret() }
}

/// getsockopt
pub fn getsockopt_syscall(
    fd: i32,
    level: i32,
    optname: i32,
    val_ptr: u64,
    _valen_ptr: u64,
) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let mut out = 0u32;
    let mut out_len = core::mem::size_of::<u32>() as u32;
    let rc = net_socket::sm_getsockopt(
        fd, level, optname,
        &mut out as *mut u32 as *mut u8,
        &mut out_len,
    );
    if rc != 0 { return Errno::ENOSYS.as_ret(); }
    if let Err(e) = raw_write_u32(val_ptr, out) {
        return e.as_ret();
    }
    0
}

/// shutdown — 简化等同 close
pub fn shutdown_syscall(fd: i32, _how: i32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let rc = net_socket::sm_close(fd);
    if rc == 0 { 0 } else { Errno::EBADF.as_ret() }
}

/// sendmsg(fd, msg, flags) — services 层入口
pub fn sendmsg_syscall(fd: i32, msg_ptr: u64, _flags: i32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let rc = net_socket::sm_sendmsg(fd, msg_ptr as *const u8, 0);
    rc as i64
}

/// recvmsg(fd, msg, flags) — services 层入口
pub fn recvmsg_syscall(fd: i32, msg_ptr: u64, _flags: i32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let rc = net_socket::sm_recvmsg(fd, msg_ptr as *mut u8, 0);
    rc as i64
}

/// getsockname(fd, addr, addrlen) — 获取本端地址
pub fn getsockname_syscall(fd: i32, addr_ptr: u64, addrlen_ptr: u64) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let rc = net_socket::sm_getsockname(
        fd,
        addr_ptr as *mut u8,
        addrlen_ptr as *mut u32,
    );
    rc as i64
}

/// getpeername(fd, addr, addrlen) — 获取对端地址
pub fn getpeername_syscall(fd: i32, addr_ptr: u64, addrlen_ptr: u64) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    let rc = net_socket::sm_getpeername(
        fd,
        addr_ptr as *mut u8,
        addrlen_ptr as *mut u32,
    );
    rc as i64
}

extern crate alloc;
