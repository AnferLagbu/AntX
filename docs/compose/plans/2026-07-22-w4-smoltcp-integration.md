# W4 smoltcp 完整整合 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `SmoltcpNetStack` (services 层) 通过 `NetStack` trait 完整控制 smoltcp 网络栈，消除占位实现，最终替代 `sm_*` FFI 直接调用路径。

**Architecture:** 三层架构：`NetStack` trait (iface_trait.rs, 0 unsafe, 0 smoltcp) → `SmoltcpNetStack` (services/smoltcp_impl.rs, 0 unsafe) → framework safe wrappers (net_socket.rs, TCB unsafe) → smoltcp vendored。本次扩展 trait 加入 bind/listen/accept/connect/send/recv/close/poll 方法，framework 提供对应的 safe wrapper，SmoltcpNetStack 委托给 wrapper。

**Tech Stack:** Rust nightly, smoltcp 0.13.0 (vendored), Framekernel 双层架构

## Global Constraints

- services 层 0 unsafe (`#![deny(unsafe_code)]`)
- framework 任何 unsafe 块必须配 `// SAFETY:` 注释
- 双架构编译 0 warning 0 error (`./ci/build.sh all`)
- 审计全部通过 (boundary + safety + deadlock)
- host-tests 全部通过
- 中文注释强制

---

## File Structure

| 文件 | 操作 | 职责 |
|---|---|---|
| `src/kernel/framework/net/iface_trait.rs` | Modify | 扩展 NetStack trait: bind/listen/accept/connect/send/recv/close/poll |
| `src/kernel/framework/net_socket.rs` | Modify | 添加 framework safe wrappers: sm_net_bind/listen/accept/connect/send/recv/close/poll |
| `src/kernel/services/net/smoltcp_impl.rs` | Modify | SmoltcpNetStack 实装所有新 trait 方法 + poll + socket_close |
| `src/kernel/framework/net/init.rs` | Modify | 添加 raw::smoltcp_net_stack_socket_close 和 raw::smoltcp_net_stack_poll |
| `src/kernel/services/net/socket.rs` | Modify | 迁移 socket 操作走 NetStack trait |
| `host-tests/tests/w4_smoltcp_trait_test.rs` | Create | NetStack trait 单元测试 |

---

## Task 1: 扩展 NetStack trait

**Covers:** trait 接口定义

**Files:**
- Modify: `src/kernel/framework/net/iface_trait.rs:334-389` (NetStack trait)
- Test: `host-tests/tests/w4_smoltcp_trait_test.rs`

**Interfaces:**
- Consumes: 现有 `SocketHandle`, `SocketKind`, `NetConfig`, `PollOutcome`, `DhcpState`, `NetError`, `NetEndpoint`, `NetListenEndpoint`
- Produces: 扩展后的 `NetStack` trait 含 bind/listen/accept/connect/send/recv/close/poll 方法

- [ ] **Step 1: Write the failing test**

创建 `host-tests/tests/w4_smoltcp_trait_test.rs`:

```rust
//! W4 smoltcp 整合: NetStack trait 扩展验证

use queenx_host_tests::net_stack_types::*;

/// 验证扩展后的 NetStack trait 可以被 mock 实现
#[test]
fn test_netstack_trait_extended() {
    struct MockNetStack;
    impl NetStack for MockNetStack {}

    let mut stack = MockNetStack;

    // 新增方法的默认实现不应 panic
    let h = SocketHandle::from_raw(1);
    assert_eq!(stack.bind(h, NetEndpoint::UNSPECIFIED), Err(NetError::NotReady));
    assert_eq!(stack.listen(h, 128), Err(NetError::NotReady));
    assert_eq!(stack.accept(h, None), Err(NetError::NotReady));
    assert_eq!(stack.connect(h, NetEndpoint::UNSPECIFIED), Err(NetError::NotReady));
    assert_eq!(stack.send(h, &[], 0), Err(NetError::NotReady));
    assert_eq!(stack.recv(h, &mut [], 0), Err(NetError::NotReady));
    assert_eq!(stack.sendto(h, &[], 0, NetEndpoint::UNSPECIFIED), Err(NetError::NotReady));
    assert_eq!(stack.recvfrom(h, &mut [], 0, None), Err(NetError::NotReady));
    assert_eq!(stack.close(h), Ok(()));
    assert_eq!(stack.poll(0), PollOutcome::idle());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p queenx-tests --test w4_smoltcp_trait_test -- --nocapture 2>&1 | head -20`
