# lwIP 2.2.1 嵌入 QX 网络子系统工程规划

> 版本: 1.0  
> 日期: 2026-05-04  
> 状态: 规划阶段  
> 目标: 将 lwIP 2.2.1 完整集成到 AntX (QX) 内核，构建现代网络栈

---

## 一、工程范围

### 1.1 lwIP 2.2.1 模块清单

lwIP 2.2.1 包含 **203 个 C 源文件** + **199 个头文件**，按目录结构分类：

| 目录 | C文件 | 头文件 | 内容 |
|------|-------|--------|------|
| `src/core/` | 17 | — | 核心协议栈 (init/mem/memp/pbuf/netif/dns/tcp/udp/ip/raw/def/timeouts/sys/stats/altcp) |
| `src/core/ipv4/` | 11 | — | IPv4 子协议 (ip4/dhcp/arp/icmp/igmp/autoip/acd/frag/addr) |
| `src/core/ipv6/` | 10 | — | IPv6 子协议 (ip6/dhcp6/nd6/mld6/icmp6/ethip6/frag/inet6) |
| `src/api/` | 9 | — | Socket / Netconn API (sockets/api_lib/api_msg/tcpip/netbuf/netdb/netifapi/err/if_api) |
| `src/netif/` | 8 | — | 网络接口层 (ethernet/slipif/lowpan6/lowpan6_common/lowpan6_ble/zepif/bridgeif/bridgeif_fdb) |
| `src/netif/ppp/` | 30 | — | PPP/PPPoE/PPPoL2TP 协议族 (ppp/lcp/ipcp/ipv6cp/ccp/eap/chap/auth/fsm/upap/vj/mppe/eui64/pppos/pppoe/pppol2tp) |
| `src/apps/altcp_tls/` | 2 | — | altcp TLS 抽象层 (mbedtls) |
| `src/apps/http/` | 6 | — | HTTP 服务器 + 客户端 (httpd/http_client/fs/fsdata/altcp_proxyconnect) |
| `src/apps/mdns/` | 3 | — | mDNS 多播 DNS |
| `src/apps/mqtt/` | 1 | — | MQTT 客户端 |
| `src/apps/netbiosns/` | 1 | — | NetBIOS 名称服务 |
| `src/apps/smtp/` | 1 | — | SMTP 邮件客户端 |
| `src/apps/snmp/` | 21 | — | SNMPv1/v2c/v3 代理 + MIB2 |
| `src/apps/sntp/` | 1 | — | SNTP 时间同步 |
| `src/apps/tftp/` | 1 | — | TFTP 客户端/服务器 |
| `src/apps/lwiperf/` | 1 | — | 网络性能测试 |
| `src/include/` | — | 199 | 所有公开/内部/协议头文件 + POSIX 兼容层 |

### 1.2 集成后 QX 网络子系统容量

| 指标 | 数值 |
|------|------|
| C 源文件总数 | 203 |
| 头文件总数 | 199 |
| 新增 NIC 驱动 (e1000) | ~1,500 行 |
| 移植层 (sys_arch) | ~500 行 |
| 预计总代码行数 | ~300,000 行 (含注释) |
| 预计内核二进制增加 | ~200-400 KB |

---

## 二、目录结构规划

