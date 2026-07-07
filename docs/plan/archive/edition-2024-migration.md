# Edition 2021 → 2024 迁移计划

> 规划 QueenX 从 Rust edition 2021 升级到 2024 的工程路径.

## 背景

Asterinas 已升级到 edition 2024 (nightly-2026-04-03). QueenX 当前使用 edition 2021 + nightly-2026-06-14.

## Edition 2024 核心变化

| 变化 | 影响 | QueenX 影响评估 |
|------|------|-----------------|
| `unsafe_op_in_unsafe_fn` deny by default | unsafe fn 内每个 unsafe 操作需包裹 `unsafe {}` | **高** — framework 107K 行, 428+ unsafe 块 (top 10 文件) |
| `unsafe extern` blocks | extern "C" 块需显式标注 unsafe | **中** — FFI 函数较多 |
| `!` (never type) 变化 | 语法层面调整 | **低** |
| `gen` blocks | 不稳定特性, 不影响 | 无 |
| 其他语法调整 | 格式/lint 变化 | **低** |

## 影响分析

### 高影响: unsafe_op_in_unsafe_fn

当前 `unsafe fn` 内的 unsafe 操作不需要额外 `unsafe {}` 块. 2024 edition 要求每个操作显式标注.

**受影响最大的文件** (unsafe 块数 top 10):

| 文件 | unsafe 块数 | 预估需改行数 |
|------|------------|------------|
| `proc/scheduler_ex.rs` | 70 | ~200 |
| `proc/user_proc.rs` | 66 | ~180 |
| `syscall/mod.rs` | 58 | ~150 |
| `mm/slab.rs` | 54 | ~140 |
| `mm/vmm_x86_64.rs` | 41 | ~100 |
| `arch/x86_64/acpi.rs` | 40 | ~100 |
| `mm/vmm_aarch64.rs` | 39 | ~90 |
| `mm/kmalloc.rs` | 37 | ~90 |
| `driver/display/dp.rs` | 32 | ~80 |
| `mm/pmm.rs` | 31 | ~80 |

**预估总工作量**: 500-1000 行改动, 2-3 周.

### 中影响: unsafe extern blocks

当前 extern "C" 函数声明散布在多个文件. 2024 edition 要求 `unsafe extern "C" { ... }` 块.

**预估工作量**: 1-2 天.

## 实施方案

### 方案 A: 增量迁移 (推荐)

1. **Phase 1**: 修改 `Cargo.toml` edition = "2024"
2. **Phase 2**: 运行 `cargo fix --edition` 自动修复简单 case
3. **Phase 3**: 手动修复剩余 unsafe 操作
4. **Phase 4**: 双架构编译验证 + 审计

**优点**: 渐进式, 每步可验证
**缺点**: 需要多次编译

### 方案 B: 一次性迁移

1. 批量修改所有 unsafe 块
2. 一次性切换 edition
3. 编译验证

**优点**: 快速
**缺点**: 风险高, 难以定位问题

## 推荐实施顺序

1. 创建 edition-2024 分支
2. 修改 `Cargo.toml`: edition = "2024"
3. 运行 `cargo fix --edition` (自动修复)
4. 手动修复剩余编译错误
5. 双架构编译 0w0e
6. clippy 0w0e
7. 全量审计
8. host-tests 通过
9. 合并到 main

## 风险

- **中风险**: unsafe_op_in_unsafe_fn 可能暴露潜在 UB (如果某个 unsafe 操作本应被保护但没有)
- **低风险**: 其他语法变化影响小
- **缓解**: 每步编译验证, 不跳步

## 预估工期

- Phase 1-2 (cargo fix): 0.5 天
- Phase 3 (手动修复): 2-3 天
- Phase 4 (验证): 1 天
- **总计**: 3-4 天

## 关联

- Asterinas edition 2024 参考: `tmp/asterinas-main/Cargo.toml`
- nightly 版本: 当前 1.98.0-nightly (2026-06-14), 可能需更新
