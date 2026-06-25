// SPDX-License-Identifier: GPL-2.0
//! 网络协议栈抽象 — Framekernel Safe API
//!
//! ## 定位
//!
//! 本文件定义 `NetStack` trait 作为 **framekernel 网络协议栈的安全抽象层**,
//! 隔离 smoltcp 第三方类型 (Interface / SocketHandle / SocketSet) 与 services
//! 层业务逻辑. 完整设计见 [docs/plan/smoltcp-framekernel-wrapper.md].
//!
//! ## 核心原则 (ASTD 四准则, framekernel-nature.md §2)
//!
//! - **Soundness**: 0 unsafe, safe API 不触发 UB
//! - **Expressiveness**: 6 个方法覆盖 smoltcp 全部核心能力
//! - **Minimalism**: 仅保留 trait + 数据类型, 无具体实现
//! - **Efficiency**: 全部 `#[inline]`, 静态分发 0 开销 (LLVM 单态化)
//!
//! ## 不变式 (framekernel-nature.md §1)
//!
//! - 1. 本文件 0 unsafe, 编译期强制
//! - 2. 不导入 smoltcp 任何类型, CI `audit_fk_trait.py` 校验
//! - 3. services 仅依赖本 trait, 不接触 smoltcp 内部
//!
//! ## 三层架构中的位置
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ Layer 1: framework/net/iface_trait.rs    │  ← 本文件
//! │   - NetStack trait 抽象                  │     0 unsafe, 0 smoltcp
//! │   - 类型擦除句柄 SocketHandle            │     framekernel safe API
//! ├──────────────────────────────────────────┤
//! │ Layer 2: services/net/smoltcp_impl.rs    │  (W3 子任务)
//! │   - impl NetStack for SmoltcpNetStack    │     唯一接触 smoltcp 的
//! │   - 类型翻译层                          │     services 文件
//! ├──────────────────────────────────────────┤
//! │ Layer 3: services/net/smoltcp/           │  (W2 子任务)
//! │   - smoltcp 0.13.0 vendored              │     整体迁移 + 只读
//! └──────────────────────────────────────────┘
//! ```
//!
//! ## 子任务归属
//!
//! REVAL-W 第 5 组 (W1), 2026-06-24 实装.

#![allow(dead_code)] // W3 实装前方法未被调用, 属正常预留

use core::fmt;

// ============================================================================
// 类型擦除句柄 (替代 smoltcp::socket::SocketHandle<usize>)
// ============================================================================

/// 网络协议栈的 Socket 句柄 (类型擦除, 不暴露 smoltcp 内部 `usize` 索引).
///
/// ## 设计动机
///
/// smoltcp 的 `SocketHandle(usize)` 直接暴露数组索引语义, 任何 `usize` 值
/// 都可能被误认作合法句柄. 此外 `as_u32_handle` (W5 移除) 中曾存在的
/// `transmute<usize, SocketHandle>` 是 UB 风险 (REVAL-4 历史包袱),
/// 已被替换为 `core::mem::transmute_copy`.
///
/// 本类型采用**新类型 (newtype) 模式**, 内部 `u32` 索引被外部不可见,
/// 句柄只能通过 `NetStack::socket_open()` 获取, 无 unsafe 构造路径.
///
/// ## 句柄生命周期
///
/// - 0 是保留值 (`INVALID`), 不可用作合法句柄
/// - 句柄由 `socket_open()` 分配, 由 `socket_close()` 释放
/// - 句柄是进程/网络栈局部 (per-NetworkStack), 不可跨栈传递
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SocketHandle(pub(crate) u32);

impl SocketHandle {
    /// 无效句柄 (句柄 0 永不分配, 用作哨兵).
    pub const INVALID: Self = Self(0);

    /// 是否为无效句柄.
    #[inline(always)]
    pub const fn is_invalid(self) -> bool {
        self.0 == 0
    }

