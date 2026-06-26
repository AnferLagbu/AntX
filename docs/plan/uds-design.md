# C3 Unix Domain Socket 设计决策

> UDS 设计 (DECISION-006/007/008), 2026-06-08 完成.

## 目标
- **目标条目**
  - 描述: 在 `framework/net/unix.rs` (TCB) 与 `services/net/unix.rs` (safe API) 实现 `AF_UNIX` 协议族, 提供 POSIX `socket(AF_UNIX, ...)` 入口的 `bind`/`listen`/`accept`/`connect`/`send`/`recv`/`sendto`/`recvfrom`/`close` 完整支持
  - 方案: 数据直接在**内核缓冲区**之间拷贝, 不经过网络协议栈, 不走 smoltcp
  - 状态: [X]

## 范围
- **v1 范围**
  - 描述: SOCK_STREAM 单 backlog + 简单阻塞 / SOCK_DGRAM 1-to-1 配对 / 路径绑定静态路径表 / 抽象命名空间 (无) / SCM_RIGHTS (无) / SO_PASSCRED (无)
  - 方案: 最小化实现, 满足基本客户端/服务器通信
  - 状态: [X]
  - 详情:

    | 项 | v1 范围 | 后续 |
    |----|---------|------|
    | `SOCK_STREAM` | ✅ 单 backlog 队列 + 简单阻塞语义 | SO_RCVBUF/SO_SNDBUF 调优 |
    | `SOCK_DGRAM` | ✅ 1-to-1 配对, 整消息边界保留 | 多路复用、SCM_RIGHTS |
    | 路径绑定 | ✅ 静态路径表 (固定数组) | 文件系统挂载 (`/var/run/foo.sock`) |
    | 抽象命名空间 | ❌ 全局共享 | UDS 命名空间隔离 (未来 namespace) |
    | `SCM_RIGHTS` / `SCM_CRED` | ❌ | v2 实现 |
    | `SO_PASSCRED` | ❌ | v2 |

- **后续范围**
  - 描述: v2 实现的子特性
  - 方案: 走独立路线扩展, 不影响 v1
  - 状态: []

## 关键设计
- **路径表与 Socket 表分离**
  - 描述: 全局表分离路径绑定与 socket 描述
  - 方案: `UDS_PATH_TABLE: IrqSpinLock<[Option<UnixPathBinding>; UNIX_MAX_BINDINGS]>` + `UDS_SOCK_TABLE: IrqSpinLock<[UnixSocket; UNIX_MAX_SOCKETS]>`
  - 状态: [X]
  - 详情:

    - 路径表: `path (≤108B) → socket_id`, 用于 `bind` 注册与 `connect` 查找
    - Socket 表: 固定大小数组, 每个槽位独立描述一个 socket (含状态、类型、对端、缓冲)

- **Socket 状态机**
  - 描述: SOCK_STREAM 5 状态 / SOCK_DGRAM 5 状态
  - 方案: SOCK_STREAM: Unbound→bind→Listening→accept→Connected→send/recv↔Connected→close→Closed; SOCK_DGRAM: Unbound→bind→Bound/connect→Connected→sendto/recvfrom→Connected→close→Closed
  - 状态: [X]

- **缓冲策略 (SOCK_STREAM)**
  - 描述: 每个 socket 独立 8KB 环形缓冲
  - 方案: 简化版: 单段连续缓冲 + `read_pos` / `write_pos` / `count` 三元组; 写满时返回 `EAGAIN` (v1 简化为非阻塞)
  - 状态: [X]

- **缓冲策略 (SOCK_DGRAM)**
  - 描述: 每个 socket 维护单消息
  - 方案: `last_datagram: Option<[u8; DGRAM_MAX]>` + `last_len`; `recvfrom` 一次性消费整条消息; `sendto` 直接覆盖写入对端
  - 状态: [X]
  - 详情: 简化: 暂不支持多消息排队, 后续可扩展为环形队列 (DECISION-007)

- **内存安全契约**
  - 描述: TCB 严格守卫, 用户指针经 services 校验
  - 方案: TCB 函数接受 `&mut UnixSocketTable` / `&mut [Option<UnixPathBinding>]`, 由 `IrqSpinLock` 守卫; 用户空间指针在 services 层 `raw::check_user_buf` 校验后才进入 TCB; TCB 内部 `unsafe` 仅出现在 `static mut` 初始化
  - 状态: [X]

