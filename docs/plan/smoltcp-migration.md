# lwIP → smoltcp 迁移工程书

> **版本**: v1.0
> **日期**: 2026-05-31
> **目标**: 将网络协议栈从 lwIP (C) 替换为 smoltcp 0.13.0 (Rust)，实现全 Rust 内核网络子系统
> **状态**: 规划阶段

---

## 一、工程背景与动机

### 1.1 当前架构

```
┌─────────────────────────────────────────────────────┐
│                    syscall 层                        │
│  sys_socket / sys_bind / sys_listen / sys_connect   │
│  sys_sendto / sys_recvfrom / sys_shutdown           │
│  sys_sendmsg / sys_recvmsg / sys_[gs]etsockopt      │
├─────────────────────────────────────────────────────┤
│                  Rust FFI 桥接层                     │
│  types_ffi.rs    ← extern "C" lwip_socket / bind …  │
│  types.rs        ← LwipErr 枚举 / NET_READY 原子    │
│  apps.rs         ← HTTP/DNS/mDNS/MQTT/SNTP/Ping     │
│  netif.rs        ← netif_add / DHCP / status cb     │
│  init.rs         ← qx_net_init() 状态机              │
│  sys_arch.rs     ← Semaphore / Mutex / Mailbox       │
├─────────────────────────────────────────────────────┤
│                  C 桥接层 (net_glue.c)                │
│  qx_netif_init()  /  qx_rx_packet()                 │
│  qx_pbuf_copyout()  /  qx_netif_ip4_addr_u32()      │
│  e1000_send()     /  virtio_net_send()  ← FFI 符号   │
├─────────────────────────────────────────────────────┤
│               lwIP C 协议栈 (~50 .c 文件)            │
│  core/ ipv4/ ipv6/ api/ apps/ netif/                │
├─────────────────────────────────────────────────────┤
│               Rust 网卡驱动                          │
│  driver/e1000.rs    driver/virtio/net.rs              │
└─────────────────────────────────────────────────────┘
```

### 1.2 替换动机

| 维度 | lwIP (现状) | smoltcp (目标) |
|------|------------|----------------|
| **语言** | C (~50 源文件，需 cc 编译) | 纯 Rust (单一 crate) |
| **FFI 开销** | `extern "C"` 跨语言调用每包 | 零 FFI，直接 trait 调用 |
| **类型安全** | `*mut c_void` 裸指针 | Rust 类型系统 + 生命周期 |
| **内存模型** | 独立 pbuf 内存池 | Rust `Vec<u8>` + 借用 |
| **构建系统** | cc 编译 + ar 链接 lwIP | cargo 统一管理 |
| **安全检查** | 无安全保障（C 代码） | Rust 编译期安全保证 |
| **调试体验** | C 代码无法单步混编调试 | Rust-gdb 统一调试 |
| **许可证** | BSD-3 | 0BSD (更宽松) |
| **维护负担** | 需维护 ~70KB 的 C patch 文件 | 零下游补丁 |

### 1.3 风险提示

| 风险 | 影响 | 缓解方案 |
|------|------|----------|
| smoltcp 缺少 lwIP 应用层 (HTTP/MQTT/mDNS) | 需自实现或裁剪 | 优先保留核心协议栈，应用层独立实现 |
| smoltcp socket API 差异 | syscall 接口需重构 | 设计统一抽象层 `NetSocket` |
| 性能差异 | 吞吐量可能变化 | Phase 4 进行基准测试对比 |
| 时间投入 | 约 10-15 人天 | 分阶段交付，每阶段可独立验证 |

---

## 二、smoltcp 0.13.0 能力评估

### 2.1 核心协议支持

| 协议层 | smoltcp 支持 | lwIP 当前使用 | 匹配 |
|--------|:--:|:--:|:--:|
| Ethernet II | ✅ | ✅ | ✅ |
| ARP | ✅ | ✅ | ✅ |
| IPv4 (fragmentation) | ✅ | ✅ | ✅ |
| IPv6 (ND, SLAAC) | ✅ | ✅ | ✅ |
| ICMPv4 / ICMPv6 | ✅ | ✅ | ✅ |
| UDP | ✅ | ✅ | ✅ |
| TCP | ✅ | ✅ | ✅ |
| DHCPv4 client | ✅ | ✅ | ✅ |
| DNS client | ✅ | ✅ | ✅ |
| IGMP / MLDv6 | ✅ | ✅ | ✅ |
| 6LoWPAN | ✅ | ❌ 未使用 | — |