    /// 是否为有效句柄.
    #[inline(always)]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    /// 内部 u32 索引 (services 内部使用, 外部不可见).
    #[inline(always)]
    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    /// 由 services 内部构造 (来自 smoltcp 句柄的 u32 转换).
    #[inline(always)]
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl fmt::Debug for SocketHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_invalid() {
            f.write_str("SocketHandle(INVALID)")
        } else {
            write!(f, "SocketHandle({})", self.0)
        }
    }
}

impl Default for SocketHandle {
    fn default() -> Self {
        Self::INVALID
    }
}

// ============================================================================
// Socket 类型枚举 (替代 smoltcp::socket::SocketSet<'a> 动态分发)
// ============================================================================

/// Socket 类型枚举 — 编译期已知, 避免 smoltcp 的运行时 `SocketSet` 容器.
///
/// ## 设计动机
///
/// smoltcp 的 `SocketSet<'a>` 是一个 **运行时类型擦除容器**, 内部用
/// `Box<dyn AnySocket>` 形式存储, 调用时需 downcast. 这与 framekernel
/// "类型安全" 不变式相悖.
///
/// 本枚举在 **API 边界** 把 socket 类型显式化, 实现层 (W3 `smoltcp_impl.rs`)
/// 负责把枚举映射到 smoltcp 的具体 socket 类型 (TcpSocket / UdpSocket / ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SocketKind {
    /// TCP (面向连接, 流式)
    Tcp,
    /// UDP (无连接, 数据报)
    Udp,
    /// ICMP (主要用于 ping)
    Icmp,
    /// RAW (raw ethernet/IP 访问, 高级)
    Raw,
    /// DHCPv4 客户端 (内部使用, 用户态不可见)
    Dhcpv4,
    /// DNS 客户端 (内部使用)
    Dns,
}

impl SocketKind {
    /// 是否是内部使用类型 (DHCP/DNS, 用户态不可见).
    #[inline(always)]
    pub const fn is_internal(self) -> bool {
        matches!(self, Self::Dhcpv4 | Self::Dns)
    }
}

// ============================================================================
// 网络配置 (替代 smoltcp::iface::Config 直接构造)
// ============================================================================

/// 网络协议栈配置 — 启动时一次性传入, 之后只读.
///
/// ## 设计动机
///
/// smoltcp 的 `Interface::new(config, ...)` 直接接受 `Config` 结构体, 包含
/// 大量易混淆的时序/缓冲区字段 (random_seed / hardware_addr / ...). 这些
/// 字段属于"配置策略", 应由 services 决定, 但 `Config` 类型本身应在
/// framework safe API 边界被封装.
#[derive(Clone, Copy, Debug)]
pub struct NetConfig {
    /// MAC 地址 (6 字节)
    pub mac_address: [u8; 6],
    /// 静态 IPv4 地址 (None = 走 DHCP)
    pub static_ipv4: Option<[u8; 4]>,
    /// 子网前缀长度 (1-32, 仅 static_ipv4 生效)
    pub prefix_len: u8,
    /// 默认网关 (仅 static_ipv4 生效)
    pub gateway: [u8; 4],
    /// 随机种子 (用于协议栈 PRNG, 0 = 自动)
    pub random_seed: u64,
}

impl NetConfig {
    /// 创建一个空配置 (全 0 / None), 由调用方填充.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            mac_address: [0; 6],
            static_ipv4: None,
            prefix_len: 24,
            gateway: [0; 4],
            random_seed: 0,
        }
    }

    /// 是否使用 DHCP 获取 IP (static_ipv4 为 None).
    #[inline(always)]
    pub const fn use_dhcp(&self) -> bool {
        self.static_ipv4.is_none()
    }
}

// ============================================================================
// Poll 轮询结果 (替代 smoltcp::iface::PollResult)
// ============================================================================

