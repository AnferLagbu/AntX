# W4.2.3 socket_open 桥接设计 — buffer 来源策略

> REVAL-W / W4.2.3 buffer 来源策略设计. 2026-06-25 实装完成.

## 目标
- **W4.2.3 socket_open_stub 实装目标**
  - 描述: W4.2.2 已实装 socket_close_stub + dhcp_state_stub. W4.2.3 任务实装 socket_open_stub, 即根据 SocketKind 构造 smoltcp socket (Tcp/Udp 等), 加入 SocketSet, 返回 smol_handle
  - 方案: 核心难点: smoltcp socket 构造需要 rx/tx buffer 来源, 当前 init.rs 中 smoltcp socket 构造在 sm_socket 系统调用路径中, 用 k_malloc 分配 buffer, 记录到 SOCKET_TABLE/TCP_RX_BUFS/TCP_TX_BUFS/UDP_RX_BUFS/UDP_TX_BUFS/UDP_RX_METAS/UDP_TX_METAS, 这些 buffer 与 fd 索引 (0..MAX_SM_FD) 绑定. SmoltcpNetStack 调用 raw stub 时, 需要独立的 buffer 来源, 不与 sm_socket fd 冲突
  - 状态: [X]
  - 详情: W4.2.3.1 数组扩展 (`36d1ecd`) + W4.2.3.2 socket_open_stub 实装 (`1599646`) + W4.2.3.3 sm_socket 迁移 (`737e213`) + W4.2.3.4 SmoltcpNetStack safe wrapper + 实际化 (`13d703f` + `9a74582`) 5 子任务全部完成.

## 工程计划 A: 方案评估

### 背景
- **背景条目**
  - 描述: 4 个候选方案评估
  - 方案: 方案 A 独立 buffer pool / 方案 B 扩展现有 SOCKET_TABLE / 方案 C SmoltcpNetStack 借用 caller buffer / 方案 D 暂不实装
  - 状态: [X]

### 现状
- **现状条目**
  - 描述: W4.2.2 已完成, 等待 W4.2.3 推进
  - 方案: 现有架构: SOCKET_TABLE 与 6 张 buffer 数组, 索引范围 0..MAX_SM_FD, sm_socket fd 占用
  - 状态: [X]

