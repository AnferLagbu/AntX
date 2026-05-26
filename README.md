# AntX

从零构建的个人操作系统内核，支持 x86_64 和 aarch64 双架构。个人兴趣驱动，持续演进中。

> **AntX = QueenX 内核 + 任意用户态**

---

## 这是什么

AntX 是一个完全自研的操作系统——从 Multiboot 引导的第一条指令到 TCP/IP 协议栈，没有使用 Linux 或任何现成内核的代码。它由 **Rust**、**NASM 汇编**和**少量 C** 写成。

它不是某个教程的克隆，也没有"对标 Linux"的野心。它是一个不断生长的实验体，每一行代码的存在原因都清晰可辨。

## 内核能做什么

- **HvFS v2** — 自研 ZFS 风格文件系统，SPA/DMU/ZAP/TXG 分层架构，COW 事务组，ZIL 意图日志，ARC 自适应缓存，RAID-Z，快照
- **PWID** — 基于能力的权限模型，令牌委托、信任链、域隔离
- **Barrier（栏栈）** — 故障恢复屏障，UndoLog 回滚，RecoveryDomain 级联恢复
- **lwIP 2.2.1** — 完整 TCP/IP 协议栈，DHCP / TCP / UDP / HTTP / DNS（因为我懒得写网络栈了就薅了个现成的）
- **测试框架** — Rust 原生 no_std 测试框架，手动注册 + QEMU Runner，31 个单元测试全部通过

## 支持的架构

| 架构 | 目标三元组 | Makefile 参数 | 状态 |
|------|-----------|--------------|------|
| x86_64 | `x86_64-unknown-none` | `ARCH=x86_64` (默认) | 生产就绪 |
| aarch64 | `aarch64-unknown-none` | `ARCH=aarch64` | Phase 6 完成，QEMU 验证中 |

### 构建

```bash
# x86_64 (默认)
make

# aarch64
make ARCH=aarch64

# 运行测试
make test-host
```

新架构移植请参考 [移植指南](docs/development/arch-porting-guide.md)。

## 设计原则

> **可理解性 > 性能** — 每行代码都应知其存在原因  
> **实验性 > 兼容性** — 不合理则改，不保留历史包袱  
> **个人表达 > 行业标准** — 按审美组织，不盲从惯例

## 进一步阅读

技术细节、开发日志和设计文档在 `docs/` 目录下：

- [内核架构设计](docs/development/kernel-architecture.md)
- [HvFS v2 文件系统](docs/development/hivefs.md)
- [PWID 权限模型](docs/development/pwid-model.md)
- [KLog 日志系统](docs/development/klog-system.md)
- [内存管理](docs/development/memory-management.md)
- [调度器设计](docs/development/thread-scheduler.md)
- [系统调用接口](docs/development/syscall.md)
- [测试框架](docs/development/test-framework.md)
- [多架构解耦工程](docs/development/multiarch-decoupling-plan.md)
- [架构移植指南](docs/development/arch-porting-guide.md)
- [完整文档索引](docs/README.md)
- [演进蓝图](docs/development/ROADMAP.md) — 内核功能路线图

---

> *"Till queendom come." — AURORA, Queendom*
