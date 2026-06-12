#![deny(unsafe_code)]
//! 网络子系统 — services 层安全代理
//!
//! ## 状态 (v2.7, 2026-06-04)
//!
//! 已完成 1/4 子系统迁移 (net 顶层), 封装 `kernel::net::*` 老 API:
//! - [x] net (本文件) — init / poll / DHCP / 状态查询
//! - [ ] smoltcp — 协议栈内部 (smoltcp 自身大量 unsafe 在 vendored code, 不在 TCB 范围)
//! - [ ] e1000/virtio-net — 走 driver 子系统, 已通过 chitin 注册
//! - [ ] socket API — 后续 Phase 2.4.x
//!
//! ## 迁移方法
//!
//! 1. 把 `unsafe extern "C" fn qx_net_init` → `safe fn init()`
//! 2. 把 `unsafe fn poll_network` → `safe fn poll()` (内部仍走 unsafe, 但锁定语义保留)
//! 3. `Result<_, NetError>` 替代 `i32` 返回码
//!
//! 评估日期: 2026-06-04

use crate::kernel::framework::net_socket as fw_net_socket;
// I-预存 (kernel_test build): 同 net_socket.rs 处理 — 用 cfg-gate `use` 别名
// + 桩模块, 让函数体保持 `init::*` 调用, 不扩散 cfg 到 fn body.
#[cfg(not(feature = "kernel_test"))]
use crate::kernel::framework::net::init as init;

// kernel_test 桩: 对齐真实 `net::init` 暴露的类型与函数签名.
#[cfg(feature = "kernel_test")]
mod init {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum InitState {
        Uninitialized,
        HardwareProbed,
        InterfaceReady,
        FullyInitialized,
        Failed,
    }
    pub fn is_network_initialized() -> bool {
        false
    }
    pub fn is_network_configured() -> bool {
        false
    }
    pub fn get_init_state() -> InitState {
        InitState::Uninitialized
    }
}

pub mod netfilter;
pub mod route;
pub mod socket;
pub mod syscall;
pub mod unix;

pub use socket::{
    Domain, SockAddrIn, SockType, SocketError, SocketResult,
    socket, bind, listen, accept, connect,
    send, recv, sendto, recvfrom, close,
    setsockopt, getsockopt, poll_all,
    parse_ipv4, endpoint_from_str,
};

pub use unix::{
    SockAddrUn, UnixSocketError, UnixResult,
    socket as unix_socket, bind as unix_bind, listen as unix_listen,
    accept as unix_accept, connect as unix_connect, send as unix_send, recv as unix_recv,
    sendto as unix_sendto, recvfrom as unix_recvfrom,
    close as unix_close, unlink as unix_unlink,
    is_uds_fd, FD_BASE as UNIX_FD_BASE, PATH_MAX as UNIX_PATH_MAX,
};

// ============================================================================
// 错误
// ============================================================================

/// 网络操作错误 — TD-20: 收敛到 KernelError, 1 字段 net 特有 + 1 共享包装.
///
/// 字段说明:
///   - `NotConfigured`: DHCP 未配置 (网络层语义, 不在 POSIX 通用集)
///   - `Kernel(KernelError)`: 共享错误 (NotReady/InvalidArgument/NotFound/Io
///     等) 全部走单一来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    /// DHCP 未配置
    NotConfigured,
    /// 共享 `KernelError` 包装
    Kernel(crate::kernel::services::error::KernelError),
}

impl NetError {
    /// 映射为 POSIX errno
    pub fn to_errno(self) -> Errno {
        use Errno as E;
        match self {
            Self::NotConfigured => E::ENETDOWN,
            Self::Kernel(e) => e.as_errno(),
        }
    }

