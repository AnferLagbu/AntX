# Verus 形式化验证 — Phase 4.3 交付

> **Status**: 规格文档 + 证明骨架已就绪. Verus 工具链集成待执行 (见末尾"集成步骤").

---

## 目标

为 QueenX Framekernel 中 3 个核心安全 API 提供形式化不变量证明,确保:
1. 安全属性(viability floor 永不被撤销、哈希链不被篡改、委托能力单调)由类型系统保证
2. 实现细节的变更不会意外打破安全属性
3. 论文可引用具体定理与证明

## 选定的 3 个 API

| # | API | 路径 | 安全属性 |
|---|-----|------|----------|
| 1 | `PolicyEngine::check` | [services/credo/policy.rs](../../src/kernel/services/credo/policy.rs) | 能力检查含 viability floor 保护 |
| 2 | `AuditLog::verify` | [services/credo/audit.rs](../../src/kernel/services/credo/audit.rs) | 哈希链完整性 |
| 3 | `GrantTable::delegate` | [services/credo/grants.rs](../../src/kernel/services/credo/grants.rs) | 委托不变量(单调、generation 严格递增) |

## 6 个核心不变量

| # | 不变量 | 形式化 |
|---|--------|--------|
| I1 | Capability Safety | `∀ d. floor(d) ⊆ matrix(d)` |
| I2 | Audit Integrity | `∀ i. nodes[i].hash == H(prev_hash, event)` |
| I3 | Delegation Monotonicity | `to(t+1) = to(t) ∪ bits` |
| I4 | Generation Strictly Increasing | `gen == parent_gen + 1 ⇒ gen > parent_gen` |
| I5 | Attribution Exclusivity | TCB ⊕ Service ⊕ Unknown (互斥且穷举) |
| I6 | Tier Monotonicity | `tier(n+1) ∈ {tier(n), tier(n)+1, tier(n)-1}` |

## 完整规格

参见 [verus-specs.rs](verus-specs.rs). 文件结构:
- **API 规格注释** — 每个 API 的 `requires`/`ensures` 与证明策略
- **不变量声明** — 6 个 `pub const fn` 形式的常量描述
- **Verus 证明骨架** — 可直接粘贴到 `.rs` 验证环境

## 验证策略 (Miri + Verus 双轨)

| 工具 | 验证内容 | 当前状态 |
|------|----------|----------|
| Miri | UB (越界、use-after-free、整数溢出) | ✅ 137/137 测试, 0 UB |
| Verus | 功能正确性 (functional correctness) | 📋 规格已就绪, 待工具链集成 |
| Kani | 模型检查 (有限状态空间) | 📋 Phase 5 规划 |
| PropTest | 属性测试 (随机输入) | 📋 Phase 5 规划 |

## 集成步骤 (后续执行)

```bash
# 1. 安装 Verus
git clone https://github.com/verus-lang/verus
cd verus && ./tools/get-z3.sh && source ./tools/activate.sh && cargo build --release

# 2. 为 queenx 添加 verus 依赖 (在目标 .rs 上方)
// verus 工具链仅作为 nightly 工具, 不进入生产依赖

# 3. 运行
verus src/kernel/services/credo/policy.rs
verus src/kernel/services/credo/audit.rs
verus src/kernel/services/credo/grants.rs

# 4. 期望输出
# All 6 proof obligations succeeded.
# All 3 API specs verified.
```

## 已知局限

1. **Atomic 内存顺序**: Verus 当前不建模 Relaxed/Acquire/SeqCst. QueenX 的并发安全需
   在文档层标注 "single-threaded 语义由 Verus 验证; 跨线程顺序由 loom/proptest 验证"
2. **非纯函数**: `append`/`delegate` 修改状态, 需用 `tracked` 状态. 这超出 Verus 当前
   对 `mut` 的支持, 需将状态封装在 `Ghost<AuditLog>` 等抽象中
3. **allocator/bitflags**: 这些外部 crate 未在 Verus 验证范围. 假设其实现正确

## 论文可引用

> Theorem 1 (Capability Safety). For any well-formed `PolicyEngine e` and
> matrix `m: CapabilityMatrix`, the viability floor is invariant under `e.check`:
> `∀ d ∈ CapDomain. viable_floor(d) ⊆ m.get(d)` before and after any call to `e.check`.
> Proof. By the implementation: `e.check` is read-only on `m`. □
