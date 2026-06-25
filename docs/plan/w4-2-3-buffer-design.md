# W4.2.3 socket_open 桥接设计 — buffer 来源策略

> **工程代号**: REVAL-W / W4.2.3
> **设计日期**: 2026-06-25
> **作者**: AI 辅助 (遵循 maintenance-cycle §9.5 W4 计划)
> **状态**: 设计评审 (待实施)

---

## 1. 目标

W4.2.2 已实装 `socket_close_stub` + `dhcp_state_stub`. W4.2.3 任务: 实装
`socket_open_stub`, 即根据 `SocketKind` 构造 smoltcp socket (Tcp/Udp 等),
加入 `SocketSet`, 返回 smol_handle.

**核心难点**: smoltcp socket 构造需要 rx/tx buffer 来源. 当前 init.rs 中
smoltcp socket 构造在 sm_socket 系统调用路径中, 用 `k_malloc` 分配 buffer,
记录到 SOCKET_TABLE / TCP_RX_BUFS / TCP_TX_BUFS / UDP_RX_BUFS / UDP_TX_BUFS /
UDP_RX_METAS / UDP_TX_METAS. 这些 buffer 与 fd 索引 (0..MAX_SM_FD) 绑定.

SmoltcpNetStack 调用 raw stub 时, 需要独立的 buffer 来源, 不与 sm_socket
fd 冲突.

---

## 2. 候选方案评估

### 方案 A: 独立 buffer pool (新增 'static mut)

```rust
// init.rs 中新增
static mut NET_STACK_TRAIT_TCP_RX_BUFS: [*mut u8; MAX_SOCKETS] = ...;
static mut NET_STACK_TRAIT_TCP_TX_BUFS: [*mut u8; MAX_SOCKETS] = ...;
static mut NET_STACK_TRAIT_UDP_RX_BUFS: [*mut u8; MAX_SOCKETS] = ...;
static mut NET_STACK_TRAIT_UDP_TX_BUFS: [*mut u8; MAX_SOCKETS] = ...;
static mut NET_STACK_TRAIT_UDP_RX_METAS: [[PacketMetadata; N]; MAX_SOCKETS] = ...;
static mut NET_STACK_TRAIT_UDP_TX_METAS: [[PacketMetadata; N]; MAX_SOCKETS] = ...;
```

| 优点 | 缺点 |
|------|------|
| 0 冲突, 完全独立 | 8KB × 32 ≈ 256KB 额外 BSS 占用 |
| 实施简单 (新增静态变量) | 与 sm_socket 路径重复, 代码冗余 |
| close 路径清晰 (k_free 独立) | 维护成本高 (2 套 buffer 表) |

**评估**: 接受度中, BSS 增长在资源受限环境是问题.

### 方案 B: 扩展现有 SOCKET_TABLE 范围 (MAX_SM_FD + MAX_SOCKETS)

```rust
// 修改现有数组大小: [T; MAX_SM_FD] → [T; MAX_SM_FD + MAX_SOCKETS]
// 0..MAX_SM_FD: sm_socket fd 范围 (不变)
// MAX_SM_FD..MAX_SM_FD+MAX_SOCKETS: SmoltcpNetStack 范围 (新增)
```

| 优点 | 缺点 |
|------|------|
| 0 新增 buffer pool | 改动 sm_socket 路径索引计算 |
| 重用 k_malloc / SOCKET_TABLE | 修改大, 影响范围广 |
| 单套 buffer 表, 维护成本低 | 实施风险中等 |

**评估**: 接受度高, 但需谨慎修改 sm_socket.

### 方案 C: SmoltcpNetStack 借用 caller 提供的 'static buffer

```rust
// init.rs::raw 提供 'static buffer pool
static mut NET_STACK_TRAIT_BUFS: ... = ...; // 'static mut

// raw::socket_open_stub 借用 buffer 构造 smoltcp socket
pub fn socket_open_stub(
    sockets: &mut SocketSet<'_>,
    kind: SocketKind,
    bufs: &'static mut BufferPool,  // caller 提供
) -> Option<SmolHandle> { ... }
```