/// 网络协议栈一次轮询的结果.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollOutcome {
    /// 是否收到了新数据包
    pub packet_received: bool,
    /// 是否有 socket 状态变化 (可读/可写/可接受)
    pub socket_woken: bool,
    /// DHCP 状态是否变化
    pub dhcp_progressed: bool,
    /// 待发送的数据包数
    pub tx_pending: u32,
}

impl PollOutcome {
    /// 构造一个空结果 (无事件).
    #[inline(always)]
    pub const fn idle() -> Self {
        Self {
            packet_received: false,
            socket_woken: false,
            dhcp_progressed: false,
            tx_pending: 0,
        }
    }

    /// 是否有任何事件 (用于调度器快速判断).
    #[inline(always)]
    pub const fn has_events(&self) -> bool {
        self.packet_received || self.socket_woken || self.dhcp_progressed || self.tx_pending > 0
    }
}

// ============================================================================
// DHCP 状态 (替代 smoltcp::socket::dhcpv4::Event)
// ============================================================================

/// DHCP 客户端状态机.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DhcpState {
    /// 未启动
    Idle,
    /// Discover 已发送, 等待 Offer
    Discovering,
    /// Request 已发送, 等待 ACK
    Requesting,
    /// 已绑定有效租约
    Bound {
        /// 分配的 IPv4
        ipv4: [u8; 4],
        /// 租约到期时间 (绝对时间, 协议栈时间单位)
        lease_expires_at: u64,
    },
    /// 租约续期中
    Renewing {
        ipv4: [u8; 4],
    },
    /// 失败 (N 次重试后), 走 fallback 静态 IP
    Failed,
}

impl DhcpState {
    /// 是否处于"已配置"状态 (Bound 或 Renewing).
    #[inline(always)]
    pub const fn is_configured(&self) -> bool {
        matches!(self, Self::Bound { .. } | Self::Renewing { .. })
    }

    /// 获取已绑定的 IPv4 (若已配置).
    #[inline]
    pub fn ipv4(&self) -> Option<[u8; 4]> {
        match *self {
            Self::Bound { ipv4, .. } | Self::Renewing { ipv4 } => Some(ipv4),
            _ => None,
        }
    }
}

impl Default for DhcpState {
    fn default() -> Self {
        Self::Idle
    }
}

// ============================================================================
// 网络错误类型 (替代 smoltcp 的复杂错误)
// ============================================================================

/// 网络协议栈统一错误类型.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    /// Socket 表已满
    NoFreeSocket,
    /// 句柄无效或已关闭
    InvalidHandle,
    /// 配置错误 (例如 IP 非法)
    BadConfig,
    /// 协议栈未就绪 (NET_READY = false)
    NotReady,
    /// 操作超时
    Timeout,
    /// 缓冲区不足
    BufferTooSmall,
    /// 通用错误
    Other,
}

pub type Result<T> = core::result::Result<T, NetError>;

// ============================================================================
// 核心抽象: NetStack trait
// ============================================================================

/// 网络协议栈抽象 — Framekernel Safe API.
///
/// ## 实现方契约
///
/// 实现方 (W3 子任务: `SmoltcpNetStack`) 必须保证:
/// - 0 unsafe (services 层铁律)
/// - 所有方法 `#[inline]` 或足够小, 静态分发 0 开销
/// - 不暴露 smoltcp 任何内部类型
/// - 句柄分配/释放幂等, 失败回滚遵循 DECISION-025/027
///
/// ## 调用方契约
///
/// - 由 `framework/net/init.rs::poll_network()` 调用 `poll()` / `poll_at()`
/// - 由 `services/net/socket.rs` 调用 `socket_open()` / `socket_close()`
/// - 由 `services/net/dhcp_policy.rs` 调用 `dhcp_state()`
///
/// ## 线程安全
///
/// 实现方不要求 `Send`/`Sync` — 调用方保证互斥访问 (在 `NET_LOCK` 下).
pub trait NetStack {
    /// 初始化网络协议栈 (启动时调用一次).
    ///
    /// 内部完成: smoltcp Interface 构造, DHCP 客户端启动 (若配置为 DHCP),
    /// 套接字表初始化.
    #[inline]
    fn init(&mut self, cfg: NetConfig) -> Result<()> {
        let _ = cfg;
        Err(NetError::NotReady)
    }