### 方案
- **方案 A: 独立 buffer pool (新增 'static mut)**
  - 描述: init.rs 中新增 8 张 NET_STACK_TRAIT_*_BUFS 静态变量
  - 方案: static mut NET_STACK_TRAIT_TCP_RX_BUFS / TCP_TX_BUFS / UDP_RX_BUFS / UDP_TX_BUFS / UDP_RX_METAS / UDP_TX_METAS = [PointerType; MAX_SOCKETS]; 优点: 0 冲突完全独立 + 实施简单 (新增静态变量) + close 路径清晰; 缺点: 8KB × 32 ≈ 256KB 额外 BSS 占用 + 与 sm_socket 路径重复代码冗余 + 维护成本高 (2 套 buffer 表)
  - 状态: []

- **方案 B: 扩展现有 SOCKET_TABLE 范围 (MAX_SM_FD + MAX_SOCKETS)**
  - 描述: 修改现有数组大小: [T; MAX_SM_FD] → [T; MAX_SM_FD + MAX_SOCKETS]
  - 方案: 0..MAX_SM_FD: sm_socket fd 范围 (不变); MAX_SM_FD..MAX_SM_FD+MAX_SOCKETS: SmoltcpNetStack 范围 (新增); 优点: 0 新增 buffer pool + 重用 k_malloc/SOCKET_TABLE + 单套 buffer 表维护成本低; 缺点: 改动 sm_socket 路径索引计算 + 修改大影响范围广 + 实施风险中等
  - 状态: [X] (推荐)

- **方案 C: SmoltcpNetStack 借用 caller 提供的 'static buffer**
  - 描述: init.rs::raw 提供 'static buffer pool
  - 方案: raw::socket_open_stub 借用 buffer 构造 smoltcp socket, bufs: &'static mut BufferPool caller 提供; 优点: 灵活 caller 控制 buffer 来源 + 0 全局状态增长 + 适用未来 W4.2.4; 缺点: caller 必须是 'static (framework 层限制) + 与 W3.2 self-referential 冲突 W3.2 阶段已确认不可行 + 当前 SmoltcpNetStack 不持 'static buffer 接口不一致
  - 状态: []

- **方案 D: 暂不实装 socket_open_stub, SmoltcpNetStack 维护类型擦除句柄分配**
  - 描述: socket_open_stub 保持 stub (返回 None)
  - 方案: SmoltcpNetStack::socket_open 调用 stub 因 stub 返回 None 句柄分配失败; 实际 smoltcp socket 仍由 sm_socket 路径创建, SmoltcpNetStack 暂不管理; 优点: 0 新增内存 0 unsafe + 0 行为变更安全稳妥 + 与 W3.2 行为一致; 缺点: SmoltcpNetStack::socket_open 不实际工作 + 等于 W3.2 现状没有进展 + 用户调用会得到 NotReady 错误
  - 状态: []

## 工程计划 B: 推荐方案 B 实施

### 背景
- **背景条目**
  - 描述: 方案 B 关键设计点
  - 方案: 0 BSS 增长 (重用现有 buffer pool) + 单套 buffer 表 (维护成本低) + 现有架构对齐 (与 sm_socket 路径 fd + buffer 模式一致) + 工作量可控 (1-2 天风险中等)
  - 状态: [X]

### 方案
- **索引空间分配**
  - 描述: 修改 MAX_SM_FD 的语义
  - 方案: 旧: MAX_SM_FD = 1024 (fd 数量); 新: TOTAL_SLOTS = MAX_SM_FD + MAX_SOCKETS, 数组大小改为 [T; TOTAL_SLOTS]; sm_socket fd 范围 0..MAX_SM_FD (不变); SmoltcpNetStack 范围 MAX_SM_FD..TOTAL_SLOTS (新增)
  - 状态: [X]

- **raw::socket_open_stub 实装**
  - 描述: 根据 kind 构造 smoltcp socket 并加入 SocketSet
  - 方案: 5 步: 校验 slot_idx 在 [0, TOTAL_SLOTS) 范围内 + 校验 SOCKET_TABLE[slot_idx] 为空 + 根据 kind (Tcp/Udp) 构造 smoltcp socket + k_malloc buffer + 记录到 SOCKET_TABLE/TCP_RX_BUFS/TCP_TX_BUFS/FD_TYPES + sockets.add 返回 handle
  - 状态: [X]

- **索引分配策略**
  - 描述: sm_socket 路径 vs SmoltcpNetStack 路径
  - 方案: sm_socket 路径: sm_alloc_fd() 仍返回 0..MAX_SM_FD 范围, fd 传给 socket_open_stub; SmoltcpNetStack 路径: 内部维护 next_smol_idx: u16 从 MAX_SM_FD 开始分配, 范围 [MAX_SM_FD, TOTAL_SLOTS)
  - 状态: [X]

- **close 路径**
  - 描述: 现有 socket_close_stub 保持不变
  - 方案: socket_close_stub 保持 W4.2.2 实装, sockets.remove(smol_handle) + 归还 buffer (k_free) + 清理 SOCKET_TABLE[slot_idx]/FD_TYPES[slot_idx]; sm_socket 路径 close 时找 smol_handle 对应 slot_idx 已在现有 sm_close 中实现不需要修改
  - 状态: [X]

## 实施步骤
- **W4.2.3.1 数组大小扩展 (0.5 天)**
  - 描述: 修改 6 张静态数组大小 [T; MAX_SM_FD] → [T; TOTAL_SLOTS]
  - 方案: const TOTAL_SLOTS: usize = MAX_SM_FD + MAX_SOCKETS; 修改 SOCKET_TABLE/FD_TYPES/TCP_RX_BUFS/TCP_TX_BUFS/UDP_RX_BUFS/UDP_TX_BUFS/UDP_RX_METAS/UDP_TX_METAS; 风险: sm_socket 路径中所有 `for i in 0..MAX_SM_FD` 仍正确 (0..MAX_SM_FD 是 sm_socket 子集), 但需要 audit 所有访问点
  - 状态: [X]
- **W4.2.3.2 socket_open_stub 实装 (0.5 天)**
  - 描述: 实现如 §3.2
  - 方案: pub fn socket_open_stub(sockets: &mut SocketSet<'_>, kind: SocketKind, slot_idx: usize) -> Option<SmolHandle>
  - 状态: [X]
- **W4.2.3.3 sm_socket 路径迁移到 socket_open_stub (0.5 天)**
  - 描述: 修改 sm_socket 内部调用 socket_open_stub
  - 方案: let sockets = &mut *raw::socket_set(); let handle = socket_open_stub(sockets, SocketKind::Tcp, fd_idx)?; 删除原 sockets.add(tcp_sock) + SOCKET_TABLE[fd_idx] = Some(handle) 重复代码
  - 状态: [X]
- **W4.2.3.4 SmoltcpNetStack::socket_open 改造 (0.5 天)**
  - 描述: SmoltcpNetStack::socket_open 调用 raw::socket_open_stub
  - 方案: 3 步: (1) 分配 SmoltcpNetStack 范围 slot_idx (从 MAX_SM_FD 开始) (2) 调用 raw stub 实际构造 socket (3) 记录 (slot_idx, smol_handle) 映射, user_id = self.alloc_user_id(), handle_map[smoltcp_slot_to_user_idx(slot_idx)] = Some((user_id, slot_idx as u16))
  - 状态: [X]
- **W4.2.3.5 验证 (0.5 天)**
  - 描述: 双架构编译 + 4 审计 + host-tests
  - 方案: 双架构编译 0w0e; 4 审计 PASS; host-tests 全部 PASS; 集成测试: sm_socket 路径正常工作, SmoltcpNetStack 路径构造 socket 成功
  - 状态: [X]

## 风险评估
- **风险清单**
  - 描述: 6 类风险 + 等级 + 缓解
  - 方案: sm_socket 路径中 `for i in 0..MAX_SM_FD` 漏改 (中/全面 audit 所有引用) / TOTAL_SLOTS 容量计算错误 (低/公式简单) / 数组大小修改导致 BSS 增长 (低/8 字节/槽位 × MAX_SOCKETS ≈ 256B 可忽略) / SmoltcpNetStack 分配器与 sm_socket 冲突 (低/范围 [MAX_SM_FD, TOTAL_SLOTS) 严格隔离) / socket_open_stub unsafe 路径引入新风险 (中/借用 W4.2.2 模式严格 safety 注释) / 集成测试覆盖不足 (中/添加 4 个 host-test 用例)
  - 状态: [X]

## 验收标准
- **全工程验收**
  - 描述: 9 项验收清单
  - 方案: 6 张数组大小修改为 [T; TOTAL_SLOTS] / socket_open_stub 实装 Tcp + Udp 路径 / sm_socket 路径迁移到 socket_open_stub (删除重复代码) / SmoltcpNetStack::socket_open 改造为委托 raw / 双架构编译 0w0e / 4 审计 PASS / host-tests 全部 PASS (含新增 4 个集成测试) / 0 行为变更 (sm_socket 路径语义保持) / 0 新增 unsafe (复用现有 unsafe 路径)
  - 状态: [X]
  - 详情: W4.2.3.1 数组扩展 (`36d1ecd`) + W4.2.3.2 socket_open_stub (`1599646`) + W4.2.3.3 sm_socket 迁移 (`737e213`) + W4.2.3.4 SmoltcpNetStack safe wrapper (`13d703f`) + 实际化 (`9a74582`) 全部实装, 9 项验收清单按 commit 验证.

## 工作量估算
- **工作量清单**
  - 描述: 5 步骤 + 总计
  - 方案: W4.2.3.1 数组大小扩展 0.5 天 / W4.2.3.2 socket_open_stub 实装 0.5 天 / W4.2.3.3 sm_socket 迁移 0.5 天 / W4.2.3.4 SmoltcpNetStack 改造 0.5 天 / W4.2.3.5 验证 0.5 天; 总计 2.5 天
  - 状态: [X]

## 后续步骤
- **后续步骤清单**
  - 描述: 7 步后续
  - 方案: W4.2.3.1 数组大小扩展 (本次会话后, 下次会话启动) / W4.2.3.2 socket_open_stub 实装 / W4.2.3.3 sm_socket 迁移 / W4.2.3.4 SmoltcpNetStack 改造 / W4.2.3.5 验证 / W4.2.4 SmoltcpNetStack::socket_close 改造 / W4.2.5 集成验证 + W4.3 启动
  - 状态: [X]

## 备选方案回退路径
- **回退路径**
  - 描述: 3 步回退策略
  - 方案: 如果方案 B 实施遇到不可解决的问题, 备选回退: (1) 回退到方案 D: 暂不实装 socket_open_stub, 推进 W4.2.4 + W4.3; (2) 回退到方案 A: 独立 buffer pool, 接受 256KB BSS 增长; (3) 回退到方案 C: SmoltcpNetStack 接受 caller 提供的 'static buffer (需 W3.2 重新设计)
  - 状态: [X]
  - 详情: 当前推荐: 方案 B (扩展现有 SOCKET_TABLE), 因为它提供最佳的工程一致性