| 优点 | 缺点 |
|------|------|
| 灵活, caller 控制 buffer 来源 | caller 必须是 'static (framework 层限制) |
| 0 全局状态增长 | 与 W3.2 self-referential 冲突, W3.2 阶段已确认不可行 |
| 适用未来 SmoltcpNetStack 增强 (W4.2.4) | 当前 SmoltcpNetStack 不持 'static buffer, 接口不一致 |

**评估**: 接受度中, 未来扩展性最好, 但当前 SmoltcpNetStack 设计不支持.

### 方案 D: 暂不实装 socket_open_stub, SmoltcpNetStack 维护类型擦除句柄分配

```rust
// socket_open_stub 保持 stub (返回 None)
// SmoltcpNetStack::socket_open 调用 stub, 但因 stub 返回 None, 句柄分配失败
// 实际 smoltcp socket 仍由 sm_socket 路径创建, SmoltcpNetStack 暂不管理
```

| 优点 | 缺点 |
|------|------|
| 0 新增内存, 0 unsafe | SmoltcpNetStack::socket_open 不实际工作 |
| 0 行为变更, 安全稳妥 | 等于 W3.2 现状, 没有进展 |
| 与 W3.2 行为一致 | 用户调用 SmoltcpNetStack::socket_open 会得到 NotReady 错误 |

**评估**: 接受度高 (作为过渡方案), 但工程进度 0 推进.

---

## 3. 推荐方案: 方案 B (扩展现有 SOCKET_TABLE)

**核心理由**:

1. **0 BSS 增长**: 重用现有 buffer pool, 不增加静态存储
2. **单套 buffer 表**: 维护成本低, 路径统一
3. **现有架构对齐**: 与 sm_socket 路径的 fd + buffer 模式一致
4. **工作量可控**: 1-2 天, 风险中等

**关键设计点**:

### 3.1 索引空间分配

```rust
// 修改 MAX_SM_FD 的语义:
//   - 旧: MAX_SM_FD = 1024 (fd 数量)
//   - 新: TOTAL_SLOTS = MAX_SM_FD + MAX_SOCKETS, 数组大小改为 [T; TOTAL_SLOTS]
//
// sm_socket fd 范围: 0..MAX_SM_FD (不变)
// SmoltcpNetStack 范围: MAX_SM_FD..TOTAL_SLOTS (新增)
```

### 3.2 raw::socket_open_stub 实装

```rust
pub fn socket_open_stub(
    sockets: &mut SocketSet<'_>,
    kind: SocketKind,
    slot_idx: usize,  // 0..MAX_SM_FD: sm_socket 路径; MAX_SM_FD..: SmoltcpNetStack 路径
) -> Option<smoltcp::iface::SocketHandle> {
    // 1. 校验 slot_idx 在 [0, TOTAL_SLOTS) 范围内
    if slot_idx >= TOTAL_SLOTS { return None; }
    
    // 2. 校验 SOCKET_TABLE[slot_idx] 为空
    if SOCKET_TABLE.0[slot_idx].is_some() { return None; }
    
    // 3. 根据 kind 构造 smoltcp socket
    let smol_handle = match kind {
        SocketKind::Tcp => {
            let rx_ptr = k_malloc(TCP_BUF_SIZE);
            let tx_ptr = k_malloc(TCP_BUF_SIZE);
            // ... 构造 tcp::Socket
            let rx_slice = unsafe { core::slice::from_raw_parts_mut(rx_ptr, TCP_BUF_SIZE) };
            let tx_slice = unsafe { core::slice::from_raw_parts_mut(tx_ptr, TCP_BUF_SIZE) };
            let tcp_sock = tcp::Socket::new(
                tcp::SocketBuffer::new(rx_slice),
                tcp::SocketBuffer::new(tx_slice),
            );
            TCP_RX_BUFS[slot_idx] = rx_ptr;
            TCP_TX_BUFS[slot_idx] = tx_ptr;
            FD_TYPES.0[slot_idx] = 1;
            let handle = sockets.add(tcp_sock);
            SOCKET_TABLE.0[slot_idx] = Some(handle);
            handle
        }
        SocketKind::Udp => {
            // ... 构造 udp::Socket, 类似 TCP
            let handle = sockets.add(udp_sock);
            SOCKET_TABLE.0[slot_idx] = Some(handle);
            handle
        }
        _ => return None,
    };
    
    Some(smol_handle)
}
```

