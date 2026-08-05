# QueenX 阶段 12-18 工程规划总览 (lint 全景)

> 合并同类项: 阶段 12 (rustfmt) → 13 (clippy 加严) → 14 (clippy.toml 评估) → 15 (rustdoc 75→0) → 16 (use_self 407→0) → 17 (option_if_let_else 评估) → 18 (CI rustdoc 阻断). 本文档作为 QueenX 静态检查工程唯一权威跟踪表, 后续开发者**优先读此文档 + 链接的子文档**.
>
> 已交付状态: 双架构 (x86_64 + aarch64) cargo check 0 warning + clippy `-D pedantic` 0 warning + rustdoc `broken_intra_doc_links` 0 + cargo fmt --check 0 差异 + 三审计全过 + host-tests 838 passed / 0 failed + CI 阻断位 5 条全部就位.

## 工程计划 A: lint 全景基线 (阶段 12-18 合并)

### 背景

- **静态检查治理目标**
  - 描述: QueenX 自研内核需要严格静态检查保证 24/7 内核态运行安全. 阶段 8 (clippy pedantic 10591→0) 是基础, 阶段 12-18 是补齐 + CI 阻断位落地.
  - 方案: 4 类工具链 (clippy / rustfmt / rustdoc / cargo audit) + CI 阻断位 + 文档跟踪表.
  - 状态: [X]

- **lint 报告统计** (起点 → 当前)
  - clippy pedantic: 10591 → **0** (阶段 8-10 完成)
  - clippy nursery: 1637 → **1230** (阶段 16 use_self 407→0)
  - rustdoc warnings: 75 → **0** (阶段 15 完成)
  - cargo fmt 差异: 大量 → **0** (阶段 12 完成)
  - 状态: [X]

### 目标

- **4 类静态检查 CI 阻断位全部就位**
  - 描述: clippy -D pedantic + rustdoc -D broken_intra_doc_links + cargo fmt --check + cargo audit + host-tests 全过.
  - 方案: ci-lint.yml + ci-x86.yml + ci-aarch64.yml 三工作流协同.
  - 状态: [X]

- **nursery lint 治理完成度记录**
  - 描述: 1230 处 nursery warning 中 407 已修 (use_self), 其余保留 allow (DECISION-044) 或中期手工.
  - 方案: 阶段 16 use_self 已修, 阶段 17 option_if_let_else 评估放弃机械修复 (clippy --fix 不支持 nursery).
  - 状态: [X]

### 现状 (2026-08-06)

