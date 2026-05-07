# 开发文档

本目录包含 AntX 操作系统的技术设计文档。

## 📚 文档列表

### 入门指南
| 文档 | 说明 |
|------|------|
| [development.md](development.md) | 开发指南 - 项目结构、构建方法、开发规范 |
| [devdoc.md](devdoc.md) | 开发文档 - 详细开发说明 |

### 系统架构
| 文档 | 说明 |
|------|------|
| [kernel-architecture.md](kernel-architecture.md) | 内核架构设计 - 整体架构、模块划分、初始化顺序 |

### 核心模块
| 文档 | 说明 |
|------|------|
| [memory-management.md](memory-management.md) | 内存管理 - PMM(位图分配器)、VMM(四级页表)、kmalloc、Slab |
| [process-session.md](process-session.md) | 进程与会话管理 - 进程模型、会话生命周期 |
| [thread-scheduler.md](thread-scheduler.md) | 线程与调度器 - MLFQ + RT 调度、线程状态机 |
| [syscall.md](syscall.md) | 系统调用接口 - API 定义、37 个已注册 syscall |
| [ipc.md](ipc.md) | 进程间通信 - 管道/信号/共享内存/消息队列/信号量 |
| [klog-system.md](klog-system.md) | KLog 日志系统 - 6级日志、12种分类、环形缓冲区 |

### 安全与权限
| 文档 | 说明 |
|------|------|
| [pwid-model.md](pwid-model.md) | PWID 权限模型 - 三级权限、原Root锚点、令牌提权 |
| [pwid-enhanced-v2.md](pwid-enhanced-v2.md) | PWID v2 增强 - 能力系统、信任链、审计 |
| [permission-model-v3.md](permission-model-v3.md) | 权限模型 v3 - sensitivity + ACE + Capability Matrix |
| [security-mechanisms.md](security-mechanisms.md) | 安全机制 - Stack Canary/PIC/NX/ASSERT/Map文件 |

### 文件系统
| 文档 | 说明 |
|------|------|
| [hivefs.md](hivefs.md) | HvFS 文件系统 - 设计与实现、VFS 层、PWID 集成 |
| [hvfs-disk.md](hvfs-disk.md) | HvFS 磁盘格式 - Super Block/Inode/间接块、持久化 |
| [smart-persistent-storage.md](smart-persistent-storage.md) | Smart Mount - 三种构建模式 (DEV/TEST/RELEASE) |

### 驱动
| 文档 | 说明 |
|------|------|
| [keyboard.md](keyboard.md) | 键盘驱动 - PS/2 键盘驱动实现 |

### 内核优化
| 文档 | 说明 |
|------|------|
| [pic-implementation.md](pic-implementation.md) | PIC 位置无关代码 - `-fPIC -mcmodel=medium` |
| [pic-quick-start.md](pic-quick-start.md) | PIC 快速开始 |

### Rust 重写 (已实施)
| 文档 | 说明 |
|------|------|
| [rust-filesystem.md](rust-filesystem.md) | 文件系统 Rust 重写 - vfs/ramfs/diskfs/hvfs/devfs/procfs |
| [rust-process.md](rust-process.md) | 进程管理 Rust 重写 - process/scheduler/session/thread/user_proc |

---

## 🔧 快速开始

1. 阅读 [开发指南](development.md) 了解项目概况
2. 阅读 [内核架构](kernel-architecture.md) 理解系统设计
3. 根据兴趣选择具体模块文档深入学习

---

*最后更新: 2026-05-07 (根据源码实现订正)*
