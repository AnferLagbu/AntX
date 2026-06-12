// SPDX-License-Identifier: Apache-2.0
// TD-08: services 层统一错误类型 `KernelError` (Single Source of Truth).
//
// 验收:
//   - 字段数为子集枚举 (不变量: 跨服务共享字段 = 1 份)
//   - `services::net::socket::SocketError` 改为 `pub type SocketError = KernelError;` 零包装
//   - `services::net::unix::UnixSocketError` 仅保留子系统特有字段 (PathNotFound) + `Kernel(KernelError)` 包装
//   - `From<fw::UdsError>` / `From<i32>` / `to_errno` 单一来源

use crate::kernel::framework::syscall::types::Errno;

/// services 层统一错误 (POSIX errno → 强类型).
///
/// 跨子系统共享: 任何 `Result<T, KernelError>` 都可直接 `?` 传播.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    /// 权限不足 (EPERM=1)
    PermissionDenied,
    /// 资源/进程不存在 (ESRCH=3)
    NoSuchProcess,
    /// 文件描述符无效 (EBADF=9)
    BadFd,
    /// 操作会阻塞 (EAGAIN=11)
    WouldBlock,
    /// 内存不足 (ENOMEM=12)
    NoMemory,
    /// 错误地址 (EFAULT=14)
    Fault,
    /// 设备不存在 (ENODEV=19)
    NoDevice,
    /// 无效参数 (EINVAL=22)
    InvalidArgument,
    /// 进程打开文件过多 (EMFILE=23)
    ProcessFileLimit,
    /// 操作不支持 (ENOSYS=95)
    NotSupported,
    /// 地址族不支持 (EAFNOSUPPORT=97)
    AddrFamilyNotSupported,
    /// 地址已被使用 (EADDRINUSE=98)
    AddrInUse,
    /// 地址不可用 (EADDRNOTAVAIL=99)
    AddrNotAvailable,
    /// 连接被重置 (ECONNRESET=104)
    ConnectionReset,
    /// 未连接 (ENOTCONN=107)
    NotConnected,
    /// 连接被拒绝 (ECONNREFUSED=111)
    ConnectionRefused,
    /// 网络未初始化 (自定义, 非 POSIX)
    NotReady,
    /// 其他未分类
    Other(i32),
}

impl KernelError {
    /// POSIX errno 强类型映射.
    pub const fn from_i32(rc: i32) -> Self {
        match rc {
            1 => Self::PermissionDenied,
            3 => Self::NoSuchProcess,
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

    /// 反向映射: 强类型 → POSIX errno.
    pub const fn as_errno(self) -> Errno {
        match self {
            Self::PermissionDenied => Errno::EPERM,
            Self::NoSuchProcess => Errno::ESRCH,
            Self::BadFd => Errno::EBADF,
            Self::WouldBlock => Errno::EAGAIN,
            Self::NoMemory => Errno::ENOMEM,
            Self::Fault => Errno::EFAULT,
            Self::NoDevice => Errno::ENODEV,
            Self::InvalidArgument => Errno::EINVAL,
            Self::ProcessFileLimit => Errno::EMFILE,
            Self::NotSupported => Errno::ENOSYS,
            Self::AddrFamilyNotSupported => Errno::EAFNOSUPPORT,
            Self::AddrInUse => Errno::EADDRINUSE,
            Self::AddrNotAvailable => Errno::EADDRNOTAVAIL,
            Self::ConnectionReset => Errno::ECONNRESET,
            Self::NotConnected => Errno::ENOTCONN,
            Self::ConnectionRefused => Errno::ECONNREFUSED,
            Self::NotReady => Errno::EAGAIN,
            Self::Other(_) => Errno::EINVAL,
        }
    }
}

impl From<i32> for KernelError {
    fn from(rc: i32) -> Self {
        Self::from_i32(rc)
    }
}

impl From<KernelError> for i32 {
    fn from(e: KernelError) -> Self {
        e.as_errno().as_i32()
    }
}

impl From<Errno> for KernelError {
    fn from(e: Errno) -> Self {
        Self::from_i32(e.as_i32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_common_errnos() {
        for raw in [1, 9, 11, 12, 14, 22, 95, 97, 98, 99, 104, 107, 111] {
            let e = KernelError::from_i32(raw);
            let back: i32 = e.into();
            assert_eq!(back, raw, "round-trip failed for {raw}");
        }
    }

    #[test]
    fn other_preserves_raw() {
        let e = KernelError::from_i32(12345);
        assert_eq!(e, KernelError::Other(12345));
        let raw: i32 = e.into();
        assert_eq!(raw, 22 /*EINVAL fallback*/);
    }
}
