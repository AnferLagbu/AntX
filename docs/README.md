# AntX 文档目录

本目录包含 AntX 操作系统的所有文档，按类型分类组织。

## 📛 命名规范

| 名称 | 说明 |
|------|------|
| **QueenX (QX)** | 内核名称 - 独立的操作系统内核 |
| **AntX** | 完整操作系统 - QueenX 内核 + 用户态组件 |

> 类似于：Linux 内核 + GNU 工具 = GNU/Linux 操作系统

```
┌─────────────────────────────────────────────────────────────┐
│                      AntX 操作系统                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              用户态组件 (Ring 3)                      │   │
│  │  • antxsh (Shell)    • 安装向导                       │   │
│  │  • 用户程序          • 用户库                         │   │
│  └─────────────────────────────────────────────────────┘   │
│                          │                                  │
│                    系统调用接口                              │
│                          │                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           QueenX 内核 (Ring 0)                        │   │
│  │  • 进程管理    • 内存管理    • 文件系统               │   │
│  │  • PWID 权限   • 设备驱动    • 中断处理               │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 📁 目录结构

```
docs/
├── development/     # 开发文档 - 系统设计、架构、API说明
├── issues/          # 问题文档 - 已知问题、改进建议
└── progress/        # 进度文档 - 开发日志、里程碑记录
```

## 📂 development/ - 开发文档

系统设计与技术文档，包含：

| 文档 | 说明 |
|------|------|
| [development.md](development/development.md) | 开发指南总览 |
| [devdoc.md](development/devdoc.md) | 开发文档 |
| [001-devdoc.md](development/001-devdoc.md) | 开发文档 (一) |
| [002-devdoc.md](development/002-devdoc.md) | 开发文档 (二) |
| [kernel-architecture.md](development/kernel-architecture.md) | 内核架构设计 |
| [memory-management.md](development/memory-management.md) | 内存管理机制 |
| [process-session.md](development/process-session.md) | 进程与会话管理 |
| [pwid-model.md](development/pwid-model.md) | PWID 权限模型 |
| [syscall.md](development/syscall.md) | 系统调用接口 |
| [hivefs.md](development/hivefs.md) | HiveFS 文件系统 |
| [hvfs-disk.md](development/hvfs-disk.md) | HVFS 磁盘格式 |

## 📂 issues/ - 问题文档

已知问题与改进建议：

| 文档 | 说明 |
|------|------|
| [issue-recommend.md](issues/issue-recommend.md) | 问题追踪与改进建议 |

## 📂 progress/ - 进度文档

开发进度与里程碑记录：

| 文档 | 说明 |
|------|------|
| [changelog.md](progress/changelog.md) | 变更日志 |
| [milestones.md](progress/milestones.md) | 里程碑记录 |

## 🔗 快速导航

### 新手入门
1. [开发指南](development/development.md) - 了解如何开始开发
2. [内核架构](development/kernel-architecture.md) - 理解系统整体设计

### 核心模块
- [内存管理](development/memory-management.md)
- [进程管理](development/process-session.md)
- [文件系统](development/hivefs.md)
- [系统调用](development/syscall.md)

### 问题追踪
- [Issue 列表](issues/issue-recommend.md) - 查看所有已知问题和改进建议

---

*最后更新: 2026-04-07*
