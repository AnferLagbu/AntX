# W4 smoltcp 剩余工程 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task.

**Goal:** 完成 W4 smoltcp 整合的所有剩余项：socket() 迁移、setsockopt/getsockopt/poll_all 迁移、ICMP/RAW socket 类型扩展。

**Architecture:** 与 W4 一致：NetStack trait → SmoltcpNetStack → framework FFI → smoltcp。

**Tech Stack:** Rust nightly, Framekernel 双层架构

## Global Constraints

- services 层 0 unsafe
- framework unsafe 块配 `// SAFETY:` 注释
- 双架构编译 0 warning 0 error
- 中文注释强制

---

## Task 1: 扩展 NetStack trait + SmoltcpNetStack (setsockopt/getsockopt/poll_all)

**Files:**
- Modify: `src/kernel/framework/net/iface_trait.rs`
- Modify: `src/kernel/services/net/smoltcp_impl.rs`

- [ ] **Step 1: Add trait methods**

在 `iface_trait.rs` 的 `NetStack` trait 中添加 (在 `close` 方法后):

```rust
    // ========================================================================
    // Socket 选项与轮询
    // ========================================================================

    /// 设置 Socket 选项.
    #[inline]
    fn setsockopt(&mut self, h: SocketHandle, level: i32, optname: i32, val: &[u8]) -> Result<()> {
        let _ = (h, level, optname, val);
        Err(NetError::NotReady)
    }

    /// 获取 Socket 选项.
    #[inline]
    fn getsockopt(&mut self, h: SocketHandle, level: i32, optname: i32, out: &mut [u8]) -> Result<usize> {
        let _ = (h, level, optname, out);
        Err(NetError::NotReady)
    }

    /// 轮询所有 Socket 状态 (驱动 select/poll).
    #[inline]
    fn poll_sockets(&mut self) -> Result<()> {
        Err(NetError::NotReady)
    }
```

- [ ] **Step 2: Add fd-based methods to SmoltcpNetStack**

在 `smoltcp_impl.rs` 的 `impl SmoltcpNetStack` 块中添加:

```rust
    pub fn setsockopt_fd(&self, fd: i32, level: i32, optname: i32, val: &[u8]) -> Result<()> {
        let rc = fw_net_socket::sm_net_setsockopt(fd, level, optname, val.as_ptr(), val.len() as u32);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }

    pub fn getsockopt_fd(&self, fd: i32, level: i32, optname: i32, out: &mut [u8]) -> Result<usize> {
        let mut out_len = out.len() as u32;
        let rc = fw_net_socket::sm_net_getsockopt(fd, level, optname, out.as_mut_ptr(), &mut out_len);
        if rc == 0 { Ok(out_len as usize) } else { Err(NetError::Other) }
    }

    pub fn poll_all_fd(&self) -> Result<()> {
        let rc = fw_net_socket::sm_net_poll_sockets();
        if rc >= 0 { Ok(()) } else { Err(NetError::Other) }
    }
```

需要在 `net_socket.rs` 中添加 `sm_net_setsockopt`/`sm_net_getsockopt`/`sm_net_poll_sockets` wrappers (如果还没有)。

- [ ] **Step 3: Verify + Commit**

```bash
cargo check --release --target x86_64-unknown-none 2>&1 | tail -5
git add -A && git commit -m "feat(net): add setsockopt/getsockopt/poll_sockets to NetStack trait"
```

---

## Task 2: 迁移 socket.rs (setsockopt/getsockopt/poll_all + socket)

**Files:**
- Modify: `src/kernel/services/net/socket.rs`

- [ ] **Step 1: Migrate setsockopt/getsockopt/poll_all**

```rust
pub fn setsockopt(fd: i32, level: i32, optname: i32, val: u32) -> SocketResult<()> {
    let val_bytes = val.to_ne_bytes();
    let s = net_stack().lock();
    s.setsockopt_fd(fd, level, optname, &val_bytes).map_err(|_| SocketError::InvalidArgument)
}

pub fn getsockopt(fd: i32, level: i32, optname: i32) -> SocketResult<u32> {
    let mut out = 0u32;
    let s = net_stack().lock();
    s.getsockopt_fd(fd, level, optname, &mut out.to_ne_bytes()).map_err(|_| SocketError::InvalidArgument)?;
    Ok(out)
}

pub fn poll_all() -> SocketResult<i32> {
    let s = net_stack().lock();
    s.poll_all_fd().map_err(|_| SocketError::InvalidArgument)?;
    Ok(0)
}
```

