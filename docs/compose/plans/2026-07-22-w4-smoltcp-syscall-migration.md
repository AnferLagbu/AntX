# W4 smoltcp Syscall Dispatch 迁移 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `services/net/socket.rs` 的 syscall dispatch 路径通过 `SmoltcpNetStack` 而非直接调用 `sm_*` FFI，完成 W4 整合的最后一环。

**Architecture:** 在 `SmoltcpNetStack` 上添加基于 fd 的便捷方法（内部委托给 framework FFI），socket.rs 调用这些方法。SmoltcpNetStack 作为唯一的网络栈抽象层，所有 socket 操作必须经过它。

**Tech Stack:** Rust nightly, Framekernel 双层架构

## Global Constraints

- services 层 0 unsafe
- framework unsafe 块配 `// SAFETY:` 注释
- 双架构编译 0 warning 0 error
- 中文注释强制

---

## File Structure

| 文件 | 操作 | 职责 |
|---|---|---|
| `src/kernel/services/net/smoltcp_impl.rs` | Modify | 添加 fd-based 便捷方法 (bind_fd/listen_fd/...) |
| `src/kernel/services/net/socket.rs` | Modify | 迁移调用路径: sm_* → SmoltcpNetStack |
| `src/kernel/services/net/mod.rs` | Modify | 添加全局 SmoltcpNetStack 实例 + 初始化 |

---

## Task 1: SmoltcpNetStack 添加 fd-based 便捷方法

**Files:**
- Modify: `src/kernel/services/net/smoltcp_impl.rs`

**Interfaces:**
- Consumes: fw_net_socket::sm_net_* wrappers (Task 2 of previous plan)
- Produces: SmoltcpNetStack 上的 bind_fd/listen_fd/accept_fd/connect_fd/send_fd/recv_fd/sendto_fd/recvfrom_fd/close_fd 方法

- [ ] **Step 1: Add fd-based methods to SmoltcpNetStack**

在 `src/kernel/services/net/smoltcp_impl.rs` 的 `impl SmoltcpNetStack` 块中添加:

```rust
impl SmoltcpNetStack {
    /// fd-based bind: 委托给 framework FFI.
    pub fn bind_fd(&self, fd: i32, addr: NetEndpoint) -> Result<()> {
        let sin = endpoint_to_sockaddr(addr);
        let rc = fw_net_socket::sm_net_bind(fd, sin.as_ptr(), 16);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }

    /// fd-based listen.
    pub fn listen_fd(&self, fd: i32, backlog: i32) -> Result<()> {
        let rc = fw_net_socket::sm_net_listen(fd, backlog);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }

    /// fd-based accept.
    pub fn accept_fd(&self, fd: i32) -> Result<i32> {
        let new_fd = fw_net_socket::sm_net_accept(fd, core::ptr::null_mut(), core::ptr::null_mut());
        if new_fd >= 0 { Ok(new_fd) } else { Err(NetError::Other) }
    }

    /// fd-based connect.
    pub fn connect_fd(&self, fd: i32, addr: NetEndpoint) -> Result<()> {
        let sin = endpoint_to_sockaddr(addr);
        let rc = fw_net_socket::sm_net_connect(fd, sin.as_ptr(), 16);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }

    /// fd-based send.
    pub fn send_fd(&self, fd: i32, buf: &[u8]) -> Result<usize> {
        let rc = fw_net_socket::sm_net_send(fd, buf.as_ptr(), buf.len() as u32, 0);
        if rc >= 0 { Ok(rc as usize) } else { Err(NetError::Other) }
    }

    /// fd-based recv.
    pub fn recv_fd(&self, fd: i32, buf: &mut [u8]) -> Result<usize> {
        let rc = fw_net_socket::sm_net_recv(fd, buf.as_mut_ptr(), buf.len() as u32, 0);
        if rc >= 0 { Ok(rc as usize) } else { Err(NetError::Other) }
    }

    /// fd-based sendto.
    pub fn sendto_fd(&self, fd: i32, buf: &[u8], addr: NetEndpoint) -> Result<usize> {
        let sin = endpoint_to_sockaddr(addr);
        let rc = fw_net_socket::sm_net_sendto(fd, buf.as_ptr(), buf.len() as u32, 0, sin.as_ptr(), 16);
        if rc >= 0 { Ok(rc as usize) } else { Err(NetError::Other) }
    }

    /// fd-based recvfrom.
    pub fn recvfrom_fd(&self, fd: i32, buf: &mut [u8]) -> Result<(usize, NetEndpoint)> {
        let mut src = [0u8; 16];
        let mut addrlen = 16u32;
        let rc = fw_net_socket::sm_net_recvfrom(fd, buf.as_mut_ptr(), buf.len() as u32, 0, src.as_mut_ptr(), &mut addrlen);
        if rc >= 0 {
            let ep = sockaddr_to_endpoint(&src).unwrap_or(NetEndpoint::UNSPECIFIED);
            Ok((rc as usize, ep))
        } else {
            Err(NetError::Other)
        }
    }

    /// fd-based close.
    pub fn close_fd(&self, fd: i32) -> Result<()> {
        let rc = fw_net_socket::sm_net_close(fd);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }
}
```