### 2.2 应用层协议（需重新实现）

| 应用 | lwIP 现状 | smoltcp 原生 | 迁移策略 |
|------|-----------|:--:|------|
| HTTP Server | lwIP httpd | ❌ | Rust 重新实现（~300 行） |
| mDNS Responder | lwIP mdns | ❌ | Rust 重新实现（~400 行） |
| MQTT Client | lwIP mqtt | ❌ | Rust 重新实现（~500 行） |
| SNTP Client | lwIP sntp | ❌ | Rust 重新实现（~150 行） |
| SMTP Client | lwIP smtp | ❌ | 延迟到 Phase 5 |
| TFTP | lwIP tftp | ❌ | 延迟到 Phase 5 |
| SNMP | lwIP snmp | ❌ | 延迟到 Phase 5 |
| NetBIOS NS | lwIP netbiosns | ❌ | 弃用（Windows 已放弃 NetBIOS） |
| lwiperf | lwIP perf | ❌ | 弃用 |

### 2.3 smoltcp 关键 API

```rust
// 网卡抽象 — 替代 lwIP netif + linkoutput
trait Device {
    type RxToken<'a>: RxToken;
    type TxToken<'a>: TxToken;
    fn receive(&mut self, timestamp: Instant) -> Option<Self::RxToken<'_>>;
    fn transmit(&mut self, timestamp: Instant) -> Option<Self::TxToken<'_>>;
    fn capabilities(&self) -> DeviceCapabilities;
}

// 协议栈实例
struct Interface<'a> { … }
impl Interface {
    fn new(config: Config, device: &mut dyn Device, now: Instant) -> Self;
    fn poll(&mut self, now: Instant, device: &mut dyn Device) -> Option<Event>;
    fn sockets(&mut self) -> &mut SocketSet<'_>;
}

// Socket 类型
enum Socket<'a> {
    Tcp(TcpSocket<'a>),
    Udp(UdpSocket<'a>),
    Icmp(IcmpSocket<'a>),
    Raw(RawSocket<'a>),
    Dns(DnsSocket<'a>),
    Dhcpv4(Dhcpv4Socket),
}
```

---

## 三、架构设计

### 3.1 目标架构

```
┌─────────────────────────────────────────────────────┐
│                    syscall 层 (不变)                  │
│  sys_socket / sys_bind / sys_listen / sys_connect   │
│  sys_sendto / sys_recvfrom / sys_shutdown           │
│          ↓ 调用 NetSocket API (新)                   │
├─────────────────────────────────────────────────────┤
│               net::sockets (新增)                    │
│  NetSocketSet  ← 封装 smoltcp SocketSet              │
│  TcpSocket / UdpSocket / DnsSocket                  │
│  poll() / send() / recv() / bind() / connect()       │
│  socket_id → SocketHandle 映射表                     │
├─────────────────────────────────────────────────────┤
│               net::iface (新增)                      │
│  AntxInterface  ← 封装 smoltcp Interface             │
│  DHCP 管理 / DNS 解析 / IP 配置                      │
│  poll() 驱动 → Device trait impl                     │
├─────────────────────────────────────────────────────┤
│               net::init (重写)                       │
│  qx_net_init()  ← 创建 Interface + SocketSet         │
│  状态机: Uninitialized → DeviceProbed → Ready        │
├─────────────────────────────────────────────────────┤
│            smoltcp 协议栈 (crate 依赖)                │
│  iface / socket / wire / phy                         │
├─────────────────────────────────────────────────────┤
│         Rust 网卡 Device trait impl (新增)            │
│  driver/e1000.rs  + impl Device for E1000Device      │
│  driver/virtio/net.rs  + impl Device for VirtioNet    │
│  driver/device.rs  ← SmolDevice<T> 统一包装 (可选)    │
└─────────────────────────────────────────────────────┘
```

### 3.2 移除清单