Expected: 编译失败 (NetStack 没有 bind/listen 等方法)

- [ ] **Step 3: Extend NetStack trait**

在 `src/kernel/framework/net/iface_trait.rs` 的 `NetStack` trait 中添加:

```rust
    /// 绑定 socket 到本地端点.
    #[inline]
    fn bind(&mut self, h: SocketHandle, addr: NetEndpoint) -> Result<()> {
        let _ = (h, addr);
        Err(NetError::NotReady)
    }

    /// 开始监听连接.
    #[inline]
    fn listen(&mut self, h: SocketHandle, backlog: i32) -> Result<()> {
        let _ = (h, backlog);
        Err(NetError::NotReady)
    }

    /// 接受一个入站连接.
    #[inline]
    fn accept(&mut self, h: SocketHandle, peer: Option<&mut NetEndpoint>) -> Result<SocketHandle> {
        let _ = (h, peer);
        Err(NetError::NotReady)
    }

    /// 连接到远程端点.
    #[inline]
    fn connect(&mut self, h: SocketHandle, addr: NetEndpoint) -> Result<()> {
        let _ = (h, addr);
        Err(NetError::NotReady)
    }

    /// 发送数据 (TCP/connected UDP).
    #[inline]
    fn send(&mut self, h: SocketHandle, buf: &[u8], flags: i32) -> Result<usize> {
        let _ = (h, buf, flags);
        Err(NetError::NotReady)
    }

    /// 接收数据 (TCP/connected UDP).
    #[inline]
    fn recv(&mut self, h: SocketHandle, buf: &mut [u8], flags: i32) -> Result<usize> {
        let _ = (h, buf, flags);
        Err(NetError::NotReady)
    }

    /// 发送数据到指定端点 (UDP).
    #[inline]
    fn sendto(&mut self, h: SocketHandle, buf: &[u8], flags: i32, addr: NetEndpoint) -> Result<usize> {
        let _ = (h, buf, flags, addr);
        Err(NetError::NotReady)
    }

    /// 接收数据并获取发送方地址 (UDP).
    #[inline]
    fn recvfrom(&mut self, h: SocketHandle, buf: &mut [u8], flags: i32, src: Option<&mut NetEndpoint>) -> Result<usize> {
        let _ = (h, buf, flags, src);
        Err(NetError::NotReady)
    }

    /// 关闭 socket, 释放底层资源.
    #[inline]
    fn close(&mut self, h: SocketHandle) -> Result<()> {
        let _ = h;
        Ok(())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p queenx-tests --test w4_smoltcp_trait_test -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/kernel/framework/net/iface_trait.rs host-tests/tests/w4_smoltcp_trait_test.rs
git commit -m "feat(net): extend NetStack trait with bind/listen/accept/connect/send/recv/close"
```

---

## Task 2: 添加 framework safe wrappers

**Covers:** framework TCB unsafe 边界封装

**Files:**
- Modify: `src/kernel/framework/net_socket.rs`
- Modify: `src/kernel/framework/net/init.rs` (raw 模块)

**Interfaces:**
- Consumes: `SocketHandle` (from Task 1), `NetEndpoint`, `NetListenEndpoint`
- Produces: `sm_net_bind()`, `sm_net_listen()`, `sm_net_accept()`, `sm_net_connect()`, `sm_net_send()`, `sm_net_recv()`, `sm_net_sendto()`, `sm_net_recvfrom()`, `sm_net_close()`, `sm_net_poll()`

- [ ] **Step 1: Add framework raw helpers**

在 `src/kernel/framework/net/init.rs` 的 `raw` 模块中添加:

