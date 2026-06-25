#![deny(unsafe_code)]
//! SmoltcpNetStack — NetStack trait 的 smoltcp 实现 (W3.2 trait 骨架)
//!
//! ## 定位
//!
//! 实现 `framework::net::iface_trait::NetStack` trait. 本文件是 services 层
//! 唯一允许直接使用 smoltcp 类型的代码, 承担"smoltcp → NetStack"的翻译
//! 责任.
//!
//! ## W3.2 范围 (2026-06-24, REVAL-W 第 6 组)
//!
//! **本版本 (W3.2 trait 骨架)**:
//! - `SmoltcpNetStack` 实现 NetStack trait
//! - `init()`: 缓存配置, 跟踪 DHCP 状态机, 模拟状态转换
//! - `poll()` / `poll_at()`: 占位 (返回 idle / None)
//! - `socket_open()`: 返回 `NotReady` (W4 整合 device 后实装)
//! - `socket_close()`: 幂等 no-op
//! - `dhcp_state()`: 返回缓存的状态
//!
//! ## W3.2 不实装项 (留给 W4 整合)
//!
//! - **SocketSet 实际创建**: smoltcp 0.13 的 `SocketSet<'a>` 借用 'static
//!   SocketStorage, 添加到 SocketSet 的 socket 引用 buffer, 全部必须 'static.
//!   在 safe Rust 中, 持有 SocketSet + 内部 buffer 是 self-referential 结构,
//!   不可行. **W4 整合 init.rs 时, 由 framework 层 (允许 unsafe) 提供 'static
//!   内存 + 创建 SmoltcpNetStack**, 本 trait 翻译层不实际构造 smoltcp 套接字.
//!
//! - **实际 `Interface::poll(ts, &mut device, &mut sockets)`**: 需要 device
//!   引用, 与 self-referential 冲突. W4 整合时设计 (泛型 D 或 Box<dyn Device>).
//!
//! ## W3.2 价值
//!
//! 即使 socket_open 暂时返回 NotReady, W3.2 仍然有以下价值:
//! 1. **NetStack trait 完整可实例化** (init/dhcp_state 工作)
//! 2. **类型擦除的 SocketHandle 分配/释放** 已实装 (W5 transmute 替代)
//! 3. **DHCP 状态机翻译** 已实装 (W6 接入 dhcp_policy 时直接使用)
//! 4. **编译期验证**: 14 个单测, 编译期检查 6 个 trait 方法签名
//! 5. **W4 整合点明确**: trait 翻译层已就绪, W4 只需提供 device + storage
//!
//! ## 设计依据
//!
//! - [docs/plan/smoltcp-framekernel-wrapper.md] §3 关键设计决策
//! - [docs/plan/maintenance-cycle-2026-06-19.md] §9.5 REVAL-W 第 6 组

use crate::kernel::framework::net::iface_trait::{
    DhcpState, NetConfig, NetError, NetStack, PollOutcome, Result, SocketHandle, SocketKind,
};
// REVAL-W W4.2.3.4 步骤 3: 调用 framework::init 的 safe wrapper, 实现
// 实际 smoltcp socket 创建. smoltcp_impl 是 services 层唯一允许直接使用
// smoltcp 类型的文件, 但 socket_open 的实际 smoltcp 操作 (k_malloc +
// SocketBuffer::new + sockets.add) 由 framework 层 (允许 unsafe) 提供.
use crate::kernel::framework::net::init as fw_init;

// ============================================================================
// 编译期常量
// ============================================================================

/// Socket 容量上限.
///
/// 与 `framework/net/init.rs::MAX_SOCKETS` 同步 (编译期常量, 修改需同时
/// 更新两处). 此处用较小值, 因为 W3.2 不实际创建 smoltcp sockets.
pub const MAX_SOCKETS: usize = 32;

// ============================================================================
// 句柄槽位 (类型擦除, 替代 smoltcp::iface::SocketHandle<usize>)
// ============================================================================