- [ ] **Step 2: Migrate socket()**

SmoltcpNetStack 需要一个 `socket_create_fd` 方法:

```rust
// 在 smoltcp_impl.rs 中
pub fn socket_create_fd(&self, domain: i32, sock_type: i32) -> Result<i32> {
    let fd = fw_net_socket::sm_net_socket(domain, sock_type, 0);
    if fd >= 0 { Ok(fd) } else { Err(NetError::Other) }
}
```

socket.rs:
```rust
pub fn socket(domain: Domain, sock_type: SockType, _protocol: i32) -> SocketResult<i32> {
    let s = net_stack().lock();
    s.socket_create_fd(domain as i32, sock_type as i32).map_err(|_| SocketError::InvalidArgument)
}
```

需要在 `net_socket.rs` 中添加 `sm_net_socket` wrapper。

- [ ] **Step 3: Verify + Commit**

```bash
cargo check --release --target x86_64-unknown-none 2>&1 | tail -5
git add -A && git commit -m "feat(net): migrate socket.rs setsockopt/getsockopt/poll_all/socket to SmoltcpNetStack"
```

---

## Task 3: 扩展 ICMP/RAW socket 类型

**Files:**
- Modify: `src/kernel/framework/net/init.rs` (raw::socket_open_stub)

- [ ] **Step 1: Add ICMP support**

在 `raw::socket_open_stub` 的 match 中添加:

```rust
SocketKind::Icmp => {
    // ICMP socket: 使用 UDP socket buffer (ICMP 无连接, 类似 UDP)
    let rx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
    if rx_ptr.is_null() { return None; }
    let tx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
    if tx_ptr.is_null() {
        crate::kernel::framework::mm::k_free(rx_ptr);
        return None;
    }
    let rx_slice = core::slice::from_raw_parts_mut(rx_ptr, UDP_BUF_SIZE);
    let tx_slice = core::slice::from_raw_parts_mut(tx_ptr, UDP_BUF_SIZE);
    let udp_sock = smoltcp::socket::udp::Socket::new(
        smoltcp::socket::udp::PacketBuffer::new(&mut UDP_RX_METAS[slot_idx][..], rx_slice),
        smoltcp::socket::udp::PacketBuffer::new(&mut UDP_TX_METAS[slot_idx][..], tx_slice),
    );
    let handle = sockets.add(udp_sock);
    SOCKET_TABLE.0[slot_idx] = Some(handle);
    FD_TYPES.0[slot_idx] = 2; // 标记为 UDP 类型 (ICMP 走 UDP socket)
    UDP_RX_BUFS[slot_idx] = rx_ptr;
    UDP_TX_BUFS[slot_idx] = tx_ptr;
    Some(handle)
}
SocketKind::Raw => {
    // RAW socket: 暂返回 None (不支持)
    None
}
SocketKind::Dhcpv4 | SocketKind::Dns => {
    // 内部类型, 用户态不可见
    None
}
```

- [ ] **Step 2: Verify + Commit**

```bash
cargo check --release --target x86_64-unknown-none 2>&1 | tail -5
git add -A && git commit -m "feat(net): extend socket_open_stub with ICMP socket type support"
```

---

## Task 4: 全量验证

- [ ] **Step 1: Clippy + dual-arch + audit + host-tests**

```bash
cd src/rust && cargo clippy --release --target x86_64-unknown-none -- -D warnings 2>&1 | tail -3
cd src/rust && cargo check --release --target aarch64-unknown-none 2>&1 | tail -3
cd /home/anfer/Code/QueenX && python3 scripts/audit_services_boundary.py 2>&1 | tail -2
cd host-tests && cargo test 2>&1 | grep FAILED
```

- [ ] **Step 2: Commit if fixes needed**
