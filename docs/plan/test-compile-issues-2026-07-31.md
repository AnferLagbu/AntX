# 测试编译问题登记 — 2026-07-31

> 9 个 `cargo check --release --tests` 编译错误完整记录 + 修复状态.
>
> **本规划状态**: [X] 全部已修 (commits `eb6aca96` + `429d931b`)
>
> **目的**: 防止后人重复尝试解决已修过的问题, 并明确 E0152 的工具链限制本质.

## 背景

2026-07-31 `cargo check --release --tests` 暴露 123 处预存编译错误
（`extern crate alloc`/`use super::*` 冗余清理后连锁效应）, 修复后剩 9 处,
进一步定位并修复, 最终通过 commit `429d931b` 的 `lib.test = false` 处理
E0152 工具链限制.

## 目标条目

### DECISION-020: 9 处编译错误完整登记

- **描述**
  - queenx 内核模块 166 个 `#[cfg(test)] mod tests` 中存在 123 处编译错误
  - 修复后剩 9 处, 完整记录如下
  - 状态: [X] 全部已修

- **方案**: 见详情表格

- **状态**: [X] 全部已修

- **详情**

  | # | 文件 | 行号 | 错误类型 | 修复 commit |
  |---|------|------|---------|-----------|
  | 1 | `src/kernel/services/debug/ebpf_verifier.rs` | 453 | `cannot find value ADD in module opcode` (缺失 `use crate::kernel::framework::debug::opcode`) | `eb6aca96` |
  | 2 | `src/kernel/services/driver/display/dp.rs` | 1287-1482 多处 | edition 2024 泛型歧义 `X as usize < Y` | `eb6aca96` |
  | 3 | `src/kernel/framework/timer/calibration.rs` | 341 | missing lifetime `Result<u64, &str>` | `eb6aca96` |
  | 4 | `src/kernel/framework/arch/x86_64/tss.rs` | 321 | cannot find macro `println!` (no_std 测试环境) | `eb6aca96` |
  | 5 | `src/kernel/framework/ipc/stress_tests.rs` | 39, 92 | cannot find macro `format!`/`println!` | `eb6aca96` |
  | 6 | `src/kernel/framework/net/iface_trait.rs` | 694, 695 | cannot find macro `format!` | `eb6aca96` |
  | 7 | `src/kernel/framework/driver/framework.rs` | 303 | cannot find macro `format!` | `eb6aca96` |
  | 8 | `src/kernel/framework/driver/usb/xhci.rs` | 988 | unresolved import `super::usb_core` | `eb6aca96` |
  | 9 | E0152 lang item 冲突 | — | `duplicate lang item sized in core` (工具链限制) | `429d931b` (`lib.test = false`) |

### DECISION-021: E0152 工具链限制明确登记

- **描述**
  - 问题 9 是 E0152, 不属于内核错误
  - 根因: host `cargo check --tests` 模式下 queenx 链接的 core 与 host std 拉入的 host core 同时存在, 两者都定义 `sized` lang item → 冲突
  - 关键洞察 (用户 2026-07-31): 消除冲突的根本方式是「不同时链接两个 core」
  - 具体实现: `Cargo.toml [lib] test = false`, 让 cargo check --tests 跳过 queenx lib test 目标, queenx 整个 lib 不参与 host 测试编译
  - 此时 host std 链接的 host core 是唯一被链接的 core, 无冲突
  - 状态: [X] 已修

- **方案**: `lib.test = false`

- **状态**: [X] 已修 (commit `429d931b`)

- **详情**

  | 子特性 | 描述 |
  |--------|------|
  | 内核 target 编译 | 完全无 E0152 (`x86_64-unknown-none`/`aarch64-unknown-none`) |
  | 内核二进制生成 | 完全无 E0152 (`ci/build.sh all` 5/5 通过) |
  | 内核 QEMU 运行 | 完全无 E0152 (`make test-unit` QEMU 路径, 走 `kernel_test` feature) |
  | host cargo check --tests | 已通过 `lib.test = false` 消除 |
  | 尝试过且失败的方案 | 6 种 (见 commit `429d931b` 详情) |

### DECISION-022: 受影响测试范围登记

- **描述**
  - E0152 + `lib.test = false` 让 1340 个 queenx 内核模块单元测试无法在 host cargo test 中运行
  - 实际可用测试入口: host-tests/ 全部 10 单元测试 + 79 集成测试文件
  - queenx 内核模块单元测试从未在 host 环境真正运行过 (项目惯例是 `make test-unit` QEMU 路径)
  - 状态: [X] 已记录

- **方案**: 在 plan 文档中明确登记

- **状态**: [X] 已记录

- **详情**

  | 测试入口 | E0152 影响 | 实际可用性 |
  |---------|-----------|----------|
  | `cargo test -p host-tests` (10 单元 + 79 集成) | ✅ 不涉及 | ✅ 可用 |
  | `make test-host` | ✅ 不涉及 | ✅ 可用 |
  | `ci/build.sh all` | ✅ 不涉及 | ✅ 5/5 通过 |
  | queenx `#[cfg(test)] mod tests` (1340 测试) | ❌ 编译失败 | ❌ 不可用 (从未在 host 环境运行) |
  | `make test-unit` QEMU 路径 | ❌ pre-existing 8 错误 | ❌ 不可用 (与 E0152 无关) |

  | 子特性 | 描述 |
  |--------|------|
  | pre-existing 错误 | kernel_test feature 启用时仍有 8 个错误, 与本任务无关 |
  | 真实测试路径 | queenx 内核模块单元测试实际通过 QEMU 运行 (历史惯例) |
  | host-tests 独立性 | 不依赖 queenx lib, 自身 std crate, 完全可用 |

## 工程约束

- 不修改测试语义
- 保持 host-tests/ 现有架构 (独立 std crate)
- 保持 §11.1 哲学 (内核不污染测试代码)
- 保持内核 purity (release 构建 0 错误 0 警告)

## 验收标准

- ✅ `cargo check --release` 0 error 0 warning
- ✅ `cargo check --release --tests` 0 error 0 warning
- ✅ `ci/build.sh all` 5/5 通过
- ✅ `cargo test -p host-tests` 全通过
- ✅ audit_services_boundary / audit_coupling / audit_safety_coverage 全通过

## 范围外 (Not In Scope)

- 修复 kernel_test feature 启用时的 8 个 pre-existing 错误 (独立历史问题)
- 迁移 queenx 内核模块 1340 个测试到 host-tests/ (哲学正确, 但工作量巨大)
- 解决 E0152 工具链限制本身 (已通过 `lib.test = false` 处理)

## 后续建议 (Future Work)

- [ ] 修复 kernel_test feature 启用时的 8 个 pre-existing 错误 (恢复 `make test-unit` QEMU 路径)
- [ ] 评估是否将 queenx 内核模块单元测试迁移到 host-tests/ (大改动, 需独立 plan)
- [ ] 关注 nightly Rust 升级是否修复 E0152 工具链限制 (可移除 `lib.test = false`)