| 移除项 | 文件/目录 | 理由 |
|--------|-----------|------|
| lwIP C 源码 | `src/kernel/net/lwip/` 全部 | 替换为 smoltcp crate |
| C 桥接代码 | `src/kernel/net/arch/net_glue.c` | qx_rx_packet → 直接调用 Device |
| C 头文件 | `src/kernel/net/arch/*.h` | 不再需要 cc 编译 |
| Rust FFI 桥接 | `src/kernel/net/types_ffi.rs` | 无 extern "C" 需要 |
| lwIP 错误码 | `LwipErr` 枚举 | 替换为 smoltcp Error |
| OS 抽象层 | `sys_arch.rs` (Mailbox/Sem/Mutex) | smoltcp 不依赖外部 OSAL |
| lwIP 初始化 | `init.rs` 中 lwip_init() 调用 | 替换为 smoltcp init |
| lwIP DHCP | `netif.rs` 中 netif_add/dhcp_start | 替换为 smoltcp dhcp |
| HTTP 静态数据 | `fsdata.rs` | 随 lwIP httpd 移除 |
| lwIP 应用 | `apps.rs` 大部分 FFI 调用 | 替换为 smoltcp socket API |
| Makefile C 规则 | `NET_CORE_C`, `NET_NETIF_C` 等 | 不再编译 C 源码 |

### 3.3 新增清单

| 新增项 | 文件 | 职责 |
|--------|------|------|
| smoltcp 依赖 | `Cargo.toml` | `smoltcp = { version = "0.13", default-features = false, features = [...] }` |
| Device trait impl (e1000) | `driver/e1000.rs` (追加) | `impl smoltcp::phy::Device for E1000Device` |
| Device trait impl (virtio) | `driver/virtio/net.rs` (追加) | `impl smoltcp::phy::Device for VirtioNet` |
| 接口管理 | `src/kernel/net/iface.rs` | `AntxInterface` 封装 + DHCP/DNS/poll |
| Socket 管理 | `src/kernel/net/sockets.rs` | `NetSocketSet` 封装 + fill/fd 映射 |
| 初始化（重写） | `src/kernel/net/init.rs` | 新状态机 |
| 时间管理（重写） | `src/kernel/net/types.rs` | sys_now 适配 smoltcp Instant |
| HTTP Server（新） | `src/kernel/net/http.rs` | 纯 Rust HTTP/1.0 服务器 |
| mDNS Responder（新） | `src/kernel/net/mdns.rs` | 纯 Rust mDNS |
| DHCP 事件（新增） | `src/kernel/net/dhcp.rs` | DHCP 状态管理 |

---

## 四、分阶段实施计划

### Phase 0: 基础设施准备 (1-2 天)

**目标**: smoltcp crate 编译通过，Device trait 实现完成

```
□ 0.1  解压 smoltcp-0.13.0.zip 到 src/kernel/net/smoltcp/
       (或作为 Cargo 依赖: smoltcp = "0.13")

□ 0.2  添加 smoltcp 到 Cargo.toml:
       [dependencies]
       smoltcp = { version = "0.13", default-features = false,
                   features = ["medium-ethernet", "proto-ipv4",
                               "proto-ipv6", "socket-raw",
                               "socket-icmp", "socket-udp",
                               "socket-tcp", "socket-dhcpv4",
                               "socket-dns", "async"] }

□ 0.3  为 E1000Device 实现 smoltcp::phy::Device trait
       - RxToken: 从 RX ring 读取数据
       - TxToken: 写入 TX ring
       - capabilities: 1500 MTU, checksum offload

□ 0.4  为 VirtioNet 实现 smoltcp::phy::Device trait
       - RxToken: 从 RX virtqueue 读取
       - TxToken: 写入 TX virtqueue

□ 0.5  cargo check 验证 Phase 0 编译
```

**验证**: `cargo check` 通过，无新增编译错误

---

### Phase 2: 协议栈核心替换 (1-2 天)

**目标**: smoltcp Interface 实例化，替代 lwIP init + netif