### 3.3 索引分配策略

**sm_socket 路径 (不变)**:
```rust
// 现有 sm_socket 内部 sm_alloc_fd() 仍返回 0..MAX_SM_FD 范围
let fd = sm_alloc_fd();  // 返回 0..MAX_SM_FD
let smol_handle = socket_open_stub(sockets, kind, fd)?;
```

**SmoltcpNetStack 路径 (新增)**:
```rust
// SmoltcpNetStack 内部维护独立的 idx 分配器
struct SmoltcpNetStack {
    next_smol_idx: u16,  // 从 MAX_SM_FD 开始
    // ...
}

impl SmoltcpNetStack {
    pub fn alloc_smol_idx(&mut self) -> Option<u16> {
        // 从 MAX_SM_FD 开始, 范围 [MAX_SM_FD, TOTAL_SLOTS)
        // 现有 W3.2 next_user_id 替换为 slot_idx
    }
}
```

### 3.4 close 路径

```rust
// socket_close_stub 保持 W4.2.2 实装, 不变
pub fn socket_close_stub(sockets: &mut SocketSet<'_>, smol_handle: SmolHandle) -> bool {
    sockets.remove(smol_handle);
    true
}

// sm_socket 路径 close 时, 需要:
//   1. 找到 smol_handle 对应的 slot_idx
//   2. 归还 buffer (k_free)
//   3. 清理 SOCKET_TABLE[slot_idx] / FD_TYPES[slot_idx]
// 这些已在现有 sm_close 中实现, 不需要修改
```

---

## 4. 实施步骤

### 4.1 W4.2.3.1: 数组大小扩展 (半天)

```rust
// 修改 6 张静态数组大小: [T; MAX_SM_FD] → [T; TOTAL_SLOTS]
// 同步修改 SOCKET_TABLE, FD_TYPES, TCP_RX_BUFS, TCP_TX_BUFS,
// UDP_RX_BUFS, UDP_TX_BUFS, UDP_RX_METAS, UDP_TX_METAS

const TOTAL_SLOTS: usize = MAX_SM_FD + MAX_SOCKETS;

static mut SOCKET_TABLE: SOCKET_TABLE_T = Align64([None; TOTAL_SLOTS]);
static mut FD_TYPES: FD_TYPES_T = Align64([0u8; TOTAL_SLOTS]);
static mut TCP_RX_BUFS: [*mut u8; TOTAL_SLOTS] = [null_mut(); TOTAL_SLOTS];
// ... 其他 4 张
```

**风险**: sm_socket 路径中所有 `for i in 0..MAX_SM_FD` 仍正确 (0..MAX_SM_FD 是 sm_socket 子集), 但需要审计所有访问点.

### 4.2 W4.2.3.2: socket_open_stub 实装 (半天)

```rust
pub fn socket_open_stub(
    sockets: &mut SocketSet<'_>,
    kind: SocketKind,
    slot_idx: usize,
) -> Option<SmolHandle> {
    // 实现如 §3.2
}
```

### 4.3 W4.2.3.3: sm_socket 路径迁移到 socket_open_stub (半天)

```rust
// 修改 sm_socket 内部:
let rx_ptr = k_malloc(TCP_BUF_SIZE);
// ... 构造 tcp_sock
let sockets = &mut *raw::socket_set();
let handle = socket_open_stub(sockets, SocketKind::Tcp, fd_idx)?;
// 删除原 sockets.add(tcp_sock) + SOCKET_TABLE[fd_idx] = Some(handle) 重复代码
```

### 4.4 W4.2.3.4: SmoltcpNetStack::socket_open 改造 (半天)

