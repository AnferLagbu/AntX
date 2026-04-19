# 开发文档

本目录包含 AntX 操作系统的技术设计文档。

## 📚 文档列表

### 入门指南
| 文档 | 说明 |
|------|------|
| [development.md](development.md) | 开发指南总览 - 项目结构、构建方法、开发规范 |
| [devdoc.md](devdoc.md) | 开发文档 - 详细开发说明 |
| [001-devdoc.md](001-devdoc.md) | 开发文档 (一) |
| [002-devdoc.md](002-devdoc.md) | 开发文档 (二) |

### 系统架构
| 文档 | 说明 |
|------|------|
| [kernel-architecture.md](kernel-architecture.md) | 内核架构设计 - 整体架构、模块划分 |

### 核心模块
| 文档 | 说明 |
|------|------|
| [memory-management.md](memory-management.md) | 内存管理 - PMM/VMM 实现 |
| [process-session.md](process-session.md) | 进程与会话管理 - 进程模型、调度算法 |
| [syscall.md](syscall.md) | 系统调用接口 - API 定义与实现 |

### 安全模型
| 文档 | 说明 |
|------|------|
| [pwid-model.md](pwid-model.md) | PWID 权限模型 - 三级权限体系 |

### 文件系统
| 文档 | 说明 |
|------|------|
| [hivefs.md](hivefs.md) | HiveFS 文件系统 - 设计与实现 |
| [hvfs-disk.md](hvfs-disk.md) | HVFS 磁盘格式 - 磁盘布局与数据结构 |

### 驱动程序
| 文档 | 说明 |
|------|------|
| [keyboard.md](keyboard.md) | 键盘驱动 - PS/2 键盘驱动实现 |

### 内核优化
| 文档 | 说明 |
|------|------|
| [pic-implementation.md](pic-implementation.md) | PIC 位置无关代码 - 提高内核稳定性和灵活性 |
| [pic-quick-start.md](pic-quick-start.md) | PIC 快速开始 - 具体实现步骤和代码示例 |

### Rust 重写计划
| 文档 | 说明 |
|------|------|
| [rust-filesystem.md](rust-filesystem.md) | 文件系统 Rust 重写 - VFS、RamFS、DevFS 等模块的 Rust 实现方案 |
| [rust-process.md](rust-process.md) | 进程管理 Rust 重写 - 进程、调度器、会话管理的 Rust 实现方案 |

---

## 🔧 快速开始

1. 阅读 [开发指南](development.md) 了解项目概况
2. 阅读 [内核架构](kernel-architecture.md) 理解系统设计
3. 根据兴趣选择具体模块文档深入学习

---

*最后更新: 2026-04-07 (已根据源码实现订正)*