```
□ 2.1  创建 src/kernel/net/iface.rs
       - AntxInterface 结构体封装 smoltcp Interface
       - AntxInterface::new(device, config)
       - AntxInterface::poll() 驱动协议栈
       - DHCP 配置管理
       - DNS 服务器列表管理

□ 2.2  重写 init.rs
       - 移除 lwip_init()、sys_init()、e1000_probe() FFI 调用
       - 新初始化流程:
         1. 实例化 E1000Device / VirtioNet
         2. 创建 smoltcp Interface
         3. 创建 SocketSet
         4. 初始化 DHCP
         5. 设置 NET_READY = true
       - 保留状态机模式

□ 2.3  更新 types.rs
       - 保留 NET_READY AtomicBool
       - 移除 LwipErr (或重命名为 NetErr)
       - sys_now() → smoltcp time::Instant 适配
       - sys_tick_inc() 保留 (驱动 poll 时钟)

□ 2.4  更新 net/mod.rs 模块导出
       - 移除 lwip/ C 模块引用
       - 移除 types_ffi 导出
       - 新增 iface 导出

□ 2.5  更新 Makefile
       - 删除所有 NET_CORE_C / NET_NETIF_C / NET_APPS_C / NET_QX_C
       - 删除 lwIP include paths
       - 删除 net_glue.c 编译规则
```

**验证**: `cargo check` 通过，`make run-net` 启动 → klog 输出 "Network subsystem initialized"

---

### Phase 3: Socket 层与 Syscall 对接 (2-3 天)

**目标**: syscall 层可通过 smoltcp Socket API 进行网络通信

```
□ 3.1  创建 src/kernel/net/sockets.rs
       - NetSocketSet: 封装 smoltcp SocketSet + fd 映射表
       - allocate_fd() → socket 句柄分配
       - poll_all() → 驱动所有 socket 的收发
       - 类型安全的 TcpHandle / UdpHandle / DnsHandle

□ 3.2  实现 syscall 适配函数
       - net_socket(domain, type, protocol) → socket_id
       - net_bind(socket_id, addr, port)
       - net_listen(socket_id, backlog)
       - net_accept(socket_id) → new_socket_id
       - net_connect(socket_id, addr, port)
       - net_send(socket_id, buf, len)
       - net_recv(socket_id, buf, len)
       - net_close(socket_id)

□ 3.3  更新 syscall/mod.rs
       - 将 extern "C" fn lwip_* 替换为：
         net::sockets::net_socket() 等纯 Rust 调用
       - 移除 types_ffi.rs 中的 stub 函数

□ 3.4  移除 types_ffi.rs
       - 确认无其他模块引用后删除
```

**验证**: host-tests 新增 net 模块测试 (TCP echo, UDP echo)

---

### Phase 4: 网络应用层重实现 (2-3 天)

**目标**: 恢复 lwIP 应用层功能

```
□ 4.1  HTTP Server
       - src/kernel/net/http.rs
       - 基于 TcpSocket 实现 HTTP/1.0
       - 支持 GET / POST
       - 内嵌静态文件 (index.html / 404.html)
       - ~300 行纯 Rust

□ 4.2  Ping (ICMP Echo)
       - 重写 apps.rs 中 ping 函数
       - 基于 smoltcp IcmpSocket

□ 4.3  DNS Resolver
       - 基于 smoltcp DnsSocket
       - 包装为 net_dns_resolve(hostname) → IpAddress

□ 4.4  mDNS Responder
       - src/kernel/net/mdns.rs
       - 基于 UdpSocket + multicast
       - ~400 行纯 Rust

□ 4.5  SNTP Client
       - src/kernel/net/sntp.rs
       - 基于 UdpSocket
       - ~150 行纯 Rust
```

**验证**: 
- `curl http://localhost:8080/` → 返回 index.html
- `ping <antx-ip>` → 有响应

---

### Phase 5: 清理与优化 (1-2 天)

**目标**: 移除所有 lwIP 残余，最终构建验证

```
□ 5.1  删除残余文件
       - rm -rf src/kernel/net/lwip/
       - rm -rf src/kernel/net/arch/
       - rm src/kernel/net/fsdata.rs
       - 确认 apps.rs 中无 lwip FFI 引用

□ 5.2  net/mod.rs 最终清理
       - 移除 sys_arch 模块
       - 移除 fsdata 模块
       - 审查所有 use 语句

□ 5.3  Makefile 清理
       - 移除所有 lwIP 相关的 make 变量和目标
       - 确认 QEMU_NET 配置不变

□ 5.4  可选的延迟应用
       - MQTT Client (src/kernel/net/mqtt.rs)
       - SMTP Client (src/kernel/net/smtp.rs)
       - TFTP (src/kernel/net/tftp.rs)

□ 5.5  全面验证
       - cargo check / cargo clippy (0/0)
       - cargo test (全部通过)
       - make run-net 完整启动测试
       - DHCP 获取 IP 地址
       - HTTP 服务器可访问
       - Ping 响应正常
```