    /// 轮询协议栈 (调度器 tick 调用, ISR 安全).
    ///
    /// 应在 `poll_at()` 指定的最近时间点调用, 可频繁 (每 ms) 调用.
    /// 返回本次轮询产生的事件, 供调度器唤醒相应等待者.
    #[inline]
    fn poll(&mut self, ts_ms: u64) -> PollOutcome {
        let _ = ts_ms;
        PollOutcome::idle()
    }

    /// 查询协议栈下次轮询时间 (`None` = 立即需要轮询).
    ///
    /// 用于调度器决定何时插入 hrtimer 唤醒, 避免空轮询.
    #[inline]
    fn poll_at(&self) -> Option<u64> {
        None
    }

    /// 打开一个 Socket, 返回句柄.
    ///
    /// 句柄由实现方分配, 调用方必须 `socket_close()` 释放.
    /// 失败时返回 `Err(NetError)`, 无副作用 (DECISION-025).
    #[inline]
    fn socket_open(&mut self, kind: SocketKind) -> Result<SocketHandle> {
        let _ = kind;
        Err(NetError::NotReady)
    }

    /// 关闭一个 Socket, 释放句柄.
    ///
    /// 幂等操作, 对 `SocketHandle::INVALID` 或已关闭句柄调用不报错.
    #[inline]
    fn socket_close(&mut self, h: SocketHandle) -> Result<()> {
        let _ = h;
        Ok(())
    }

