//! Socket 子系统 — services 层安全代理
//!
//! ## 状态 (v2.8, 2026-06-04)
//!
//! Phase 2.4 net 2/4 子系统迁移: 封装 `kernel::net::init::sm_*` 12 个 FFI:
//! - [x] socket/bind/listen/accept/connect (TCP 客户端/服务器)
//! - [x] send/recv (TCP 字节流)
//! - [x] sendto/recvfrom (UDP 数据报)
//! - [x] close/setsockopt/getsockopt
//! - [x] poll_sockets
//!
//! ## 迁移方法
//!
//! 1. `unsafe extern "C" fn sm_*(...) -> i32` → `safe fn (...) -> Result<_, SocketError>`
//! 2. 内部 unsafe 块带 SAFETY 注释, 委托给 smoltcp 协议栈
//! 3. `&[u8]` / `&mut [u8]` 切片替代 `*const u8` / `*mut u8` 裸指针
//! 4. Socket 类型 (`1=TCP`, `2=UDP`) 改为强类型枚举
//!
//! 评估日期: 2026-06-04

extern crate alloc;

use crate::kernel::net;

// ============================================================================
// 错误
// ============================================================================

/// Socket 操作错误 (POSIX errno → 强类型)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketError {
    /// 权限不足 (E_PERM=1)
    PermissionDenied,
    /// 文件描述符无效 (E_BADF=9)
    BadFd,
    /// 操作会阻塞 (E_AGAIN=11)
    WouldBlock,
    /// 内存不足 (E_NOMEM=12)
    NoMemory,
    /// 错误地址 (E_FAULT=14)
    Fault,
    /// 无效参数 (E_INVAL=22)
    InvalidArgument,
    /// 进程打开文件过多 (E_NFILE=23)
    ProcessFileLimit,
    /// 设备不存在 (E_NODEV=19)
    NoDevice,
    /// 操作不支持 (E_NOTSUPP=95)
    NotSupported,
    /// 地址族不支持 (E_AFNOSUPPORT=97)
    AddrFamilyNotSupported,
    /// 地址已被使用 (E_ADDRINUSE=98)
    AddrInUse,
    /// 地址不可用 (E_ADDRNOTAVAIL=99)
    AddrNotAvailable,
    /// 连接被重置 (E_CONNRESET=104)
    ConnectionReset,
    /// 未连接 (E_NOTCONN=107)
    NotConnected,
    /// 连接被拒绝 (E_CONNREFUSED=111)
    ConnectionRefused,
    /// 网络未初始化
    NotReady,
    /// 其他
    Other(i32),
}

impl SocketError {
    pub fn from_i32(rc: i32) -> Self {
        match rc {
            1 => Self::PermissionDenied,
            9 => Self::BadFd,
            11 => Self::WouldBlock,
            12 => Self::NoMemory,
            14 => Self::Fault,
            19 => Self::NoDevice,
            22 => Self::InvalidArgument,
            23 => Self::ProcessFileLimit,
            95 => Self::NotSupported,
            97 => Self::AddrFamilyNotSupported,
            98 => Self::AddrInUse,
            99 => Self::AddrNotAvailable,
            104 => Self::ConnectionReset,
            107 => Self::NotConnected,
            111 => Self::ConnectionRefused,
            _ => Self::Other(rc),
        }
    }
}

/// services 层结果类型
pub type SocketResult<T> = Result<T, SocketError>;

// ============================================================================
// 协议 / 类型
// ============================================================================

/// Socket 协议族
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Domain {
    /// IPv4 (`AF_INET = 2`)
    Inet = 2,
}

impl Domain {
    pub fn from_i32(d: i32) -> Option<Self> {
        match d {
            2 => Some(Self::Inet),
            _ => None,
        }
    }
}

/// Socket 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum SockType {
    /// TCP 流 (`SOCK_STREAM = 1`)
    Stream = 1,
    /// UDP 数据报 (`SOCK_DGRAM = 2`)
    Dgram = 2,
}

impl SockType {
    pub fn from_i32(t: i32) -> Option<Self> {
        match t {
            1 => Some(Self::Stream),
            2 => Some(Self::Dgram),
            _ => None,
        }
    }
}

/// IPv4 Socket 地址 (端口 + 4 字节 IP)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SockAddrIn {
    pub port: u16,
    pub ip: [u8; 4],
}

impl SockAddrIn {
    pub fn new(port: u16, ip: [u8; 4]) -> Self {
        Self { port, ip }
    }
}

// ============================================================================
// 字节序转换
// ============================================================================

