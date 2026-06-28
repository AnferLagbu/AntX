# AGENTS.md

> QueenX 内核工程 agent 工作指南.
>
> **硬规则**: 见 [§6 硬规则 (零容忍)](#6-硬规则-零容忍违反即拒收). 违反任一即拒收 PR.
>
> **必读** (开始任何任务前): 本文件 + `CLAUDE.md` + `docs/README.md` + `docs/explain/framekernel-nature.md` + `docs/explain/framekernel-dev-guide.md` + `docs/explain/engineering-discipline-spec.md`.

***

## 1. 仓库布局

| 目录                                        | 用途                               |
| ----------------------------------------- | -------------------------------- |
| `src/kernel/framework/`                   | TCB 子树 (允许 unsafe, 硬件抽象)         |
| `src/kernel/services/`                    | 100% safe Rust 子树 (策略与业务)        |
| `src/kernel/services/net/smoltcp/`        | smoltcp vendored (3rd-party, 锁定) |
| `src/user/`, `src/userland/`, `src/rust/` | 用户态程序与工具                         |
| `host-tests/`                             | 主机端单元/集成测试 (no\_std + std)       |
| `docs/plan/`                              | 工程与任务计划                          |
| `docs/explain/`                           | 项目引导与解释                          |
| `docs/CHANGELOG.md`                       | 面向用户/接手人的变更日志                    |
| `scripts/`                                | 审计/构建/集成脚本                       |
| `ci/`                                     | CI 入口 (build.sh + audit.sh)      |
| `tools/`                                  | 工具脚本 (track\_todo.py 等)          |

***

## 2. 构建与测试

### 2.1 构建命令

```bash
# 双架构编译
./ci/build.sh all                  # x86_64 + aarch64, 0 error / 0 warning

# 单架构 (开发时)
./ci/build.sh x86_64

# 集成测试 (QEMU)
./ci/test_qemu.sh x86_64           # 启动 QEMU 跑 boot + 内核测试
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

任何一项失败视为本轮未完成.

### 2.3 主机测试

```bash
make test-host                     # host-tests 全套
cargo test -p host-tests           # 等价
```

### 2.4 验证门槛

每轮开发完成, **必须** 全部满足:

1. 双架构 `cargo check --release` 0 error / 0 warning
2. clippy 0 warning (`cargo clippy --release -- -D warnings`)
3. 三审计全部通过 (boundary + safety\_coverage + deadlock\_matrix)
4. host-tests 全部通过
5. QEMU 集成测试通过 (如改动 boot/架构相关)

***

## 3. 工具链

- **Rust nightly** 锁定在 `src/rust/rust-toolchain.toml`
- **Edition:** 2021 (与上游对齐, QueenX 未升 2024)
- **目标架构:** x86\_64 (主) + aarch64 (次)
- **`rustfmt.toml`:** 4 空格缩进, 尾逗号允许
- **Clippy 配置:** `clippy.toml` (cognitive-complexity-threshold = 25, missing-docs-in-crate-items = true)

***

## 4. 架构责任分离 (核心)

> 这是 QueenX 与 Asterinas 最关键的区别: 我们用 `framework/` + `services/`, 不是 `ostd/` + `kernel/`.

### 4.1 一句话判据

**要 unsafe 吗? 要 → framework. 不要 → services. 进一步: 涉及硬件/MMU/中断/上下文切换? → framework. 纯算法/策略/业务? → services.**

### 4.2 资源分类 (OSTD 四准则 + 6 不变式)

| 类别    | 定义         | 归属                       |
| ----- | ---------- | ------------------------ |
| 敏感资源  | 被篡改可导致 UB  | **framework** (TCB)      |
| 非敏感资源 | 被篡改仅导致逻辑错误 | **services** (safe Rust) |

详见 `docs/explain/framekernel-nature.md` 与 `docs/explain/framekernel-dev-guide.md`.

### 4.3 6 安全不变式自检

修改 framework 时逐项确认 (详见 `engineering-discipline-spec.md`):

- I1 内核态 CPU 状态不可被 services 篡改
- I2 内核内存不可被 services 非法访问
- I3 用户态 CPU 状态只能通过 framework 安全入口
- I4 用户内存只能通过 framework 安全代理
- I5 外设 MMIO/PIO 只能通过 framework 安全代理
- I6 外设 DMA 不可写入内核内存

任何一项回答"是" = 违反安全不变式, 必须重新设计.

***

## 5. 编码规范

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
| F6 | 三审计全部通过                                  | boundary + safety + deadlock                           |
| F7 | 中文注释强制                                   | `audit_comment_language.py` 0 violations               |
| F8 | 公共 API 中文文档注释                            | clippy `missing_docs_in_crate_items`                   |

***

## 7. 文档规范说明

`docs/README.md` 中定义的"标题 + 章节 + 条目(描述+方案+状态) + 详情"格式**只适用于** **`docs/`** **下文档** (`docs/plan/`, `docs/explain/`, `docs/CHANGELOG.md`).

**AGENTS.md (本文件) 与 CLAUDE.md 是规则/指导文档, 不受该格式约束, 保持自然描述风格**:

- 用 H1/H2/H3 自由组织, 不强制"每条带状态 \[X]/\[]"
- 用表格 + 代码块 + 列表自然描述
- 引用其他文档的格式仅作为参考, 本身不需要 `描述:` `方案:` `状态:` `详情:` 结构
- 修改这两份文件时, 优先考虑"可读性 + 可执行性"而非"格式统一"

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

项目用双远程仓库, 语义化命名, 区分主仓库与镜像:

| remote 名 | URL 协议                                     | 角色         | 默认推送 |
| -------- | ------------------------------------------ | ---------- | ---- |
| `Gitee`  | `git@gitee.com:AnferLagbu/QueenX.git`      | 主仓库 (推送首发) | ✅ 是  |
| `GitHub` | `https://github.com/AnferLagbu/QueenX.git` | 镜像 (国际可访问) | 否    |

**常用命令:**

```bash
# 推两处 (推荐)
git pushall   # alias: git push Gitee main && git push GitHub main

# 推单处
git push Gitee main
git push GitHub main

# 拉取
git pull Gitee main --rebase
git pull GitHub main --rebase
git pullall   # alias: 优先 Gitee, 尝试 GitHub
```

**首次克隆后配置:**

```bash
git clone git@gitee.com:AnferLagbu/QueenX.git  # 克隆主仓库 (默认 remote = origin)
cd QueenX
git remote rename origin Gitee
git remote add GitHub https://github.com/AnferLagbu/QueenX.git
git remote -v  # 验证
git config alias.pushall '!git push Gitee main && git push GitHub main'
git config alias.pullall '!git pull --rebase Gitee main && git pull --rebase GitHub main 2>/dev/null; true'
```

**禁止:**

- 禁止用 `origin` 模糊命名 (无歧义)
- 禁止假设 GitHub 是主仓库 (网络可达性 + Gitee 是项目作者所属平台)
- 禁止在脚本/CI 中硬编码 `"origin"` 字面字符串 (用变量或 remote 名)

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

开发中遇到与本任务无关的预存问题 (编译告警/死代码/未使用 import/过期 TODO/CI 缺陷/文档不一致) 必须立即修复并补测试或更新文档. 修复后重跑双架构编译、相关审计、相关测试. 不接受:

- 留下 TODO 等下一轮
- 以不在本任务范围为由略过
- 删除有意义的测试以让编译通过

存量代码可按渐进式策略修复 (`触及时修复` → `标记待修` → `禁止忽视` → `新代码零容忍`).

***

## 11. 开发规定

### 11.1 实施前

每次开发工作进行前必须深度理解项目源码实现. 除非特殊必要场景, 避免代码高耦合. 开发过程中坚决不允许出现功能不全或功能实现简化导致后期维护难度大的代码. 项目代码仅在必要时参考业界惯例或 Linux 实现, 但**绝不盲从 Linux 实现**.

### 11.2 实施中

- **外科手术式修改** (详见 CLAUDE.md): 只改必须改的, 不顺手优化.
- **简单优先** (详见 CLAUDE.md): 200 行能 50 行写完, 重写.
- **目标驱动**: 先定义成功标准, 再循环推进到验证通过.
- **不要假设, 不要掩饰困惑**: 实现前明确假设, 多种解释先列出来.

### 11.3 实施后

完成开发后, **必须**:

- 双架构编译 0 warning 0 error
- 所有审计 (clippy + 项目脚本) 通过
- 所有测试 (host-tests + QEMU 集成) 通过
- 文档同步更新 (CHANGELOG.md + plan/ + explain/)
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
| 不读 CLAUDE.md                                 | 必读! LLM 行为准则在此                                    |

***

## 13. CI 状态徽章

本地跑全量 CI:

```bash
./ci/build.sh all
python3 scripts/audit_services_boundary.py
python3 scripts/audit_safety_coverage.py
python3 scripts/audit_deadlock_matrix.py
python3 scripts/audit_coupling.py
python3 scripts/audit_invariants.py
python3 scripts/audit_comment_language.py
make test-host
```

任何一项失败 → 视为本轮未完成.

***

## 14. 关联文档

### 14.1 依赖 (开发必读)

- [docs/README.md](docs/README.md) — 文档写作规范
- [docs/explain/framekernel-nature.md](docs/explain/framekernel-nature.md) — 框内核原理 + 6 不变式
- [docs/explain/framekernel-dev-guide.md](docs/explain/framekernel-dev-guide.md) — 架构开发场景
- [docs/explain/engineering-discipline-spec.md](docs/explain/engineering-discipline-spec.md) — 工程纪律规范
- [CLAUDE.md](CLAUDE.md) — LLM 行为准则

### 14.2 进度 (了解状态)

到 `docs/` 下阅读工程所需的文档, 根据文档的命名语义来进行阅读.

### 14.3 引用 (按需查阅)

到 `docs/plan/` 下按命名语义检索子系统设计文档; 历史设计已归档至 `docs/plan/archive/`, 按主题关键字检索定位.

### 14.4 归档 (历史决策)

- `docs/plan/archive/` — 旧维护文档 + 旧审计报告, 保留历史决策