    /// 查询 DHCP 客户端状态.
    ///
    /// 由 services/net/dhcp_policy.rs 实现策略 (何时重试, 何时 fallback).
    #[inline]
    fn dhcp_state(&self) -> DhcpState {
        DhcpState::default()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 SocketHandle 的 INVALID 哨兵.
    #[test]
    fn test_socket_handle_invalid() {
        assert!(SocketHandle::INVALID.is_invalid());
        assert!(!SocketHandle::INVALID.is_valid());
        assert_eq!(SocketHandle::INVALID, SocketHandle::default());
        assert_eq!(SocketHandle::INVALID.0, 0);
    }

    /// 验证 SocketHandle 的有效性判断.
    #[test]
    fn test_socket_handle_validity() {
        let h = SocketHandle::from_raw(42);
        assert!(!h.is_invalid());
        assert!(h.is_valid());
        assert_eq!(h.raw(), 42);
        assert_eq!(h.0, 42);
    }

    /// 验证 SocketKind 内部类型判断.
    #[test]
    fn test_socket_kind_internal() {
        assert!(SocketKind::Dhcpv4.is_internal());
        assert!(SocketKind::Dns.is_internal());
        assert!(!SocketKind::Tcp.is_internal());
        assert!(!SocketKind::Udp.is_internal());
        assert!(!SocketKind::Icmp.is_internal());
        assert!(!SocketKind::Raw.is_internal());
    }

    /// 验证 NetConfig 的 DHCP 决策.
    #[test]
    fn test_net_config_dhcp_decision() {
        let mut cfg = NetConfig::empty();
        assert!(cfg.use_dhcp(), "默认应走 DHCP");
        cfg.static_ipv4 = Some([192, 168, 1, 100]);
        assert!(!cfg.use_dhcp(), "设置 static_ipv4 后应走静态");
        cfg.static_ipv4 = None;
        assert!(cfg.use_dhcp());
    }

    /// 验证 NetConfig::empty 的零值默认.
    #[test]
    fn test_net_config_empty_defaults() {
        let cfg = NetConfig::empty();
        assert_eq!(cfg.mac_address, [0; 6]);
        assert_eq!(cfg.static_ipv4, None);
        assert_eq!(cfg.prefix_len, 24);
        assert_eq!(cfg.gateway, [0; 4]);
        assert_eq!(cfg.random_seed, 0);
    }

    /// 验证 PollOutcome 事件判断.
    #[test]
    fn test_poll_outcome_events() {
        let mut o = PollOutcome::idle();
        assert!(!o.has_events(), "空结果应无事件");

        o.packet_received = true;
        assert!(o.has_events());
        o = PollOutcome::idle();
        o.socket_woken = true;
        assert!(o.has_events());
        o = PollOutcome::idle();
        o.dhcp_progressed = true;
        assert!(o.has_events());
        o = PollOutcome::idle();
        o.tx_pending = 1;
        assert!(o.has_events());
    }

    /// 验证 PollOutcome::idle 是真正的零状态.
    #[test]
    fn test_poll_outcome_idle_zero() {
        let o = PollOutcome::idle();
        assert!(!o.packet_received);
        assert!(!o.socket_woken);
        assert!(!o.dhcp_progressed);
        assert_eq!(o.tx_pending, 0);
        assert_eq!(o, PollOutcome::default());
    }

    /// 验证 DhcpState 状态转移.
    #[test]
    fn test_dhcp_state_transitions() {
        let s = DhcpState::Idle;
        assert!(!s.is_configured());
        assert_eq!(s.ipv4(), None);

        let s = DhcpState::Discovering;
        assert!(!s.is_configured());

        let s = DhcpState::Requesting;
        assert!(!s.is_configured());

        let s = DhcpState::Bound {
            ipv4: [10, 0, 2, 15],
            lease_expires_at: 3600_000,
        };
        assert!(s.is_configured());
        assert_eq!(s.ipv4(), Some([10, 0, 2, 15]));

        let s = DhcpState::Renewing { ipv4: [10, 0, 2, 15] };
        assert!(s.is_configured());
        assert_eq!(s.ipv4(), Some([10, 0, 2, 15]));

        let s = DhcpState::Failed;
        assert!(!s.is_configured());
        assert_eq!(s.ipv4(), None);
    }

    /// 验证 DhcpState 默认值.
    #[test]
    fn test_dhcp_state_default() {
        assert_eq!(DhcpState::default(), DhcpState::Idle);
    }

    /// 验证 NetError 的等值比较.
    #[test]
    fn test_net_error_eq() {
        assert_eq!(NetError::NoFreeSocket, NetError::NoFreeSocket);
        assert_ne!(NetError::NoFreeSocket, NetError::InvalidHandle);
        assert_ne!(NetError::Timeout, NetError::BufferTooSmall);
    }

    /// 验证 Result<T, NetError> 的标准用法.
    #[test]
    fn test_result_standard_usage() {
        let ok: Result<u32> = Ok(42);
        let err: Result<u32> = Err(NetError::NoFreeSocket);
        assert_eq!(ok.unwrap(), 42);
        assert_eq!(err.unwrap_err(), NetError::NoFreeSocket);
    }

    /// 验证 SocketHandle 的 PartialOrd (用于 BTreeMap / 二分查找).
    #[test]
    fn test_socket_handle_ord() {
        use core::cmp::Ordering;
        let a = SocketHandle::from_raw(1);
        let b = SocketHandle::from_raw(2);
        let c = SocketHandle::from_raw(2);
        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
        assert_eq!(b.cmp(&c), Ordering::Equal);
        assert!(a < b);
        assert!(b > a);
        assert!(b <= c);
        assert!(b >= c);
    }

    /// 验证 SocketHandle 的 Debug 格式化.
    #[test]
    fn test_socket_handle_debug() {
        assert_eq!(format!("{:?}", SocketHandle::INVALID), "SocketHandle(INVALID)");
        assert_eq!(format!("{:?}", SocketHandle::from_raw(7)), "SocketHandle(7)");
    }

    /// 验证 NetStack trait 默认实现的健壮性 (不应 panic, 不应 UB).
    #[test]
    fn test_netstack_default_impls() {
        // 用一个 mock 实现验证 trait 默认实现
        struct Mock;
        impl NetStack for Mock {}

        let mut mock = Mock;
        assert_eq!(mock.init(NetConfig::empty()), Err(NetError::NotReady));
        assert_eq!(mock.poll(0), PollOutcome::idle());
        assert_eq!(mock.poll_at(), None);
        assert_eq!(mock.socket_open(SocketKind::Tcp), Err(NetError::NotReady));
        assert_eq!(mock.socket_close(SocketHandle::INVALID), Ok(()));
        assert_eq!(mock.dhcp_state(), DhcpState::Idle);
    }
}

// ============================================================================
// W4.4: 线协议类型 newtype 包装 (替代 smoltcp::wire::Ipv4Address / IpCidr / IpEndpoint)
//
// 全部使用 `[u8; 4]` / `(addr, port)` 元组, 不引入 IPv6 路径.
// 仅 IPv4 域足够覆盖 QEMU/QueenX 目标环境的现有需求.
//
// ## 设计动机
//
// smoltcp 的 wire 模块是**协议栈实现的内部细节**, 包含:
// - `Ipv4Address::new(a, b, c, d)` (4 元组)
// - `Ipv4Cidr::new(addr, prefix)`
// - `IpEndpoint { addr, port }`
//
// 这些类型在 smoltcp::iface / socket API 中作为参数/返回值反复出现.
// 直接使用意味着:
// 1. services 间接依赖 smoltcp 公开 wire API
// 2. 切换协议栈实现 (e.g. 未来换 smoltcp-new / 其它) 需要替换全部调用方
// 3. 与 NetStack trait 类型擦除的"无 smoltcp 内部类型"不变式冲突
//
// 本模块把 wire 类型全部用新类型 (newtype) 包装, 实现层
// (smoltcp_impl.rs) 提供与 smoltcp wire 类型的翻译.
//
// ## 与 SmoltcpNetStack trait 方法的关系
//
// 抽象类型 → smoltcp wire 类型 的转换由 SmoltcpNetStack 的方法提供.
// 调用方从不直接构造 smoltcp::wire::* 类型的值.
//
// ## 线程 / 锁约束
//
// 全部 Copy, 0 分配, 0 unsafe.
// ============================================================================

/// IPv4 地址 (替代 smoltcp::wire::Ipv4Address / IpAddress::Ipv4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    /// 任意地址 (0.0.0.0) — 用于"未指定"语义.
    pub const UNSPECIFIED: Self = Self([0, 0, 0, 0]);
    /// 255.255.255.255 广播地址.
    pub const BROADCAST: Self = Self([255, 255, 255, 255]);