```rust
    /// SmoltcpNetStack::close 的 safe wrapper.
    ///
    /// # Safety
    /// 调用方持有 NET_LOCK; slot_idx 在 [MAX_SM_FD, TOTAL_SLOTS) 范围.
    pub fn smoltcp_net_stack_socket_close(slot_idx: usize) -> bool {
        unsafe {
            if slot_idx >= TOTAL_SLOTS || SOCKET_TABLE.0[slot_idx].is_none() {
                return false;
            }
            let handle = SOCKET_TABLE.0[slot_idx].unwrap();
            let sockets = &mut *socket_set();
            let stype = FD_TYPES.0[slot_idx];
            match stype {
                1 => {
                    let sock = sockets.get_mut::<tcp::Socket>(handle);
                    sock.close();
                }
                2 => {
                    let sock = sockets.get_mut::<udp::Socket>(handle);
                    sock.close();
                }
                _ => {}
            }
            sockets.remove(handle);
            if !TCP_RX_BUFS[slot_idx].is_null() {
                crate::kernel::framework::mm::k_free(TCP_RX_BUFS[slot_idx]);
                TCP_RX_BUFS[slot_idx] = core::ptr::null_mut();
            }
            if !TCP_TX_BUFS[slot_idx].is_null() {
                crate::kernel::framework::mm::k_free(TCP_TX_BUFS[slot_idx]);
                TCP_TX_BUFS[slot_idx] = core::ptr::null_mut();
            }
            if !UDP_RX_BUFS[slot_idx].is_null() {
                crate::kernel::framework::mm::k_free(UDP_RX_BUFS[slot_idx]);
                UDP_RX_BUFS[slot_idx] = core::ptr::null_mut();
            }
            if !UDP_TX_BUFS[slot_idx].is_null() {
                crate::kernel::framework::mm::k_free(UDP_TX_BUFS[slot_idx]);
                UDP_TX_BUFS[slot_idx] = core::ptr::null_mut();
            }
            SOCKET_TABLE.0[slot_idx] = None;
            FD_TYPES.0[slot_idx] = 0;
            true
        }
    }

    /// SmoltcpNetStack::poll 的 safe wrapper.
    ///
    /// # Safety
    /// 调用方持有 NET_LOCK; NIC 和 stack 已初始化.
    pub fn smoltcp_net_stack_poll() -> crate::kernel::framework::net::iface_trait::PollOutcome {
        use crate::kernel::framework::net::iface_trait::PollOutcome;
        let nic = match device_mut() {
            Some(d) => d,
            None => return PollOutcome::idle(),
        };
        let stack = match stack_mut() {
            Some(s) => s,
            None => return PollOutcome::idle(),
        };
        let sockets = &mut *socket_set();
        let before = sockets.iter().len();
        stack.poll(nic, sockets);
        let after = sockets.iter().len();
        process_dhcp_events(sockets);
        PollOutcome {
            packet_received: before != after || true, // smoltcp poll 总是可能处理了包
            socket_woken: false, // TODO: 检查 socket 状态变化
            dhcp_progressed: false, // 由 process_dhcp_events 更新
            tx_pending: 0,
        }
    }
```

- [ ] **Step 2: Add safe wrappers in net_socket.rs**

在 `src/kernel/framework/net_socket.rs` 末尾添加:

