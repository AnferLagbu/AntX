# AntX 开发文档

> **最后更新**: 2026-05-13 | **版本**: v3.0 (Rust 重写后)

## 文档索引

### 核心架构

- [内核架构设计](kernel-architecture.md) - 系统整体架构、模块划分、初始化流程
- [内存管理](memory-management.md) - PMM、VMM、kmalloc、Slab 分配器
- [进程调度](thread-scheduler.md) - MLFQ 调度器、实时任务、线程模型
- [文件系统](hivefs.md) - VFS 层、HvFS、RamFS、DiskFS、DevFS、ProcFS

### 核心子系统

- [PWID 权限模型](pwid-model.md) - v4 能力流动模型、令牌系统、信任链
- [Barrier 栈设计](barrier-stack-design.md) - 故障恢复、增量回滚、循环防护
- [系统调用接口](syscall.md) - 72 个系统调用、调用约定、实现状态
- [IPC 子系统](ipc.md) - 管道、信号、共享内存、消息队列、信号量

### 硬件相关

- [DMA 引擎](dma-engine.md) - 一致性 DMA、流式 DMA、MMIO 映射
- [中断处理](interrupt-handling.md) - IDT、ISR、IRQ 管理
- [驱动开发](driver-development.md) - ATA、键盘、串口、E1000 网卡

### 网络子系统

- [网络架构](network-architecture.md) - lwIP 集成、E1000 驱动、DHCP
- [TCP/IP 协议栈](tcpip-stack.md) - lwIP 2.2.1 配置和使用

## Rust 重写状态

### 已完成 Rust 重写的模块

| 模块 | 文件数 | 估计行数 | 状态 |
|------|--------|----------|------|
| 内存管理 | ~5 | ~2,400 | ✅ |
| 进程调度 | ~8 | ~2,000 | ✅ |
| 文件系统 | ~15 | ~3,000 | ✅ |
| PWID | ~12 | ~2,500 | ✅ |
| Barrier | ~1 | ~620 | ✅ |
| DMA | ~3 | ~500 | ✅ |
| IPC | ~10 | ~1,000 | ✅ |
| 同步原语 | ~6 | ~800 | ✅ |
| IDT | ~6 | ~1,000 | ✅ |
| KLog | ~1 | ~500 | ✅ |
| 驱动 | ~6 | ~2,000 | ✅ |

### 仍为 C 实现的模块

| 模块 | 文件数 | 估计行数 | 状态 |
|------|--------|----------|------|
| lwIP 网络栈 | ~100 | ~50,000 | 第三方库 |
| 引导/中断入口 | ~3 | ~500 | 汇编 |

## 开发指南

### 构建系统

```bash
# 完整构建
make all

# 运行内核
make run

# 网络测试
make log-net

# 清理
make clean
```

### 代码风格

- Rust 代码：遵循 `rustfmt` 标准
- C 代码：遵循 `clang-format` 标准
- 汇编代码：NASM 语法

### 测试框架

```bash
# 单元测试
make test-unit

# 集成测试
make test-integration

# 压力测试
make test-stress
```

## 文档维护规范

1. **同步更新**：源码修改后，同步更新相关文档
2. **版本标记**：每个文档标注最后更新日期和版本
3. **代码引用**：使用相对路径引用源码文件
4. **语言要求**：文档使用中文编写，代码注释使用英文

---
**文档维护者**: AI Assistant
**创建日期**: 2026-05-06
**最后更新**: 2026-05-13
