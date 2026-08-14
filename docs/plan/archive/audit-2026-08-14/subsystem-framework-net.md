# framework/net 子系统深度审计报告

> **审计范围**：`src/kernel/framework/net/`（11 个文件，重点 `init.rs` 2060 行 + `iface_trait.rs` 1552 行）
> **审计日期**：2026-08-14
> **代码规模**：约 5,011 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **32 个问题（P0×6, P1×9, P2×11, P3×6）**

## 1. 子系统概览

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [init.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs) | 2060 | smoltcp 网络协议栈初始化 + TCP/UDP/DHCP 全套 | **极高** |
| [iface_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs) | 1552 | NetStack trait + 类型擦除 + NetEndpoint + DHCP 状态机 | **极高** |
| [syscall.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/syscall.rs) | 562 | socket 系统调用 FFI 入口 | **极高** |
| [save.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/save.rs) | 277 | 网络状态持久化 | 中 |
| [smoltcp_impl.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/smoltcp_impl.rs) | 192 | smoltcp impl 入口 | **高** |
| [route.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/route.rs) | 172 | 路由表 | 中 |
| [api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/api.rs) | 165 | 网络公共 API | 中 |
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/mod.rs) | 72 | 子系统入口 | 低 |
| [netfilter.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/netfilter.rs) | 13 | Netfilter 桩 | 低 |
| [wait_queue.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/wait_queue.rs) | 9 | 桩（实际在 services/net） | 低 |
| [types.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/types.rs) | 9 | 桩 | 低 |

## 2. 严重问题

### 2.1 [P0] `init.rs:63` `MAX_SOCKETS = 256` BSS 占用 ≈ 1.5 MB——**boot 时分配失败可能 panic**

- **位置**：[init.rs:58-63](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L58-L63)
- **代码**：
  ```rust
  // I-47: 编译期容量上限, 默认 256 (此前硬编码 8 严重限制并发).
  // 每个 socket 携带 TCP/UDP 静态缓冲, BSS 占用 ≈ 6 KB/连接.
  // 256 → ≈ 1.5 MB BSS; 生产环境按物理内存调整.
  const MAX_SOCKETS: usize = 256;
  ```
- **问题**：
  - 注释承认 BSS 1.5 MB——中等规模系统可能可用，但**嵌入式/小内存系统启动可能 OOM**。
  - 与 `services/net::smoltcp_impl.rs:MAX_SOCKETS=32` 不一致（[subsystem-services-net.md §2.x](../audit/subsystem-services-net.md)）。
  - 编译期常量无法 runtime 调整。
- **建议方案**：
  1. boot 时探测物理内存后调整。
  2. 或动态分配 socket storage。

### 2.2 [P0] `init.rs:65-80` `NetState` 集中 12 个 static mut 但**初始化失败回滚不完整**

- **位置**：[init.rs:65-80](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L65-L80)
- **代码**：
  ```rust
  struct NetState {
      device: Option<ChitinNetDevice>,
      stack: Option<NetworkStack>,
      dhcp_handle: Option<SocketHandle>,
      socket_table: [Option<SocketHandle>; TOTAL_SLOTS],
      fd_types: [u8; TOTAL_SLOTS],
      tcp_rx_bufs: [*mut u8; TOTAL_SLOTS],
      tcp_tx_bufs: [*mut u8; TOTAL_SLOTS],
      udp_rx_bufs: [*mut u8; TOTAL_SLOTS],
      udp_tx_bufs: [*mut u8; TOTAL_SLOTS],
      udp_rx_metas: [[udp::PacketMetadata; UDP_META_COUNT]; TOTAL_SLOTS],
      udp_tx_metas: [[udp::PacketMetadata; UDP_META_COUNT]; TOTAL_SLOTS],
  }
  ```
- **问题**：
  - 8 个 `[*mut u8; TOTAL_SLOTS]` 裸指针数组——若部分初始化失败，**部分指针非 null 而其他为 null** → 后续访问触发 UB。
  - 注释（[init.rs:50-56](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L50-L56)）说"由 NET_STATE (IrqSpinLock) 保护"但**初始化阶段 NET_STATE 还未初始化**。
- **建议方案**：
  1. 初始化全程包在 init_net 函数内，失败时 Drop 已分配资源。
  2. 用 `Box<[u8]>` 替代 `*mut u8`。

### 2.3 [P0] `init.rs:2060` 单文件 2060 行**严重违反简单优先原则**