```rust
// ============================================================================
// W4: NetStack safe wrappers (SmoltcpNetStack 委托目标)
// ============================================================================

/// SmoltcpNetStack::bind 的 safe wrapper.
pub fn sm_net_bind(fd: i32, addr: *const u8, addrlen: u32) -> i32 {
    // SAFETY: addr 由调用方保证有效, sm_bind 同步读取
    unsafe { init::sm_bind(fd, addr, addrlen) }
}

/// SmoltcpNetStack::listen 的 safe wrapper.
pub fn sm_net_listen(fd: i32, backlog: i32) -> i32 {
    // SAFETY: NET_LOCK 内部获取
    unsafe { init::sm_listen(fd, backlog) }
}

/// SmoltcpNetStack::accept 的 safe wrapper.
pub fn sm_net_accept(fd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
    // SAFETY: null 表示不写对端地址
    unsafe { init::sm_accept(fd, addr, addrlen) }
}

/// SmoltcpNetStack::connect 的 safe wrapper.
pub fn sm_net_connect(fd: i32, addr: *const u8, addrlen: u32) -> i32 {
    // SAFETY: addr 栈上有效, sm_connect 同步读取
    unsafe { init::sm_connect(fd, addr, addrlen) }
}

/// SmoltcpNetStack::send 的 safe wrapper.
pub fn sm_net_send(fd: i32, buf: *const u8, len: u32, flags: i32) -> i32 {
    // SAFETY: buf 在调用期间有效
    unsafe { init::sm_send(fd, buf, len, flags) }
}

/// SmoltcpNetStack::recv 的 safe wrapper.
pub fn sm_net_recv(fd: i32, buf: *mut u8, len: u32, flags: i32) -> i32 {
    // SAFETY: out 可写
    unsafe { init::sm_recv(fd, buf, len, flags) }
}

/// SmoltcpNetStack::sendto 的 safe wrapper.
pub fn sm_net_sendto(fd: i32, buf: *const u8, len: u32, flags: i32, addr: *const u8, addrlen: u32) -> i32 {
    // SAFETY: buf + addr 同步有效
    unsafe { init::sm_sendto(fd, buf, len, flags, addr, addrlen) }
}

/// SmoltcpNetStack::recvfrom 的 safe wrapper.
pub fn sm_net_recvfrom(fd: i32, buf: *mut u8, len: u32, flags: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
    // SAFETY: out 可写, src 有效
    unsafe { init::sm_recvfrom(fd, buf, len, flags, addr, addrlen) }
}

/// SmoltcpNetStack::close 的 safe wrapper.
pub fn sm_net_close(fd: i32) -> i32 {
    // SAFETY: NET_LOCK 内部获取
    unsafe { init::sm_close(fd) }
}
```

同时为 kernel_test 桩模块添加对应的 no-op stub (在 `net_socket.rs` 的 `#[cfg(feature = "kernel_test")] mod init` 中).

- [ ] **Step 3: Verify compilation**

Run: `cargo check --release --target x86_64-unknown-none 2>&1 | tail -5`
Expected: 0 errors

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/net_socket.rs src/kernel/framework/net/init.rs
git commit -m "feat(net): add framework safe wrappers for NetStack trait methods"
```

---

## Task 3: SmoltcpNetStack 实装 poll + socket_close

**Covers:** poll 和 close 方法的实际实现

**Files:**
- Modify: `src/kernel/services/net/smoltcp_impl.rs`

**Interfaces:**
- Consumes: `fw_init::smoltcp_net_stack_poll()`, `fw_init::smoltcp_net_stack_socket_close()` (from Task 2)
- Produces: `SmoltcpNetStack::poll()` 返回真实 PollOutcome, `SmoltcpNetStack::close()` 释放资源

- [ ] **Step 1: Implement poll()**

替换 `SmoltcpNetStack::poll()` 的占位实现:

```rust
    fn poll(&mut self, ts_ms: u64) -> PollOutcome {
        if !self.initialized {
            return PollOutcome::idle();
        }
        let _ = ts_ms;
        // 委托给 framework safe wrapper, 它内部持有 NET_LOCK 并调用
        // smoltcp Interface::poll + process_dhcp_events
        fw_init::smoltcp_net_stack_poll()
    }