注意: `endpoint_to_sockaddr` 和 `sockaddr_to_endpoint` 已在前一个 plan 的 Task 4 中定义在文件作用域。`fw_net_socket` 需要添加 import: `use crate::kernel::framework::net_socket as fw_net_socket;`

- [ ] **Step 2: Verify compilation**

Run: `cargo check --release --target x86_64-unknown-none 2>&1 | tail -5`
Expected: 0 errors

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/net/smoltcp_impl.rs
git commit -m "feat(net): add fd-based convenience methods to SmoltcpNetStack"
```

---

## Task 2: 迁移 socket.rs 调用路径

**Files:**
- Modify: `src/kernel/services/net/socket.rs`

**Interfaces:**
- Consumes: SmoltcpNetStack fd-based methods (from Task 1)
- Produces: socket.rs 所有函数通过 SmoltcpNetStack 而非 net_socket::sm_* FFI

- [ ] **Step 1: Modify socket.rs to use SmoltcpNetStack**

需要:
1. 添加 import: `use super::smoltcp_impl::SmoltcpNetStack;`
2. 添加全局 SmoltcpNetStack 访问 (Task 3 会提供, 这里先用占位)
3. 替换每个函数中的 `net_socket::sm_*` 调用为 `SmoltcpNetStack` 方法

```rust
use super::smoltcp_impl::SmoltcpNetStack;

/// 获取全局 SmoltcpNetStack 实例 (Task 3 提供)
fn stack() -> &'static spin::Mutex<SmoltcpNetStack> {
    super::NET_STACK.get().expect("网络栈未初始化")
}
```

替换函数:

```rust
pub fn socket(domain: Domain, sock_type: SockType, _protocol: i32) -> SocketResult<i32> {
    // socket 保持 FFI 路径 (fd 分配在 framework 层)
    let fd = net_socket::sm_socket(domain as i32, sock_type as i32, 0);
    if fd < 0 { Err(SocketError::from_i32(fd)) } else { Ok(fd) }
}

pub fn bind(fd: i32, addr: &SockAddrIn) -> SocketResult<()> {
    let ep = NetEndpoint::new(Ipv4Addr::from_octets(addr.ip), addr.port);
    let s = stack().lock();
    s.bind_fd(fd, ep).map_err(|_| SocketError::InvalidArgument)
}

pub fn listen(fd: i32, backlog: i32) -> SocketResult<()> {
    let s = stack().lock();
    s.listen_fd(fd, backlog).map_err(|_| SocketError::InvalidArgument)
}

pub fn accept(fd: i32) -> SocketResult<i32> {
    let s = stack().lock();
    s.accept_fd(fd).map_err(|_| SocketError::InvalidArgument)
}

pub fn connect(fd: i32, addr: &SockAddrIn) -> SocketResult<()> {
    let ep = NetEndpoint::new(Ipv4Addr::from_octets(addr.ip), addr.port);
    let s = stack().lock();
    s.connect_fd(fd, ep).map_err(|_| SocketError::InvalidArgument)
}

