# AntX 文档

> **AntX** —— 一个具有创新故障恢复机制的现代化宏内核操作系统

---

## 📂 文档结构

`docs/` 仅包含两个目录与两份入口文档:

```
docs/
├── readme.md        ← 本文件: 文档总览
├── changelog.md     ← 版本变更历史
├── plan/            ← 计划: 路线图 / 迁移 / 审计 / 修复报告
└── explain/         ← 解释: 架构 / 设计 / API / 教程 / 参考
```

### 存放规则

- **`plan/`** —— 计划/未来/历史决策
  - 路线图、迁移计划、设计计划
  - 审计报告、修复报告、交付总结
  - 演进蓝图 (ROADMAP) / 待办 (FIX_TASKS) / 已知问题 (KNOWN_ISSUES)

- **`explain/`** —— 解释/现状
  - 架构说明、子系统文档、API 参考
  - 开发指南、教程、编码规范
  - 论文、设计文档、参考材料

- **根目录两份文档**
  - `readme.md` —— 文档总览 (本文件)
  - `changelog.md` —— 当前维护的版本变更日志

> 详细规则参见本文末尾 [存放规则详解](#存放规则详解) 章节.

---

## 📚 快速导航

### 🏗️ 架构与设计 ([explain/](./explain/))

| 主题 | 文档 |
|------|------|
| 系统总览 | [overview.md](./explain/overview.md) |
| 内核架构 | [kernel-architecture.md](./explain/kernel-architecture.md) |
| 启动流程 | [boot-process.md](./explain/boot-process.md) |
| 系统调用 | [syscall.md](./explain/syscall.md) |
| 测试框架 | [test-framework.md](./explain/test-framework.md) |
| 编码规范 | [coding-style.md](./explain/coding-style.md) |
| 编码指南 (Rust) | [api-rs.md](./explain/api-rs.md) |
| CI 集成 | [ci-integration.md](./explain/ci-integration.md) |
| Miri 覆盖 | [miri-coverage.md](./explain/miri-coverage.md) |
| TCB 清单 | [tcb-inventory.md](./explain/tcb-inventory.md) |
| 死锁矩阵 | [deadlock-matrix.md](./explain/deadlock-matrix.md) |
| 服务边界 | [services-boundary.md](./explain/services-boundary.md) |
| Credo DID 设计 | [credo-did.md](./explain/credo-did.md) |

### 🔧 子系统

| 子系统 | 文档 |
|--------|------|
| 内存管理 | [memory.md](./explain/memory.md) |
| 进程管理 | [process.md](./explain/process.md) |
| 文件系统 | [filesystem.md](./explain/filesystem.md) |
| 安全子系统 | [security.md](./explain/security.md) |
| 栏栈恢复 | [barrier.md](./explain/barrier.md) |

### 🛠️ 驱动与硬件

| 主题 | 文档 |
|------|------|
| 驱动总览 | [drivers.md](./explain/drivers.md) |
| 高级驱动 | [drivers-advanced.md](./explain/drivers-advanced.md) |
| 驱动目录结构 | [drivers-directory.md](./explain/drivers-directory.md) |
| 显示驱动 | [display-drivers.md](./explain/display-drivers.md) |
| 存储驱动 | [ssd-drivers.md](./explain/ssd-drivers.md) |

### 🛠️ 开发与构建

| 主题 | 文档 |
|------|------|
| 快速开始 | [getting-started.md](./explain/getting-started.md) |
| 构建系统 | [build-system.md](./explain/build-system.md) |
| 架构移植 | [arch-porting.md](./explain/arch-porting.md) |
| 静态检查 | [checktools.md](./explain/checktools.md) |

### 📋 计划与路线 ([plan/](./plan/))

| 主题 | 文档 |
|------|------|
| 演进蓝图 | [roadmap.md](./plan/roadmap.md) |
| 框内核迁移 | [framekernel.md](./plan/framekernel.md) |
| 已知问题 | [known-issues.md](./plan/known-issues.md) |
| 修复任务 | [fix-tasks.md](./plan/fix-tasks.md) |
| 多架构解耦 | [multiarch-decoupling.md](./plan/multiarch-decoupling.md) |
| 内核可配置 | [kernel-configurability.md](./plan/kernel-configurability.md) |
| 图形子系统 | [graphics-subsystem.md](./plan/graphics-subsystem.md) |
| POSIX 接口 | [posix-interface.md](./plan/posix-interface.md) |
| smoltcp 迁移 | [smoltcp-migration.md](./plan/smoltcp-migration.md) |
| 系统调用优化 | [syscall-optimization.md](./plan/syscall-optimization.md) |
| 栏栈服务 (M6.6) | [barrier-services.md](./plan/barrier-services.md) |

### 📊 审计与修复

| 主题 | 文档 |
|------|------|
| 审计报告 2026-05-30 | [audit-2026-05-30.md](./plan/audit-2026-05-30.md) |
| 审计报告 2026-06-03 | [audit-2026-06-03.md](./plan/audit-2026-06-03.md) |
| 修复报告 2026-05-31 | [fix-report-2026-05-31.md](./plan/fix-report-2026-05-31.md) |
| 修复报告 2026-06-09 | [fix-report-2026-06-09.md](./plan/fix-report-2026-06-09.md) |
| 交付总结 2026-06-03 | [delivery-summary-2026-06-03.md](./plan/delivery-summary-2026-06-03.md) |

### 🔬 研究与论文

- [barrier-stack-paper.md](./explain/barrier-stack-paper.md) - 栏栈机制学术论文
- [hymenoptera-display.md](./explain/hymenoptera-display.md) - 多用户多会话显示服务器设计
- [verus-specs.md](./explain/verus-specs.md) - Verus 形式化规约
- [verus-specs.rs](./explain/verus-specs.rs) - Verus 规约源代码

### 📝 变更日志

- [changelog.md](./changelog.md) - 版本变更历史

---

## 🚀 新手入门

1. [系统概述](./explain/overview.md) —— 了解 AntX 是什么
2. [快速开始](./explain/getting-started.md) —— 编译运行第一个内核
3. [内核架构](./explain/kernel-architecture.md) —— 理解整体设计
4. [编码规范](./explain/coding-style.md) —— 准备贡献代码

### 核心特性

- **栏栈恢复 (BBR/BSR/BHR)**: [barrier.md](./explain/barrier.md)
- **Credo DID 安全**: [credo-did.md](./explain/credo-did.md)
- **Posix 系统调用**: [syscall.md](./explain/syscall.md)

---

## 存放规则详解

### 判定标准: 面向"过去/现在"还是"未来/决策"

| 文档类型 | 目录 | 典型示例 |
|----------|------|----------|
| 架构说明 | `explain/` | kernel-architecture, overview, boot-process |
| API 参考 | `explain/` | syscall, api-rs, drivers |
| 教程/指南 | `explain/` | getting-started, coding-style, arch-porting |
| 设计文档 | `explain/` | credo-did, hymenoptera-display, verus-specs |
| 学术/研究 | `explain/` | barrier-stack-paper |
| 路线图 | `plan/` | roadmap, framekernel, multiarch-decoupling |
| 计划/迁移 | `plan/` | kernel-configurability, smoltcp-migration |
| 演进蓝图 | `plan/` | ROADMAP (旧) |
| 已知问题 | `plan/` | KNOWN_ISSUES (旧) |
| 待办清单 | `plan/` | FIX_TASKS (旧) |
| 审计报告 | `plan/` | audit-2026-* |
| 修复报告 | `plan/` | fix-report-2026-* |
| 交付总结 | `plan/` | delivery-summary-2026-* |

### 命名规范

- **目录**: 小写 + 连字符 (`-`): `plan/`, `explain/`
- **文件**: 小写 + 连字符 (`-`): `framekernel.md`, `barrier-services.md`
- **历史命名**: 历史文件可保留原本的 SHOUTING_SNAKE (如 `AUDIT_REPORT_2026-05-30.md`) 或迁移到 kebab-case (如 `audit-2026-05-30.md`). 当前采用 kebab-case.

### 链接格式

- **文档内引用**: 优先使用相对路径 (`./xxx.md` 或 `../explain/xxx.md`)
- **外部引用**: 可使用 `file://` 绝对路径或纯相对路径
- **失效链接**: 必须立即修复, 不允许指向不存在的位置

### 归档策略

- 已废弃的设计/计划: 移入 `plan/` 即可, 不删除 (保留历史决策)
- 过时的内容: 修改时间戳, 标注 `[DEPRECATED]`, 指向替代文档

---

## 📜 许可证

本项目采用 MIT 许可证 —— 详见 [LICENSE](../LICENSE) 文件