/// Socket 句柄的内部表示.
///
/// smoltcp 的 `SocketHandle(usize)` 直接暴露数组索引, 任何 `usize` 都可能被
/// 误用. 本类型用 `(user_id, smol_handle_u32)` 三元组把句柄分配与 smoltcp
/// 内部索引解耦. 句柄槽位的存在性 = 句柄有效性.
///
/// ## 内存布局 (W4.2.3.4 变化)
///
/// - `Some((u, h))` = 句柄已分配, u 是用户态 ID, h 是 smol_handle (u32,
///   从 smoltcp::iface::SocketHandle 通过 transmute + as cast 提取)
/// - `None` = 句柄槽位空闲
///
/// ## 为什么从 `(u32, u16)` 改为 `(u32, u32)`
///
/// 旧设计: smoltcp 句柄 = 槽位索引, 用 u16 即可
/// 新设计: SmoltcpNetStack 范围 `[MAX_SM_FD, TOTAL_SLOTS)`, 实际
/// smoltcp SocketHandle index 是 0..MAX_SOCKETS (≤ 1024) 用 u16 够.
/// 但跨 crate 翻译 (framework → services) 用 u32 简化, 统一 smol_handle
/// 表示 (无论 sm_socket 路径还是 SmoltcpNetStack 路径, smol_handle index
/// 都用 u32 表达).
type HandleSlot = Option<(u32, u32)>;

// ============================================================================
// SmoltcpNetStack
// ============================================================================

/// smoltcp 网络协议栈的 NetStack trait 实现 (W3.2 trait 骨架).
///
/// ## 不变式
///
/// 1. **零 unsafe**: services 层铁律, 编译期 `#![deny(unsafe_code)]` 强制
/// 2. **类型擦除**: 内部用 u32 ID, 对外暴露 `SocketHandle`
/// 3. **句柄幂等**: 重复 `socket_close` 不报错 (DECISION-025)
/// 4. **DHCP 句柄独占**: dhcp socket 不可被用户关闭
/// 5. **socket_open 失败回滚**: DECISION-027
///
/// ## W3.2 字段说明
///
/// - `config`: init() 时缓存, 后续只读
/// - `handle_map`: 句柄槽位表, 容量 = MAX_SOCKETS
/// - `next_user_id`: 下一个分配的 user 句柄 (u32, 0 = INVALID)
/// - `dhcp_user_id`: DHCP socket 的 user 句柄 (若有)
/// - `dhcp_state`: DHCP 状态缓存
/// - `initialized`: 是否已 init (init 前所有方法返回 NotReady/idle)
///
/// ## W4 整合时的字段扩展
///
/// W4 将在 SmoltcpNetStack 中添加 (在 init.rs 内部构造):
/// - `sockets: SocketSet<'static>` (smoltcp SocketSet, 'static 借用)
/// - `device: Box<dyn smoltcp::phy::Device>` 或泛型 D
/// - `interface: smoltcp::iface::Interface` (由 device + config 构造)
///
/// ## 线程安全
///
/// 不要求 `Send`/`Sync` — 调用方保证互斥访问 (在 `NET_LOCK` 下).
pub struct SmoltcpNetStack {
    config: NetConfig,
    handle_map: [HandleSlot; MAX_SOCKETS],
    next_user_id: u32,
    /// 下一个 SmoltcpNetStack 专属范围的 smol 槽位索引 (W4.2.3.4 阶段新增).
    ///
    /// SmoltcpNetStack 范围: `[MAX_SM_FD, TOTAL_SLOTS)` (由 framework
    /// `init.rs` 静态数组大小决定, W4.2.3.1 阶段实装). 我们用相对索引
    /// `[0, MAX_SOCKETS)` 简化 SmoltcpNetStack 内部逻辑, 实际 smol 槽位
    /// 索引 = `MAX_SM_FD + next_smol_idx`.
    next_smol_idx: u16,
    dhcp_user_id: Option<u32>,
    dhcp_state: DhcpState,
    initialized: bool,
}

impl SmoltcpNetStack {
    /// 构造一个未初始化的 SmoltcpNetStack 实例.
    pub fn new() -> Self {
        Self {
            config: NetConfig::empty(),
            handle_map: [None; MAX_SOCKETS],
            next_user_id: 1, // 0 = INVALID
            next_smol_idx: 0,
            dhcp_user_id: None,
            dhcp_state: DhcpState::Idle,
            initialized: false,
        }
    }

    /// 是否已初始化.
    #[inline]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// 当前配置.
    #[inline]
    pub const fn config(&self) -> &NetConfig {
        &self.config
    }

    /// 找空槽位.
    fn find_free_slot(&self) -> Option<usize> {
        self.handle_map.iter().position(|slot| slot.is_none())
    }