    /// 构造一个 IPv4 地址.
    #[inline(always)]
    pub const fn new(o0: u8, o1: u8, o2: u8, o3: u8) -> Self {
        Self([o0, o1, o2, o3])
    }

    /// 从 4 元组数组构造.
    #[inline(always)]
    pub const fn from_octets(octets: [u8; 4]) -> Self {
        Self(octets)
    }

    /// 获取 4 元组数组.
    #[inline(always)]
    pub const fn octets(self) -> [u8; 4] {
        self.0
    }

    /// 是否为未指定地址 (0.0.0.0).
    #[inline(always)]
    pub const fn is_unspecified(self) -> bool {
        self.0[0] == 0 && self.0[1] == 0 && self.0[2] == 0 && self.0[3] == 0
    }
}

impl From<[u8; 4]> for Ipv4Addr {
    #[inline(always)]
    fn from(o: [u8; 4]) -> Self {
        Self(o)
    }
}

impl From<Ipv4Addr> for [u8; 4] {
    #[inline(always)]
    fn from(a: Ipv4Addr) -> Self {
        a.0
    }
}

/// IPv4 CIDR (地址 + 前缀长度), 替代 smoltcp::wire::Ipv4Cidr / IpCidr::Ipv4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ipv4Cidr {
    /// 网络地址 (主机字节序, 大端视图)
    pub address: Ipv4Addr,
    /// 前缀长度 (0-32)
    pub prefix_len: u8,
}