/// IPv4 Socket 地址 → 8 字节 C 结构体 (`SockaddrIn`)
///
/// 布局 (Linux `struct sockaddr_in`):
/// ```text
/// offset 0:  sin_family (u16, BE) = 2 (AF_INET)
/// offset 2:  sin_port   (u16, BE)
/// offset 4:  sin_addr   ([u8; 4])
/// offset 8:  zero padding
/// ```
fn sockaddr_in_to_bytes(addr: &SockAddrIn) -> [u8; 8] {
    let mut buf = [0u8; 8];
    buf[0..2].copy_from_slice(&(2u16).to_be_bytes()); // AF_INET
    buf[2..4].copy_from_slice(&addr.port.to_be_bytes());
    buf[4..8].copy_from_slice(&addr.ip);
    buf
}

/// 8 字节 C 结构体 → IPv4 Socket 地址
fn bytes_to_sockaddr_in(buf: &[u8; 8]) -> Option<SockAddrIn> {
    if buf.len() < 8 {
        return None;
    }
    let family = u16::from_be_bytes([buf[0], buf[1]]);
    if family != 2 {
        return None;
    }
    let port = u16::from_be_bytes([buf[2], buf[3]]);
    let mut ip = [0u8; 4];
    ip.copy_from_slice(&buf[4..8]);
    Some(SockAddrIn { port, ip })
}

// ============================================================================
// Socket API
// ============================================================================

/// POSIX `socket(domain, type, protocol)`
///
/// 成功返回新 socket 的 FD, 失败返回 `SocketError`。
pub fn socket(domain: Domain, sock_type: SockType, _protocol: i32) -> SocketResult<i32> {
    // SAFETY: NET_LOCK 由 sm_socket 内部获取
    let fd = unsafe { net::init::sm_socket(domain as i32, sock_type as i32, 0) };
    if fd < 0 {
        Err(SocketError::from_i32(fd))
    } else {
        Ok(fd)
    }
}

/// POSIX `bind(fd, addr, addrlen)`
pub fn bind(fd: i32, addr: &SockAddrIn) -> SocketResult<()> {
    let bytes = sockaddr_in_to_bytes(addr);
    // SAFETY: bytes 在栈上, 8 字节有效, 由 sm_bind 同步读取
    let rc = unsafe { net::init::sm_bind(fd, bytes.as_ptr(), bytes.len() as u32) };
    if rc == 0 { Ok(()) } else { Err(SocketError::from_i32(rc)) }
}

/// POSIX `listen(fd, backlog)`
pub fn listen(fd: i32, backlog: i32) -> SocketResult<()> {
    // SAFETY: NET_LOCK 内部获取
    let rc = unsafe { net::init::sm_listen(fd, backlog) };
    if rc == 0 { Ok(()) } else { Err(SocketError::from_i32(rc)) }
}

/// POSIX `accept(fd, addr, addrlen)` — 返回新连接的 FD
pub fn accept(fd: i32) -> SocketResult<i32> {
    // SAFETY: NULL 地址, 不返回对端地址
    let new_fd = unsafe { net::init::sm_accept(fd, core::ptr::null_mut(), core::ptr::null_mut()) };
    if new_fd < 0 {
        Err(SocketError::from_i32(new_fd))
    } else {
        Ok(new_fd)
    }
}

/// POSIX `connect(fd, addr, addrlen)`
pub fn connect(fd: i32, addr: &SockAddrIn) -> SocketResult<()> {
    let bytes = sockaddr_in_to_bytes(addr);
    // SAFETY: bytes 栈上有效
    let rc = unsafe { net::init::sm_connect(fd, bytes.as_ptr(), bytes.len() as u32) };
    if rc == 0 { Ok(()) } else { Err(SocketError::from_i32(rc)) }
}

/// POSIX `send(fd, buf, len, flags)` → 实际发送字节数
pub fn send(fd: i32, buf: &[u8]) -> SocketResult<usize> {
    // SAFETY: buf 在调用期间有效, sm_send 同步读取
    let n = unsafe {
        net::init::sm_send(fd, buf.as_ptr(), buf.len() as u32, 0)
    };
    if n < 0 {
        Err(SocketError::from_i32(n))
    } else {
        Ok(n as usize)
    }
}

