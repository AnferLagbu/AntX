//! 网络子系统 API 层
//!
//! 基于 smoltcp 协议栈的网络初始化和 Socket 操作入口。
//!
//! ## 调用方契约
//! - `syscall::mod` —— `sys_socket/sys_connect/sys_accept/sys_sendto/sys_recvfrom`
//!   (全部需要 `#[cfg(feature = "net")]`)
//! - `proc::api` —— 进程创建/销毁时关联 socket fd
//! - `chitin::proto_net` —— 网卡设备注册/注销
//! - `barrier::recovery` —— 网络子系统纳入恢复域
//!
//! ## 内部接口
//! - `init.rs` —— 初始化状态机, DHCP, Socket API (`poll_network`/`tcp_*`/`udp_*`)
//! - `smoltcp_impl.rs` —— `ChitinNetDevice` + `NetworkStack` + Device trait 实现
//! - `types.rs` —— 公共状态 (`NET_READY` / `NET_CONFIGURED`)
//!
//! ## 安全约束
//! - 所有 `static mut` 变量在 `NET_LOCK: Mutex<()>` 保护下访问
//! - `poll_network()` 使用 `try_lock()` 避免 ISR 上下文阻塞
//! - Socket 操作在 `#[cfg(feature = "net")]` 控制下, kernel_test 模式不链接
//!
//! ## 性能特征
//! - `poll_network()`: 单次轮询, 无阻塞; ISR 安全
//! - Socket 创建: O(1) 数组扫描 (MAX_SOCKETS = 8)
//! - DHCP: 异步状态机, 不阻塞内核主循环

pub use super::types::*;

#[cfg(not(feature = "kernel_test"))]
pub use super::init::{
    poll_network,
    InitState,
    is_network_initialized,
    is_network_configured,
};

#[cfg(not(feature = "kernel_test"))]
pub use super::smoltcp_impl::{ChitinNetDevice, NetworkStack};