- **错误映射 (POSIX errno)**
  - 描述: 8 类内部错误映射到 POSIX errno
  - 方案: EAGAIN(11)/ECONNREFUSED(111)/EINVAL(22)/EADDRINUSE(98)/ENOENT(2)/EAFNOSUPPORT(97)/ENOMEM(12)/ENOSYS(38)
  - 状态: [X]

- **不与 Linux 兼容的设计选择**
  - 描述: 3 处刻意偏离 Linux
  - 方案: 无文件系统绑定 (不走 VFS inode) / 无 autobind / listen backlog 固定为 5
  - 状态: [X]
  - 详情: 无 VFS inode 理由: VFS inode 系统为真实磁盘文件设计, 引入 UDS 需要走 open/close/create 全流程, 工作量翻倍且与未来 `SOCKET_FS` 扩展冲突。Linux 历史上也是后来才加入的。

## 文件结构
- **TCB 侧**
  - 描述: framework/net 新增 + net_socket.rs/net/mod.rs 修改
  - 方案: `src/kernel/framework/net/unix.rs` (新增 TCB) + `src/kernel/framework/net/mod.rs` (添加 pub mod) + `src/kernel/framework/net_socket.rs` (添加 6 个 `sm_unix_*` FFI 代理) + `src/kernel/framework/net/syscall.rs` (在 socket_syscall/bind_syscall 分流 AF_UNIX)
  - 状态: [X]
- **Services 侧**
  - 描述: services/net 新增 + socket.rs/mod.rs/syscall.rs 修改
  - 方案: `src/kernel/services/net/unix.rs` (新增 safe, 含 SockAddrUn/强类型 API) + `src/kernel/services/net/mod.rs` (re-export) + `src/kernel/services/net/socket.rs` (Domain 枚举加 Unix 变体) + `src/kernel/services/net/syscall.rs` (分流到 unix::xxx_syscall)
  - 状态: [X]
- **测试**
  - 描述: host-tests 新增 UDS 集成测试
  - 方案: `host-tests/src/unix_smoke.rs` (新增, 状态机 + 缓冲集成测试) + `host-tests/Cargo.toml` (注册)
  - 状态: [X]

## 验证
- **编译验证**
  - 描述: 双架构编译 + clippy
  - 方案: `cargo check -p queenx --target x86_64-unknown-none` 0 warning 0 error; `cargo check -p queenx --target aarch64-unknown-none` 0 warning 0 error; `cargo clippy -p queenx --target x86_64-unknown-none` 0 warning
  - 状态: [X]
- **测试与审计**
  - 描述: host-tests + 4 项审计脚本
  - 方案: `cargo test -p host-tests` 全通过; `audit_safety_coverage.py` 100% SAFETY 覆盖; `audit_services_boundary.py` services 不越界; `ci_check_services_unsafe.py` services 0 unsafe; `audit_deadlock_matrix.py` UDS 锁调用链登记
  - 状态: [X]

## 决策记录
- **DECISION-006**
  - 描述: UDS 不走 VFS inode, 走独立路径表
  - 方案: 理由: VFS inode 系统为真实磁盘文件设计, 引入 UDS 工作量翻倍, 后续 `SOCKET_FS` 扩展时再统一抽象
  - 状态: [X] (2026-06-08)
- **DECISION-007**
  - 描述: UDS SOCK_DGRAM 单消息排队, 多消息队列延后到 v2
  - 方案: 理由: v1 范围最小化, 满足基本客户端/服务器通信即可
  - 状态: [X] (2026-06-08)
- **DECISION-008**
  - 描述: UDS 阻塞语义 v1 退化为 EAGAIN
  - 方案: 理由: 完整阻塞需要调度器集成 + waitqueue, 与现有 pipe 异步路径对齐避免额外复杂度
  - 状态: [X] (2026-06-08)

## 变更历史
- **2026-06-26**
  - 描述: 按新文档规则重写 (标题+条目(描述+方案+状态)+详情)
  - 方案: 结构重组, 保留原意
  - 状态: [X]