/// POSIX `recv(fd, buf, len, flags)` → 实际接收字节数
///
/// 写入 `out` 切片, 返回字节数; `WouldBlock` 表示无数据可读。
pub fn recv(fd: i32, out: &mut [u8]) -> SocketResult<usize> {
    // SAFETY: out 在调用期间有效可写
    let n = unsafe {
        net::init::sm_recv(fd, out.as_mut_ptr(), out.len() as u32, 0)
    };
    if n < 0 {
        Err(SocketError::from_i32(n))
    } else {
        Ok(n as usize)
    }
}

/// POSIX `sendto(fd, buf, len, flags, dest_addr, addrlen)`
pub fn sendto(fd: i32, buf: &[u8], dest: &SockAddrIn) -> SocketResult<usize> {
    let bytes = sockaddr_in_to_bytes(dest);
    // SAFETY: buf + bytes 同步有效
    let n = unsafe {
        net::init::sm_sendto(
            fd,
            buf.as_ptr(),
            buf.len() as u32,
            0,
            bytes.as_ptr(),
            bytes.len() as u32,
        )
    };
    if n < 0 {
        Err(SocketError::from_i32(n))
    } else {
        Ok(n as usize)
    }
}

/// POSIX `recvfrom(fd, buf, len, flags, src_addr, addrlen)` → (字节数, 源地址)
pub fn recvfrom(fd: i32, out: &mut [u8]) -> SocketResult<(usize, SockAddrIn)> {
    let mut src = [0u8; 8];
    // SAFETY: out 可写, src 8 字节栈缓冲
    let n = unsafe {
        net::init::sm_recvfrom(
            fd,
            out.as_mut_ptr(),
            out.len() as u32,
            0,
            src.as_mut_ptr(),
            &mut (src.len() as u32),
        )
    };
    if n < 0 {
        Err(SocketError::from_i32(n))
    } else {
        let addr = bytes_to_sockaddr_in(&src)
            .ok_or(SocketError::InvalidArgument)?;
        Ok((n as usize, addr))
    }
}

/// POSIX `close(fd)`
pub fn close(fd: i32) -> SocketResult<()> {
    // SAFETY: sm_close 内部 NET_LOCK 串行化
    let rc = unsafe { net::init::sm_close(fd) };
    if rc == 0 { Ok(()) } else { Err(SocketError::from_i32(rc)) }
}

/// POSIX `setsockopt(fd, level, optname, optval, optlen)`
///
/// # 参数
/// - `level`: 协议层 (e.g. `1` = SOL_SOCKET)
/// - `optname`: 选项名
/// - `val`: 选项值 (u32)
pub fn setsockopt(fd: i32, level: i32, optname: i32, val: u32) -> SocketResult<()> {
    let val_bytes = val.to_ne_bytes();
    // SAFETY: val_bytes 栈上有效
    let rc = unsafe {
        net::init::sm_setsockopt(
            fd,
            level,
            optname,
            val_bytes.as_ptr(),
            val_bytes.len() as u32,
        )
    };
    if rc == 0 { Ok(()) } else { Err(SocketError::from_i32(rc)) }
}

/// POSIX `getsockopt(fd, level, optname, optval, optlen)` → u32
pub fn getsockopt(fd: i32, level: i32, optname: i32) -> SocketResult<u32> {
    let mut out = 0u32;
    let mut out_len = core::mem::size_of::<u32>() as u32;
    // SAFETY: out 可写, 4 字节
    let rc = unsafe {
        net::init::sm_getsockopt(
            fd,
            level,
            optname,
            &mut out as *mut u32 as *mut u8,
            &mut out_len,
        )
    };
    if rc == 0 { Ok(out) } else { Err(SocketError::from_i32(rc)) }
}

/// 轮询所有 socket (驱动事件分发)
///
/// 由 timer ISR 或专用网络任务周期性调用。
pub fn poll_all() -> SocketResult<i32> {
    // SAFETY: try_lock 内部使用, ISR 安全
    let n = unsafe { net::init::sm_poll_sockets() };
    if n < 0 { Err(SocketError::from_i32(n)) } else { Ok(n) }
}

// ============================================================================
// 便利: 字符串 IP 解析
// ============================================================================

/// 解析 "a.b.c.d" 格式 IP 字符串
pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut parts = s.split('.');
    let mut out = [0u8; 4];
    for i in 0..4 {
        let p = parts.next()?;
        let v: u32 = p.parse().ok()?;
        if v > 255 {
            return None;
        }
        out[i] = v as u8;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

/// 构造 `SockAddrIn` from IP 字符串 + 端口
pub fn endpoint_from_str(ip: &str, port: u16) -> Option<SockAddrIn> {
    let bytes = parse_ipv4(ip)?;
    Some(SockAddrIn::new(port, bytes))
}
