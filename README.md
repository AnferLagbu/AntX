# AntX

AntX 是一台从零构建的 x86_64 操作系统。它不是 Linux 的简化版，也不是某个教程的课后作业——它的每一行代码都是为了回答一个问题：**如果由我自己来设计，操作系统应该是什么样子？**

内核名为 **QueenX**，全部用户态组件加上内核，合称 **AntX**。

技术栈是 C、Rust 和一点点 x86_64 汇编，运行在 QEMU 模拟器上。

## 设计原则

三条原则贯穿整个项目：

- **可理解性优先** — 代码量控制在五万行以内，每一行都有其存在的理由
- **独立性优先** — 借鉴 Unix/Linux 的思想但不盲从，不做"Linux 的缩水版"
- **实验性优先** — 这是个人探索项目，不合理就改，不背历史包袱

## 它有什么特别的

### 没有"用户"这个概念

传统操作系统都有 UID/GID，有 `/etc/passwd`，有 `root` 用户。AntX 没有这些。

取而代之的是 **PWID**——一个由密码加一段备注信息，经 SHA-256 哈希生成的 64 位身份标识。你不需要先"创建账户"，知道密码就能登录；同一密码配合不同备注就是不同的身份。三级权限（Root / Trustworthy / Untrustworthy）直接嵌入 PWID 的高位。

原 Root 是系统中唯一不可删除的身份锚点，首次启动时设定，内核硬编码保护。派生的 Root 可以由原 Root 授权创建和撤销。临时提权通过令牌实现——验证密码后获得一个有时限、有次数限制的提权令牌，执行完毕自动恢复原身份。

这套模型的内核实现约 2500 行 Rust，完整支持了令牌系统、信任链委派、能力矩阵、暴力破解防护和审计日志。

### C 和 Rust 共同构成内核

安全攸关的部分全部用 Rust 写了：

| 模块 | 语言 | 
|------|------|
| 物理/虚拟内存管理 | Rust |
| MLFQ 调度器 + 实时任务 | Rust |
| 文件系统 (VFS/HvFS/RamFS/DiskFS) | Rust |
| PWID 权限系统 | Rust |
| DMA 引擎 | Rust |

C 负责驱动层、系统调用分发、IPC 和与汇编的衔接。汇编只出现在启动代码和上下文切换这种无法避免的地方。

### 调度器不只是"先来先服务"

实现的是多级反馈队列（MLFQ），四个优先级层级，时间片从 10ms 递增到 80ms。用完时间片降级，定期统一提升防止饥饿。另外有一个独立的实时任务队列，支持 FIFO 和 Round-Robin 策略。

### 文件系统是自己设计的

HvFS（Hive File System）是 AntX 的原生文件系统，有自己的 Super Block、Inode 结构、间接块索引和 LRU 块缓存。上面架了一层 VFS，统一了 RamFS、DiskFS、DevFS、ProcFS 五个后端的接口。启动时通过 Smart Mount 自动选择：开发模式下默认用 RamFS 快速启动，检测到磁盘就自动切换持久化存储；发布模式强制要求磁盘。

### 网络栈是"拿来即用"的

集成了完整的 lwIP 2.2.1 协议栈加 Intel E1000 网卡驱动，DHCP 自动拿 IP，ICMP Ping 通了，HTTP Server 能返回页面，mDNS、MQTT、SNMP 这些应用层协议也都挂着。

## 跑起来看看

```bash
# 装依赖（Fedora）
bash scripts/requirements.sh --auto

# 三条命令就能启动
make all
make run-iso        # ISO 启动
make run-net        # 带网络启动
```

启动后你会进入一个叫 **antxsh** 的 Shell，在用户态（Ring 3）运行。它认得十几个命令：`fls` 列目录、`fcat` 看文件、`ilogin` 切换身份、`sver` 显示基于 Git commit 的动态版本号。

还有一套 192+ 测试用例的测试框架：

```bash
make test-quick     # 60 秒快速验证
make test-unit      # 120 秒完整单元测试
```

## 更多内容

代码本身是最好的文档。除此之外：

- [内核架构设计](docs/development/kernel-architecture.md) — 模块划分、初始化顺序、代码量统计
- [PWID 权限模型](docs/development/pwid-model.md) — 为什么不要"用户"、PWID 如何生成、权限如何流转
- [测试框架与进度](docs/progress/milestones.md) — 当前完成度、历史里程碑、下一步计划
- [变更日志](docs/progress/changelog.md) — 从第一天到现在的所有重要变更

## 许可证

MIT © 2026 Anfer
