//! 网络子系统 API 层
//!
//! 基于 smoltcp 协议栈的网络初始化和 Socket 操作入口。
//!
//! ## 调用方契约
//! - `syscall::mod` —— sys_socket/sys_connect/sys_accept/sys_sendto/sys_recvfrom
//! - `proc::api` —— 进程创建/销毁时关联 socket fd
//! - `chitin::proto_net` —— 网卡设备注册/注销
//! - `barrier::recovery` —— 网络子系统纳入恢复域
//!
//! ## 内部接口
//! - `init.rs` —— 初始化状态机, DHCP, Socket API (poll_network/tcp_*/udp_*)
//! - `smoltcp_impl.rs` —— ChitinNetDevice + NetworkStack + Device trait 实现
//! - `types.rs` —— 公共状态 (NET_READY / NET_CONFIGURED)
//!
//! ## 安全约束
//! - 所有 static mut 变量在 NET_LOCK: Mutex<()> 保护下访问
//! - poll_network() 使用 try_lock() 避免 ISR 上下文阻塞
//! - Socket 操作在 #[cfg(feature = "net")] 控制下, kernel_test 模式不链接
//!
//! ## 性能特征
//! - poll_network(): 单次轮询, 无阻塞; ISR 安全
//! - Socket 创建: O(1) 数组扫描 (MAX_SOCKETS = 8)
//! - DHCP: 异步状态机, 不阻塞内核主循环

// ============================================================================
// 契约 trait: NetworkDevice — 所有网卡驱动必须实现
// ============================================================================

/// 网卡设备抽象。
///
/// chitin 注册网络设备时,驱动必须提供此 trait 的实现。
pub trait NetworkDevice: Send + Sync {
    /// 设备名称 (MAC 地址格式)
    fn name(&self) -> &'static str;

    /// 发送以太网帧
    fn transmit(&self, buf: &[u8]) -> Result<(), ()>;

    /// 轮询接收 (非阻塞, ISR 安全)
    fn poll(&self);
}

// ============================================================================
// 契约: 初始化
// ============================================================================

#[cfg(not(feature = "kernel_test"))]
use super::init::InitState;

/// 轮询网络设备 (由内核主循环或定时器调用)
///
/// # 安全约束
/// - ISR 安全: 使用 try_lock() 避免阻塞
/// - kernel_test 模式下不可用 (无真实硬件)
#[cfg(not(feature = "kernel_test"))]
pub fn poll_network() {
    unsafe { super::init::poll_network() }
}

/// 查询网络是否已完全初始化
#[cfg(not(feature = "kernel_test"))]
pub fn is_network_initialized() -> bool {
    super::init::is_network_initialized()
}

/// 查询网络是否已完成 DHCP 配置
#[cfg(not(feature = "kernel_test"))]
pub fn is_network_configured() -> bool {
    super::init::is_network_configured()
}

/// 查询当前初始化状态
#[cfg(not(feature = "kernel_test"))]
pub fn get_init_state() -> InitState {
    super::init::get_init_state()
}
