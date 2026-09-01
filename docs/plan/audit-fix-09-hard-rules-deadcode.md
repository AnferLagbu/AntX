# 审计修复分册 09：硬规则合规与死代码治理

> 修复 F1/F9 硬规则违反（services 缺 deny、host-tests allow(dead_code)）与全项目死代码（R1-R4 分类 + framework→services 反向依赖 D8）。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.6 节 + 第 6.5 章 + 第 7 章 TOP 20 + 第 11 章决策点。

## 工程计划 A: 硬规则合规（F1/F9）

### 背景

- **B09-01. F1/F9 硬规则违反**
  - 描述：services 42 文件缺 `#![deny(unsafe_code)]`（F1）；host-tests 20 处 `#![allow(dead_code)]`（F9 零容忍）。
  - 方案：一次性补齐 deny；死代码 allow 通过实现使用路径消除。
  - 状态：[]

### 待办

- **B09-02. services 缺 deny(unsafe_code) 补齐（P0-22）**
  - 描述：非 vendored 260 文件中 42 个缺 `#![deny(unsafe_code)]`（wasm/wasi 9、fs/hvfs 16、driver/display 3、fs/snapshot.rs、fs/xattr.rs、proc/canary.rs、proc/memfd.rs、proc/oomd.rs、proc/pidfd.rs、sync/lockdep.rs、config/*、timer/mod.rs、credo/storage/disk.rs 等）。
  - 方案：一次性在缺 deny 文件首行添加；含 unsafe 的先迁移（分册 01 F2 门禁修复后验证）。
  - 状态：[]

- **B09-03. host-tests allow(dead_code) 消除（P0-23）**
  - 描述：host-tests/src + tests 下 20 处 `#![allow(dead_code)]` 违反 F9 零容忍。
  - 方案：逐处审查——真死代码删除，被 cfg 引用则用 cfg_attr 精确化；不保留裸 allow。
  - 状态：[]

## 工程计划 B: 死代码分类治理（R1-R4）

### 背景

- **B09-04. 死代码零容忍（§9.3）**
  - 描述：第 6.5 章对死代码分类标注：R1 pub fn 死代码 362 项、R2 未接线 syscall 161 项、R3 零引用 pub mod 36 项、R4 核心 pub struct/enum 零引用 1 项。
  - 方案：按处置工作流（阶段 1 高确定删除 → 阶段 2 中确定 → 阶段 3 决策类）推进。
  - 状态：[]

### 待办

- **B09-05. R2 未接线 syscall 处置（161 项）**
  - 描述：表 A `[A:激活]` 5 项（dispatch 缺项，函数已实装）→ 接线；表 R `[R:替代]` 119 项（QX_* 备用命名，禁用）→ 核实后删除或保留；表 D `[D:删除]` 37 项（真未实装）→ 删除。
  - 方案：用 `audit_unwired_pub_fn.py`（分册 01 修复后）生成清单，按表分类逐项处置。
  - 状态：[]

- **B09-06. R1 pub fn 死代码（362 项）**
  - 描述：pub fn 死代码 362 项，高密度文件 Top 5 需优先（vfs/api.rs、syscall/types.rs 等）；已确认 `[X:CFG]` 跨架构项 3 项保留。
  - 方案：按模块分布 Top 10 逐文件清理；跨架构项保留并标注。
  - 状态：[]

- **B09-07. R3 零引用 pub mod（36 项）**
  - 描述：36 个 pub mod 零引用。
  - 方案：核实后删除（或确认 cfg 条件引用）。
  - 状态：[]

- **B09-08. R4 核心 pub struct/enum 零引用（1 项）**
  - 描述：核心 pub struct/enum 零引用 1 项。
  - 方案：核实后删除。
  - 状态：[]

- **B09-09. services "策略上移"模式违反 OSTD Minimalism（H.3.5 P2-B）**
  - 描述：services 层策略上移模式与 OSTD Minimalism 立场冲突（P2-B）。
  - 方案：登记立场冲突，决策是否调整策略归属。
  - 状态：[]

- **B09-10. 28 处 TODO(TRACK-...) 注释（H.3.5 P2-C）**
  - 描述：28 处 `TODO(TRACK-...)` 注释违反 AGENTS.md §9.4"不留 TODO"；其中 ISSUE-SRC-002（Ed25519）等已在分册 07 登记。
  - 方案：逐一处置——实装、转 plan 任务或删除；完成后 grep 复核为 0。
  - 状态：[]

## 工程计划 C: 架构责任边界（F2/D8）

### 背景

- **B09-11. framework→services 反向依赖**
  - 描述：实测 framework→services 反向依赖 84 文件 / 146 处（TOP 20 #19、决策点 D8），vfs/api.rs 严重违反 F2 单向数据流。
  - 方案：按子模块分类治理（决策点 D8），DECISION-039 仅修 userctx 一处，覆盖不足。
  - 状态：[] (2026-08-31 注记：**部分进展**——vfs/api.rs 反向依赖已由 B09-12 治理（api.rs 3 处直 use 消除，见 B09-12 状态）；但 framework 仍有 ~10 处直接 `use crate::kernel::services`（net/syscall.rs、sm_fi.rs、user_proc.rs、sendfile.rs 等）+ ~70 处 pub use re-export 壳，全量治理未完成，详见 B09-13)

### 待办

- **B09-12. vfs/api.rs 反向依赖治理（H.5.1 P0-31）**
  - 描述：framework/fs/vfs/api.rs 直调 services 层类型。
  - 方案：services 类型迁回 framework（DECISION-H13/H19），api.rs 恢复单向依赖。
  - 状态：[X] (2026-08-31 实装，commit 0fd608b9 + ee7dab4b + 53b7bf51 + 5a9c69b6：Errno/KernelError 迁回 framework 打破循环依赖，dcache/icache/VFS 机制迁回；api.rs 原 3 处直 use services (devfs::DevfsData / open_file_table::OPEN_FILE_TABLE / vfs_types::OpenFile) 已消除——open_file_table/vfs_types 迁回 framework，devfs 经 vfs.rs re-export 壳访问。复验 (2026-08-31)：audit_services_boundary 0 违规。注：本项由分册 6 委托人实施，作为 B06-10 串行前置)
  - 详情：⚠ **同文件冲突约束**——`vfs/api.rs` 同时被 B06-10（拆分）、B06-11（直调 F2）涉及。**必须串行执行，顺序：B09-12（依赖方向治理）→ B06-10（文件拆分）**。先修依赖方向再拆分，避免拆分后 import 返工；并发委派时 B09-12 与 B06-10/11 不得并行。（已按序执行：B09-12 → B06-10，见分册 6 B06-10/11 状态）

- **B09-13. framework→services 全量反向依赖清单与治理（D8）**
  - 描述：146 处反向依赖按子模块分类（fs/api、userctx、syscall 等）。
  - 方案：建立清单 → 分类（类型迁回 / 顶层 re-export / 接口抽象）→ 分批治理；每批跑 F2 门禁（分册 01 修复后）。
  - 状态：[]

- **B09-14. F3 循环依赖门禁接入**
  - 描述：audit_coupling.py 修复（分册 01）后接入 CI，新增代码禁止引入模块间循环依赖。
  - 方案：见分册 01 coupling/invariants 接入项。
  - 状态：[]

### 验证门槛

- **B09-15. 死代码回归**
  - 描述：每批删除后跑双架构编译（0 error/0 warning）+ host-tests。
  - 方案：`./ci/build.sh all` + `make test-host`。
  - 状态：[]

- **B09-16. 边界回归**
  - 描述：F2 治理后跑修复版 `audit_services_boundary.py` 0 违规。
  - 方案：分册 01 完成后的门禁脚本。
  - 状态：[]

### 决策记录

- **DECISION-051**
  - 描述：死代码治理采用"审计清单驱动"方式，每批删除后立即跑验证门槛，不做无目标的大规模清扫。
  - 方案：R2 表 A 接线优先，表 D 删除次之，表 R 核实后处置。
  - 状态：[]
