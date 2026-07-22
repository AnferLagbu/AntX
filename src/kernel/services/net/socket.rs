#![deny(unsafe_code)]
//! Socket 子系统 — services 层安全代理
//!
//! ## 状态 (v2.8, 2026-06-04)
//!
//! Phase 2.4 net 2/4 子系统迁移: 封装 `kernel::net::init::sm_*` 12 个 FFI:
//! - [x] socket/bind/listen/accept/connect (TCP 客户端/服务器)
//! - [x] send/recv (TCP 字节流)
//! - [x] sendto/recvfrom (UDP 数据报)
//! - [x] 关闭/设置选项/获取选项 (close/setsockopt/getsockopt)
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

use super::net_stack;
use crate::kernel::framework::net::iface_trait::{Ipv4Addr, NetEndpoint};

// ============================================================================
// 错误
// ============================================================================

/// Socket 错误 (TD-08: 改为统一 `KernelError` 的 type alias, 单一来源).
///
/// 历史: `SocketError` 自带 17 个字段 (与 `UnixSocketError` 高度重叠).
/// 现在所有共享错误统一在 `services::error::KernelError`, `SocketError` 仅保留别名.
/// 子系统特有错误 (无 — INET socket 全部错误都在 `KernelError`) 用 0 字段表达.
pub use crate::kernel::services::error::KernelError as SocketError;

/// services 层结果类型
pub type SocketResult<T> = Result<T, SocketError>;

// ============================================================================
// 协议 / 类型
// ============================================================================

/// Socket 协议族
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Domain {
    /// Unix Domain (`AF_UNIX = 1`) — Phase C.3 新增
    Unix = 1,
    /// IPv4 (`AF_INET = 2`)
    Inet = 2,
}

impl Domain {
    pub fn from_i32(d: i32) -> Option<Self> {
        match d {
            1 => Some(Self::Unix),
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
// Socket API
// ============================================================================

/// POSIX `socket(domain, type, protocol)`
///
/// 成功返回新 socket 的 FD, 失败返回 `SocketError`。
pub fn socket(domain: Domain, sock_type: SockType, _protocol: i32) -> SocketResult<i32> {
    let s = net_stack().lock();
    s.socket_create_fd(domain as i32, sock_type as i32)
        .map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `bind(fd, addr, addrlen)`
pub fn bind(fd: i32, addr: &SockAddrIn) -> SocketResult<()> {
    let ep = NetEndpoint::new(Ipv4Addr::from_octets(addr.ip), addr.port);
    let s = net_stack().lock();
    s.bind_fd(fd, ep).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `listen(fd, backlog)`
pub fn listen(fd: i32, backlog: i32) -> SocketResult<()> {
    let s = net_stack().lock();
    s.listen_fd(fd, backlog).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `accept(fd, addr, addrlen)` — 返回新连接的 FD
pub fn accept(fd: i32) -> SocketResult<i32> {
    let s = net_stack().lock();
    s.accept_fd(fd).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `connect(fd, addr, addrlen)`
pub fn connect(fd: i32, addr: &SockAddrIn) -> SocketResult<()> {
    let ep = NetEndpoint::new(Ipv4Addr::from_octets(addr.ip), addr.port);
    let s = net_stack().lock();
    s.connect_fd(fd, ep).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `send(fd, buf, len, flags)` → 实际发送字节数
pub fn send(fd: i32, buf: &[u8]) -> SocketResult<usize> {
    let s = net_stack().lock();
    s.send_fd(fd, buf).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `recv(fd, buf, len, flags)` → 实际接收字节数
///
/// 写入 `out` 切片, 返回字节数; `WouldBlock` 表示无数据可读。
pub fn recv(fd: i32, out: &mut [u8]) -> SocketResult<usize> {
    let s = net_stack().lock();
    s.recv_fd(fd, out).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `sendto(fd, buf, len, flags, dest_addr, addrlen)`
pub fn sendto(fd: i32, buf: &[u8], dest: &SockAddrIn) -> SocketResult<usize> {
    let ep = NetEndpoint::new(Ipv4Addr::from_octets(dest.ip), dest.port);
    let s = net_stack().lock();
    s.sendto_fd(fd, buf, ep).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `recvfrom(fd, buf, len, flags, src_addr, addrlen)` → (字节数, 源地址)
pub fn recvfrom(fd: i32, out: &mut [u8]) -> SocketResult<(usize, SockAddrIn)> {
    let s = net_stack().lock();
    let (n, ep) = s.recvfrom_fd(fd, out).map_err(|_| SocketError::InvalidArgument)?;
    let addr = SockAddrIn::new(ep.port, ep.addr.octets());
    Ok((n, addr))
}

/// POSIX `close(fd)`
pub fn close(fd: i32) -> SocketResult<()> {
    let s = net_stack().lock();
    s.close_fd(fd).map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `setsockopt(fd, level, optname, optval, optlen)`
///
/// # 参数
/// - `level`: 协议层 (e.g. `1` = SOL_SOCKET)
/// - `optname`: 选项名
/// - `val`: 选项值 (u32)
pub fn setsockopt(fd: i32, level: i32, optname: i32, val: u32) -> SocketResult<()> {
    let val_bytes = val.to_ne_bytes();
    let s = net_stack().lock();
    s.setsockopt_fd(fd, level, optname, &val_bytes)
        .map_err(|_| SocketError::InvalidArgument)
}

/// POSIX `getsockopt(fd, level, optname, optval, optlen)` → u32
pub fn getsockopt(fd: i32, level: i32, optname: i32) -> SocketResult<u32> {
    let mut buf = [0u8; 4];
    let s = net_stack().lock();
    s.getsockopt_fd(fd, level, optname, &mut buf)
        .map_err(|_| SocketError::InvalidArgument)?;
    Ok(u32::from_ne_bytes(buf))
}

/// 轮询所有 socket (驱动事件分发)
///
/// 由 timer ISR 或专用网络任务周期性调用。
pub fn poll_all() -> SocketResult<i32> {
    let s = net_stack().lock();
    s.poll_all_fd()
        .map_err(|_| SocketError::InvalidArgument)?;
    Ok(0)
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