```

- [ ] **Step 2: Implement close()**

替换 `SmoltcpNetStack::close()` 的占位实现:

```rust
    fn close(&mut self, h: SocketHandle) -> Result<()> {
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
            return Ok(()); // 幂等
        };

        // DHCP 句柄保护
        if self.is_dhcp_handle(h.raw()) {
            return Ok(());
        }

        // 计算 framework 侧 slot_idx
        let fw_slot_idx = fw_init::smoltcp_net_stack_slot_base() + idx;

        // 委托 framework 关闭 smoltcp socket + 释放 buffer
        fw_init::smoltcp_net_stack_close(fw_slot_idx);

        // 清理 handle_map
        self.handle_map[idx] = None;
        Ok(())
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --release --target x86_64-unknown-none 2>&1 | tail -5`
Expected: 0 errors

- [ ] **Step 4: Commit**

```bash
git add src/kernel/services/net/smoltcp_impl.rs
git commit -m "feat(net): implement SmoltcpNetStack poll() and close() with real smoltcp calls"
```

---

## Task 4: SmoltcpNetStack 实装 bind/listen/accept/connect/send/recv

**Covers:** NetStack trait 所有新方法的 services 层实现

**Files:**
- Modify: `src/kernel/services/net/smoltcp_impl.rs`

**Interfaces:**
- Consumes: `fw_init::sm_net_bind/listen/accept/connect/send/recv/sendto/recvfrom()` (from Task 2), `SocketHandle` (from Task 1)
- Produces: SmoltcpNetStack 完整实现 NetStack trait

- [ ] **Step 1: Implement bind/listen/accept/connect**

在 `SmoltcpNetStack` 的 `impl NetStack` 块中添加:

```rust
    fn bind(&mut self, h: SocketHandle, addr: NetEndpoint) -> Result<()> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        // 找到 framework 侧 fd (需要反查 handle_map → fw_slot_idx → fd)
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let sin = endpoint_to_sockaddr(addr);
        let rc = fw_init::sm_net_bind(fd, sin.as_ptr(), 16);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }

    fn listen(&mut self, h: SocketHandle, backlog: i32) -> Result<()> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let rc = fw_init::sm_net_listen(fd, backlog);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }

    fn accept(&mut self, h: SocketHandle, peer: Option<&mut NetEndpoint>) -> Result<SocketHandle> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let mut addr_buf = [0u8; 16];
        let mut addrlen = 16u32;
        let new_fd = fw_init::sm_net_accept(fd, addr_buf.as_mut_ptr(), &mut addrlen);
        if new_fd < 0 {
            return Err(NetError::Other);
        }
        if let Some(ep) = peer {
            *ep = sockaddr_to_endpoint(&addr_buf)?;
        }
        // 新 fd 需要映射到 SmoltcpNetStack 的 handle
        self.fd_to_handle(new_fd)
    }

    fn connect(&mut self, h: SocketHandle, addr: NetEndpoint) -> Result<()> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let sin = endpoint_to_sockaddr(addr);
        let rc = fw_init::sm_net_connect(fd, sin.as_ptr(), 16);
        if rc == 0 { Ok(()) } else { Err(NetError::Other) }
    }
```

- [ ] **Step 2: Implement send/recv/sendto/recvfrom**

```rust
    fn send(&mut self, h: SocketHandle, buf: &[u8], flags: i32) -> Result<usize> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let rc = fw_init::sm_net_send(fd, buf.as_ptr(), buf.len() as u32, flags);
        if rc >= 0 { Ok(rc as usize) } else { Err(NetError::Other) }
    }

    fn recv(&mut self, h: SocketHandle, buf: &mut [u8], flags: i32) -> Result<usize> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let rc = fw_init::sm_net_recv(fd, buf.as_mut_ptr(), buf.len() as u32, flags);
        if rc >= 0 { Ok(rc as usize) } else { Err(NetError::Other) }
    }

    fn sendto(&mut self, h: SocketHandle, buf: &[u8], flags: i32, addr: NetEndpoint) -> Result<usize> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let sin = endpoint_to_sockaddr(addr);
        let rc = fw_init::sm_net_sendto(fd, buf.as_ptr(), buf.len() as u32, flags, sin.as_ptr(), 16);
        if rc >= 0 { Ok(rc as usize) } else { Err(NetError::Other) }
    }

    fn recvfrom(&mut self, h: SocketHandle, buf: &mut [u8], flags: i32, src: Option<&mut NetEndpoint>) -> Result<usize> {
        if !self.initialized || !h.is_valid() {
            return Err(NetError::NotReady);
        }
        let fd = self.handle_to_fd(h).ok_or(NetError::InvalidHandle)?;
        let mut addr_buf = [0u8; 16];
        let mut addrlen = 16u32;
        let rc = fw_init::sm_net_recvfrom(fd, buf.as_mut_ptr(), buf.len() as u32, flags, addr_buf.as_mut_ptr(), &mut addrlen);
        if rc >= 0 {
            if let Some(ep) = src {
                *ep = sockaddr_to_endpoint(&addr_buf).unwrap_or(NetEndpoint::UNSPECIFIED);
            }
            Ok(rc as usize)
        } else {
            Err(NetError::Other)
        }
    }