pub fn send(fd: i32, buf: &[u8]) -> SocketResult<usize> {
    let s = stack().lock();
    s.send_fd(fd, buf).map_err(|_| SocketError::InvalidArgument)
}

pub fn recv(fd: i32, out: &mut [u8]) -> SocketResult<usize> {
    let s = stack().lock();
    s.recv_fd(fd, out).map_err(|_| SocketError::InvalidArgument)
}

pub fn sendto(fd: i32, buf: &[u8], dest: &SockAddrIn) -> SocketResult<usize> {
    let ep = NetEndpoint::new(Ipv4Addr::from_octets(dest.ip), dest.port);
    let s = stack().lock();
    s.sendto_fd(fd, buf, ep).map_err(|_| SocketError::InvalidArgument)
}

pub fn recvfrom(fd: i32, out: &mut [u8]) -> SocketResult<(usize, SockAddrIn)> {
    let s = stack().lock();
    let (n, ep) = s.recvfrom_fd(fd, out).map_err(|_| SocketError::InvalidArgument)?;
    let addr = SockAddrIn::new(ep.port, ep.addr.octets());
    Ok((n, addr))
}

pub fn close(fd: i32) -> SocketResult<()> {
    let s = stack().lock();
    s.close_fd(fd).map_err(|_| SocketError::InvalidArgument)
}
```

注意: `setsockopt`/`getsockopt`/`poll_all` 暂不迁移 (保持 FFI 路径), 因为它们不在 NetStack trait 中。

- [ ] **Step 2: Verify compilation**

Run: `cargo check --release --target x86_64-unknown-none 2>&1 | tail -10`
Expected: 0 errors

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/net/socket.rs
git commit -m "feat(net): migrate socket.rs syscall dispatch to SmoltcpNetStack"
```

---

## Task 3: 添加全局 SmoltcpNetStack 实例

**Files:**
- Modify: `src/kernel/services/net/mod.rs`

**Interfaces:**
- Consumes: SmoltcpNetStack::new() (existing)
- Produces: `pub static NET_STACK: OnceLock<Mutex<SmoltcpNetStack>>` + init() 初始化

- [ ] **Step 1: Add static instance and init**

在 `src/kernel/services/net/mod.rs` 中添加:

```rust
use crate::kernel::framework::sync::OnceLock;
use spin::Mutex; // 或 framework 的 Mutex
use smoltcp_impl::SmoltcpNetStack;

/// 全局 SmoltcpNetStack 实例 (初始化后只读, 内部状态由 Mutex 保护)
pub static NET_STACK: OnceLock<Mutex<SmoltcpNetStack>> = OnceLock::new();

/// 初始化网络子系统 (在 kernel_init 中调用)
pub fn init() {
    // ... 现有 init 逻辑 ...
    // 初始化 SmoltcpNetStack
    NET_STACK.get_or_init(|| Mutex::new(SmoltcpNetStack::new()));
}
```

注意: 需要确认 `OnceLock` 和 `Mutex` 的来源 (framework 还是 spin crate)。如果 `SmoltcpNetStack` 不是 `Send`，需要用 `unsafe impl Send` 或换用 framework 的同步原语。

- [ ] **Step 2: Verify compilation**

Run: `cargo check --release --target x86_64-unknown-none 2>&1 | tail -10`
Expected: 0 errors

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/net/mod.rs
git commit -m "feat(net): add global SmoltcpNetStack instance with init"
```

---

## Task 4: 全量验证

- [ ] **Step 1: Clippy**

Run: `cd src/rust && cargo clippy --release --target x86_64-unknown-none -- -D warnings 2>&1 | tail -5`
Expected: 0 warnings

- [ ] **Step 2: Dual-arch build**

Run: `./ci/build.sh x86_64 2>&1 | tail -5`
Expected: 0 errors

- [ ] **Step 3: Audit**

Run: `python3 scripts/audit_services_boundary.py && python3 scripts/audit_safety_coverage.py`
Expected: all PASS

- [ ] **Step 4: Host-tests**

Run: `cd host-tests && cargo test 2>&1 | grep FAILED`
Expected: 0 failures

- [ ] **Step 5: Commit (if fixes needed)**

```bash
git add -A && git commit -m "fix(net): address review findings from syscall migration"
```
