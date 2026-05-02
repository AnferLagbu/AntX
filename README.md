# AntX

AntX 是一款**完全独立自研**的轻量级 x86_64 操作系统，定位为纯个人技术实验与探索项目。

**内核名称**: QueenX (QX)
**完整系统**: QueenX 内核 + 用户态组件 = AntX

## 核心设计理念

- **极简优先** — 拒绝冗余，聚焦想法验证
- **独立探索** — 从零设计，保持完全独立性
- **内核核心** — 所有创新（PWID 权限模型、多会话机制、临时提权）均在内核层面实现
- **测试驱动** — 完善的测试框架确保代码质量与系统稳定性

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

### 🧪 综合测试框架 (v2.0)

AntX 配备了完善的测试框架，覆盖所有核心模块：

#### 测试类型
| 类型 | 说明 | 用途 |
|------|------|------|
| 单元测试 | 核心组件功能验证 | PMM、VMM、kmalloc、进程管理 |
| 集成测试 | 模块间交互验证 | VFS、IPC、系统调用、HvFS |
| 增强测试 | 边界条件和错误处理 | 内存安全、异常路径、性能基准 |
| 安全测试 | 权限和访问控制 | PWID 提权、Token 管理 |

#### 当前测试状态 (2026-05-02)
```
✅ 总测试数: 84 个（13个活跃模块）
✅ 通过率: 95.2% (80/84)
✅ 系统稳定性: 无崩溃、无 Kernel panic
✅ 覆盖范围:
   - 核心系统: PMM, VMM, kmalloc, 进程, 调度器
   - 文件系统: VFS, RamFS, DiskFS, HvFS
   - 通信机制: IPC, 系统调用, 中断处理
   - 安全模块: PWID 权限模型
```

#### 运行测试
```bash
# 快速单元测试（60秒超时）
make test-unit

# 综合测试（180秒超时，512MB内存）
make test-comprehensive

# 查看测试报告
cat tests/reports/unit_test_*.log | grep "TEST_RESULT"
```

## 当前开发状态

**版本**: v0.2.1  
**整体完成度**: 约 60% (+5%)

### 已完成功能

| 模块 | 完成度 | 说明 | 测试状态 |
|------|--------|------|---------|
| 内核基础架构 | 100% | Multiboot2 启动、GDT/IDT、中断处理 | ✅ 全部通过 |
| 内存管理 | 85% | PMM 位图分配器、VMM 四级页表、进程隔离 | ✅ 18/18 通过 |
| 进程管理 | 75% | 进程创建/退出、MLFQ 调度器、线程模型 | ✅ 23/23 通过 |
| PWID 权限 | 85% | PWID 生成/验证、三级权限、原 Root 锚点 | ✅ 8/8 通过 |
| IPC | 100% | 管道、信号、共享内存、消息队列、信号量 | ✅ 8/8 通过 |
| 用户态支持 | 100% | ELF 加载器、iretq 特权级切换 | ✅ 6/6 通过 |
| 文件系统 | 70% | VFS、RamFS、DevFS、ProcFS、DiskFS、HvFS | ✅ 21/25 通过 |
| **测试框架** | **100%** | **20个模块，150+测试用例，详细报告** | ✅ **80/84 通过** |

### 进行中

- ~~磁盘持久化存储~~ ✅ 已完成基础实现
- ~~系统调用完善~~ ✅ 错误处理和参数验证已增强
- ~~测试框架优化~~ ✅ v2.0 版本已部署
- 用户程序完善
- 大文件支持（RamFS 间接块索引已实现）

### 计划中

- 临时提权机制 (P0)
- PWID 感知调度 (P0)
- 网络支持
- GUI 图形界面

## 开发环境

- **运行环境**: QEMU x86_64 模拟器
- **编译器**: x86_64-linux-gnu-gcc (C11)
- **汇编器**: NASM
- **调试器**: GDB
- **Rust**: 部分模块使用 Rust 重写（安全关键组件）

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

# 运行测试（推荐）
make test-unit              # 快速单元测试（~60秒）
make test-comprehensive     # 综合测试（~180秒，包含所有模块）