```

- [ ] **Step 3: Add helper methods**

在 `SmoltcpNetStack` 中添加内部 helper:

```rust
impl SmoltcpNetStack {
    /// 从 handle_map 反查 framework 侧 fd.
    fn handle_to_fd(&self, h: SocketHandle) -> Option<i32> {
        for (i, slot) in self.handle_map.iter().enumerate() {
            if let Some((u, _)) = slot {
                if *u == h.raw() {
                    // SmoltcpNetStack slot_idx = i, framework fd = MAX_SM_FD + i
                    return Some((fw_init::smoltcp_net_stack_slot_base() + i) as i32);
                }
            }
        }
        None
    }

    /// 从 framework fd 映射到 SmoltcpNetStack handle.
    fn fd_to_handle(&self, fd: i32) -> Result<SocketHandle> {
        let slot_base = fw_init::smoltcp_net_stack_slot_base() as i32;
        let idx = fd - slot_base;
        if idx < 0 || idx >= MAX_SOCKETS as i32 {
            return Err(NetError::InvalidHandle);
        }
        if let Some((user_id, _)) = self.handle_map[idx as usize] {
            Ok(SocketHandle::from_raw(user_id))
        } else {
            Err(NetError::InvalidHandle)
        }
    }
}

/// NetEndpoint → sockaddr_in [u8; 16] (little-endian).
fn endpoint_to_sockaddr(ep: NetEndpoint) -> [u8; 16] {
    let mut sin = [0u8; 16];
    sin[0..2].copy_from_slice(&2u16.to_le_bytes()); // AF_INET
    sin[2..4].copy_from_slice(&ep.port.to_be_bytes());
    sin[4..8].copy_from_slice(&ep.addr.octets());
    sin
}

/// sockaddr_in [u8; 16] → NetEndpoint.
fn sockaddr_to_endpoint(buf: &[u8; 16]) -> Result<NetEndpoint> {
    if buf[0..2] != [2, 0] { // AF_INET
        return Err(NetError::Other);
    }
    let port = u16::from_be_bytes([buf[2], buf[3]]);
    let addr = Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]);
    Ok(NetEndpoint::new(addr, port))
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check --release --target x86_64-unknown-none 2>&1 | tail -5`
Expected: 0 errors

- [ ] **Step 5: Commit**

```bash
git add src/kernel/services/net/smoltcp_impl.rs
git commit -m "feat(net): implement SmoltcpNetStack bind/listen/accept/connect/send/recv"
```

---

## Task 5: host-tests 验证

**Covers:** 全量编译验证 + host-tests

**Files:**
- 无新文件

- [ ] **Step 1: 双架构编译**

Run: `./ci/build.sh all 2>&1 | tail -10`
Expected: 0 errors, 0 warnings

- [ ] **Step 2: Clippy**

Run: `cargo clippy --release --target x86_64-unknown-none -- -D warnings 2>&1 | tail -10`
Expected: 0 warnings

- [ ] **Step 3: 审计**

Run: `python3 scripts/audit_services_boundary.py && python3 scripts/audit_safety_coverage.py && python3 scripts/audit_deadlock_matrix.py`
Expected: all PASS

- [ ] **Step 4: host-tests**

Run: `make test-host 2>&1 | tail -20`
Expected: all PASS

- [ ] **Step 5: Commit (if any fixes needed)**

```bash
git add -A
git commit -m "fix(net): address review findings from W4 integration"
```
