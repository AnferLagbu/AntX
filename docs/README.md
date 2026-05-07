# AntX 项目文档索引

> **最后更新**: 2026-05-07 | **规范版本**: v2.0

## 文档目录结构

```
docs/
├── README.md                           # 本文件 (文档索引)
├── ai-autonomous-development-spec.md   # AI自主开发规范 [必读]
├── CODE_STYLE.md                       # 代码风格指南
│
├── implementation-report.md            # Smart Mount 实施报告
├── implementation-report-user-mode-entry.md  # 用户态进入实施报告
├── implementation-report-user-mode-init-crash.md # init崩溃修复报告
│
├── development/                         # 开发文档
│   ├── README.md                        # 开发文档索引
│   ├── development.md                   # 开发指南
│   ├── devdoc.md                        # 开发文档
│   ├── kernel-architecture.md           # 内核架构设计
│   ├── memory-management.md             # 内存管理
│   ├── process-session.md               # 进程与会话
│   ├── thread-scheduler.md              # 线程与调度器
│   ├── syscall.md                       # 系统调用接口
│   ├── pwid-model.md                    # PWID 权限模型
│   ├── pwid-enhanced-v2.md             # PWID v2增强
│   ├── permission-model-v3.md           # 权限模型 v3
│   ├── security-mechanisms.md           # 安全机制
│   ├── hivefs.md                        # HvFS 文件系统设计
│   ├── hvfs-disk.md                     # HvFS 磁盘持久化
│   ├── smart-persistent-storage.md      # Smart Mount 设计
│   ├── ipc.md                           # 进程间通信
│   ├── klog-system.md                   # KLog 日志系统
│   ├── keyboard.md                      # 键盘驱动
│   ├── pic-implementation.md            # PIC 实现
│   ├── pic-quick-start.md               # PIC 快速开始
│   ├── rust-filesystem.md               # 文件系统 Rust 重写 [已实施]
│   └── rust-process.md                  # 进程管理 Rust 重写 [已实施]
│
├── issues/                              # 问题记录
│   ├── README.md
│   ├── issue-recommend.md               # 问题追踪建议
│   ├── stability-issues.md              # 稳定性问题
│   ├── user-mode-gpf.md                 # 用户态 GPF (已解决)
│   └── user-mode-init-crash.md          # init 崩溃 (已修复)
│
├── plans/                               # 计划文档
│   ├── infrastructure-strengthening-plan.md
│   ├── lwip-integration-plan.md
│   └── network-phase6-8.md
│
├── progress/                            # 进度跟踪
│   ├── README.md
│   ├── current-tasks.md                 # 当前任务
│   ├── milestones.md                    # 里程碑
│   ├── changelog.md                     # 变更日志
│   ├── antx-focused-priority.md         # 优先级规划
│   └── maintenance-plan.md              # 维护计划
│
├── reports/                             # 报告存档
│   └── stability-report-2026-04-25.md
│
└── archive/                             # 历史归档
    └── stability-report-2026-04-25.md
```

## 快速导航

### 如果你是 AI 助手

1. **先读**: `ai-autonomous-development-spec.md` (开发规范)
2. **再读**: `implementation-report.md` (最近做了什么)
3. **参考**: `development/*.md` (具体技术细节)

### 如果你是人类开发者

1. **了解架构**: `development/kernel-architecture.md`
2. **理解文件系统**: `development/hivefs.md` → `development/hvfs-disk.md` → `development/smart-persistent-storage.md`
3. **理解权限系统**: `development/pwid-model.md` → `development/pwid-enhanced-v2.md` → `development/permission-model-v3.md`
4. **查看进度**: `progress/current-tasks.md` → `progress/milestones.md`
5. **查看变更**: `progress/changelog.md`
6. **报告问题**: `issues/` 目录下对应文件

## 文档状态总览

| 文档 | 最后更新 | 状态 | 说明 |
|------|----------|------|------|
| ai-autonomous-development-spec.md | 2026-05-06 | ✅ 最新 | v2.0 |
| CODE_STYLE.md | 2026-05-01 | ✅ 最新 | Commit 规范 |
| kernel-architecture.md | 2026-05-07 | ✅ 已更新 | 含网络栈/驱动 |
| hvfs-disk.md | 2026-05-06 | ✅ 已更新 | 含 FFI 导出列表 |
| smart-persistent-storage.md | 2026-05-06 | ✅ 最新 | Smart Mount |
| memory-management.md | 2026-05-07 | ✅ 已更新 | Rust 实现 + Slab |
| syscall.md | 2026-05-07 | ✅ 已更新 | 37个syscall全部注册 |
| process-session.md | 2026-05-07 | ✅ 已更新 | Rust + MLFQ |
| thread-scheduler.md | 2026-04-19 | ✅ 最新 | MLFQ + RT |
| pwid-model.md | 2026-05-07 | ✅ 已更新 | Token/Trust/Elevate均已实现 |
| permission-model-v3.md | 2026-05-02 | ✅ 最新 | v3架构 |
| ipc.md | 2026-05-07 | ✅ 已更新 | 5种IPC均已实现 |
| klog-system.md | 2026-04-25 | ✅ 最新 | KLog v1.0 |
| security-mechanisms.md | 2026-04-25 | ✅ 最新 | 7种机制已实施 |
| milestones.md | 2026-05-07 | ✅ 已更新 | 反映实际进度 |
| current-tasks.md | 2026-05-07 | ✅ 已更新 | 反映实际完成 |
| changelog.md | 2026-05-07 | ✅ 已更新 | 补全所有变更 |
| user-mode-init-crash.md | 2026-05-07 | ✅ 已修复 | 3个Bug修复 |
| development.md | 2026-05-07 | ✅ 已更新 | 项目结构/构建命令 |

## 文档维护规范

**何时更新文档**:
- 完成新功能实现后
- 修改架构后
- 发现文档与代码不一致时

**更新要求**:
1. 在文件头标注最后更新日期
2. 标注变更的版本号
3. 保持格式一致 (Markdown)

---
**维护者**: AI Assistant (遵循 ai-autonomous-development-spec.md v2.0)
