# C3 Unix Domain Socket 设计决策

> 状态: 待实施 (Phase C.3)
> 日期: 2026-06-08
> 关联: DECISION-006

## 目标

在 `framework/net/unix.rs` (TCB) 与 `services/net/unix.rs` (safe API) 中实现 `AF_UNIX` 协议族, 提供 POSIX `socket(AF_UNIX, ...)` 入口的 `bind`/`listen`/`accept`/`connect`/`send`/`recv`/`sendto`/`recvfrom`/`close` 完整支持。数据直接在**内核缓冲区**之间拷贝, 不经过网络协议栈, 不走 smoltcp。

## 范围

| 项 | v1 范围 | 后续 |
|----|---------|------|
| `SOCK_STREAM` | ✅ 单 backlog 队列 + 简单阻塞语义 | SO_RCVBUF/SO_SNDBUF 调优 |
| `SOCK_DGRAM` | ✅ 1-to-1 配对, 整消息边界保留 | 多路复用、SCM_RIGHTS |
| 路径绑定 | ✅ 静态路径表 (固定数组) | 文件系统挂载 (`/var/run/foo.sock`) |
| 抽象命名空间 | ❌ 全局共享 | UDS 命名空间隔离 (未来 namespace) |
| `SCM_RIGHTS` / `SCM_CRED` | ❌ | v2 实现 |
| `SO_PASSCRED` | ❌ | v2 |

## 关键设计

### 1. 路径表与 Socket 表分离

```text
UDS_PATH_TABLE: IrqSpinLock<[Option<UnixPathBinding>; UNIX_MAX_BINDINGS]>
UDS_SOCK_TABLE: IrqSpinLock<[UnixSocket; UNIX_MAX_SOCKETS]>
```

- 路径表: `path (≤108B) → socket_id`, 用于 `bind` 注册与 `connect` 查找
- Socket 表: 固定大小数组, 每个槽位独立描述一个 socket (含状态、类型、对端、缓冲)

### 2. Socket 状态机

```text
SOCK_STREAM:
  Unbound → bind(path) → Listening
  Listening → accept() → Connected
  Connected → send/recv ↔ Connected (peer)
  Connected → close() → Closed

SOCK_DGRAM:
  Unbound → bind(path) → Bound
  Unbound → connect(path) → Connected (绑到对端)
  Bound → connect(path) → Connected (连到对端)
  Connected → sendto/recvfrom → Connected
  Connected → close() → Closed
```

### 3. 缓冲策略 (SOCK_STREAM)

- 每个 socket 独立 8KB 环形缓冲
- 简化版: 单段连续缓冲 + `read_pos` / `write_pos` / `count` 三元组
- 写满时返回 `EAGAIN` (非阻塞) 或 `block` (阻塞; v1 简化为 `EAGAIN`)

### 4. 缓冲策略 (SOCK_DGRAM)

- 每个 socket 维护 `last_datagram: Option<[u8; DGRAM_MAX]>` + `last_len`
- `recvfrom` 一次性消费整条消息 (消息边界保留)
- `sendto` 直接覆盖写入对端的 `last_datagram`
- 简化: 暂不支持多消息排队, 后续可扩展为环形队列

### 5. 内存安全契约

- 所有 TCB 函数接受 `&mut UnixSocketTable` / `&mut [Option<UnixPathBinding>]`, 由 `IrqSpinLock` 守卫
- 用户空间指针在 services 层 `raw::check_user_buf` 校验后才进入 TCB
- TCB 内部 `unsafe` 仅出现在 `static mut` 初始化 (POSIX-like, 与 IPC_NAMESPACE 一致)

### 6. 错误映射 (POSIX errno)

| 内部错误 | errno | 说明 |
|----------|-------|------|
| `EAGAIN` | 11 | 缓冲满 (非阻塞) |
| `ECONNREFUSED` | 111 | 目标路径未 bind |
| `EINVAL` | 22 | 参数非法 / 状态非法 |
| `EADDRINUSE` | 98 | 路径已绑定 |
| `ENOENT` | 2 | 路径未找到 (connect) |
| `EAFNOSUPPORT` | 97 | 非 AF_UNIX 调用本 API |
| `ENOMEM` | 12 | 资源耗尽 |
| `ENOSYS` | 38 | 子特性未启用 |

### 7. 不与 Linux 兼容的设计选择

- **无文件系统绑定**: 路径不进入 VFS inode, 走独立路径表。理由: VFS inode 系统为真实磁盘文件设计, 引入 UDS 需要走 open/close/create 全流程, 工作量翻倍且与未来 `SOCKET_FS` 扩展冲突。Linux 历史上也是后来才加入的 (`/dev/socket/...`), 不必一开始就有。
- **无 `autobind`**: 用户必须显式 `bind` 或 `connect` 才能通信, 隐式匿名路径不入表。
- **listen backlog 固定为 5**: 不支持 `SOMAXCONN` 协商, 内核统一值。

## 文件结构

| 路径 | 类型 | 职责 |
|------|------|------|
| `src/kernel/framework/net/unix.rs` | 新增 (TCB) | `UnixSocket`、`UnixPathBinding`、全局表、`*_safe` 函数 |
| `src/kernel/framework/net/mod.rs` | 修改 | 添加 `pub mod unix;` |
| `src/kernel/framework/net_socket.rs` | 修改 | 添加 6 个 `sm_unix_*` FFI 代理 |
| `src/kernel/framework/net/syscall.rs` | 修改 | 在 `socket_syscall` / `bind_syscall` / 等处分流 AF_UNIX |
| `src/kernel/services/net/unix.rs` | 新增 (safe) | `SockAddrUn`、强类型 API |
| `src/kernel/services/net/mod.rs` | 修改 | re-export |
| `src/kernel/services/net/socket.rs` | 修改 | `Domain` 枚举添加 `Unix` 变体 |
| `src/kernel/services/net/syscall.rs` | 修改 | 分流到 `unix::xxx_syscall` |
| `host-tests/src/unix_smoke.rs` | 新增 | 状态机 + 缓冲集成测试 |
| `host-tests/Cargo.toml` | 修改 | 注册新测试 |

## 验证

1. `cargo check -p queenx --target x86_64-unknown-none` 0 warning 0 error
2. `cargo check -p queenx --target aarch64-unknown-none` 0 warning 0 error
3. `cargo clippy -p queenx --target x86_64-unknown-none` 0 warning
4. `cargo test -p host-tests` 全通过
5. `scripts/audit_safety_coverage.py` 100% SAFETY 覆盖
6. `scripts/audit_services_boundary.py` services 不越界
7. `scripts/ci_check_services_unsafe.py` services 0 unsafe
8. `scripts/audit_deadlock_matrix.py` UDS 锁调用链登记

## 决策记录

- DECISION-006: UDS 不走 VFS inode, 走独立路径表。理由: VFS inode 系统为真实磁盘文件设计, 引入 UDS 工作量翻倍, 后续 `SOCKET_FS` 扩展时再统一抽象。
- DECISION-007: UDS SOCK_DGRAM 单消息排队, 多消息队列延后到 v2。理由: v1 范围最小化, 满足基本客户端/服务器通信即可。
- DECISION-008: UDS 阻塞语义 v1 退化为 EAGAIN。理由: 完整阻塞需要调度器集成 + waitqueue, 与现有 pipe 异步路径对齐避免额外复杂度。