---

## 五、关键接口设计

### 5.1 Device trait 实现示例 (e1000)

```rust
impl<'a> smoltcp::phy::Device for &'a mut E1000Device {
    type RxToken<'b> = E1000RxToken<'b> where Self: 'b;
    type TxToken<'b> = E1000TxToken<'b> where Self: 'b;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.rx_ready() {
            let buf = self.rx_buffer();
            let len = self.rx_len();
            let rx = E1000RxToken { buf, len };
            let tx = E1000TxToken { device: self };
            Some((rx, tx))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.tx_ready() {
            Some(E1000TxToken { device: self })
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(64);
        caps.checksum = ChecksumCapabilities::ignored();
        caps
    }
}
```

### 5.2 Interface 创建与 poll

```rust
pub struct AntxInterface {
    iface: Interface,
    dhcp_handle: Option<Dhcpv4Handle>,
    sockets: NetSocketSet,
    device_held: bool,
}

impl AntxInterface {
    pub fn new(device: &mut impl Device, mac: [u8; 6]) -> Self {
        let config = Config::new(HardwareAddress::Ethernet(
            EthernetAddress::from_bytes(&mac)
        ));
        let now = Instant::from_millis(sys_now() as i64);
        let mut iface = Interface::new(config, device, now);
        iface.update_ip_addrs(|addrs| {
            addrs.push(IpCidr::new(IpAddress::v4(0,0,0,0), 0)).unwrap();
        });
        iface.routes_mut().add_default_ipv4_route().unwrap();

        let dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
        let mut sockets = SocketSet::new(vec![]);
        let dhcp_handle = sockets.add(dhcp_socket);

        Self {
            iface,
            dhcp_handle: Some(dhcp_handle),
            sockets: NetSocketSet::new(sockets),
            device_held: false,
        }
    }

    pub fn poll(&mut self, device: &mut impl Device) {
        let now = Instant::from_millis(sys_now() as i64);
        self.iface.poll(now, device, &mut self.sockets.inner);
        // 驱动 DHCP
        if let Some(h) = self.dhcp_handle {
            if let Some(event) = self.sockets.inner.get_mut::<Dhcpv4Socket>(h).poll() {
                match event {
                    Dhcpv4Event::Configured(config) => {
                        // 更新 IP、路由、DNS
                        self.on_dhcp_configured(config);
                        self.dhcp_handle = None;
                    }
                    _ => {}
                }
            }
        }
    }
}
```

### 5.3 Socket 抽象层

```rust
pub struct NetSocketSet {
    inner: SocketSet<'static>,
    next_fd: AtomicU32,
    fd_map: [Option<SocketHandle>; MAX_SOCKETS],
}

impl NetSocketSet {
    pub fn socket_tcp(&mut self) -> Result<i32, NetErr> {
        let tcp = TcpSocket::new(
            TcpSocketBuffer::new(vec![0; TCP_RX_BUF]),
            TcpSocketBuffer::new(vec![0; TCP_TX_BUF]),
        );
        let handle = self.inner.add(tcp);
        self.allocate_fd(handle)
    }

    pub fn socket_udp(&mut self) -> Result<i32, NetErr> {
        let udp = UdpSocket::new(
            UdpSocketBuffer::new(
                vec![UdpMetadata::EMPTY; UDP_RX_META],
                vec![0; UDP_RX_BUF],
            ),
            UdpSocketBuffer::new(
                vec![UdpMetadata::EMPTY; UDP_TX_META],
                vec![0; UDP_TX_BUF],
            ),
        );
        let handle = self.inner.add(udp);
        self.allocate_fd(handle)
    }
    // ...
}
```

---

## 六、构建系统变更

### 6.1 Makefile diff 概要

