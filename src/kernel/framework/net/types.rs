use core::sync::atomic::AtomicBool;

/// 网络子系统公共状态 (smoltcp 状态机共享)
///
/// - `NET_READY`     : 协议栈已就绪 (qx_net_init 完成, 可收发原始帧)
/// - `NET_CONFIGURED`: 已配置 IP (DHCP 完成或静态 IP 已设置)
pub static NET_READY: AtomicBool = AtomicBool::new(false);

/// 网络已配置 IP (DHCP 完成或静态 IP 设置)
pub static NET_CONFIGURED: AtomicBool = AtomicBool::new(false);