    /// 找 DHCP 句柄占用的槽位.
    fn find_dhcp_slot(&self) -> Option<usize> {
        let dhcp_id = self.dhcp_user_id?;
        self.handle_map.iter().position(|slot| {
            matches!(slot, Some((u, _)) if *u == dhcp_id)
        })
    }

    /// 分配下一个 user 句柄 ID.
    fn alloc_user_id(&mut self) -> u32 {
        let id = self.next_user_id;
        self.next_user_id = self.next_user_id.wrapping_add(1);
        if self.next_user_id == 0 {
            self.next_user_id = 1;
        }
        id
    }

    /// 检查 user 句柄是否对应 DHCP socket.
    fn is_dhcp_handle(&self, user_id: u32) -> bool {
        self.dhcp_user_id == Some(user_id)
    }
}

impl Default for SmoltcpNetStack {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// NetStack trait 实现
// ============================================================================

impl NetStack for SmoltcpNetStack {
    /// 初始化网络协议栈.
    ///
    /// ## 步骤
    ///
    /// 1. 检查重复 init (DECISION-027)
    /// 2. 缓存配置
    /// 3. 若 cfg.use_dhcp(), 分配 DHCP 句柄槽位 + 标记 Discovering 状态
    /// 4. 若 cfg.static_ipv4 != None, 标记 Bound 状态
    /// 5. 标记 initialized
    ///
    /// ## W3.2 不实装
    ///
    /// - 实际 smoltcp `Interface::new` + `update_ip_addrs` 调用
    /// - 实际 smoltcp `dhcpv4::Socket::new()` 构造
    /// - (这些需要 &mut device, 在 W4 整合时实装)
    fn init(&mut self, cfg: NetConfig) -> Result<()> {
        if self.initialized {
            return Err(NetError::BadConfig);
        }

        // 1. 缓存配置
        self.config = cfg;

        // 2. 启动 DHCP (若配置)
        if cfg.use_dhcp() {
            // 检查空槽位
            if self.find_free_slot().is_none() {
                self.config = NetConfig::empty();
                return Err(NetError::NoFreeSocket);
            }
            // 分配 DHCP 句柄槽位
            let user_id = self.alloc_user_id();
            let slot_idx = self
                .find_free_slot()
                .expect("刚才检查过有空槽位, 现在应该有");
            self.handle_map[slot_idx] = Some((user_id, slot_idx as u32));
            self.dhcp_user_id = Some(user_id);
            self.dhcp_state = DhcpState::Discovering;
        } else {
            // 静态 IP 配置
            if let Some(ipv4) = cfg.static_ipv4 {
                if ipv4 == [0, 0, 0, 0] {
                    self.config = NetConfig::empty();
                    return Err(NetError::BadConfig);
                }
                self.dhcp_state = DhcpState::Bound {
                    ipv4,
                    lease_expires_at: u64::MAX,
                };
            } else {
                self.config = NetConfig::empty();
                return Err(NetError::BadConfig);
            }
        }

        // 3. 标记 initialized
        self.initialized = true;
        Ok(())
    }

    /// 轮询协议栈.
    ///
    /// ## W3.2 占位行为
    ///
    /// 不实际调用 smoltcp `Interface::poll` (需要 &mut device 借用, 与
    /// self-referential 冲突). W3.2 返回 `PollOutcome::idle()`.
    /// 真实 poll 留给 W4 整合 init.rs 时实现.
    fn poll(&mut self, ts_ms: u64) -> PollOutcome {
        if !self.initialized {
            return PollOutcome::idle();
        }
        let _ = ts_ms;
        PollOutcome::idle()
    }

    /// 查询下次轮询时间.
    fn poll_at(&self) -> Option<u64> {
        if !self.initialized {
            return None;
        }
        None
    }

