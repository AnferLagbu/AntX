# AntX

AntX 是一款**完全独立自研**的轻量级 x86_64 操作系统，定位为纯个人技术实验与探索项目。

**内核名称**: QueenX (QX)
**完整系统**: QueenX 内核 + 用户态组件 = AntX
**技术栈**: C11 + Rust + NASM 汇编  |  **运行环境**: QEMU x86_64

## 核心设计理念

- **极简优先** — 拒绝冗余，聚焦想法验证
- **独立探索** — 从零设计，保持完全独立性
- **C + Rust 混合** — 安全关键模块全部 Rust 重写
- **测试驱动** — 完善的测试框架确保代码质量

## 特色功能

### PWID 权限模型

摒弃传统"用户"概念，采用「密码 + 备注 → 唯一 PWID」的创新身份模型：

- **无预先账户** — 只需密码和备注即可创建/切换身份
- **同一密码 + 不同备注 = 不同身份** — 灵活的身份管理
- **三级权限** — Root / Trustworthy / Untrustworthy
- **原 Root 锚点** — 全局唯一、不可删除、内核硬编码保护
- **令牌提权** — 临时提权令牌，支持时间/次数限制，自动恢复
- **信任链** — 支持多跳信任委托，含能力掩码
- **暴力破解防护** — 5 次失败锁定 300 秒
- **审计日志** — 9 种操作类型，支持持久化

### 宏内核架构

```
用户态 (Ring 3)         内核态 (Ring 0)
┌────────────┐         ┌──────────────────────────┐
│  antxsh    │  int    │ MLFQ+RT 调度  PMM/VMM   │
│  用户程序  │  0x80   │ VFS 层 (5种后端)        │
│  用户库    │ ──────▶ │ lwIP 网络栈  E1000 驱动 │
└────────────┘         │ PWID 权限  IPC(5种)     │
                       │ DMA 引擎   KLog 日志    │
                       └──────────────────────────┘
```

### 技术特性

| 类别 | 实现 |
|------|------|
| **调度器** | MLFQ 多级反馈队列 + RT 实时任务 (FIFO/RR) |
| **内存管理** | 位图 PMM / 四级页表 VMM (2MB/1GB 大页) |
| **文件系统** | VFS + HvFS/RamFS/DiskFS/DevFS/ProcFS + Smart Mount |
| **IPC** | 管道 / 信号 / 共享内存 / 消息队列 / 信号量 |
| **网络** | lwIP 2.2.1 完整 TCP/IP 栈 + Intel E1000 驱动 |
| **同步原语** | Spinlock / Atomic / R/W Lock / Mutex / Slab |
| **安全** | PIC/PIE / NX+SMEP / Stack Canary / KLog 日志 |

## 快速开始

```bash
# 安装依赖 (Fedora)
bash scripts/requirements.sh --auto

# 手动安装
sudo dnf install -y make nasm qemu-system-x86 gdb xorriso grub2-tools \
    gcc-x86_64-linux-gnu binutils-x86_64-linux-gnu

# 构建
make all

# 运行
make run-iso                   # ISO 模式启动（推荐）
make run-net                   # 带网络启动

# 调试
make debug                     # GDB 端口 1234

# 测试
make test-quick                # 快速测试 (60s)
make test-unit                 # 单元测试 (120s)
```

## 项目结构

```
AntX/
├── docs/              # 全部文档 (设计/进度/问题/规范)
├── src/
│   ├── kernel/        # 内核核心 (C + 汇编)
│   ├── mm/            # 内存管理 (Rust)
│   ├── proc/          # 进程/调度/线程 (Rust)
│   ├── pwid/          # PWID 权限 (Rust)
│   ├── fs/            # 文件系统 (Rust)
│   │   ├── vfs/ ramfs/ diskfs/ hvfs/ devfs/ procfs/
│   ├── dma/           # DMA 引擎 (Rust)
│   ├── driver/        # 驱动 (C)
│   ├── ipc/           # IPC (C)
│   ├── net/           # lwIP 网络栈
│   ├── rust/          # Rust 运行时入口
│   └── user/          # 用户态程序 (init/axsh/安装向导)
├── scripts/           # 构建脚本
├── tests/             # 测试框架
└── Makefile           # 构建配置
```

## 文档导航

| 类别 | 文档 |
|------|------|
| **开发规范** | [AI 自主开发规范](docs/ai-autonomous-development-spec.md) \| [代码风格](docs/CODE_STYLE.md) |
| **架构设计** | [内核架构](docs/development/kernel-architecture.md) \| [开发指南](docs/development/development.md) |
| **核心模块** | [内存管理](docs/development/memory-management.md) \| [进程调度](docs/development/thread-scheduler.md) \| [系统调用](docs/development/syscall.md) |
| **权限安全** | [PWID 模型](docs/development/pwid-model.md) \| [权限 v3](docs/development/permission-model-v3.md) \| [安全机制](docs/development/security-mechanisms.md) |
| **文件系统** | [HvFS](docs/development/hivefs.md) \| [磁盘](docs/development/hvfs-disk.md) \| [Smart Mount](docs/development/smart-persistent-storage.md) |
| **其他** | [IPC](docs/development/ipc.md) \| [KLog](docs/development/klog-system.md) |
| **进度追踪** | [当前任务](docs/progress/current-tasks.md) \| [里程碑](docs/progress/milestones.md) \| [变更日志](docs/progress/changelog.md) |
| **问题记录** | [问题索引](docs/issues/README.md) \| [稳定性](docs/issues/stability-issues.md) |

## 许可证

[MIT License](LICENSE) © 2026 Anfer