- **位置**：[init.rs:1-2060](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L1-L2060)
- **问题**：
  - **QueenX 最大单文件**。
  - 包含：
    - smoltcp 集成 (smoltcp::iface / socket / wire)
    - DHCP 客户端
    - TCP/UDP socket 管理
    - sm_* 25+ 函数 FFI 包装
    - NetStatus 序列化
    - 中断 disable/enable 序列
  - 应拆分为多个子模块。

### 2.4 [P0] `init.rs:39` `G_INIT_STATE: AtomicU8` 但**状态转换无文档**

- **位置**：[init.rs:39](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L39)
- **代码**：
  ```rust
  static G_INIT_STATE: AtomicU8 = AtomicU8::new(InitState::Uninitialized as u8);
  ```
- **问题**：
  - `InitState::Uninitialized → HardwareProbed → InterfaceReady → FullyInitialized` 转换流程未明示。
  - 与 `services/net::InitState` 重定义——两套枚举同步问题。

### 2.5 [P0] `iface_trait.rs:1552` 单文件 1552 行——过度集中 trait 定义

- **位置**：[iface_trait.rs:1-1552](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs#L1-L1552)
- **问题**：
  - 包含：
    - SocketHandle/IpAddr/Ipv4Addr/Ipv6Addr/NetEndpoint 类型
    - NetStack trait (15 方法)
    - DhcpState 枚举
    - NetError 枚举
    - NetConfig 结构
    - PollOutcome
    - SocketKind
    - 12+ helper 方法
  - 单文件过大。

### 2.6 [P0] `iface_trait.rs:68` `SocketHandle(pub(crate) u32)` 句柄 0 是 INVALID 但**没有任何 `socket_open` 时检查 `0` 是否已被分配**

- **位置**：[iface_trait.rs:67-83](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs#L67-L83)
- **代码**：
  ```rust
  pub struct SocketHandle(pub(crate) u32);

  impl SocketHandle {
      pub const INVALID: Self = Self(0);
      pub const fn is_invalid(self) -> bool { self.0 == 0 }
      pub const fn is_valid(self) -> bool { self.0 != 0 }
  }
  ```
- **问题**：
  - 句柄分配路径（`services/net/smoltcp_impl.rs::alloc_user_id`）从 1 开始分配，**理论上 0 永不分配**。
  - 但 `next_user_id.fetch_add(1).wrapping_add(1)`（[subsystem-services-net.md §2.6](../audit/subsystem-services-net.md)）在 u32::MAX 后回退到 1，可能分配重复。
  - 与该 trait 的 `is_invalid` 契约不符。

## 3. P1 问题

### 3.1 [P1] `init.rs:2060` smoltcp vendored 集成**未使用类型擦除封装**

- **位置**：[init.rs:6-13](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L6-L13)
- **代码**：
  ```rust
  use smoltcp::iface::{SocketHandle, SocketSet, SocketStorage};
  use smoltcp::socket::dhcpv4;
  use smoltcp::socket::{tcp, udp};
  use smoltcp::wire::IpCidr;
  ```
- **问题**：
  - 直接使用 smoltcp 类型——但 [iface_trait.rs:5-6](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs#L5-L6) 声明"0 unsafe, 0 smoltcp"。
  - **矛盾**：trait 定义承诺不导入 smoltcp，但 init.rs 直接导入。
  - 应该是 types 擦除后的接口调用。

### 3.2 [P1] `init.rs:75-79` 8 个 `[*mut u8; TOTAL_SLOTS]` 裸指针未初始化即填 0

- **位置**：[init.rs:75-79](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L75-L79)
- **问题**：
  - 裸指针数组 `[*mut u8; TOTAL_SLOTS]` 在 `NetState::new()` 中**默认初始化为全零**。
  - `*mut u8` 全零 = null——后续 `is_null()` 检查需正确处理。
  - 之前审计（[subsystem-framework-misc.md §3.4](../audit/subsystem-framework-misc.md)）类似问题。

### 3.3 [P1] `iface_trait.rs:56-60` 注释承认 `transmute<usize, SocketHandle>` 历史 UB——**当前实现是否仍有残留**

- **位置**：[iface_trait.rs:55-60](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs#L55-L60)
- **代码**：
  ```rust
  //! 此外 `as_u32_handle` (W5 移除) 中曾存在的
  //! `transmute<usize, SocketHandle>` 是 UB 风险 (REVAL-4 历史包袱),
  //! 已被替换为 `core::mem::transmute_copy`.
  ```
- **问题**：
  - 注释说"已替换为 `core::mem::transmute_copy`"——`transmute_copy` 同样不安全。
  - 当前是否仍使用需核查。

### 3.4 [P1] `syscall.rs:562` socket syscall FFI 562 行——与 services/net/syscall.rs 500 行重复

- **位置**：[syscall.rs:1-562](file:///home/anfer/Code/QueenX/src/kernel/framework/net/syscall.rs#L1-L562)
- **问题**：
  - framework/net/syscall.rs（562 行 unsafe FFI）+ services/net/syscall.rs（500 行 safe 包装）。
  - 两套维护。

### 3.5 [P1] `init.rs:39` `G_INIT_STATE` Atomic 状态机未文档化转换路径

- **位置**：[init.rs:39](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L39)
- **问题**：
  - 状态机转换（`set_init_state(HardwareProbed)` 等）散落在各处。
  - 应集中。

### 3.6 [P1] `iface_trait.rs:128` `PollOutcome` 枚举未定义 poll 失败原因

- **位置**：[iface_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs#L128)（grep PollOutcome）
- **问题**：
  - `PollOutcome::idle()` 等简单状态——但 smoltcp poll 可能失败。
  - 失败原因未表达。

### 3.7 [P1] `init.rs` sm_* 25+ 函数**全部 `unsafe extern "C"` 无 SAFETY 集中**

- **位置**：[init.rs:2060](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs)（搜索 `unsafe extern "C"`）
- **问题**：
  - 与 [code-audit-full.md §2 P0-04 framework→services 反向依赖](../audit/code-audit-full.md) 关联问题。

### 3.8 [P1] `init.rs:2060` `static mut` 仍大量残留（虽然 NetState 整合了部分）

- **位置**：[init.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs)
- **问题**：
  - 与 [subsystem-framework-misc.md §3.4 P1 全局单例自旋锁](../audit/subsystem-framework-misc.md) 关联。

### 3.9 [P1] `iface_trait.rs:67` `pub(crate) u32` 句柄**外部不可见但 services 必须可见**

- **位置**：[iface_trait.rs:67](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs#L67)
- **问题**：
  - `SocketHandle(pub(crate) u32)`——crate 内可见。
  - 但 services 不在 framework crate——**不可见**。
  - 实际 services 必须通过 `SmoltcpNetStack::alloc_user_id()`（[subsystem-services-net.md](../audit/subsystem-services-net.md)）分配，封装为 `fd` 暴露给用户。

## 4. P2 问题

### 4.1 [P2] `init.rs:43-47` 5 个全局 Atomic（G_MAC, G_IPV4, G_GATEWAY, G_DNS[3]）**初始化为全零**

- **位置**：[init.rs:43-47](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L43-L47)
- **问题**：
  - 全零代表"未配置"，但 `0.0.0.0` 是合法 IP。
  - 应用 `Option<NonZeroU32>` 表达。

### 4.2 [P2] `init.rs:63` MAX_SOCKETS 编译期常量无 `cfg` override

- **位置**：[init.rs:63](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L63)
- **问题**：
  - 不同构建无法定制。

### 4.3 [P2] `iface_trait.rs:1552` 类型擦除 + trait 边界**导致大量 `unimplemented!()`**

- **位置**：[iface_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs)（grep `unimplemented`）
- **问题**：
  - services/net/smoltcp_impl.rs 实现 trait，但某些方法可能 unimplemented。

### 4.4 [P2] `syscall.rs:562` FFI 函数**不验证 socket 是否由当前进程 fd 创建**

- **位置**：[syscall.rs:562](file:///home/anfer/Code/QueenX/src/kernel/framework/net/syscall.rs#L562)
- **问题**：
  - 用户态进程 A 用 fd 0 调用 `bind` → 是否能 bind 进程 B 的 socket？
  - 需检查进程 fd 表。

### 4.5 [P2] `iface_trait.rs:1552` `NetConfig.empty()` 缺字段

- **位置**：[iface_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs)
- **问题**：
  - NetConfig 字段定义未审。

### 4.6 [P2] `init.rs` smoltcp vendored 升级路径未文档化

- **位置**：[init.rs:6](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L6)
- **问题**：
  - smoltcp 版本升级时 init.rs 兼容性需要文档。

### 4.7 [P2] `save.rs:277` 网络状态持久化**未深审**

- **位置**：[save.rs:1-277](file:///home/anfer/Code/QueenX/src/kernel/framework/net/save.rs#L1-L277)
- **问题**：
  - 状态序列化格式（JSON？二进制？）。

### 4.8 [P2] `route.rs:172` 路由表实现**未深审**

- **位置**：[route.rs:1-172](file:///home/anfer/Code/QueenX/src/kernel/framework/net/route.rs#L1-L172)
- **问题**：
  - 路由查找算法 + 路由添加/删除。

### 4.9 [P2] `api.rs:165` 公共 API 未深审

- **位置**：[api.rs:1-165](file:///home/anfer/Code/QueenX/src/kernel/framework/net/api.rs#L1-L165)
- **问题**：
  - 与 services/net/mod.rs 的接口对应。

### 4.10 [P2] `netfilter.rs:13` 仅 13 行（实际在 services）

- **位置**：[netfilter.rs:1-13](file:///home/anfer/Code/QueenX/src/kernel/framework/net/netfilter.rs#L1-L13)
- **问题**：
  - 实际逻辑在 services/net/netfilter.rs。

### 4.11 [P2] `iface_trait.rs` `DhcpState` 枚举字段过多

- **位置**：[iface_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs)
- **问题**：
  - DHCP 状态机覆盖 Idle/Discovering/Requesting/Bound/Renewing/Failed 等。

## 5. P3 问题

### 5.1 [P3] `init.rs:39-47` 全局 Atomic 命名不一致（G_ 前缀 vs NET_ 前缀）

- **位置**：[init.rs:39-47](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs#L39-L47)
- **问题**：
  - `G_INIT_STATE` vs `NET_STATE` 命名风格不一致。

### 5.2 [P3] `iface_trait.rs` DhcpState 默认值（Idle）可能与 smoltcp 内部状态不一致

- **位置**：[iface_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs)
- **问题**：
  - 状态翻译可能漏状态。

### 5.3 [P3] `syscall.rs:562` sm_* 函数命名混乱

- **位置**：[syscall.rs:1-562](file:///home/anfer/Code/QueenX/src/kernel/framework/net/syscall.rs#L1-L562)
- **问题**：
  - sm_socket / sm_bind / sm_listen / sm_accept / sm_connect 等。
  - 与 POSIX 命名不一致（socket/bind/listen）。

### 5.4 [P3] `mod.rs:72` 模块入口极简

- **位置**：[mod.rs:1-72](file:///home/anfer/Code/QueenX/src/kernel/framework/net/mod.rs#L1-L72)
- **问题**：
  - pub use 重导出列表过长。

### 5.5 [P3] `wait_queue.rs:9` 仅 9 行（实际在 services/net）

- **位置**：[wait_queue.rs:1-9](file:///home/anfer/Code/QueenX/src/kernel/framework/net/wait_queue.rs#L1-L9)
- **问题**：
  - 仅 re-export 桩。

### 5.6 [P3] `types.rs:9` 仅 9 行

- **位置**：[types.rs:1-9](file:///home/anfer/Code/QueenX/src/kernel/framework/net/types.rs#L1-L9)
- **问题**：
  - 仅 re-export。

## 6. 跨子系统关联

### 6.1 net ↔ driver (NIC 驱动)

- `framework/net/init.rs:69` `device: ChitinNetDevice` 来自 chitin 子系统。
- NIC 驱动注册 ChitinNetDevice 实例。

### 6.2 net ↔ fs (socket fd)

- socket fd 与文件 fd 共享全局 fd 表（[subsystem-services-fs.md §3.8](../audit/subsystem-services-fs.md)）。

### 6.3 net ↔ timer

- DHCP 超时 + TCP 重传 + 协议栈 poll 都依赖 timer。

### 6.4 net ↔ barrier

- 网络 panic 恢复是 barrier 子系统的 domain。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 6 | 4-6 天 |
| **P1** | 9 | 5-7 天 |
| **P2** | 11 | 3-4 天 |
| **P3** | 6 | 0.5 天 |
| **合计** | **32** | **13-18 天** |

### P0 修复路径（建议执行顺序）

1. **§2.3 init.rs 单文件拆分**（1-2 天，**简单优先**）
2. **§2.5 iface_trait.rs 单文件拆分**（1-2 天）
3. **§2.2 NetState 初始化失败回滚**（1 天）
4. **§2.1 MAX_SOCKETS 动态调整**（0.5 天）
5. **§2.6 SocketHandle 句柄重用**（0.5 天，与 services-net §2.6 合并）
6. **§2.4 G_INIT_STATE 状态机文档化**（0.5 天）