    pub fn from_i32(rc: i32) -> Self {
        use crate::kernel::services::error::KernelError as K;
        match rc {
            -1 => Self::Kernel(K::NotReady),
            -2 => Self::Kernel(K::NoSuchProcess),
            -5 => Self::Kernel(K::Fault),
            -19 => Self::Kernel(K::NoDevice),
            -22 => Self::Kernel(K::InvalidArgument),
            -101 => Self::NotConfigured,
            _ => Self::Kernel(K::Other(rc)),
        }
    }
}

/// services 层结果类型别名
pub type NetResult<T> = Result<T, NetError>;

use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 状态
// ============================================================================

/// 网络初始化状态 (与 kernel::net::init::InitState 对齐)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InitState {
    Uninitialized = 0,
    HardwareProbed = 1,
    InterfaceReady = 2,
    FullyInitialized = 3,
    Failed = 255,
}

impl From<init::InitState> for InitState {
    fn from(s: init::InitState) -> Self {
        match s {
            init::InitState::Uninitialized => Self::Uninitialized,
            init::InitState::HardwareProbed => Self::HardwareProbed,
            init::InitState::InterfaceReady => Self::InterfaceReady,
            init::InitState::FullyInitialized => Self::FullyInitialized,
            init::InitState::Failed => Self::Failed,
        }
    }
}

// ============================================================================
// 顶层 API
// ============================================================================

/// 初始化网络子系统
///
/// 探测网卡 (e1000 / virtio-net), 启动协议栈, 启动 DHCP 异步获取 IP。
/// 如果无 NIC 则进入 "NoNetwork" 状态。
pub fn init() {
    fw_net_socket::qx_net_init();
}

/// 轮询网络栈 (驱动 TX/RX、定时器、DHCP)
///
/// 在 timer ISR 或网络任务中调用, 内部 `try_lock` 避免阻塞。
/// 若网络锁已被持有则直接返回, 不会等待。
pub fn poll() {
    fw_net_socket::poll_network();
}

/// 启动 DHCP (异步, 由 timer ISR 驱动 poll 完成)
///
/// 调用后 DHCP Discover 会在下一个 timer tick 发出。
/// 用户态通过 `is_configured()` 轮询等待完成。
pub fn start_dhcp() -> NetResult<()> {
    let rc = fw_net_socket::qx_net_start_dhcp();
    if rc == 0 { Ok(()) } else { Err(NetError::from_i32(rc)) }
}

/// 设置静态 IP (格式: "10.0.2.15/24,10.0.2.2")
///
/// # 参数
/// - `cidr_str`: CIDR 字符串, 如 "192.168.1.100/24"
/// - `gw_str`: 网关地址字符串, 如 "192.168.1.1"
///
/// # 返回
/// 成功返回 `Ok(())`, 失败返回 `NetError`
pub fn static_ip(cidr_str: &str, gw_str: &str) -> NetResult<()> {
    // 复制到 C 字符串 (添加 NUL 终止符)
    let mut cidr_c = alloc::vec::Vec::with_capacity(cidr_str.len() + 1);
    cidr_c.extend_from_slice(cidr_str.as_bytes());
    cidr_c.push(0);

    let mut gw_c = alloc::vec::Vec::with_capacity(gw_str.len() + 1);
    gw_c.extend_from_slice(gw_str.as_bytes());
    gw_c.push(0);

    let rc = fw_net_socket::qx_net_static_ip(cidr_c.as_ptr(), gw_c.as_ptr());
    if rc == 0 { Ok(()) } else { Err(NetError::from_i32(rc)) }
}

// ============================================================================
// 状态查询
// ============================================================================

/// 网络是否已完全初始化
pub fn is_initialized() -> bool {
    init::is_network_initialized()
}

/// 网络是否已完成 DHCP/Static IP 配置
pub fn is_configured() -> bool {
    init::is_network_configured()
}

/// 当前初始化状态
pub fn state() -> InitState {
    InitState::from(init::get_init_state())
}

/// 重置网络状态 (恢复机制)
pub fn reset_state() {
    fw_net_socket::reset_network_state();
}