```diff
-NET_CFLAGS += -Isrc/kernel/net/lwip -Isrc/kernel/net/lwip/src/include ...
-NET_CORE_C = $(wildcard src/kernel/net/lwip/src/core/*.c) ...
-NET_NETIF_C = src/kernel/net/lwip/src/netif/ethernet.c
-NET_APPS_C = src/kernel/net/lwip/src/apps/...
-NET_QX_C   = src/kernel/net/arch/net_glue.c
-NET_ALL_C = $(NET_CORE_C) $(NET_NETIF_C) $(NET_APPS_C) $(NET_QX_C)
-NET_OBJS = $(patsubst src/kernel/net/%.c,build/net/%.o,$(NET_ALL_C))
# 删除 ~20 行 C 编译规则

+NET_RS_SRC = $(wildcard src/kernel/net/*.rs) $(wildcard src/kernel/net/driver/*.rs)
+# smoltcp 作为 cargo 依赖，无需 Makefile 特殊处理
```

### 6.2 smoltcp feature 选择

```toml
[dependencies]
smoltcp = { version = "0.13", default-features = false, features = [
    "medium-ethernet",    # 1500 MTU + 128 socket slots (折中选择)
    "proto-ipv4",         # IPv4
    "proto-ipv6",         # IPv6
    "socket-raw",         # Raw socket (Ping)
    "socket-icmp",        # ICMP socket (Ping)
    "socket-udp",         # UDP socket (DNS/mDNS/DHCP)
    "socket-tcp",         # TCP socket (HTTP)
    "socket-dhcpv4",      # DHCPv4 client
    "socket-dns",         # DNS client
]}
```

---

## 七、风险评估与缓解

| # | 风险 | 概率 | 影响 | 缓解措施 |
|---|------|:--:|:--:|------|
| R1 | smoltcp 缺少 lwIP 特定应用 (mDNS/MQTT) | 高 | 中 | Rust 重新实现，Phase 4 交付 |
| R2 | TCP 性能不如 lwIP | 中 | 中 | Phase 4 benchmark；smoltcp 0.13 对 TCP 有显著优化 |
| R3 | Device trait 实现复杂 (e1000/virtio) | 中 | 高 | 参考 smoltcp 官方 examples + 现有 Rust 驱动代码 |
| R4 | socket API 语义不一致 | 低 | 高 | Phase 3 编写连接测试验证语义 |
| R5 | DHCP 行为差异 | 低 | 中 | smoltcp dhcpv4 socket 成熟稳定 |
| R6 | 内核 `no_std` + `alloc` 兼容性 | 低 | 高 | smoltcp 原生支持 `no_std`，已验证 |

---

## 八、验收标准

| 阶段 | 验收标准 |
|------|----------|
| Phase 0 | `cargo check` 通过；Device trait 编译通过 |
| Phase 2 | `make run-net` 启动 → klog 显示 "Network subsystem initialized"；Interface 实例化无 panic |
| Phase 3 | TCP echo client/server host-test 通过；UDP echo 通过 |
| Phase 4 | `curl http://localhost:8080/` 返回 HTTP 响应；ping 有回复 |
| Phase 5 | `cargo clippy` 0/0；`cargo test` 182/182；`make clean && make run-net` 完整启动；DHCP 获取 IP；HTTP 可访问 |

---

## 九、工时估算

| 阶段 | 内容 | 估算 |
|------|------|:--:|
| Phase 0 | smoltcp 引入 + Device trait | 1-2 天 |
| Phase 2 | Interface + init 重写 | 1-2 天 |
| Phase 3 | Socket 层 + syscall | 2-3 天 |
| Phase 4 | 应用层重实现 | 2-3 天 |
| Phase 5 | 清理 + 验证 | 1-2 天 |
| **合计** | | **7-12 人天** |

---

## 十、后续演进

1. **全异步网络**: 启用 smoltcp `async` feature + 内核 async executor
2. **TLS 支持**: 在 TCP 层之上集成 `rustls` (纯 Rust TLS)
3. **HTTP/2**: 替换 HTTP/1.0 为 HTTP/2
4. **WebSocket**: 基于 TCP 实现 WebSocket 升级
5. **QUIC**: 在 UDP 之上集成 `quinn` 或 `s2n-quic`