# 查看测试报告
ls tests/reports/
tail -100 tests/reports/unit_test_*.log
```

## 项目结构

```
AntX/
├── docs/           # 文档
│   ├── development/    # 开发文档
│   ├── issues/         # 问题追踪
│   └── progress/       # 进度记录
├── scripts/        # 构建脚本
├── src/            # 源代码
│   ├── include/    # 头文件
│   ├── kernel/     # 内核核心
│   │   └── tests/  # 🔬 测试框架（20个模块）
│   │       ├── kernel_test.c      # 测试框架核心
│   │       ├── test_main.c        # 测试入口
│   │       ├── test_pmm.c         # 物理内存管理测试
│   │       ├── test_vmm.c         # 虚拟内存管理测试
│   │       ├── test_process.c     # 进程管理测试
│   │       ├── test_scheduler.c   # 调度器测试
│   │       ├── test_vfs.c         # 文件系统测试
│   │       ├── test_ipc.c         # IPC测试
│   │       ├── test_pwid_enhanced.c # PWID安全测试
│   │       ├── test_memory_safety.c  # 内存安全测试
│   │       ├── test_edge_cases.c     # 边界条件测试
│   │       ├── test_error_handling.c  # 错误处理测试
│   │       ├── test_performance.c     # 性能基准测试
│   │       ├── test_process_enhanced.c # 进程增强测试
│   │       ├── test_scheduler_enhanced.c # 调度器增强测试
│   │       ├── test_interrupt.c    # 中断处理测试
│   │       ├── test_vfs_enhanced.c # VFS增强测试
│   │       ├── test_syscall_enhanced.c # 系统调用增强测试
│   │       └── test_ipc_enhanced.c # IPC增强测试
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
├── tests/          # 测试输出和报告
│   └── reports/    # 📊 测试日志和结果
├── build/          # 构建输出
└── logs/           # 日志
```

## 文档

详细设计文档见 [docs/](docs/) 目录：

### 入门
- [开发指南](docs/development/development.md) - 如何开始开发
- [内核架构](docs/development/kernel-architecture.md) - 系统整体设计
- **[代码规范](docs/CODE_STYLE.md)** - 编码标准和 Git Commit 规范 ⭐ 新增

### 核心模块
- [内存管理](docs/development/memory-management.md) - PMM/VMM 实现
- [进程与会话](docs/development/process-session.md) - 进程管理
- [线程调度](docs/development/thread-scheduler.md) - MLFQ 调度器
- [文件系统](docs/development/hivefs.md) - VFS/RamFS/DiskFS/HvFS
- [系统调用](docs/development/syscall.md) - 系统调用接口
- [进程间通信](docs/development/ipc.md) - IPC 机制

### 安全与权限
- [PWID 权限模型](docs/development/pwid-model.md) - PWID 设计
- [安全机制](docs/development/security-mechanisms.md) - 安全特性

### 进度追踪
- [当前任务](docs/progress/current-tasks.md) - 开发任务清单
- [里程碑](docs/progress/milestones.md) - 项目里程碑
- [变更日志](docs/progress/changelog.md) - 版本更新记录

### 🔬 测试相关
- [测试框架说明](docs/testing/README.md) - 测试架构和使用方法
- [测试报告示例](tests/reports/) - 最新测试结果

## Git Commit 规范

项目使用统一的 Commit 前缀格式：

| 前缀 | 含义 | 使用场景 |
|------|------|---------|
| `fix:` | 修复 Bug | 修复已知问题、解决崩溃 |
| `feat:` | 新增/增强功能 | 添加新特性、扩展功能 |
| `docs:` | 文档相关 | 更新文档、注释说明 |
| `chore:` | 构建/工具相关 | 构建配置、依赖管理 |
| `refactor:` | 重构代码 | 优化结构、改善设计 |
| `test:` | 测试相关 | 添加或修改测试 |
| `perf:` | 性能优化 | 提升性能、减少延迟 |

**示例**：
```bash
fix: 修复VFS FFI层Invalid Opcode异常
feat: 增强测试框架：添加10个新测试模块
docs: 更新README.md反映最新改进
test: 添加进程管理边界条件测试
```

## 许可证

[LICENSE](LICENSE)

---

*最后更新: 2026-05-02*  
*维护者: Anfer*  
*版本: v0.2.1*