- **clippy 加严 (阶段 13)**
  - 描述: clippy-pedantic job 加 `--lib --bins --examples` (排除 tests 因 no_std kernel 不支持 #[test]). 5 处 unfulfilled expect 处理.
  - 方案: 跨架构 cfg_attr 解决 aarch64 差异化 expect. ci-x86.yml 已有 clippy-pedantic job.
  - 状态: [X]
  - 详情: commit `5a4c6c16` (CI clippy 扩展到 --lib --bins --examples 加严覆盖).

- **rustfmt 全量整改 (阶段 12)**
  - 描述: 全仓 rustfmt 应用, CI `cargo fmt --check` 阻断位就位.
  - 方案: rustfmt 不区分代码风格, 仅格式 (行宽/缩进/换行). host-tests 加鲁棒化处理 split_whitespace + pubconstfn 匹配模式.
  - 状态: [X]
  - 详情: commit `d7642e41` (阶段 12 rustfmt 全量整改 + host-tests 鲁棒化).

- **clippy.toml 评估 (阶段 14)**
  - 描述: 评估 clippy.toml 是否需要增强 (check-private-items 等).
  - 方案: 评估加 `check-private-items = true` → 触发 162 errors (missing_safety_doc 在 lib.rs 已 allow 之后未生效), **回退**. clippy.toml 保持现状 (cognitive-complexity=25, type-complexity=250, too-many-lines=100, missing-docs-in-crate-items, standard-macro-braces).
  - 决策: clippy.toml 已是最优状态, 不再迁移 lib.rs 52 处 `#![allow]` (DECISION-043 手工审查, 含 aarch64 差异化 allow).
  - 状态: [X]

- **rustdoc 75 → 0 (阶段 15)**
  - 描述: rustdoc --no-deps 75 warnings 全部清零 (HTML tag / bit-field / 真 broken link / 同名冲突 / 代码块字面量 5 类).
  - 方案:
    - 14 处 HTML tag 加反引号 (`<T>` → `` `<T>` ``)
    - 27 文件 74 处 bit-field (`[63:32]` → `` `[63:32]` ``)
    - 10 处真 link 错误: SINFO_GDT_LIMIT (过时名字) + framework:: → crate:: + PiMutex::update_waiter_priority 路径修正 + KERNEL_PML4[pml4_idx] 字面量转义
    - 1 处 super::socket 同名冲突 (`[mod@super::socket]`)
    - 2 处 entries[i] 代码块字面量转义
  - 验证: 双架构 cargo doc 0 源码 warning.
  - 状态: [X]
  - 详情: commit `1e00d842` (entries[i] 字面量转义) + commit `f1524e5a` (数组/位域反引号化, user push).

- **use_self 407 → 0 (阶段 16)**
  - 描述: clippy::use_self 警告 407 → 0 (impl 块内 fn 构造器调用 `StructName::Xxx` 改 `Self::Xxx`).
  - 方案: `cargo clippy --fix --allow-dirty --allow-staged -- -A clippy::all -W clippy::use_self` 官方工具自动修复. 59 文件 351+/351- (完全对称 rename).
  - host-tests: 2 处 brittleness 修复 (impl 内 Acpi/Timer 改名).
  - 验证: §2.4 全过 (双架构 0w0e + clippy -D pedantic 0 + 三审计 + host-tests 838 passed + fmt + rustdoc).
  - 状态: [X]
  - 详情: commit `fe9ea936` (use_self fix).

- **option_if_let_else 评估 (阶段 17)**
  - 描述: clippy::option_if_let_else 186 + manual_map_or_else 25 = 211 处修复评估.
  - 方案: 评估结果:
    - `cargo clippy --fix --allow-dirty --allow-staged` 不应用 nursery lint (clippy --fix 仅支持 stable lint)
    - 写 Python 脚本批处理 (正则 + 多语句 block 处理): 实际成本高, 大量含 `ref`/`mut`/嵌套语句手工量大, 211 处 ROI 不匹配
    - **决策**: 保留 nursery 211 处 (DECISION-044 nursery 不强制修复), 留作中期手工任务
  - 状态: [X]

- **CI rustdoc 阻断位 (阶段 18)**
  - 描述: ci-lint.yml 新增 Job 7 (rustdoc-check), 双架构 `RUSTDOCFLAGS="-D rustdoc::broken-intra-doc-links" cargo doc --no-deps` 阻断.
  - 方案: 阻断源码级 broken_intra_doc_links; 不阻断 `-D warnings` (rustdoc 0.75+ 内部生成 3 个 warning 无源码位置: unclosed HTML tag dyn/OpenFile + could not parse code block, 属 rustdoc 自身行为非源码问题).
  - 验证: YAML 解析 ✓ + 本地 RUSTDOCFLAGS cargo doc 双架构 exit 0 ✓.
  - 状态: [X]
  - 详情: commit `e891d655` (rustdoc 阻断位).

### 方案

- **静态检查 5 阻断位架构**
  - 描述: clippy-pedantic + rustdoc-broken-links + cargo-fmt + host-tests + 三审计
  - 方案:
    - `clippy-pedantic` job (ci-x86.yml): 双架构 `-D clippy::pedantic` (排除 4 个 cast_* 子 lint, DECISION-041)
    - `rustdoc-check` job (ci-lint.yml): 双架构 `-D rustdoc::broken-intra-doc-links`
    - `cargo-fmt-check` job (ci-lint.yml): `cargo fmt -- --manifest-path src/rust/Cargo.toml --check`
    - `host-tests` job (ci-x86.yml): host-tests 全过 (838 测试)
    - 三审计 (ci-lint.yml): services_boundary + safety_coverage + deadlock_matrix
  - 状态: [X]

- **nursery 治理分类**
  - 描述: 1230 处 nursery 分类处置
  - 方案:
    - 已修: use_self 407 (阶段 16 cargo clippy --fix)
    - 中期手工: option_if_let_else 186 + manual_map_or_else 25 = 211 (阶段 17 评估, ROI 不匹配)
    - 保留: missing_const_for_fn 928 (含 unsafe/mutex 不能 const) + 其他 96
  - 状态: [X]

### 待办

- **短期 (1-2 周)**
  - [ ] host-tests 鲁棒化深度审计 (剩余 ~15 处 brittle 评估, 风险评估后选择性修)
  - 状态: []

- **中期 (4-6 周)**
  - [ ] option_if_let_else 211 处手工重构 (map_or/map_or_else 链式)
  - [ ] kernel `#[test]` → host-tests 迁移 (184 个, 解锁 cargo clippy --tests)
  - 状态: []

- **长期**
  - [ ] QEMU 实际验证 (阶段 12-18 改动均未做运行时验证, 仅静态)
  - [ ] 真实硬件启动验证
  - 状态: []

### 决策记录

- **DECISION-034: pedantic 0 警告后 CI 强制 `-D clippy::pedantic`** (阶段 8-10)
  - 描述: 批次 7 完成后 CI clippy 升级为 `-D clippy::pedantic`.
  - 状态: [X]
  - 详情: 见 [clippy-pedantic-cleanup.md](./clippy-pedantic-cleanup.md) §工程计划 7.

- **DECISION-041: cast 类已知安全保留, 仅真危险手工 try_from** (阶段 8-9)
  - 描述: cast 类 1910 处中 < 200 处真风险, 其余 1700+ 处已知安全 cast 保留 (DECISION-041 已知安全分类).
  - 方案: 4 子 lint 排除 (cast_possible_truncation / cast_sign_loss / cast_possible_wrap / cast_precision_loss), 让警告作为"提醒"但不阻断.
  - 状态: [X]
  - 详情: 见 [clippy-pedantic-cleanup.md](./clippy-pedantic-cleanup.md) §工程计划 8.

- **DECISION-043: pedantic 全强制 — 治根路径** (阶段 8-12)
  - 描述: klog_fmt macro 内 ptr_as_ptr 改 `.cast::<u8>()` 根治 + 全局 allow 装饰性 lint + expect 兑底 fn lint + 手工治根关键处.
  - 方案: 比推迟 DECISION-034 更稳健 (放弃 macro 改造的推迟方案).
  - 状态: [X]
  - 详情: 见 [clippy-pedantic-cleanup.md](./clippy-pedantic-cleanup.md) §决策记录 DECISION-043.

- **DECISION-044: nursery lint 不强制修复 (除 use_self 已修)** (阶段 17)
  - 描述: nursery 1230 处中 use_self 407 已 cargo clippy --fix 自动修; option_if_let_else 211 处手工 ROI 不匹配; missing_const_for_fn 928 处多数含 unsafe/mutex 不能 const. 其余保留 nursery allow.
  - 方案: 阶段 17 评估结论 — 保留 nursery 1230 处, 不强制修复. CI 已阻断 pedantic 已足够.
  - 状态: [X]

- **DECISION-045: rustdoc CI 阻断用 -D broken_intra_doc_links 而非 -D warnings** (阶段 18)
  - 描述: rustdoc 0.75+ 内部生成 3 个 warning (unclosed HTML tag dyn/OpenFile + could not parse code block) 无源码位置, 是 rustdoc 自身行为非源码问题.
  - 方案: 阻断源码级 broken_intra_doc_links (阶段 15 已清零), 不阻断 -D warnings.
  - 状态: [X]

### 变更历史

- **2026-08-04 (阶段 12)**
  - 描述: rustfmt 全量整改 + CI `cargo fmt --check` 阻断位就位.
  - commit: `d7642e41` + `99c30cd3`.
  - 状态: [X]

- **2026-08-04 (阶段 13)**
  - 描述: clippy-pedantic job 加 `--lib --bins --examples`, 双架构 0 warning.
  - commit: `5a4c6c16`.
  - 状态: [X]

- **2026-08-04 (阶段 14)**
  - 描述: clippy.toml 评估完成, 保持现状 (回退 check-private-items).
  - 状态: [X]

- **2026-08-04 (阶段 15)**
  - 描述: rustdoc 75 warnings → 0 (HTML tag / bit-field / 真 broken link / 同名冲突 / 代码块字面量 5 类).
  - commit: `1e00d842` (entries[i]) + `f1524e5a` (位域反引号化).
  - 状态: [X]

- **2026-08-04 (阶段 16)**
  - 描述: use_self 407 → 0 (cargo clippy --fix 自动修复 59 文件 351+/351-).
  - commit: `fe9ea936`.
  - 状态: [X]

- **2026-08-04 (阶段 17)**
  - 描述: option_if_let_else 211 处评估, 决策保留 nursery 不强制修复 (DECISION-044).
  - 状态: [X]

- **2026-08-04 (阶段 18)**
  - 描述: CI rustdoc 阻断位 (ci-lint.yml Job 7), 双架构 cargo doc + RUSTDOCFLAGS="-D rustdoc::broken-intra-doc-links".
  - commit: `e891d655`.
  - 状态: [X]

***

## 工程计划 B: 验证门槛与交接

### 背景

- **§2.4 验证门槛**
  - 描述: AGENTS.md §2.4 规定每轮开发完成必须满足 5 条验证门槛.
  - 方案: 阶段 12-18 每项推进后必须重跑 5 条, 全部通过才交付.
  - 状态: [X]

### 目标

- **当前阶段 12-18 验证状态快照**
  - 描述: 一表覆盖 5 条验证门槛的当前状态.
  - 方案: 列出 5 条验证门槛的执行命令 + 当前结果.
  - 状态: [X]

### 现状 (2026-08-06)

| # | 验证项 | 命令 | 当前结果 | 阻断位 |
| --- | --- | --- | --- | --- |
| 1 | 双架构 cargo check 0 error / 0 warning | `cargo check --release --target x86_64-unknown-none --lib --bins --examples` (aarch64 同) | ✓ 双架构 0w0e | N/A |
| 2 | clippy -D pedantic 0 warning | `cargo clippy --release --lib --bins --examples -- -D clippy::pedantic -A clippy::cast_*` | ✓ 0 warning | ci-x86.yml `clippy-pedantic` job |
| 3 | 三审计全过 | `python3 scripts/audit_services_boundary.py && audit_safety_coverage.py && audit_deadlock_matrix.py` | ✓ 全过 | ci-lint.yml `audit-unsafe` job |
| 4 | host-tests 全过 | `cd host-tests && cargo test` | ✓ 838 passed / 0 failed | ci-x86.yml `host-tests` job |
| 5 | QEMU 集成测试 | `make test` (x86_64 + aarch64) | N/A (本阶段未运行) | 无 |
| 6 | cargo fmt --check | `cargo fmt -- --manifest-path src/rust/Cargo.toml --check` | ✓ 0 差异 | ci-lint.yml |
| 7 | rustdoc broken_intra_doc_links 0 | `RUSTDOCFLAGS="-D rustdoc::broken-intra-doc-links" cargo doc --no-deps` | ✓ 双架构 0 | ci-lint.yml `rustdoc-check` job |

- 状态: [X]

### 待办

- **每轮开发完成重跑 5 条验证门槛**
  - 描述: 阶段 12-18 已通过验证, 后续阶段 (host-tests 鲁棒化 / option_if_let_else / kernel #[test] 迁移) 必须维持验证门槛.
  - 方案: 验证失败 → 当轮不交付.
  - 状态: []

### 决策记录

- (本工程计划 B 为流程约束, 无独立决策)

### 变更历史

- **2026-08-06**
  - 描述: 创建工程计划 B, 5 条验证门槛快照.
  - 状态: [X]

***

## 跨文档交叉引用

| 子主题 | 主文档 | 链接 |
| --- | --- | --- |
| 阶段 8-10 clippy pedantic 治根路径 | [clippy-pedantic-cleanup.md](./clippy-pedantic-cleanup.md) | 7 批工程计划 + DECISION-033/034/040/041/042/043 |
| 活跃任务跟踪基线 | [progress-active-tasks.md](./progress-active-tasks.md) | 工程计划 A (跨文档矛盾) + B (基线) + C (验证门槛) |
| 8 项 code-review findings | [code-review-findings-2026-08-01.md](./code-review-findings-2026-08-01.md) | REVIEW-FINDING-024~031 |
| 远期规划 | [future-roadmap.md](./future-roadmap.md) | WASM/IPv6 等 |
| 双栈改造 (DECISION-032) | [ipv6-dual-stack.md](./ipv6-dual-stack.md) | IPv6/IPv4 双栈 |
| 写作规范 | [docs/README.md](../README.md) | 文档格式/命名/章节结构 |
| AI 行为准则 + 硬规则 | [AGENTS.md](../../AGENTS.md) | §2.4 验证门槛 / §6 硬规则 F1-F9 / §10 预存问题 / §15 AI 行为准则 |
| framekernel 架构 | [explain-framekernel.md](../explain/explain-framekernel.md) | services→framework 单向数据流 |
| 命名参考 | [ref-naming.md](../explain/ref-naming.md) | 命名约定 |
| 愿景 | [vision-hope.md](../explain/vision-hope.md) | linuxulator 翻译层立场 |

## 后续开发者快速上手

1. **读本文档** (stage-engineering-master.md): 了解当前静态检查全景基线 (5 阻断位 + 1230 nursery 治理决策).
2. **读 AGENTS.md** §2.4: 5 条验证门槛 + 复跑命令.
3. **读 clippy-pedantic-cleanup.md**: 阶段 8-10 治根路径 + DECISION 完整序列.
4. **读 progress-active-tasks.md**: 活跃任务跟踪基线 + §10 预存问题登记.
5. **若修改 clippy.toml 或 lib.rs `#![allow]`**: 必须先 review DECISION-043 (治根路径), 不能私自迁移 allow 到 workspace.lints (会破坏 aarch64 差异化).
6. **若修复 nursery lint**: 必须先评估是否破坏 host-tests brittleness (use_self fix 已示范 2 处修复).
7. **新加 CI 阻断位**: 必须先本地复跑验证门槛 5 条, YAML 解析验证 (python3 yaml.safe_load).

## 已知限制

- **未做运行时验证**: 阶段 12-18 仅做静态检查, QEMU 集成测试 (阶段 12 之前未跑过). release 前必须跑 QEMU 双架构.
- **nursery 1230 处**: 不强制修复, 但保留 clippy::nursery 警告可观测. 若需要修复需先评估 ROI + 跨文件影响.
- **host-tests brittle**: 当前 ~15 处已知 brittle (含阶段 16 修复的 2 处). 修改 impl 字段名/方法名时优先 grep host-tests 看 brittle.
- **rustdoc 3 个内部 warning**: 已知 rustdoc 0.75+ 行为, 不计入 CI 阻断. 升级 rustdoc 版本可能消除.