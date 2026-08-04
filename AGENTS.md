# AGENTS.md

> QueenX 内核工程 agent 工作指南.
>
> **硬规则**: 见 [§6 硬规则 (零容忍)](#6-硬规则-零容忍违反即拒收). 违反任一即拒收 PR.
>
> **必读** (开始任何任务前): 本文件 + `docs/README.md` + `docs/explain/` 下所有文档.

## 目录

### 核心契约 (不可妥协)

- [§1. 仓库布局](#1-仓库布局) — 含决策者-实施者分工
- [§4. 架构责任分离 (核心)](#4-架构责任分离-核心) — 含 6 安全不变式 (§4.3 核心契约 / §4.4 自检清单)
- [§6. 硬规则 (零容忍)](#6-硬规则-零容忍违反即拒收) — F1-F8 机器可检查

### 开发流程

- [§2. 构建与测试](#2-构建与测试) — 含 §2.4 验证门槛 5 条
- [§3. 工具链](#3-工具链) — Rust nightly + 双架构
- [§5. 编码规范](#5-编码规范) — 软规范 (非拒收条款)
- [§9. 测试规范](#9-测试规范)
- [§13. CI 状态徽章](#13-ci-状态徽章-§24-验证门槛的具体命令清单) — §2.4 命令清单

### 协作与决策

- [§7. 文档规范说明](#7-文档规范说明)
- [§8. Git 规范](#8-git-规范) — 含 §8.4 Remote 命名约定
- [§10. 预存问题处理](#10-预存问题处理) — 含 §10.2 文档同步 / §10.4 AI 输出审查
- [§11. 开发规定](#11-开发规定) — 含决策者-实施者分工
- [§12. AI 常见踩坑](#12-ai-常见踩坑)
- [§14. 关联文档](#14-关联文档) — 按主题模糊定位
- [§15. AI 行为准则 (原 CLAUDE.md)](#15-ai-行为准则-原-claudemd) — 编码前先思考、简单优先、外科手术、目标驱动

***

## 1. 仓库布局

| 目录                                        | 用途                               | 维护者                  |
| ----------------------------------------- | -------------------------------- | -------------------- |
| `src/kernel/framework/`                   | TCB 子树 (允许 unsafe, 硬件抽象)         | 用户 决策 / AI 实施  |
| `src/kernel/services/`                    | 100% safe Rust 子树 (策略与业务)        | 用户 决策 / AI 实施  |
| `src/kernel/services/net/smoltcp/`        | smoltcp vendored (3rd-party, 锁定) | 升级时 用户 授权 + AI |
| `src/user/`, `src/userland/`, `src/rust/` | 用户态程序与工具                         | AI 实施 / 用户 审查  |
| `host-tests/`                             | 主机端单元/集成测试 (no\_std + std)       | AI 实施 / 用户 审查  |
| `docs/plan/`                              | 工程与任务计划                          | 用户 决策 + AI 撰写  |
| `docs/explain/`                           | 项目引导与解释                          | 用户 决策 + AI 撰写  |
| `docs/CHANGELOG.md`                       | 面向用户/接手人的变更日志                    | AI 起草 / 用户 定稿  |
| `scripts/`                                | 审计/构建/集成脚本                       | AI 实施 / 用户 审查  |
| `ci/`                                     | CI 入口 (build.sh + audit.sh)      | AI 实施 / 用户 审查  |
| `tools/`                                  | 工具脚本 (track\_todo.py 等)          | AI 实施 / 用户 审查  |

> **项目分工**: 用户 负责边界约束与方向决策, AI (LLM agent) 负责具体实施. 见 §10.4 与 §11.2.

***

## 2. 构建与测试

### 2.1 构建命令

```bash
# 双架构编译
./ci/build.sh all                  # x86_64 + aarch64, 0 error / 0 warning

# 单架构 (开发时)
./ci/build.sh x86_64

# 集成测试 (QEMU)
./scripts/qemu_boot_test.sh x86_64       # 启动 QEMU 跑 boot + 内核测试
```

### 2.2 审计脚本

| 脚本                                    | 作用                                  | 阈值      |
| ------------------------------------- | ----------------------------------- | ------- |
| `scripts/audit_services_boundary.py`  | services 0 unsafe + 顶层 re-export 强制 | 硬       |
| `scripts/audit_safety_coverage.py`    | framework unsafe 块 SAFETY 100% 覆盖   | 硬       |
| `scripts/audit_deadlock_matrix.py`    | 锁顺序 + 中断上下文 + 递归锁检测                 | 硬       |
| `scripts/audit_coupling.py`           | 跨模块循环依赖 + 公开接口统计                    | 硬       |
| `scripts/audit_comment_language.py`   | 中文注释强制 (除非豁免)                       | 硬 0     |
| `scripts/audit_invariants.py`         | 6 安全不变式断言                           | 硬       |
| `scripts/audit_tcb_ratio.py`          | TCB 占比统计                            | 软 < 30% |
| `scripts/audit_once_cell.py`          | OnceCell 模式统一                       | 硬       |
| `scripts/audit_c_naming.py`           | C 命名规范                              | 硬       |
| `scripts/audit_block_registration.py` | 块设备注册                               | 硬       |
| `scripts/audit_repr_c.py`           | LTO 字段错位防线: 关键 struct repr(C) 检查    | 硬       |
| `scripts/audit_volatile_access.py`  | LTO 字段错位防线: volatile 访问检查        | 硬       |
| `scripts/audit_static_mut.py`       | framework 层 static mut 使用审查            | 硬       |
| `scripts/audit_public_api_docs.py`  | pub API 中文文档检查 (informational)          | 软       |
| `scripts/audit_dead_code.py`        | dead_code 零容忍                          | 已移除, 依赖 Rust 编译器 |

任何一项失败视为本轮未完成.

### 2.3 主机测试

```bash
make test-host                     # host-tests 全套
cargo test -p host-tests           # 等价
```

### 2.4 验证门槛

每轮开发完成, **必须** 全部满足:

1. 双架构 `cargo check --release` 0 error / 0 warning
2. clippy 0 warning (`cargo clippy --release -- -D warnings`) (当前 CI 仅强制 `unsafe_code` lint, 全量 `-D warnings` 是中长期目标)
3. 审计全部通过: `ci/audit.sh` (边界/不变式/unsafe扫描/块注册/OnceCell/C命名/注释语言) + GitHub Actions (safety_coverage + services_boundary + deadlock_matrix)
4. host-tests 全部通过
5. QEMU 集成测试通过 (如改动 boot/架构相关)

***

## 3. 工具链

- **Rust nightly** 锁定在 `src/rust/rust-toolchain.toml`
- **Edition:** 2024 (2026-07-31 起全项目统一; vendored smoltcp 跟随其上游)
- **目标架构:** x86\_64 (主) + aarch64 (次)
- **`rustfmt.toml`:** 4 空格缩进, 尾逗号允许
- **Clippy 配置:** `clippy.toml` (cognitive-complexity-threshold = 25, missing-docs-in-crate-items = true)

***

## 4. 架构责任分离 (核心)

### 4.1 一句话判据

**要 unsafe 吗? 要 → framework. 不要 → services. 进一步: 涉及硬件/MMU/中断/上下文切换? → framework. 纯算法/策略/业务? → services.**

### 4.2 资源分类 (OSTD 四准则 + 6 不变式)

| 类别    | 定义         | 归属                       |
| ----- | ---------- | ------------------------ |
| 敏感资源  | 被篡改可导致 UB  | **framework** (TCB)      |
| 非敏感资源 | 被篡改仅导致逻辑错误 | **services** (safe Rust) |

详见 `docs/explain/explain-framekernel.md` 与 `docs/explain/guide-dev.md`.

### 4.3 6 安全不变式 (核心契约)

> **本节是 AGENTS.md 的核心安全契约. 任何修改 framework 的 PR 必须先逐项自检 §4.4 的 6 条不变式, 再动代码.** 违反任一不变式 = 重新设计 (不允许补丁式补救).

### 4.4 6 安全不变式自检清单

修改 framework 时逐项确认 (详见 `spec-engineering.md`):

- I1 内核态 CPU 状态不可被 services 篡改
- I2 内核内存不可被 services 非法访问
- I3 用户态 CPU 状态只能通过 framework 安全入口
- I4 用户内存只能通过 framework 安全代理
- I5 外设 MMIO/PIO 只能通过 framework 安全代理
- I6 外设 DMA 不可写入内核内存

任何一项回答"是" = 违反安全不变式, 必须重新设计.

***

## 5. 编码规范

> **本节是"建议性规范", 违反不直接拒收, 但应在 PR 评审中提出.** 真正的零容忍条款见 §6. 与 §6 关系: §6 是机器可检查的硬约束 (CI 脚本判定), §5 是人可评审的软规范 (代码评审关注).

### 5.1 通用

- **Be descriptive.** 无单字母命名. 避免歧义缩写.
- **Explain why, not what.** 注释解释动机, 不复述代码.
- **One concept per file.** 文件过长即拆分.
- **Top-down reading.** 入口在前, 实现细节在后.
- **Narrowest visibility by default.** 默认 `pub(super)`/`pub(crate)`, 仅在必要时 `pub`.
- **Validate at boundaries, trust internally.** 系统调用入口校验, 内部信任已校验值.
- **No I/O or blocking under spinlock.** 自旋锁内禁止调度/yield/分配.
- **No premature optimization.** 无 benchmark 证据不优化.

### 5.2 Rust

- **命名:**
  - CamelCase 类型与枚举; snake\_case 函数与变量; SCREAMING\_SNAKE\_CASE 常量.
  - 大写首字母缩写 (如 `IoMemory` 而非 `IOMemory`).
  - 闭包变量以 `_fn` 结尾 (如 `compare_fn`).
- **函数:** 保持小而聚焦; 嵌套 ≤ 3 层; 用 early return, `let...else`, `?`.
- **类型:** 强类型优先 (NewType/Pid/Handle). 闭合集用 enum 而非 trait object. 字段封装在 getter 之后.
- **算术:** `checked_*` / `saturating_*` 显式标注; 禁止裸 `+`/`*` 在敏感路径.
- **Unsafe:**
  - 每个 `unsafe` 块必须配 `// SAFETY: <前提>; <调用方保证>; <硬件契约>`.
  - 每个 `unsafe fn`/`unsafe trait` 必须配 `# Safety` doc section.
  - services/ 顶层 `mod.rs` `#![deny(unsafe_code)]`, 0 unsafe 编译期强制.
  - 仅 framework/ 允许 unsafe.
- **模块:** 默认 `pub(super)`/`pub(crate)`; 导入 free function 与 static 走父模块 (`use std::mem;` + `mem::replace()`, 不 `use std::mem::replace;` + `replace()`).
- **错误处理:** 传播用 `?`. 禁止 `.unwrap()` 在可失败处. 统一用 `KernelError` (跨子系统) 不可传递子系统内部错误.
- **日志:** 用 `klog::printk` 或 framework 提供的日志宏. no\_std 下无 `println!`.
- **并发:** 建立并文档化锁顺序. 持锁期禁止 sleep/yield/分配. atomics 不滥用.
- **属性:** 派生 macro 放最后 (`#[derive(...)]`), 内部 trait 按字母序. `#[expect(...)]` 优于 `#[allow(...)]`.
- **性能:** 热路径避免 O(n) 扫描; 减少拷贝/分配/`Arc::clone`. 无 benchmark 不优化.
- **文档注释:** 第一行第三人称单数现在时 (Returns/Creates/...). 句末加标点. 标识符用反引号.
- **中文注释:** 强制 (见 [§6 硬规则](#6-硬规则-零容忍违反即拒收)).

### 5.3 Trait 抽象

- 策略 trait 定义在 `framework::api`, 实现放 `services`.
- 静态分发优先 (`impl Trait` 优于 `Box<dyn Trait>`).
- 关键方法配 `#[inline(always)]` 强制内联.
- 避免在 poll 路径分配/锁.

### 5.4 类型与内存

- 优先强类型 (NewType/Pid/Handle).
- 公共 API 禁止裸指针 (`&T`/`&mut T`/智能指针).
- 分配失败必须显式处理 (`Result`/`Option`, 禁止 `unwrap`).

### 5.5 Assembly (少量)

- `.balign` 优于 `.align` (字节对齐明确).
- Rust-callable 函数配 `.type` + `.size`.
- 标签前缀唯一 (避免 `global_asm!` 冲突).

***

## 6. 硬规则 (零容忍, 违反即拒收)

| #  | 规则                                       | 检查方式                                                   |
| -- | ---------------------------------------- | ------------------------------------------------------ |
| F1 | services 层 0 unsafe                      | `#![deny(unsafe_code)]` + `audit_services_boundary.py` |
| F2 | services 禁止访问 framework 内部模块             | `audit_services_boundary.py` 黑名单                       |
| F3 | 新增代码禁止引入模块间循环依赖                          | `audit_coupling.py`                                    |
| F4 | framework 任何 unsafe 块必须配 `// SAFETY:` 注释 | `audit_safety_coverage.py`                             |
| F5 | 双架构编译 0 warning 0 error                  | `./ci/build.sh all`                                    |
| F6 | 审计全部通过 | boundary + safety + deadlock + ci/audit.sh 全量 |
| F7 | 中文注释强制                                   | `audit_comment_language.py` 0 violations               |
| F8 | 公共 API 中文文档注释                            | clippy `missing_docs_in_crate_items`                   |
| F9 | 新增代码禁止 `#[allow(dead_code)]`                | Rust 编译器 dead_code lint |

***

## 7. 文档规范说明

`docs/README.md` 中定义的"标题 + 章节 + 条目(描述+方案+状态) + 详情"格式**只适用于** **`docs/`** **下文档** (`docs/plan/`, `docs/explain/`).

**本文件是规则/指导文档, 不受该格式约束, 保持自然描述风格**:

- 用 H1/H2/H3 自由组织, 不强制"每条带状态 \[X]/\[]"
- 用表格 + 代码块 + 列表自然描述
- 引用其他文档的格式仅作为参考, 本身不需要 `描述:` `方案:` `状态:` `详情:` 结构
- 优先考虑"可读性 + 可执行性"而非"格式统一"

***

## 8. Git 规范

### 8.1 Commit 规范

- **主题行:** 命令式, ≤ 72 字符.
- **常见前缀:** `Fix` / `Add` / `Remove` / `Refactor` / `Rename` / `Implement` / `Enable` / `Clean up` / `Bump` / `docs` / `test` / `build` / `chore`.
- **原子 commit:** 一个 commit 一个逻辑变更.
- **重构与功能分离:** 不在同一 commit 混重构与新功能.
- **Scope 标识:** 主题行 `<prefix>(<scope>): <description>`, scope = `net`/`mm`/`fs`/`proc`/`driver`/`net-stack`/`tests`/`docs` 等.

### 8.2 示例

```
feat(net): W4.2.3 socket_open_stub Tcp/Udp 实装
fix(tests): test_runner_init 缺 init_global — DevFS::mount panic 修复
docs(plan): 重写 5 个 plan/ 文档
test(integration): DRIVER-2 QEMU virtio-vga 双层验证
```

### 8.3 PR 规范

- 一个 PR 一个主题.
- CI 必须全部通过.
- TCB 占比上升需在 PR 描述中说明原因与后续降低计划.
- 新增 framework unsafe 块需 review 多名 reviewer.

### 8.4 Remote 命名约定

项目使用 Gitee 作为唯一远程仓库, remote 名为 `origin` (2026-07-31 起统一).

```bash
git remote add origin git@gitee.com:AnferLagbu/QueenX.git
git push origin main
git pull origin main --rebase
```

**历史归档例外:** `docs/plan/archive/*` 中 git 命令示例保留 `origin` 字面字符串 (2026-06-13 历史快照), 不得修改. `docs/plan/smoltcp-framekernel-wrapper.md` 中 `git fetch origin` 指 smoltcp 子模块的 origin (非 QueenX remote), 不得修改.

***

## 9. 测试规范

- **每个 bug 修复加回归测试** (附 issue 引用).
- **测用户可见行为** (公共 API), 不测内部实现.
- **用 assertion macro**, 不用手动输出检查.
- **清理资源** (fd, 临时文件, 子进程).
- 修改跨模块接口必须补充集成测试 (host-tests).
- 公共 API 必须有单元测试 (no\_std 单元 + host-tests 集成).
- 性能基线 (`host-tests/benches/baseline.json`) 每次 PR 更新.

***

## 10. 预存问题处理

### 10.1 一般预存问题

开发中遇到任何与本任务无关或有关的预存问题 (例如：编译告警/死代码/未使用 import/过期 TODO/CI 缺陷/文档不一致) 必须立即修复并补测试或更新文档. 修复后重跑双架构编译、相关审计、相关测试. 不接受:

- 留下 TODO 等下一轮
- 以不在本任务范围为由略过
- 删除有意义的测试以让编译通过

存量代码可按渐进式策略修复 (`触及时修复` → `标记待修` → `禁止忽视` → `新代码零容忍`).

**死代码零容忍**: 新增代码和预存代码一律禁止 `#[allow(dead_code)]`. 无豁免, 无例外. 硬件规范常量必须通过实现使用路径消除. 见 §6 F9.

### 10.2 文档与代码不同步

当代码实装完成但对应 plan/explain 文档状态仍标 `[]` 或未更新时, 视为预存问题, 立即同步. 流程:

1. `grep '状态: \[\]' docs/plan/*.md` 找未完成项
2. 对照 git log 验证实装是否完成
3. 已完成的项改为 `[X]` + 详情列出 commit hash
4. 重跑 §2.4 全部 5 条验证门槛

### 10.3 决策灰色地带

遇到方案 A vs B 选哪个的决策时, AI 应停下询问 用户 而非自行选择. 这类决策属 §11.2 决策者-实施者分工中的"决策者"职责.

### 10.4 AI 输出审查

> **本节是 AI 实施模型下的特殊处理流程.** 用户 负责最终审查与决策, AI 负责实施. 任何 AI 输出 (代码/文档/脚本) 在合并前必须经用户审查以下清单:

- **架构合规**: 未越过 §6 硬规则 (F1-F9), 未引入 services unsafe
- **安全注释**: framework `unsafe` 块都有 `// SAFETY:` 注释 (§6 F4)
- **决策溯源**: 关键设计选择 (方案 A/B) 有对应 commit 消息或 plan 文档记录
- **测试覆盖**: 新增代码有单元测试, 跨模块接口有集成测试 (host-tests)
- **文档同步**: API 改动对应 docs/explain 或 docs/plan 同步更新
- **不留 TODO**: 无 `// TODO(TRACK-...)` 未处理项 (除明确登记在 plan 文档的)
- **风格一致**: 命名/注释/格式符合 §5 编码规范
- **不盲目重构**: 未对用户未要求的部分做"顺手优化" (见 §15.3 外科手术式修改)

AI 输出若不通过上述审查, 视为预存问题, 必须修复后才能合并.

***

## 11. 开发规定

### 11.1 实施前

每次开发工作进行前必须深度理解项目源码实现. 除非特殊必要场景, 避免代码高耦合. 开发过程中坚决不允许出现功能不全或功能实现简化导致后期维护难度大的代码. 项目代码仅在必要时参考业界惯例或 Linux 实现, 但**绝不盲从 Linux 实现**. 详见 [docs/explain/linux-compat-philosophy.md](docs/explain/linux-compat-philosophy.md) 三层兼容策略.

### 11.2 实施中

> **项目分工 (见 §1)**: 用户 负责方向决策与边界约束, AI 负责具体实施. 实施中的"外科手术式修改"与"简单优先"由 AI 严格遵守; 决策灰色地带 (§10.3) 由用户决策.

- **外科手术式修改** (见 §15.3): 只改必须改的, 不顺手优化.
- **简单优先** (见 §15.2): 200 行能 50 行写完, 重写.
- **目标驱动**: 先定义成功标准, 再循环推进到验证通过.
- **不要假设, 不要掩饰困惑**: 实现前明确假设, 多种解释先列出来.

### 11.3 实施后

完成开发后, **必须**:

- 双架构编译 0 warning 0 error
- 所有审计 (clippy + 项目脚本) 通过
- 所有测试 (host-tests + QEMU 集成) 通过
- 文档同步更新 (plan/ + explain/; 变更记录由 git commit 承担, 不维护独立 CHANGELOG)
- 代码无新增 services unsafe / 循环依赖 / 跨子系统内部访问

***

## 12. AI 常见踩坑

| 踩坑                                           | 解决                                                |
| -------------------------------------------- | ------------------------------------------------- |
| 把 unsafe 写进 services/                        | 编译失败, 改用 framework 公开的 safe API                   |
| 在 services/ 用 `println!`                     | no\_std 不可用, 改用 `klog::printk` 或 framework 日志 API |
| 中断上下文持 Mutex 或分配 `GFP_KERNEL`                | 死锁, 中断路径只持自旋锁并 disable IRQ                        |
| 修 bug 时顺手清理无关代码                              | 禁止, 每一行改动追溯到用户请求                                  |
| 在 services/ 直接 `use framework::arch::x86_64` | 边界审计拒绝, 走顶层 re-export 公共 API                      |
| 跳过 SAFETY 注释                                 | `audit_safety_coverage.py` 检测, 100% 强制            |
| 跨子系统硬编码常量                                    | 走 `framework::config` 或 `services::config`        |
| 测试代码 `unwrap()`                              | 测试允许, 生产代码禁止                                      |
| 提交前忘跑审计                                      | CI 会拦, 不会合入                                       |
| 顺手添加"灵活配置"                                   | 禁止, 准则 §0 严格适用                                    |
| 不读 AGENTS.md §15 AI 行为准则                     | 必读! LLM 行为准则在此                                    |
| 引入 `#[allow(dead_code)]`                        | 零容忍 (§6 F9), 必须通过实现使用路径消除                  |

***

## 13. CI 状态徽章 (§2.4 验证门槛的具体命令清单)

> **本节是 §2.4 验证门槛 5 条标准的对应命令清单.** 一一对应: 双架构编译 → build.sh / 审计 → 4 个 audit / clippy → clippy / host-tests → make test-host / QEMU → test\_qemu.sh. 任何一项失败 → 该轮未完成 (见 §2.4).

完整命令:

```bash
# §2.4 #1 双架构编译 (替代 cargo check)
./ci/build.sh all                  # x86_64 + aarch64, 0 error / 0 warning

# §2.4 #2 clippy 0 warning (替代 cargo clippy)
cargo clippy --release -- -D warnings

# §2.4 #3 三审计 (F6 硬规则, CI 门槛)
python3 scripts/audit_services_boundary.py    # F1 + F2
python3 scripts/audit_safety_coverage.py      # F4
python3 scripts/audit_deadlock_matrix.py      # 锁顺序 + 中断上下文 + 递归锁

# §2.4 #3 扩展审计 (软, 仍建议跑)
python3 scripts/audit_coupling.py
python3 scripts/audit_invariants.py
python3 scripts/audit_comment_language.py     # F7

# §2.4 #4 host-tests
make test-host
# 或等价: cargo test -p host-tests

# §2.4 #5 QEMU 集成测试 (改动 boot/架构相关时必跑)
./scripts/qemu_boot_test.sh x86_64
```

***

## 14. 关联文档

### 14.1 依赖 (开发必读)

- [docs/README.md](docs/README.md) — 文档写作规范
- [docs/explain/](docs/explain/) — 项目引导与架构文档 (全部必读)

### 14.2 进度 (了解状态)

到 `docs/` 下阅读工程所需的文档, 根据文档的命名语义来进行阅读.

### 14.3 引用 (按需查阅)

到 `docs/plan/` 下按命名语义检索子系统设计文档; 历史设计已归档至 `docs/plan/archive/`, 按主题关键字检索定位.

### 14.4 归档 (历史决策)

- `docs/plan/archive/` — 旧维护文档 + 旧审计报告, 保留历史决策

***

## 15. AI 行为准则

> QueenX 项目的 **LLM 通用行为准则**. 以下准则与上述项目特定规则协同使用.
>
> **关系**: 前述各节 (§§1–14) 是项目主体 (硬规则/边界/流程/验证门槛), 本节是 AI 行为底线. 冲突时 **以前述项目规则为准**.
>
> **权衡:** 这些准则更偏向谨慎而不是速度. 对于琐碎任务, 请自行判断.

### 15.1 编码前先思考

**不要假设. 不要掩饰困惑. 明确呈现权衡.**

在实现之前:

- 明确写出你的假设. 如果不确定, 就提问.
- 如果存在多种解释, 先把它们列出来, 不要默默自行选择.
- 如果有更简单的方法, 就直接指出来. 在有必要时提出异议.
- **决策灰色地带** (方案 A vs B 选择): 停下询问用户, 不擅自选择. 见 §10.3.
- 如果有不清楚的地方, 就停下来. 说清楚困惑点, 并提问.

### 15.2 简单优先

**只写解决问题所需的最少代码. 不做任何预设性扩展.**

- 不要加入超出需求范围的功能.
- 不要为一次性代码做抽象.
- 不要加入未被要求的"灵活性"或"可配置性".
- 不要为不可能发生的场景写错误处理.
- 如果你写了 200 行, 但 50 行就够, 就重写.

问问自己: "一个资深工程师会认为这太复杂了吗?" 如果答案是会, 那就继续简化.

### 15.3 外科手术式修改

**只改必须改的内容. 只清理你自己造成的问题.**

编辑现有代码时:

- 不要"顺手优化"相邻代码、注释或格式.
- 不要重构没有坏掉的部分.
- 保持现有风格, 即使你个人会写成别的样子.
- **如果发现无关的预存问题** (未使用 import / 死代码 / 文档不一致等): 可以指出, 但 **不要顺手删除**. 报告给用户, 等待单开 PR 或明确授权. 见 §15.4 工程外问题代码处置.

当你的改动产生遗留项时:

- 删除那些因你的修改而变成未使用的 import、变量或函数.
- **死代码零容忍**: 发现任何死代码 (包括预存的), 必须一并消除. 见 §6 F9.

检验标准: 每一行改动都应当能直接追溯到用户请求.

### 15.4 工程外问题代码处置

**在执行当前工程任务时, 若发现与本次工程无关的问题代码 (无论源自 AGENTS.md 规则审计还是自主阅读源码), 一律通过提问工具向用户确认处置方式, 不得自行决定跳过或顺手修复.**

三种处置选项:

| 选项 | 含义 | 适用场景 |
|------|------|---------|
| **规划方案并修复** | 单开 PR 或在本轮一并修复 | 问题明确、修复成本低、不影响当前工程 |
| **记录到 docs/plan/** | 写入 plan 文档登记, 等待后续处理 | 问题有效但修复依赖其他前置工作, 或工作量较大 |
| **搁置与跳过** | 当前不做处理, 也不记录 | 问题暂不需要修复 (如远期重构、非关键瑕疵) |

**流程**: 发现问题 → 停下当前工程思考 → 通过提问工具向用户呈现问题描述 + 三个选项 → 按用户选择执行. 禁止在用户未确认前自行处置.

**例外**: 因本次工程改动**直接导致**的遗留项 (未使用 import / 死代码 / 文档不一致) 属于 §10.1 范畴, 必须在本轮修复, 不需询问.

### 15.5 目标驱动执行

**先定义成功标准, 再循环推进, 直到验证通过.**

把任务转换成可验证的目标:

- "添加校验" → "先为非法输入写测试, 再让测试通过"
- "修复这个 bug" → "先写能复现它的测试, 再让测试通过"
- "重构 X" → "确保改动前后测试都通过"

对于多步骤任务, 先给出简短计划:

```
1. [步骤] → 验证: [检查项]
2. [步骤] → 验证: [检查项]
3. [步骤] → 验证: [检查项]
```

具体的项目验证门槛 (双架构编译 / 三审计 / host-tests / QEMU) 见 §2.4 + §13. 任何一项失败 → 本轮未完成.

---

**如果这些准则正在发挥作用, 你会看到:** diff 中不必要的改动更少, 因为过度复杂而返工的次数更少, 而且澄清性问题会出现在实现之前, 而不是出错之后.