```rust
// SmoltcpNetStack::socket_open 调用 raw::socket_open_stub
fn socket_open(&mut self, kind: SocketKind) -> Result<SocketHandle> {
    // 1. 分配 SmoltcpNetStack 范围的 slot_idx (从 MAX_SM_FD 开始)
    let slot_idx = self.alloc_smol_idx().ok_or(NetError::NoFreeSocket)?;
    
    // 2. 调用 raw stub 实际构造 socket
    let sockets = unsafe { &mut *raw::socket_set() };
    let smol_handle = raw::socket_open_stub(sockets, kind, slot_idx)
        .ok_or(NetError::NoFreeSocket)?;
    
    // 3. 记录 (slot_idx, smol_handle) 映射
    let user_id = self.alloc_user_id();
    self.handle_map[smoltcp_slot_to_user_idx(slot_idx)] = Some((user_id, slot_idx as u16));
    
    Ok(SocketHandle::from_raw(user_id))
}
```

### 4.5 W4.2.3.5: 验证 (半天)

- 双架构编译 0w0e
- 4 审计 PASS
- host-tests 全部 PASS
- 集成测试: sm_socket 路径正常工作, SmoltcpNetStack 路径构造 socket 成功

---

## 5. 风险评估

| 风险 | 等级 | 缓解 |
|------|------|------|
| sm_socket 路径中 `for i in 0..MAX_SM_FD` 漏改 | 中 | 全面 audit 所有 MAX_SM_FD 引用 |
| TOTAL_SLOTS 容量计算错误 | 低 | 公式简单: MAX_SM_FD + MAX_SOCKETS |
| 数组大小修改导致 BSS 增长 | 低 | 8 字节/槽位 × MAX_SOCKETS ≈ 256B (可忽略) |
| SmoltcpNetStack 分配器与 sm_socket 冲突 | 低 | 范围 [MAX_SM_FD, TOTAL_SLOTS) 严格隔离 |
| socket_open_stub 的 unsafe 路径引入新风险 | 中 | 借用 W4.2.2 socket_close_stub 的实现模式, 严格 safety 注释 |
| 集成测试覆盖不足 | 中 | 添加 4 个 host-test 用例 (Tcp 创建, Udp 创建, idx 范围, 冲突检测) |

---

## 6. 验收标准

- [ ] 6 张数组大小修改为 `[T; TOTAL_SLOTS]`
- [ ] `socket_open_stub` 实装 Tcp + Udp 路径
- [ ] `sm_socket` 路径迁移到 `socket_open_stub` (删除重复代码)
- [ ] `SmoltcpNetStack::socket_open` 改造为委托 raw
- [ ] 双架构编译 0w0e
- [ ] 4 审计 PASS
- [ ] host-tests 全部 PASS (含新增 4 个集成测试)
- [ ] 0 行为变更 (sm_socket 路径语义保持)
- [ ] 0 新增 unsafe (复用现有 unsafe 路径)

---

## 7. 工作量估算

| 步骤 | 工作量 |
|------|--------|
| W4.2.3.1 数组大小扩展 | 0.5 天 |
| W4.2.3.2 socket_open_stub 实装 | 0.5 天 |
| W4.2.3.3 sm_socket 迁移 | 0.5 天 |
| W4.2.3.4 SmoltcpNetStack 改造 | 0.5 天 |
| W4.2.3.5 验证 | 0.5 天 |
| **总计** | **2.5 天** |

---

## 8. 后续步骤

1. W4.2.3.1: 数组大小扩展 (本次会话后, 下次会话启动)
2. W4.2.3.2: socket_open_stub 实装
3. W4.2.3.3: sm_socket 迁移
4. W4.2.3.4: SmoltcpNetStack 改造
5. W4.2.3.5: 验证
6. W4.2.4: SmoltcpNetStack::socket_close 改造
7. W4.2.5: 集成验证 + W4.3 启动

---

## 9. 备选方案回退路径

如果方案 B 实施遇到不可解决的问题, 备选回退:

1. **回退到方案 D**: 暂不实装 socket_open_stub, 推进 W4.2.4 (SmoltcpNetStack::socket_close 改造) + W4.3
2. **回退到方案 A**: 独立 buffer pool, 接受 256KB BSS 增长
3. **回退到方案 C**: SmoltcpNetStack 接受 caller 提供的 'static buffer (需 W3.2 重新设计)

**当前推荐**: 方案 B (扩展现有 SOCKET_TABLE), 因为它提供最佳的工程一致性.

---

**设计者注**: 此设计文档需用户审核. 实施前等待用户决策.