```
src/net/                          # QX 网络子系统根目录
├── mod.rs                        # Rust 模块声明 (如需要)
├── qx_net.h                      # QX 网络子系统总头文件
│
├── lwip/                         # lwIP 2.2.1 源码 (只读区域)
│   ├── lwipopts.h                # ★ QX 定制 lwIP 配置 (核心文件)
│   ├── src/
│   │   ├── core/                 # → lwip-2.2.1/src/core/ (17 .c)
│   │   ├── core/ipv4/            # → lwip-2.2.1/src/core/ipv4/ (11 .c)
│   │   ├── core/ipv6/            # → lwip-2.2.1/src/core/ipv6/ (10 .c)
│   │   ├── api/                  # → lwip-2.2.1/src/api/ (9 .c)
│   │   ├── netif/                # → lwip-2.2.1/src/netif/ (8 .c)
│   │   ├── netif/ppp/            # → lwip-2.2.1/src/netif/ppp/ (30 .c)
│   │   ├── apps/http/            # → lwip-2.2.1/src/apps/http/ (6 .c)
│   │   ├── apps/mdns/            # → lwip-2.2.1/src/apps/mdns/ (3 .c)
│   │   ├── apps/mqtt/            # → lwip-2.2.1/src/apps/mqtt/ (1 .c)
│   │   ├── apps/netbiosns/       # → lwip-2.2.1/src/apps/netbiosns/ (1 .c)
│   │   ├── apps/smtp/            # → lwip-2.2.1/src/apps/smtp/ (1 .c)
│   │   ├── apps/snmp/            # → lwip-2.2.1/src/apps/snmp/ (21 .c)
│   │   ├── apps/sntp/            # → lwip-2.2.1/src/apps/sntp/ (1 .c)
│   │   ├── apps/tftp/            # → lwip-2.2.1/src/apps/tftp/ (1 .c)
│   │   ├── apps/lwiperf/         # → lwip-2.2.1/src/apps/lwiperf/ (1 .c)
│   │   └── include/              # → lwip-2.2.1/src/include/ (199 .h)
│   └── lwip-version.h            # 版本记录
│
├── arch/                         # ★ QX 平台移植层
│   ├── sys_arch.c                # 信号量/邮箱/线程/互斥锁实现
│   ├── sys_arch.h                # 移植层类型定义 (sys_sem_t/sys_mbox_t/sys_thread_t)
│   └── cc.h                      # 编译器/平台类型定义
│
├── driver/                       # ★ 网卡驱动
│   ├── e1000.c                   # Intel E1000 PCI 网卡驱动
│   ├── e1000.h                   # E1000 寄存器定义
│   ├── e1000_regs.h              # E1000 寄存器偏移宏
│   └── rtl8139.c                 # Realtek RTL8139 (可选, Phase 2)
│
├── qx_net_init.c                 # 网络子系统初始化
├── qx_netif.c                    # QX netif 适配 (将 E1000 挂到 lwIP)
└── qx_sockets.c                  # QX 系统调用 → lwIP Socket API 桥接
```

---

## 三、模块启用决策

### 3.1 全部启用 ✅

