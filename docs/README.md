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

## 🆕 版本管理文档（2026-05-03 新增）

AntX 已从硬编码版本号迁移至 **基于 Git 的动态版本系统**：

### 核心组件
| 组件 | 文件 | 说明 |
|------|------|------|
| **动态版本生成脚本** | `scripts/generate_version.sh` | 从 Git 提取 commit hash、分支、构建时间 |
| **自动生成头文件** | `src/include/version_auto.h` | 包含 GIT_COMMIT_HASH, BUILD_DATE 等宏定义 |
| **模块注册表接口** | `src/include/version_registry.h` | 定义 version_module_t 结构体和注册 API |
| **模块注册表实现** | `src/kernel/version_registry.c` | 实现版本查询、导出（JSON/表格）功能 |

### 使用方法
```bash
# 生成版本信息（make all 时自动调用）
make generate-version

# 在代码中使用
#include "version_registry.h"

// 注册新模块
version_register("MyModule", 1, 0, 0,
                "Description", MODULE_TYPE_CORE);

// 查询版本信息
const version_module_t *mod = version_query("MyModule");
```

### 已注册的核心模块（7个）
- **QueenX** (0.1.0) - AntX Kernel Core [MODULE_TYPE_CORE]
- **KLog** (1.0.0) - Kernel Logging System [MODULE_TYPE_LIB]
- **VFS** (1.0.0) - Virtual File System Layer [MODULE_TYPE_LIB]
- **RamFS** (1.0.0) - RAM-based File System [MODULE_TYPE_FS]
- **HvFS** (2.0.0) - Hybrid Virtual File System [MODULE_TYPE_FS]
- **PWID** (1.0.0) - Permission & Identity System [MODULE_TYPE_SECURITY]
- **MLFQ** (1.0.0) - Multi-Level Feedback Queue Scheduler [MODULE_TYPE_CORE]

详细设计见：[根目录 README.md](../README.md#️-版本管理-2026-05-03-新增)

## 📂 development/ - 开发文档

系统设计与技术文档，包含：

### 入门指南
| 文档 | 说明 |
|------|------|
| [README.md](development/README.md) | 开发文档索引 |
| [development.md](development/development.md) | 开发指南总览 |
| [devdoc.md](development/devdoc.md) | 开发文档 |

### 系统架构
| 文档 | 说明 |
|------|------|
| [kernel-architecture.md](development/kernel-architecture.md) | 内核架构设计 |

### 核心模块
| 文档 | 说明 |
|------|------|
| [memory-management.md](development/memory-management.md) | 内存管理 - PMM/VMM 实现 |
| [process-session.md](development/process-session.md) | 进程与会话管理 |
| [thread-scheduler.md](development/thread-scheduler.md) | 线程与调度器 |
| [syscall.md](development/syscall.md) | 系统调用接口 |
| [ipc.md](development/ipc.md) | 进程间通信 |

### 安全模型
| 文档 | 说明 |
|------|------|
| [pwid-model.md](development/pwid-model.md) | PWID 权限模型 |
| [pwid-enhanced-v2.md](development/pwid-enhanced-v2.md) | PWID 增强版 |
| [security-mechanisms.md](development/security-mechanisms.md) | 安全机制 |

### 文件系统
| 文档 | 说明 |
|------|------|
| [hivefs.md](development/hivefs.md) | HiveFS 文件系统 |
| [hvfs-disk.md](development/hvfs-disk.md) | HVFS 磁盘格式 |

### 驱动程序
| 文档 | 说明 |
|------|------|
| [keyboard.md](development/keyboard.md) | 键盘驱动 |

### 内核优化
| 文档 | 说明 |
|------|------|
| [pic-implementation.md](development/pic-implementation.md) | PIC 位置无关代码 |
| [pic-quick-start.md](development/pic-quick-start.md) | PIC 快速开始 |

### Rust 重写计划
| 文档 | 说明 |
|------|------|
| [rust-filesystem.md](development/rust-filesystem.md) | 文件系统 Rust 重写 |
| [rust-process.md](development/rust-process.md) | 进程管理 Rust 重写 |

## 📂 issues/ - 问题文档

已知问题与改进建议：

| 文档 | 说明 |
|------|------|
| [README.md](issues/README.md) | 问题文档索引 |
| [issue-recommend.md](issues/issue-recommend.md) | 问题追踪与改进建议 |
| [user-mode-gpf.md](issues/user-mode-gpf.md) | 用户态 GPF 问题分析 |

## 📂 progress/ - 进度文档

开发进度与里程碑记录：

### 进度追踪
| 文档 | 说明 |
|------|------|
| [README.md](progress/README.md) | 进度文档索引 |
| [changelog.md](progress/changelog.md) | 变更日志 ⭐ **已更新 (2026-05-02)** |
| [current-tasks.md](progress/current-tasks.md) | 当前任务清单 |
| [milestones.md](progress/milestones.md) | 里程碑记录 |
| [antx-focused-priority.md](progress/antx-focused-priority.md) | 优先级规划 |

### 📚 规范与标准
| 文档 | 说明 |
|------|------|
| [CODE_STYLE.md](CODE_STYLE.md) | **代码规范 + Git Commit 规范 + 测试规范** ⭐ **新增** |

---

*最后更新: 2026-05-03*
*文档版本: v2.1 (新增版本管理文档)*

## 🔗 快速导航

### 新手入门
1. [开发指南](development/development.md) - 了解如何开始开发
2. [内核架构](development/kernel-architecture.md) - 理解系统整体设计

### 核心模块
- [内存管理](development/memory-management.md)
- [进程管理](development/process-session.md)
- [线程调度](development/thread-scheduler.md)
- [文件系统](development/hivefs.md)
- [系统调用](development/syscall.md)
- [进程间通信](development/ipc.md)

### 安全与权限
- [PWID 权限模型](development/pwid-model.md)
- [安全机制](development/security-mechanisms.md)

### 问题追踪
- [Issue 列表](issues/issue-recommend.md) - 查看所有已知问题和改进建议

### 开发进度
- [当前任务](progress/current-tasks.md) - 查看当前开发任务
- [里程碑](progress/milestones.md) - 查看项目里程碑

---

*最后更新: 2026-04-21*
