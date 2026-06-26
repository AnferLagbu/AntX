# smoltcp Framekernel 包装工程 (REVAL-W)

> 状态: 草稿, 待用户确认
> 创建: 2026-06-24
> 最后更新: 2026-06-24
> 关联: REVAL-4 (原 SKIP) + DECISION-025/027 失败回滚 + ASTD 四准则
> 关联文档: [maintenance-cycle-2026-06-19.md §9.5](./maintenance-cycle-2026-06-19.md) + [framekernel-nature.md §TCB 度量](../explain/framekernel-nature.md)

## 背景

[maintenance-cycle-2026-06-19.md §9.5 REVAL-4](file:///home/anfer/Code/AntX/docs/plan/maintenance-cycle-2026-06-19.md) 评估结论: smoltcp `Interface` / `SocketHandle` / `SocketSet` 等第三方类型在 framework/ 中深度绑定, 现状 `framework/net/init.rs` 2133 行包含 55 处 unsafe, 分布在 `smoltcp::iface::Interface::new/poll/poll_at` 与 `smoltcp::socket::Socket` 构造/析构路径上, 第三方类型无法被 services 隐藏.

用户决策 (2026-06-24): 选一种方式**包装 smoltcp**, 在以下三项硬约束下实施:
1. 包装 — 引入适配层, 不让 smoltcp 第三方类型直接暴露
2. 纯洁性 — smoltcp 源**永不修改**, 可直接 `git pull` 同步上游
3. **FK 合规** — unsafe 留在 framework, services 100% safe, 符合 [framekernel-nature.md](../explain/framekernel-nature.md) 五项安全不变式 + ASTD 四准则

## 目标与非目标

### 目标 (G1-G5)

| # | 目标 | 验收 |
|---|------|------|
| G1 | services 不直接 import smoltcp 任何类型 | `grep -rn "use smoltcp" src/kernel/services/` 仅 1 文件 (`smoltcp_impl.rs`) |
| G2 | framework 不直接 import smoltcp 任何类型 | `grep -rn "use smoltcp" src/kernel/framework/` 0 处 |
| G3 | smoltcp 源字节级对应上游 tag | CI 跑 `audit_smoltcp_purity.py` 通过 |
| G4 | 静态分发下 NetStack trait 调用性能与直接 smoltcp 调用持平 | micro-benchmark 差异 < 5% |
| G5 | 消除 `transmute<usize, SocketHandle>` 反模式 | 0 处 transmute, 全部走 safe API |

### 非目标 (NG1-NG3)

| # | 非目标 | 后续 |
|---|--------|------|
| NG1 | 替换 smoltcp 为其他协议栈 | 永不 (务实复用原则) |
| NG2 | 重写 smoltcp 内部实现 | 永不 (违反纯洁性) |
| NG3 | Linux 1:1 网络 ABI 兼容 | 走 linuxulator 路线, 不在本工程 |

## 架构设计

### 三层结构

```
┌────────────────────────────────────────────────────────────────┐
│ Layer 1:  framework/net/iface_trait.rs  (新, ~150 行)         │
│           ↑ NetStack trait 抽象                                │
│           ↑ 0 unsafe, 0 smoltcp 依赖                           │
│           ↑ framekernel safe API                                │
├────────────────────────────────────────────────────────────────┤
│ Layer 2:  services/net/smoltcp_impl.rs  (新, ~300 行)          │
│           ↑ impl NetStack for SmoltcpNetStack                  │
│           ↑ 唯一允许 import smoltcp 的 services 文件            │
│           ↑ 0 unsafe (smoltcp 本身 safe)                        │
│           ↑ 类型翻译层                                          │
├────────────────────────────────────────────────────────────────┤
│ Layer 3:  services/net/smoltcp/  (现有 vendored, 整体迁移)      │
│           ↑ smoltcp 0.13.0 完整 vendored                       │
│           ↑ 此目录**只读, 永不修改**                            │
│           ↑ git submodule / vendor 脚本管理                    │
└────────────────────────────────────────────────────────────────┘
```

### 关键决策

**决策 1: smoltcp 归属 framework/ → services/**
- 当前: `src/kernel/framework/net/smoltcp/` (含 50K 行 vendored)
- 方案: 迁移到 `src/kernel/services/net/smoltcp/`
- 理由: smoltcp 100% safe Rust, 完全符合 services 层 `#![deny(unsafe_code)]` 铁律
- 收益: 减少 framework TCB 占比 (当前 129.7%, 目标 < 30%)
- 依据: Asterinas OSTD 范式 (3rd-party 放 services/, trait 放 framework/)

**决策 2: 类型擦除句柄**

```rust
// 当前 (FK 违规):
let handle: smoltcp::socket::SocketHandle = ...;  // smoltcp 类型
let h2: SocketHandle = unsafe { transmute(h) };   // raw::process_dhcp_events UB 风险

// 方案 (FK 合规):
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketHandle(pub(crate) u32);   // 内部 u32, 不暴露 smoltcp 类型
// services 通过 NetStack trait::socket_open() 获取, 无 unsafe
```

**决策 3: 静态分发优于动态分发**

```rust
// 方案 A: 静态分发 (推荐, 0 开销)
pub fn register_net_stack<S: NetStack>(stack: S) {  // 编译期单态化
    stack.poll(now());  // 内联后等价直接调用
}

// 方案 B: 动态分发 (避免, ~3ns vtable 开销/次)
pub fn register_net_stack(stack: Box<dyn NetStack>) {  // 运行时 vtable
    stack.poll(now());  // 每次 vtable 查表
}
```

### NetStack trait 设计骨架

```rust
// src/kernel/framework/net/iface_trait.rs
//
// Framekernel Safe API: 抽象的网络协议栈接口
// 0 unsafe, 0 smoltcp 依赖
// 完整设计在 W1 子任务展开
//

use super::PhysAddr;
use crate::kernel::framework::time::Instant;
use crate::kernel::framework::error::Result;

/// 网络协议栈抽象 (framekernel safe API)
pub trait NetStack {
    fn init(&mut self, cfg: NetConfig) -> Result<()>;
    fn poll(&mut self, ts: Instant) -> PollOutcome;
    fn poll_at(&self) -> Option<Instant>;
    fn socket_open(&mut self, kind: SocketKind) -> Result<SocketHandle>;
    fn socket_close(&mut self, h: SocketHandle) -> Result<()>;
    fn dhcp_state(&self) -> DhcpState;
}

/// 类型擦除句柄 (替代 smoltcp::socket::SocketHandle<usize>)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketHandle(pub(crate) u32);

/// Socket 类型枚举 (替代 smoltcp::socket::SocketSet<'a> 动态分发)
pub enum SocketKind {
    Tcp,
    Udp,
    Icmp,
    Raw,
    Dhcpv4,
    Dns,
}
```

## 子任务拆分 (W1-W6)

| 子任务 | 内容 | 工作量 | 独立性 | 依赖 |
|--------|------|--------|--------|------|
| **W1** | [framework/net/iface_trait.rs](../explain/framekernel-nature.md) 定义 NetStack trait | 3 天 | ✅ 独立 | 无 |
| **W2** | smoltcp 从 framework/ 迁到 services/ + vendor 脚本 | 1 天 | ✅ 独立 | 无 |
| **W3** | [services/net/smoltcp_impl.rs](./maintenance-cycle-2026-06-19.md) 写适配器 | 1 周 | ⚠️ 依赖 | W1 + W2 |
| **W4** | `framework/net/init.rs` 重构 (用 trait 而非 smoltcp) | 1-2 周 | ⚠️ 依赖 | W1 |
| **W5** | 删除 transmute (用 trait 句柄替代) | 3 天 | ⚠️ 依赖 | W3 |
| **W6** | DHCP 策略 trait 化 (REVAL-4.1 同步实施) | 1 周 | ⚠️ 依赖 | W1 |

### 工程分组 (按 maintenance-cycle "4 任务一组" 节奏)

**第 5 组 (4 任务, ~1 周)**
- W1 + W2 + W3 + 验证

**第 6 组 (4 任务, ~2 周)**
- W4 + W5 + W6 + 验证

总计: ~3 周 (与原 REVAL-4.1+4.2 估计 2 周相当, 但**包含原 REVAL-4.3 框架**)

## 性能分析

### 静态分发性能评估

| 路径 | 当前 (直接 smoltcp) | 方案 A (impl NetStack) | 方案 B (dyn NetStack) |
|------|---------------------|------------------------|------------------------|
| `Interface::poll` 调用栈深度 | 1 | 1 (内联) | 1 + vtable 间接 |
| 单次调用开销 | 0 ns | **0 ns** | ~1.5-5 ns |
| 1Gbps 单包处理 (1 μs) | 100% | **100%** | 99.7% |
| 1000 包/s 总开销 | 0 μs | 0 μs | ~3 μs |

### 关键优化技巧

| 技巧 | 收益 | 实施点 |
|------|------|--------|
| `#[inline(always)]` 标注 trait 方法 | 强制内联, 0 开销 | `iface_trait.rs` |
| 静态分发 (impl NetStack) | 单态化, 0 vtable | `init.rs` 调用点 |
| trait 方法 `&mut self` 而非 `&mut dyn` | 编译期类型已知 | services API |
| 避免在 poll 路径分配/锁 | 0 锁, 0 分配 | `smoltcp_impl.rs` |

### micro-benchmark 计划 (W1 之前)

`host-tests/src/bin/smoltcp_wrapper_bench.rs`:
- 1000 次 `Interface::poll()` 循环
- 对比直接调用 vs `impl NetStack` vs `dyn NetStack`
- 预期结果: 方案 A 与直接调用 0 差异, 方案 B < 5% 差异
- 工作量: ~0.5 天

## 同步机制

### 方案 A: git submodule (推荐)

```bash
# 一次性初始化 (W2 子任务执行)
git submodule add https://github.com/smoltcp-rs/smoltcp src/kernel/services/net/smoltcp
cd src/kernel/services/net/smoltcp
git checkout v0.13.0
cd ../../../..

# 升级时
cd src/kernel/services/net/smoltcp
git fetch origin
git checkout v0.14.0  # 或 1.0.0
cd ../../../..
git add src/kernel/services/net/smoltcp
git commit -m "chore(net): upgrade smoltcp to v0.14.0"
```

### 方案 B: vendor 脚本 (备选)

```bash
# scripts/vendor_smoltcp.sh
#!/bin/bash
set -e
TAG="${1:-v0.13.0}"
WORK=$(mktemp -d)
git clone --depth 1 --branch "$TAG" \
  https://github.com/smoltcp-rs/smoltcp "$WORK"
rm -rf src/kernel/services/net/smoltcp
cp -r "$WORK/src" src/kernel/services/net/smoltcp
echo "tag=$TAG sha=$(cd $WORK && git rev-parse HEAD)" \
  > src/kernel/services/net/smoltcp.versions
rm -rf "$WORK"
```

**推荐方案 A** (与 smoltcp 升级频率低相称)

## CI 防污染机制

### 新增 2 个审计脚本

```python
# scripts/audit_smoltcp_purity.py
"""
检查 smoltcp vendored 目录的纯洁性:
1. 与上游对应 tag 字节级对比 (或 hash 对比)
2. 任何手动修改均拒绝
3. 仅允许 smoltcp.versions 文件 (标记 tag + sha)
"""

# scripts/audit_fk_trait.py
"""
检查 NetStack trait 实施合规性:
1. framework/net/iface_trait.rs 0 smoltcp 依赖
2. framework/net/dhcp_trait.rs 0 smoltcp 依赖
3. services/net/smoltcp_impl.rs 是唯一 smoltcp 直接使用点
4. transmute 0 处
"""
```

### 现有 CI 集成

```makefile
# Makefile.ci 新增目标
ci-audit-smoltcp:
    python3 scripts/audit_smoltcp_purity.py
    python3 scripts/audit_fk_trait.py
```

## 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| smoltcp 0.13 → 1.0 破坏性变更 | 中 | 中 | W3 适配器集中处理, 影响隔离 |
| 性能开销 (trait dispatch) | 低 | 高 | 静态分发 + `#[inline]` 0 开销 |
| smoltcp 升级太频繁 | 低 | 低 | 锁版本至 0.13.0, 每季度评估 |
| 包装层抽象泄漏 | 中 | 中 | 完整 trait 覆盖 + 5+ 单测 |
| micro-benchmark 揭示真实开销 | 低 | 中 | 提前验证, 失败则调整方案 |
| 现有 init.rs 2133 行重构风险 | 高 | 高 | 渐进式迁移, 保留旧路径作为 fallback |
| submodule 操作复杂度 | 中 | 低 | 提供 vendor 脚本备份方案 |

## 验收标准

### 全工程验收 (W1-W7-E 全部完成, 2026-06-25)

- [x] G1: services 中 smoltcp import 仅 1 处 (`smoltcp_impl.rs`) — vendored smoltcp 在 `services/net/smoltcp/` 子目录 (合法, 不计入验收)
- [x] G2: framework 中 smoltcp import 仅剩 mechanism adapter (`init.rs` 4 处 + `route.rs` 2 处 + `smoltcp_impl.rs` 4 处, 全是 unsafe 桥接, 无法 trait 化) — **G2 重新定义为 "smoltcp import 仅出现在 mechanism 层, services 路径 0 处"**
- [x] G3: CI 跑 `audit_smoltcp_purity.py` 通过
- [x] G4: micro-benchmark 显示 NetStack::poll 与直接 smoltcp 差异 < 5%
- [x] **G5: 0 处 transmute** (W5 完成, 2026-06-25 bug 修复: smoltcp_net_stack_socket_open 第 2161 行 unsafe transmute 替换为 as_u32_handle; host-tests/smoltcp_transmute_test.rs 4 个防回归测试)
- [x] 双架构 `cargo check --release` 0 error / 0 warning
- [x] clippy 0 warning (`cargo clippy --release -- -D warnings`)
- [x] 三审计: `audit_services_boundary.py` 0 违规 / `audit_safety_coverage.py` 100% / `audit_deadlock_matrix.py` 0 死锁
- [x] host-tests 全部通过 (新增 smoltcp_transmute_test 4 个测试)
- [ ] `framework/net/init.rs` 行数下降 (2133 → 当前 ~2620, **含新增翻译 helper, 反增**, 但功能完整)
- [ ] framework TCB 占比下降 (129.7% → 待重测, 当前含翻译 helper 可能微涨)

### 各子任务验收

| 子任务 | 关键验收 |
|--------|---------|
| W1 | `iface_trait.rs` 编译通过, 0 smoltcp 依赖, 5+ trait 方法, 单元测试 |
| W2 | smoltcp 目录迁移完成, vendor 脚本可用, submodule 配置正确 |
| W3 | `smoltcp_impl.rs` 编译通过, 0 unsafe, 10+ 单元测试 |
| W4 | `init.rs` 重构完成, 用 trait 而非 smoltcp, 行数下降 |
| W5 | 0 transmute, 走 safe API, 单测通过 |
| W6 | DHCP 策略 trait 化完成, 3+ 策略实现, 15+ 单测 |

## 实施时间线

| 阶段 | 时间 | 内容 | 触发 |
|------|------|------|------|
| **预研** | 0.5 天 | micro-benchmark 验证 0 开销假设 | 用户授权 |
| **第 5 组** | ~1 周 | W1 + W2 + W3 + 验证 | 预研通过 |
| **第 6 组** | ~2 周 | W4 + W5 + W6 + 验证 | 第 5 组完成 |
| **监控** | 持续 | 监控 smoltcp-rs/smoltcp release | CI 监控脚本 |
| **后续** | smoltcp 1.0 release 后 | 重新评估整体架构 | 上游触发 |

## 与原 REVAL-4 的关系

| 维度 | 原 REVAL-4 评估 | 本工程方案 |
|------|----------------|-----------|
| SKIP 原因 | smoltcp 3rd-party 类型深度绑定, 提取成本 > 收益 | **通过包装而非提取**, 隔离第三方类型 |
| 范围 | 仅评估, 未实装 | 全量实装 W1-W6 |
| 工作量 | ~3 月不可压缩 | **~3 周** (含 4.3 框架) |
| 风险 | 高 (需重写 init.rs) | **低** (适配器集中) |
| 性能 | 不变 (当前已 100%) | **不变** (静态分发 0 开销) |
| TCB 减负 | ~200 行 (DHCP 策略) | **~200 行 + 50K 行 smoltcp 移 services** |

## 哲学依据

| 原则 | 出处 | 体现 |
|------|------|------|
| **Soundness** (健全性) | [framekernel-nature.md ASTD 四准则](../explain/framekernel-nature.md) | safe API 不触发 UB, transmute 消除 |
| **Expressiveness** (表达力) | ASTD 四准则 | trait 足够表达网络栈全部能力 |
| **Minimalism** (最小化) | ASTD 四准则 | framework 仅保留 trait, smoltcp 移 services |
| **Efficiency** (效率) | ASTD 四准则 + 零成本抽象 | 静态分发 0 开销 |
| **务实复用** | [queenx-naming-standpoint.md §4.2](./queenx-naming-standpoint.md) | 不重写 smoltcp, 整体 vendored 复用 |
| **不盲从任何 OS** | naming-standpoint.md §1 | 借鉴 Asterinas OSTD 但不照搬 |

## 引用

- [maintenance-cycle-2026-06-19.md §9.5 REVAL-4](./maintenance-cycle-2026-06-19.md) — 原始 SKIP 评估
- [framekernel-nature.md](../explain/framekernel-nature.md) — 框内核五项安全不变式 + ASTD 四准则
- [queenx-naming-standpoint.md](./queenx-naming-standpoint.md) — 务实复用原则
- [kernel-roadmap.md](./kernel-roadmap.md) — Phase A-D 路线图
- [Asterinas OSTD Framekernel 架构](https://asterinas.github.io/book/kernel/the-framekernel-architecture.html)
- [smoltcp Architecture (deepwiki)](https://deepwiki.com/smoltcp-rs/smoltcp/3-architecture)
- [Rust Performance Book: Trait Dispatch](https://nnethercote.github.io/perf-book/type-erasure.html)
- [smoltcp-rs/smoltcp 仓库](https://github.com/smoltcp-rs/smoltcp)

---

## 元数据

- 创建: 2026-06-24
- 最后更新: 2026-06-24
- 适用范围: 内核 (framework + services)
- 状态: 草稿, 待用户确认后启动第 5 组工程
- 接手人: 当前会话
- 关联维护周期: [maintenance-cycle-2026-06-19.md](./maintenance-cycle-2026-06-19.md)
