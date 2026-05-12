# AntX

从零构建的 x86_64 操作系统。个人兴趣驱动，持续演进中。

> **AntX = QueenX 内核 + 任意用户态**

---

## 这是什么

AntX 是一个完全自研的操作系统——从 Multiboot 引导的第一条指令到 TCP/IP 协议栈，没有使用 Linux 或任何现成内核的代码。它由 **Rust**、**NASM 汇编**和**少量 C** 写成。

它不是某个教程的克隆，也没有"对标 Linux"的野心。它是一个不断生长的实验体，每一行代码的存在原因都清晰可辨。

## 内核能做什么

- **HVFS** — 自研文件系统，类 ext2 设计，三级间接块，FSCK，磁盘持久化
- **PWID** — 基于能力的权限模型，令牌委托、信任链、域隔离
- **Barrier（栏栈）** — 故障恢复屏障，VFS 快照与级联回滚
- **lwIP 2.2.1** — 完整 TCP/IP 协议栈，DHCP / TCP / UDP / HTTP / DNS（因为懒得写网络栈了就薅了个现成的）

## 设计原则

> **可理解性 > 性能** — 每行代码都应知其存在原因  
> **实验性 > 兼容性** — 不合理则改，不保留历史包袱  
> **个人表达 > 行业标准** — 按审美组织，不盲从惯例

## 进一步阅读

技术细节、开发日志和设计文档在 `docs/` 目录下：

- [内核架构设计](docs/development/kernel-architecture.md)
- [HVFS 文件系统](docs/development/hivefs.md)
- [PWID 权限模型](docs/development/pwid-model.md)
- [KLog 日志系统](docs/development/klog-system.md)
- [内存管理](docs/development/memory-management.md)
- [调度器设计](docs/development/thread-scheduler.md)
- [系统调用接口](docs/development/syscall.md)
- [完整文档索引](docs/README.md)

---

> *"一个从零开始的操作系统，因为想知道每一层到底是怎么运作的。"*