| 模块 | 理由 |
|------|------|
| **core/** (完整) + ipv4/ipv6 | 基础协议栈，必须 |
| **api/** (完整) | Socket/Netconn API 提供给用户态 |
| **netif/ethernet.c** | 以太网帧处理 |
| **apps/http/** | HTTP 服务器/客户端，内核仪表盘 |
| **apps/mdns/** | 本地网络服务发现 |
| **apps/mqtt/** | IoT 协议支持 |
| **apps/netbiosns/** | Windows 名称解析 |
| **apps/smtp/** | 邮件告警 |
| **apps/snmp/** (完整) | SNMPv1/v2c/v3 网络管理 |
| **apps/sntp/** | 时间同步 |
| **apps/tftp/** | 简单文件传输 |
| **apps/lwiperf/** | 网络性能基准 |

### 3.2 条件启用 ⚠️

| 模块 | 条件 |
|------|------|
| **altcp_tls** | 需 mbedtls，Phase 3 启用 |
| **netif/ppp/** (全部30文件) | PPP 协议族。先编入但不初始化，通过 lwipopts.h 开关控制 |
| **netif/slipif.c** | 串口 IP，Phase 3 启用 |
| **netif/lowpan6\*** | 6LoWPAN，硬件依赖，Phase 4 |
| **netif/zepif.c** | ZigBee，硬件依赖，Phase 4 |

### 3.3 暂不启用 ❌

| 模块 | 原因 |
|------|------|
| **contrib/addons/tcp_isn** | ISO 标准 TCP ISN，内核级复杂度 |
| **contrib/addons/tcp_md5** | TCP MD5 签名 (BGP)，需 OpenSSL |
| **contrib/apps/chargen** | 调试工具，非核心 |
| **contrib/apps/httpserver** (netconn版) | 已有 httpd raw API 版 |
| **contrib/addons/netconn/external_resolve** | DNS-SD，实验性 |

---

## 四、lwipopts.h 配置策略

### 4.1 内存配置

```c
#define MEM_LIBC_MALLOC          0   // 使用 lwIP 内部内存池
#define MEMP_MEM_MALLOC           0   // 使用静态 memp 数组
#define MEM_ALIGNMENT             4   // 4 字节对齐 (内核栈已 16B 对齐)
#define MEM_SIZE                  (64 * 1024)       // 堆内存 64KB
#define MEMP_NUM_PBUF             32   // pbuf 数量
#define MEMP_NUM_UDP_PCB          16   // UDP PCB
#define MEMP_NUM_TCP_PCB          32   // TCP PCB
#define MEMP_NUM_TCP_PCB_LISTEN   8    // 监听 TCP PCB
#define MEMP_NUM_TCP_SEG          256  // TCP 分段
#define PBUF_POOL_SIZE            64   // pbuf 池大小
#define PBUF_POOL_BUFSIZE         1536 // 单 pbuf 缓冲区 (MTU)
```

### 4.2 协议启用

```c
// IPv4 — 全启用
#define LWIP_IPV4                 1
#define LWIP_ARP                  1
#define LWIP_ICMP                 1
#define LWIP_IGMP                 1
#define LWIP_AUTOIP               1   // 链路本地地址
#define LWIP_DHCP                 1   // DHCP 客户端
#define LWIP_ACD                  1   // IPv4 地址冲突检测
#define LWIP_DNS                  1

// IPv6 — 全启用
#define LWIP_IPV6                 1
#define LWIP_IPV6_DHCP6           1   // DHCPv6
#define LWIP_IPV6_MLD             1
#define LWIP_ND6                  1   // 邻居发现
#define LWIP_IPV6_AUTOCONFIG      1   // SLAAC

// 传输层 — 全启用
#define LWIP_TCP                  1
#define LWIP_UDP                  1
#define LWIP_RAW                  1   // Raw socket

// 应用层 — 全启用
#define LWIP_HTTPD                1
#define LWIP_HTTPD_CGI            1
#define LWIP_HTTPD_SSI            1
#define LWIP_HTTP_CLIENT          1
#define LWIP_MDNS                 1
#define LWIP_MQTT                 1
#define LWIP_NETBIOSNS            1
#define LWIP_SMTP                 1
#define LWIP_SNMP                 1
#define LWIP_SNTP                 1
#define LWIP_TFTP                 1
#define LWIP_LWIPERF              1

// PPP — 可选启用
#define PPP_SUPPORT               1   // Phase 3 启用

// Socket API
#define LWIP_NETCONN              1
#define LWIP_SOCKET               1
#define LWIP_COMPAT_SOCKETS       0   // 不开启 POSIX 兼容 (使用自己的桥接)

// 调试
#define LWIP_DEBUG                0
#define LWIP_STATS                1
#define LWIP_STATS_DISPLAY        1
```

### 4.3 网络接口配置

```c
#define LWIP_NETIF_HOSTNAME       1
#define LWIP_NETIF_API            1
#define LWIP_NETIF_STATUS_CALLBACK 1
#define LWIP_NETIF_LINK_CALLBACK  1
#define LWIP_NETIF_REMOVE_CALLBACK 1
#define LWIP_NETIF_HWADDRHINT     1
#define LWIP_NETIF_TX_SINGLE_PBUF 1
```

---

## 五、移植层 (sys_arch) 设计

### 5.1 类型映射

lwIP `sys.h` 要求的平台抽象 → QX 实现：

| lwIP 类型 | QX 实现 |
|-----------|---------|
| `sys_sem_t` | `mutex_t` (用 `mutex_lock/unlock`) |
| `sys_mbox_t` | 基于 `wait_queue_t` + 环形缓冲区 |
| `sys_thread_t` | `proc_create_internal()` + `user_proc_load_elf()` |
| `sys_prot_t` | `spinlock_t` (中断临界区保护) |
| `sys_mutex_t` | `mutex_t` |
| `u32_t` / `s32_t` | `uint32_t` / `int32_t` (已定义) |
| `u16_t` / `s16_t` | `uint16_t` / `int16_t` |
| `u8_t` / `s8_t` | `uint8_t` / `int8_t` |
| `mem_ptr_t` | `uintptr_t` |

### 5.2 信号量实现 (sys_arch.c)

```c
// sys_sem_t → mutex_t 封装
typedef mutex_t sys_sem_t;
#define sys_sem_new()     ({ mutex_t m = MUTEX_INIT(sem); m; })
#define sys_sem_free(s)   // no-op (静态分配)
#define sys_arch_sem_wait(s, timeout)  mutex_lock_timeout(s, timeout)
#define sys_sem_signal(s)              mutex_unlock(s)
```

### 5.3 邮箱实现

```c
#define MBOX_MAX_MSGS  32
typedef struct {
    wait_queue_t wq;
    spinlock_t lock;
    void *messages[MBOX_MAX_MSGS];
    int head, tail, count;
} sys_mbox_t;
```

### 5.4 定时器集成

```c
// 在 timer_isr 中调用:
//   sys_check_timeouts();   // lwIP 超时处理 (每 250ms 一次)
// 
// 通过 QX timer_init 注册:
//   idt_register_irq(IRQ_TIMER, timer_handler);
```

### 5.5 线程实现

lwIP `tcpip_thread` 作为一个内核线程:
```c
void tcpip_thread_main(void) {
    LOCK_TCPIP_CORE();
    tcpip_init(NULL, NULL);
    UNLOCK_TCPIP_CORE();
    while (1) {
        sys_check_timeouts();
        ethernetif_input(netif);  // 轮询 NIC
    }
}
```

---

## 六、NIC 驱动 (E1000) 设计

### 6.1 PCI 发现

```
pci_find_class(PCI_CLASS_NETWORK, 0x00 (Ethernet), &dev)
  → BAR0 → MMIO 基地址
  → IRQ pin → IDT 中断号
```

### 6.2 寄存器映射

```
E1000_CTRL    (0x0000)  — 设备控制
E1000_STATUS  (0x0008)  — 设备状态
E1000_EERD    (0x0014)  — EEPROM 读 (获取 MAC)
E1000_RDBAL   (0x2800)  — 接收描述符基址 (低32位)
E1000_RDBAH   (0x2804)  — 接收描述符基址 (高32位)
E1000_RDLEN   (0x2808)  — 接收描述符环长度
E1000_RDH     (0x2810)  — 接收描述符头指针
E1000_RDT     (0x2818)  — 接收描述符尾指针
E1000_TDBAL   (0x3800)  — 发送描述符基址
E1000_TDLEN   (0x3808)  — 发送描述符环长度
E1000_TDH     (0x3810)  — 发送描述符头指针
E1000_TDT     (0x3818)  — 发送描述符尾指针
E1000_ICR     (0x00C0)  — 中断原因
E1000_ICS     (0x00C8)  — 中断设置
E1000_IMS     (0x00D0)  — 中断掩码
E1000_RCTL    (0x0100)  — 接收控制
E1000_TCTL    (0x0400)  — 发送控制
```

### 6.3 发送/接收环形队列

```
TX Ring: 256 描述符 × 16 字节 = 4 KB
RX Ring: 256 描述符 × 16 字节 = 4 KB
RX Buffers: 256 × 2048 字节 = 512 KB
合计: ~520 KB (通过 kmalloc 分配)
```

### 6.4 中断处理

```
e1000_isr:
  读取 ICR (中断原因)
  if (接收中断):
     遍历 RX Ring, 将帧提交给 lwIP ethernet_input()
  if (发送完成中断):
     回收 TX 描述符
  写 ICR 清除中断
  发送 EOI
```

---

## 七、编译集成 (Makefile)

### 7.1 新增变量

```makefile
NET_CFLAGS = -Isrc/net/lwip/src/include -Isrc/net/arch -Isrc/net/driver

NET_CORE_C = $(wildcard src/net/lwip/src/core/*.c) \
             $(wildcard src/net/lwip/src/core/ipv4/*.c) \
             $(wildcard src/net/lwip/src/core/ipv6/*.c)

NET_API_C  = $(wildcard src/net/lwip/src/api/*.c)

NET_NETIF_C = src/net/lwip/src/netif/ethernet.c

NET_APPS_C = $(wildcard src/net/lwip/src/apps/http/*.c) \
             $(wildcard src/net/lwip/src/apps/mdns/*.c) \
             $(wildcard src/net/lwip/src/apps/mqtt/*.c) \
             $(wildcard src/net/lwip/src/apps/netbiosns/*.c) \
             $(wildcard src/net/lwip/src/apps/smtp/*.c) \
             $(wildcard src/net/lwip/src/apps/snmp/*.c) \
             $(wildcard src/net/lwip/src/apps/sntp/*.c) \
             $(wildcard src/net/lwip/src/apps/tftp/*.c) \
             $(wildcard src/net/lwip/src/apps/lwiperf/*.c)

NET_QX_C   = src/net/arch/sys_arch.c \
             src/net/driver/e1000.c \
             src/net/qx_net_init.c \
             src/net/qx_netif.c \
             src/net/qx_sockets.c

NET_OBJS   = $(NET_CORE_C:.c=.o) $(NET_API_C:.c=.o) $(NET_NETIF_C:.c=.o) \
             $(NET_APPS_C:.c=.o) $(NET_QX_C:.c=.o)
```

---

## 八、系统调用桥接

### 8.1 syscall 号映射

已预留的 81-88 (syscall.h):

| 编号 | 宏 | lwIP 对应 |
|------|-----|-----------|
| 81 | `SYS_NET_SOCKET` | `lwip_socket()` |
| 82 | `SYS_NET_BIND` | `lwip_bind()` |
| 83 | `SYS_NET_LISTEN` | `lwip_listen()` |
| 84 | `SYS_NET_ACCEPT` | `lwip_accept()` |
| 85 | `SYS_NET_CONNECT` | `lwip_connect()` |
| 86 | `SYS_NET_SEND` | `lwip_send()` |
| 87 | `SYS_NET_RECV` | `lwip_recv()` |
| 88 | `SYS_NET_SHUTDOWN` | `lwip_shutdown()` |

### 8.2 新增 syscall (扩展)

| 编号 | 宏 | 功能 |
|------|-----|------|
| 124 | `SYS_NET_SETSOCKOPT` | `lwip_setsockopt()` |
| 125 | `SYS_NET_GETSOCKOPT` | `lwip_getsockopt()` |
| 126 | `SYS_NET_GETPEERNAME` | `lwip_getpeername()` |
| 127 | `SYS_NET_GETHOSTBYNAME` | `lwip_gethostbyname()` |
| 128 | `SYS_NET_IFCONFIG` | 网络接口配置 |

---

## 九、初始化流程

```
kernel_main()
  ...
  MODULE_CHECK_VOID("Network", qx_net_init)
  ...

qx_net_init():
  ① sys_arch_init()           — 初始化移植层信号量/邮箱
  ② lwip_init()               — 初始化 lwIP 核心 (memp/pbuf/netif/tcp/udp/...)
  ③ e1000_probe()             — PCI 探测 + MMIO 映射 + MAC 读取
  ④ qx_netif_add()            — 将 e1000 注册为 lwIP netif (netif_add)
  ⑤ dhcp_start()              — 启动 DHCP 获取 IP (或使用静态 IP)
  ⑥ idt_register_irq(...)     — 注册 E1000 中断处理
  ⑦ sys_thread_new(tcpip_thread) — 启动 TCP/IP 线程
```

---

## 十、实施阶段

### Phase 1: 基础集成 (1-2 周)
- [ ] 解压 lwIP 到 `src/net/lwip/`
- [ ] 编写 `cc.h` / `sys_arch.h` / `lwipopts.h`
- [ ] 实现 `sys_arch.c` (信号量/邮箱/线程/定时器)
- [ ] 编写 `Makefile` 规则编译所有 lwIP core + api
- [ ] 链接验证 (内核二进制不崩溃)
- [ ] 测试: `lwip_init()` 成功初始化

### Phase 2: NIC 驱动 (1 周)
- [ ] E1000 寄存器头文件
- [ ] E1000 PCI 探测 + MMIO 映射
- [ ] 发送/接收环形队列
- [ ] 中断处理
- [ ] `qx_netif.c` 适配 lwIP netif 层
- [ ] 测试: QEMU `-netdev user -device e1000` 发包/收包

### Phase 3: 协议栈验证 (1 周)
- [ ] DHCP 获取 IP 地址
- [ ] ARP 解析验证
- [ ] ICMP Ping 回显 (QEMU user-mode 网络)
- [ ] TCP 回显服务器
- [ ] HTTP 服务器 (httpd) 内核仪表盘
- [ ] DNS 解析测试

### Phase 4: 用户态接入 (1 周)
- [ ] Socket 系统调用桥接
- [ ] 用户态 `int 0x80` → Socket API
- [ ] 用户态网络应用测试
- [ ] SNMP/MQTT/SMTP 应用层协议测试

### Phase 5: 高级特性 (1-2 周)
- [ ] PPP/PPPoE 启用
- [ ] altcp TLS (mbedtls)
- [ ] 多网卡支持
- [ ] 性能优化 (零拷贝 RX)

---

## 十一、QEMU 测试环境

### 11.1 开发测试

```bash
# 基础网络测试 (user-mode, 无需 root)
qemu-system-x86_64 -m 512 -kernel build/kernel.bin \
  -netdev user,id=n0,hostfwd=tcp::8080-:80 \
  -device e1000,netdev=n0

# 或使用 tap (需 root, 可访问外部网络)
qemu-system-x86_64 -m 512 -kernel build/kernel.bin \
  -netdev tap,id=n0,ifname=tap0,script=no,downscript=no \
  -device e1000,netdev=n0
```

### 11.2 网络功能测试矩阵

| 测试项 | QEMU user-mode | QEMU tap | 验证方法 |
|--------|---------------|----------|----------|
| DHCP 获取 IP | ✅ | ✅ | 日志输出 IP |
| ICMP Ping | ✅ | ✅ | host `ping 10.0.2.15` |
| TCP HTTP 服务 | ✅ | ✅ | host `curl localhost:8080` |
| DNS 解析 | ✅ | ✅ | `lwip_gethostbyname("example.com")` |
| MQTT 发布 | ❌ | ✅ | 外部 MQTT broker |
| SNMP 查询 | ❌ | ✅ | snmpwalk |

---

## 十二、风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| lwIP 内存占用过大 | RAM 不足 | 精确配置 lwipopts.h，启用 MEMP_SANITY_CHECK |
| sys_arch 信号量死锁 | 内核挂起 | 所有 mutex 使用 irqsave 版本 |
| 中断上下文中调用阻塞 API | 内核崩溃 | 检查所有 lwIP API 调用上下文 |
| PCI BAR MMIO 未映射 | #PF 崩溃 | 使用 Rust VMM ioremap |
| QEMU E1000 寄存器差异 | 驱动异常 | 使用 lwIP 推荐的最小 E1000 子集 |

---

## 十三、交付物

| 数量 | 类型 | 描述 |
|------|------|------|
| 203 | .c 文件 | lwIP 2.2.1 完整源码 (只读) |
| 199 | .h 文件 | lwIP 2.2.1 完整头文件 |
| 4 | 新文件 | sys_arch.c / sys_arch.h / cc.h / lwipopts.h |
| 3 | 驱动文件 | e1000.c / e1000.h / e1000_regs.h |
| 3 | 桥接文件 | qx_net_init.c / qx_netif.c / qx_sockets.c |
| 1 | Makefile | 网络子系统编译规则 |
| 8 | syscall | Socket 系统调用注册 |
| ∞ | 网络能力 | TCP/IP/UDP/ICMP/DHCP/DNS/ARP/HTTP/mDNS/MQTT/SNMP/SMTP/TFTP/SNTP/PPP |

---

## 附录 A: lwIP 源码提取命令

```bash
cd /home/anfer/Code/C/AntX
mkdir -p src/net/lwip/src
unzip lwip-2.2.1.zip -d /tmp/
cp -r /tmp/lwip-2.2.1/src/core     src/net/lwip/src/
cp -r /tmp/lwip-2.2.1/src/api      src/net/lwip/src/
cp -r /tmp/lwip-2.2.1/src/netif    src/net/lwip/src/
cp -r /tmp/lwip-2.2.1/src/apps     src/net/lwip/src/
cp -r /tmp/lwip-2.2.1/src/include  src/net/lwip/src/
cp /tmp/lwip-2.2.1/CHANGELOG       src/net/lwip/
cp /tmp/lwip-2.2.1/COPYING         src/net/lwip/
rm -rf /tmp/lwip-2.2.1
```

## 附录 B: 最小可测试子集 (快速验证)

为快速验证集成可行性，建议先编译以下最小子集:

```
core + ipv4 + api + netif/ethernet + arch/sys_arch + driver/e1000
= 约 50 个 .c 文件
目标: lwip_init() 成功 → DHCP 获取 IP → ICMP Ping 回显
```
