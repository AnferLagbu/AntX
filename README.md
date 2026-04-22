# AntX

AntX 是一款**完全独立自研**的轻量级 x86_64 操作系统，定位为纯个人技术实验与探索项目。

**内核名称**: QueenX (QX)  
**完整系统**: QueenX 内核 + 用户态组件 = AntX

## 核心设计理念

- **极简优先** — 拒绝冗余，聚焦想法验证
- **独立探索** — 从零设计，保持完全独立性
- **内核核心** — 所有创新（PWID 权限模型、多会话机制、临时提权）均在内核层面实现

## 特色功能

### PWID 权限模型

AntX 摒弃传统"用户"概念，采用「密码+备注→唯一 PWID」模型：
- 无需预先创建账户
- 同一密码 + 不同备注 = 不同身份
- 三级权限：Root / Trustworthy / Untrustworthy

### 多会话系统

- 多终端同时登录
- 会话级 PWID 权限绑定
- 身份切换即时生效

### 原 Root 锚点

- 全局唯一，不可删除
- 首次启动强制设置
- 身份通过内核硬编码保护

### 临时提权 (计划中)

- 向原 Root 提权
- 验证密码后执行单一命令
- 执行完毕自动恢复原身份

## 当前开发状态

**版本**: v0.2.0  
**整体完成度**: 约 55%

### 已完成功能

| 模块 | 完成度 | 说明 |
|------|--------|------|
| 内核基础架构 | 100% | Multiboot2 启动、GDT/IDT、中断处理 |
| 内存管理 | 80% | PMM 位图分配器、VMM 四级页表、进程隔离 |
| 进程管理 | 70% | 进程创建/退出、MLFQ 调度器、线程模型 |
| PWID 权限 | 80% | PWID 生成/验证、三级权限、原 Root 锚点 |
| IPC | 100% | 管道、信号、共享内存、消息队列、信号量 |
| 用户态支持 | 100% | ELF 加载器、iretq 特权级切换 |
| 文件系统 | 60% | VFS、RamFS、DevFS、ProcFS、DiskFS |

### 进行中

- 磁盘持久化存储
- 系统调用完善
- 用户程序完善

### 计划中

- 临时提权机制 (P0)
- PWID 感知调度 (P0)
- 网络支持

## 开发环境

- **运行环境**: QEMU x86_64 模拟器
- **编译器**: x86_64-linux-gnu-gcc (C11)
- **汇编器**: NASM
- **调试器**: GDB
- **Rust**: 部分模块使用 Rust 重写

## 快速开始

```bash
# 安装依赖 (Fedora)
sudo dnf install -y make nasm qemu-system-x86 gdb xorriso grub2-tools \
    gcc-x86_64-linux-gnu binutils-x86_64-linux-gnu

# 构建
make all

# 运行
make run-iso

# 调试
make debug
```

## 文档

详细设计文档见 [docs/](docs/) 目录：

### 入门
- [开发指南](docs/development/development.md)
- [内核架构](docs/development/kernel-architecture.md)

### 核心模块
- [内存管理](docs/development/memory-management.md)
- [进程与会话](docs/development/process-session.md)
- [线程调度](docs/development/thread-scheduler.md)
- [文件系统](docs/development/hivefs.md)
- [系统调用](docs/development/syscall.md)
- [进程间通信](docs/development/ipc.md)

### 安全与权限
- [PWID 权限模型](docs/development/pwid-model.md)
- [安全机制](docs/development/security-mechanisms.md)

### 进度追踪
- [当前任务](docs/progress/current-tasks.md)
- [里程碑](docs/progress/milestones.md)
- [变更日志](docs/progress/changelog.md)

## 项目结构

```
AntX/
├── docs/           # 文档
├── scripts/        # 构建脚本
├── src/            # 源代码
│   ├── include/    # 头文件
│   ├── kernel/     # 内核核心
│   ├── mm/         # 内存管理
│   ├── proc/       # 进程管理 (C + Rust)
│   ├── pwid/       # PWID 权限 (C + Rust)
│   ├── fs/         # 文件系统 (模块内聚结构)
│   │   ├── vfs/    # VFS 核心层
│   │   ├── ramfs/  # 内存文件系统
│   │   ├── diskfs/ # 磁盘文件系统
│   │   ├── hvfs/   # HvFS 文件系统
│   │   ├── devfs/  # 设备文件系统
│   │   └── procfs/ # 进程文件系统
│   ├── ipc/        # 进程间通信
│   ├── disk/       # 磁盘驱动
│   ├── lib/        # 内核库
│   ├── rust/       # Rust 运行时入口
│   └── user/       # 用户态程序
├── tests/          # 测试框架
├── build/          # 构建输出
└── logs/           # 日志
```

## 许可证

[LICENSE](LICENSE)