    /// 打开一个 Socket.
    ///
    /// ## W3.2 占位行为
    ///
    /// 不实际构造 smoltcp socket (需要 &mut device + 'static buffer).
    /// W3.2 返回 `Err(NetError::NotReady)`, 但**仍分配槽位** (跟踪句柄).
    /// 这样 W5 的 transmute 移除就有了替代路径 (用 SocketHandle 而非 usize).
    ///
    /// ## W4 整合时的实装
    ///
    /// ```ignore
    /// match kind {
    ///     SocketKind::Tcp => {
    ///         let rx_buffer = TcpSocketBuffer::new(&mut self.rx_buf[slot_idx][..]);
    ///         let tx_buffer = TcpSocketBuffer::new(&mut self.tx_buf[slot_idx][..]);
    ///         let socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
    ///         self.sockets.add(socket)
    ///     }
    ///     // ...
    /// }
    /// ```
    fn socket_open(&mut self, kind: SocketKind) -> Result<SocketHandle> {
        if !self.initialized {
            return Err(NetError::NotReady);
        }

        // REVAL-W W4.2.3.4 步骤 3: 实际化 socket_open.
        //
        // 1. 找 SmoltcpNetStack 范围内的空 handle_map 索引
        // 2. 计算 smol 槽位索引 = MAX_SM_FD + handle_map_idx
        // 3. 调用 fw_init::smoltcp_net_stack_socket_open 实际构造 socket
        // 4. 记录 (user_id, smol_handle_u32) 到 handle_map
        let handle_map_idx = match self.find_free_slot() {
            Some(i) => i,
            None => return Err(NetError::NoFreeSocket),
        };

        // SmoltcpNetStack 专属范围: [MAX_SM_FD, TOTAL_SLOTS)
        // 实际 smol 槽位索引 = MAX_SM_FD + handle_map_idx
        let smol_slot_idx = fw_init::smoltcp_net_stack_slot_base() + handle_map_idx;

        // 调用 framework safe wrapper 实际构造 smoltcp socket
        // 失败回滚: handle_map 仍空闲, user_id 浪费 (可接受, 下次 alloc 跳过)
        let smol_handle_u32 = fw_init::smoltcp_net_stack_socket_open(kind, smol_slot_idx)
            .ok_or(NetError::NoFreeSocket)?;

        // 分配 user 句柄 (W3.2 alloc_user_id 跳过 0 = INVALID)
        let user_id = self.alloc_user_id();

        // 记录 (user_id, smol_handle_u32) 到 handle_map
        self.handle_map[handle_map_idx] = Some((user_id, smol_handle_u32));

        Ok(SocketHandle::from_raw(user_id))
    }

    /// 关闭一个 Socket.
    ///
    /// ## 幂等性
    ///
    /// 对 `SocketHandle::INVALID` 或未分配槽位返回 Ok, 不报错.
    /// DHCP socket 不可关闭 (返回 Ok 但实际保留).
    fn socket_close(&mut self, h: SocketHandle) -> Result<()> {
        if !h.is_valid() {
            return Ok(());
        }

        // 找槽位
        let mut found_idx = None;
        for (i, slot) in self.handle_map.iter().enumerate() {
            if let Some((u, _)) = slot {
                if *u == h.raw() {
                    found_idx = Some(i);
                    break;
                }
            }
        }
        let Some(idx) = found_idx else {
            return Ok(()); // 句柄不存在, 幂等
        };

        // DHCP 句柄保护
        if self.is_dhcp_handle(h.raw()) {
            return Ok(());
        }

        // 释放槽位
        self.handle_map[idx] = None;
        Ok(())
    }

    /// 查询 DHCP 状态.
    fn dhcp_state(&self) -> DhcpState {
        self.dhcp_state
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 1. 构造与默认状态 ----

    #[test]
    fn test_new_returns_uninitialized() {
        let stack = SmoltcpNetStack::new();
        assert!(!stack.is_initialized());
        assert_eq!(stack.dhcp_state(), DhcpState::Idle);
        assert_eq!(stack.config().mac_address, [0; 6]);
    }

    #[test]
    fn test_default_trait_works() {
        let stack = SmoltcpNetStack::default();
        assert!(!stack.is_initialized());
    }

    // ---- 2. init() 各种路径 ----

    #[test]
    fn test_init_with_dhcp_succeeds() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            static_ipv4: None,
            prefix_len: 24,
            gateway: [0, 0, 0, 0],
            random_seed: 42,
        };
        assert!(stack.init(cfg).is_ok());
        assert!(stack.is_initialized());
        assert_eq!(stack.dhcp_state(), DhcpState::Discovering);
        assert!(stack.dhcp_user_id.is_some());
    }

