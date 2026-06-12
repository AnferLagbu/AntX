#![deny(unsafe_code)]
//! Unix Domain Socket (AF_UNIX) — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 强类型 API
//! - 委托 `framework::net::unix` 完成所有底层操作
//! - 错误统一映射为 [`UnixSocketError`], 与 [`super::socket::SocketError`] 平级
//!
//! ## 范围 (Phase C.3 v1)
//!
//! - [x] SOCK_STREAM: bind/listen/accept/connect/send/recv/close
//! - [x] SOCK_DGRAM: bind/connect/sendto/recvfrom/close
//! - [x] 路径绑定 (独立路径表, 不进 VFS inode)
//! - [ ] 阻塞语义 (v1 退化为 `WouldBlock`)
//! - [ ] SCM_RIGHTS / SCM_CRED (v2)
//!
//! ## 评估日期
//!
//! 2026-06-08

use crate::kernel::framework::net::unix as fw;

// ============================================================================
// 重新导出 TCB 常量
// ============================================================================

/// UDS FD 起点
pub const FD_BASE: i32 = fw::UDS_FD_BASE;
/// UDS FD 上限 (不含)
pub const FD_END: i32 = fw::UDS_FD_BASE + fw::MAX_UDS_FD as i32;
/// 路径最大长度
pub const PATH_MAX: usize = fw::UNIX_PATH_MAX;

// ============================================================================
// 类型
// ============================================================================

/// UDS socket 类型 (STREAM=1, DGRAM=2 与 AF_INET 共用)
pub use fw::UnixSockType as SockType;

/// UDS 错误 (TD-08: 共享字段全部下沉到 `services::error::KernelError`,
/// 本枚举仅保留子系统特有字段 + 包装. 字段数 = 2, 满足验收 "≤2").
///
/// 字段说明:
///   - `PathNotFound`: UDS 特有 (路径查找失败, `ENOENT`). `KernelError` 无此语义.
///   - `Kernel(KernelError)`: 其余共享错误统一走 `KernelError` 单一来源.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnixSocketError {
    /// UDS 特有: 路径未找到 (`ENOENT`)
    PathNotFound,
    /// 共享错误统一包装
    Kernel(crate::kernel::services::error::KernelError),
}

impl UnixSocketError {
    /// 映射为 POSIX errno
    pub fn to_errno(self) -> crate::kernel::framework::syscall::types::Errno {
        use crate::kernel::framework::syscall::types::Errno as E;
        match self {
            Self::PathNotFound => E::ENOENT,
            Self::Kernel(e) => e.as_errno(),
        }
    }
}

impl From<fw::UdsError> for UnixSocketError {
    fn from(e: fw::UdsError) -> Self {
        use crate::kernel::services::error::KernelError as K;
        match e {
            fw::UdsError::NotFound => Self::PathNotFound,
            fw::UdsError::BadFd => Self::Kernel(K::BadFd),
            fw::UdsError::Again => Self::Kernel(K::WouldBlock),
            fw::UdsError::NoMem => Self::Kernel(K::NoMemory),
            fw::UdsError::AddrFamily => Self::Kernel(K::AddrFamilyNotSupported),
            fw::UdsError::AddrInUse => Self::Kernel(K::AddrInUse),
            fw::UdsError::ConnRefused => Self::Kernel(K::ConnectionRefused),
            fw::UdsError::Invalid => Self::Kernel(K::InvalidArgument),
            fw::UdsError::NoSys => Self::Kernel(K::NotSupported),
        }
    }
}

impl From<crate::kernel::services::error::KernelError> for UnixSocketError {
    fn from(e: crate::kernel::services::error::KernelError) -> Self {
        Self::Kernel(e)
    }
}

pub type UnixResult<T> = Result<T, UnixSocketError>;

// ============================================================================
// SockAddrUn — 路径地址
// ============================================================================

/// `struct sockaddr_un` 包装: 路径字节 + 长度
#[derive(Debug, Clone, Copy)]
pub struct SockAddrUn {
    pub path: [u8; PATH_MAX],
    pub path_len: u16,
}

impl SockAddrUn {
    /// 从路径构造; 路径空或超过 PATH_MAX 返回 None
    pub fn new(path: &[u8]) -> Option<Self> {
        if path.is_empty() || path.len() > PATH_MAX {
            return None;
        }
        let mut p = [0u8; PATH_MAX];
        p[..path.len()].copy_from_slice(path);
        Some(Self {
            path: p,
            path_len: path.len() as u16,
        })
    }

    /// 路径切片 (去除尾部 NUL 填充)
    pub fn path_slice(&self) -> &[u8] {
        &self.path[..self.path_len as usize]
    }
}

// ============================================================================
// Socket API (safe 包装)
// ============================================================================

/// 创建 UDS socket, 返回 FD (在 [FD_BASE, FD_END) 区间)
pub fn socket(sock_type: SockType) -> UnixResult<i32> {
    fw::uds_create(sock_type).map_err(Into::into)
}

/// bind
pub fn bind(fd: i32, addr: &SockAddrUn) -> UnixResult<()> {
    fw::uds_bind(fd, addr.path_slice()).map_err(Into::into)
}

/// listen — 仅 STREAM
pub fn listen(fd: i32) -> UnixResult<()> {
    fw::uds_listen(fd).map_err(Into::into)
}

/// accept — 仅 STREAM; 无 pending 时返回 `WouldBlock`
pub fn accept(fd: i32) -> UnixResult<i32> {
    fw::uds_accept(fd).map_err(Into::into)
}

/// connect — STREAM/DGRAM 通用
pub fn connect(fd: i32, addr: &SockAddrUn) -> UnixResult<()> {
    fw::uds_connect(fd, addr.path_slice()).map_err(Into::into)
}

/// STREAM send
pub fn send(fd: i32, data: &[u8]) -> UnixResult<usize> {
    fw::uds_send(fd, data).map_err(Into::into)
}

/// STREAM recv
pub fn recv(fd: i32, out: &mut [u8]) -> UnixResult<usize> {
    fw::uds_recv(fd, out).map_err(Into::into)
}

/// DGRAM sendto
pub fn sendto(fd: i32, data: &[u8], dest: &SockAddrUn) -> UnixResult<usize> {
    fw::uds_sendto(fd, data, dest.path_slice()).map_err(Into::into)
}

/// DGRAM recvfrom
pub fn recvfrom(fd: i32, out: &mut [u8]) -> UnixResult<usize> {
    fw::uds_recvfrom(fd, out).map_err(Into::into)
}

/// close
pub fn close(fd: i32) -> UnixResult<()> {
    fw::uds_close(fd).map_err(Into::into)
}

/// unlink — 显式解除路径绑定
pub fn unlink(addr: &SockAddrUn) -> UnixResult<()> {
    fw::uds_unlink(addr.path_slice()).map_err(Into::into)
}

/// FD 是否属于 UDS 范围
#[inline]
pub fn is_uds_fd(fd: i32) -> bool {
    (FD_BASE..FD_END).contains(&fd)
}