impl Ipv4Cidr {
    /// 构造一个 CIDR.
    #[inline(always)]
    pub const fn new(address: Ipv4Addr, prefix_len: u8) -> Self {
        Self { address, prefix_len }
    }
}

/// IP 端点 (地址 + 端口), 替代 smoltcp::wire::IpEndpoint.
///
/// 当前仅支持 IPv4 (与 Ipv4Addr 配对), 不引入 IPv6 路径.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetEndpoint {
    /// IPv4 地址
    pub addr: Ipv4Addr,
    /// 端口 (主机字节序)
    pub port: u16,
}

impl NetEndpoint {
    /// 构造一个端点.
    #[inline(always)]
    pub const fn new(addr: Ipv4Addr, port: u16) -> Self {
        Self { addr, port }
    }

    /// 未指定端点 (0.0.0.0:0).
    pub const UNSPECIFIED: Self = Self {
        addr: Ipv4Addr::UNSPECIFIED,
        port: 0,
    };
}

/// IP 监听端点 (地址可通配 + 端口), 替代 smoltcp::wire::IpListenEndpoint.
///
/// 与 NetEndpoint 区别: addr 可为 `None` (通配, 接受任何地址).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetListenEndpoint {
    /// 监听地址 (None = 通配)
    pub addr: Option<Ipv4Addr>,
    /// 监听端口
    pub port: u16,
}

impl NetListenEndpoint {
    /// 通配地址监听 (0.0.0.0:port).
    #[inline(always)]
    pub const fn wildcard(port: u16) -> Self {
        Self { addr: None, port }
    }

    /// 指定地址监听.
    #[inline(always)]
    pub const fn new(addr: Ipv4Addr, port: u16) -> Self {
        Self { addr: Some(addr), port }
    }
}

#[cfg(test)]
mod wire_type_tests {
    use super::*;

    #[test]
    fn test_ipv4_addr_constructors() {
        let a = Ipv4Addr::new(192, 168, 1, 100);
        assert_eq!(a.octets(), [192, 168, 1, 100]);
        assert_eq!(Ipv4Addr::from_octets([10, 0, 0, 1]).octets(), [10, 0, 0, 1]);
        assert!(Ipv4Addr::UNSPECIFIED.is_unspecified());
        assert!(!a.is_unspecified());
    }

    #[test]
    fn test_ipv4_addr_conversions() {
        let octets = [10, 0, 0, 1];
        let a: Ipv4Addr = octets.into();
        let back: [u8; 4] = a.into();
        assert_eq!(octets, back);
    }

    #[test]
    fn test_ipv4_cidr() {
        let c = Ipv4Cidr::new(Ipv4Addr::new(10, 0, 0, 0), 8);
        assert_eq!(c.address.octets(), [10, 0, 0, 0]);
        assert_eq!(c.prefix_len, 8);
    }

    #[test]
    fn test_net_endpoint() {
        let e = NetEndpoint::new(Ipv4Addr::new(192, 168, 1, 1), 8080);
        assert_eq!(e.addr.octets(), [192, 168, 1, 1]);
        assert_eq!(e.port, 8080);
        assert_eq!(NetEndpoint::UNSPECIFIED.port, 0);
    }

    #[test]
    fn test_net_listen_endpoint() {
        let wc = NetListenEndpoint::wildcard(80);
        assert!(wc.addr.is_none());
        assert_eq!(wc.port, 80);

        let sp = NetListenEndpoint::new(Ipv4Addr::new(127, 0, 0, 1), 22);
        assert_eq!(sp.addr.unwrap().octets(), [127, 0, 0, 1]);
        assert_eq!(sp.port, 22);
    }
}