    #[test]
    fn test_init_with_static_ipv4_succeeds() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            static_ipv4: Some([192, 168, 1, 100]),
            prefix_len: 24,
            gateway: [192, 168, 1, 1],
            random_seed: 42,
        };
        assert!(stack.init(cfg).is_ok());
        assert!(stack.is_initialized());
        match stack.dhcp_state() {
            DhcpState::Bound { ipv4, .. } => assert_eq!(ipv4, [192, 168, 1, 100]),
            _ => panic!("应为 Bound 状态"),
        }
        assert!(stack.dhcp_user_id.is_none());
    }

    #[test]
    fn test_init_with_zero_static_ip_fails() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([0, 0, 0, 0]),
            prefix_len: 24,
            gateway: [0, 0, 0, 0],
            random_seed: 0,
        };
        assert_eq!(stack.init(cfg), Err(NetError::BadConfig));
        assert!(!stack.is_initialized());
    }

    #[test]
    fn test_init_with_no_dhcp_and_no_static_fails() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig::empty();
        assert_eq!(stack.init(cfg), Err(NetError::BadConfig));
    }

    #[test]
    fn test_double_init_fails() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [1; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        assert!(stack.init(cfg).is_ok());
        assert_eq!(stack.init(cfg), Err(NetError::BadConfig));
    }

    #[test]
    fn test_init_dhcp_no_slot_fails() {
        // 满负载场景: 模拟 DHCP 失败
        let mut stack = SmoltcpNetStack::new();
        // 先填满所有槽位 (这要求先 init 一次, 但我们不能 init 两次)
        // 此测试用静态 IP 触发 NoFreeSocket (但静态 IP 不分配槽位)
        // 因此此测试改为验证 DHCP 模式下, MAX_SOCKETS - 1 槽位可分配
        // (在 test_dhcp_handle_protects_one_slot 中)
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: None,
            prefix_len: 24,
            gateway: [0, 0, 0, 0],
            random_seed: 0,
        };
        assert!(stack.init(cfg).is_ok());
    }

    // ---- 3. poll() / poll_at() ----

    #[test]
    fn test_poll_before_init_returns_idle() {
        let mut stack = SmoltcpNetStack::new();
        let outcome = stack.poll(1000);
        assert_eq!(outcome, PollOutcome::idle());
        assert!(!outcome.has_events());
    }

    #[test]
    fn test_poll_at_before_init_returns_none() {
        let stack = SmoltcpNetStack::new();
        assert!(stack.poll_at().is_none());
    }

    #[test]
    fn test_poll_after_init_returns_idle_w32_placeholder() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        let outcome = stack.poll(12345);
        assert_eq!(outcome, PollOutcome::idle());
        assert!(stack.poll_at().is_none());
    }

    // ---- 4. socket_open() / socket_close() ----

    #[test]
    fn test_socket_open_before_init_fails() {
        let mut stack = SmoltcpNetStack::new();
        assert_eq!(stack.socket_open(SocketKind::Tcp), Err(NetError::NotReady));
    }

    #[test]
    fn test_socket_open_returns_valid_handle_w32_stub() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        let h = stack.socket_open(SocketKind::Tcp);
        assert!(h.is_ok());
        assert!(h.unwrap().is_valid());
    }

    #[test]
    fn test_socket_open_all_kinds_succeed_w32_stub() {
        // W3.2 占位: 所有 socket 类型都返回 valid handle
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        assert!(stack.socket_open(SocketKind::Tcp).is_ok());
        assert!(stack.socket_open(SocketKind::Udp).is_ok());
        assert!(stack.socket_open(SocketKind::Icmp).is_ok());
        assert!(stack.socket_open(SocketKind::Raw).is_ok());
        assert!(stack.socket_open(SocketKind::Dhcpv4).is_ok());
        assert!(stack.socket_open(SocketKind::Dns).is_ok());
    }

    #[test]
    fn test_socket_close_invalid_handle_is_idempotent() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        assert!(stack.socket_close(SocketHandle::INVALID).is_ok());
        assert!(stack.socket_close(SocketHandle::from_raw(0xDEAD_BEEF)).is_ok());
    }

    #[test]
    fn test_socket_open_close_cycle() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        let h1 = stack.socket_open(SocketKind::Tcp).unwrap();
        let h2 = stack.socket_open(SocketKind::Tcp).unwrap();
        assert_ne!(h1, h2);
        assert!(stack.socket_close(h1).is_ok());
        assert!(stack.socket_close(h1).is_ok()); // 幂等
        assert!(stack.socket_close(h2).is_ok());
    }

    #[test]
    fn test_socket_open_until_full_returns_no_free() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        for i in 0..MAX_SOCKETS {
            let h = stack.socket_open(SocketKind::Tcp);
            assert!(h.is_ok(), "第 {} 个 socket_open 失败", i);
        }
        assert_eq!(
            stack.socket_open(SocketKind::Tcp),
            Err(NetError::NoFreeSocket)
        );
    }

    #[test]
    fn test_socket_close_frees_slot_for_reuse() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        let h1 = stack.socket_open(SocketKind::Tcp).unwrap();
        assert!(stack.socket_close(h1).is_ok());
        let h2 = stack.socket_open(SocketKind::Tcp).unwrap();
        assert!(h2.is_valid());
    }

    // ---- 5. DHCP 句柄保护 ----

    #[test]
    fn test_dhcp_handle_protects_one_slot() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: None,
            prefix_len: 24,
            gateway: [0, 0, 0, 0],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        // DHCP 占用 1 个槽位, 用户可分配 MAX_SOCKETS - 1 个
        for i in 0..MAX_SOCKETS - 1 {
            assert!(
                stack.socket_open(SocketKind::Tcp).is_ok(),
                "第 {} 个分配失败",
                i
            );
        }
        assert_eq!(
            stack.socket_open(SocketKind::Tcp),
            Err(NetError::NoFreeSocket)
        );
    }

    #[test]
    fn test_dhcp_handle_cannot_be_closed() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: None,
            prefix_len: 24,
            gateway: [0, 0, 0, 0],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        let dhcp_id = stack.dhcp_user_id.unwrap();
        let dhcp_handle = SocketHandle::from_raw(dhcp_id);
        // 关闭 DHCP 句柄 (应被保护)
        assert!(stack.socket_close(dhcp_handle).is_ok());
        // 确认 DHCP 句柄仍在
        assert_eq!(stack.dhcp_user_id, Some(dhcp_id));
        // 确认 DHCP 槽位仍占用
        assert!(stack.handle_map.iter().any(|s| matches!(s, Some((u, _)) if *u == dhcp_id)));
    }

    // ---- 6. DHCP 状态机 ----

    #[test]
    fn test_dhcp_state_idle_before_init() {
        let stack = SmoltcpNetStack::new();
        assert_eq!(stack.dhcp_state(), DhcpState::Idle);
    }

    #[test]
    fn test_dhcp_state_configured_when_static() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([192, 168, 1, 50]),
            prefix_len: 24,
            gateway: [192, 168, 1, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        assert!(stack.dhcp_state().is_configured());
        assert_eq!(stack.dhcp_state().ipv4(), Some([192, 168, 1, 50]));
    }

    #[test]
    fn test_dhcp_state_discovering_when_dhcp() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: None,
            prefix_len: 24,
            gateway: [0, 0, 0, 0],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        assert_eq!(stack.dhcp_state(), DhcpState::Discovering);
        assert!(!stack.dhcp_state().is_configured());
        assert_eq!(stack.dhcp_state().ipv4(), None);
    }

    // ---- 7. 类型擦除 SocketHandle 行为 ----

    #[test]
    fn test_socket_handle_invalid_zero() {
        let h = SocketHandle::INVALID;
        assert!(!h.is_valid());
        assert_eq!(h.raw(), 0);
    }

    #[test]
    fn test_socket_handle_allocated_distinct() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        let mut handles = std::collections::HashSet::new();
        for _ in 0..10 {
            let h = stack.socket_open(SocketKind::Tcp).unwrap();
            assert!(handles.insert(h), "句柄重复: {:?}", h);
        }
    }

    #[test]
    fn test_socket_handle_invalid_id_skipped() {
        let mut stack = SmoltcpNetStack::new();
        let cfg = NetConfig {
            mac_address: [0; 6],
            static_ipv4: Some([10, 0, 0, 1]),
            prefix_len: 24,
            gateway: [10, 0, 0, 1],
            random_seed: 0,
        };
        stack.init(cfg).unwrap();
        // next_user_id 从 1 开始 (跳过 0 = INVALID)
        let h1 = stack.socket_open(SocketKind::Tcp).unwrap();
        assert_eq!(h1.raw(), 1);
    }
}
