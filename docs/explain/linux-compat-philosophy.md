# QueenX Linux 兼容设计理念

> 2026-07-10 初版. 定义 QueenX 内核在 Linux 兼容性上的分层策略：对外 ABI 兼容，内部实现自主，借鉴但不盲从.

## 一、三层策略

### 1. 对外接口层 — 保持 Linux 兼容

对外暴露给用户态的接口遵循 Linux/POSIX 标准，确保生态兼容性：

- **syscall 编号**: 直接使用 Linux x86_64 标准编号 (0-299)，QueenX 私有扩展使用 500+
- **文件系统接口**: /proc、/sys、/dev 遵循 Linux 标准格式，用户态工具 (free/top/ps) 可直接运行
- **ELF 格式**: 支持 Linux ELF 二进制，PT_INTERP 检测并改写动态链接器路径
- **信号语义**: 与 POSIX/Linux 一致 (kill/sigaction/sigaltstack)
- **socket 接口**: AF_INET/AF_UNIX/AF_INET6 遵循 POSIX 标准
- **文件锁**: flock/POSIX record locks 遵循标准语义

**理由**: ABI 兼容是获得生态的最低成本路径。syscall 编号是 ABI 约定，不是内核实现细节。

### 2. 内核内部层 — 使用 QX 自己的方式

内核内部实现保持 Rust 纯净，不盲目复制 Linux 内部实现：

- **进程状态**: 使用 7 状态模型 (Created/Ready/Running/Blocked/Zombie/Terminated/Frozen)，比 Linux 的 TASK_RUNNING/TASK_INTERRUPTIBLE/TASK_UNINTERRUPTIBLE 更简洁
- **COW 实现**: 简化为全局 BTreeMap 跟踪引用计数，不引入 Linux 的 per-VMA anon_vma + rmap 反向映射
- **Page Cache**: 固定大小哈希表 (64 桶 x 16 条目)，不引入 Linux 的 xarray/radix tree
- **Swap**: 固定数组 LRU，不引入 Linux 的 zone-based multi-LRU
- **CFS 调度器**: 保留 vruntime + BTreeMap + 权重表核心算法，不引入 load_weight/load_avg 等复杂度
- **同步原语**: 自研 SpinLock/Mutex/RwLock/RCU，不直接复制 Linux 的 locking 原语

**理由**: Linux 内部实现为大规模生产环境优化，复杂度极高。QueenX 作为学习/验证项目，应选择更简洁的实现，在保证功能正确的前提下降低维护成本。

### 3. 借鉴层 — Linux 验证过的最佳方式允许使用

Linux 验证过的成熟算法和设计模式可以在 QueenX 中使用：

- **CFS 调度算法**: vruntime 公平调度是经过 Linux 十几年验证的成熟方案
- **Tickless NO_HZ_IDLE**: 空闲时停止 tick 是经过验证的省电技术
- **伙伴分配器**: 经典的物理内存管理算法
- **Slab 分配器**: 经典的内核对象缓存算法
- **ELF core dump 格式**: NT_PRSTATUS/NT_SIGINFO 是 ELF 标准，gdb/lldb 兼容

**理由**: 不重复造轮子。Linux 社区投入了大量工程精力验证的算法，直接借鉴比从零实现更可靠。

## 二、判断标准

遇到"是否采用 Linux 方式"的决策时，按以下标准判断：

| 场景 | 决策 | 理由 |
|------|------|------|
| 对外 ABI (syscall/文件格式/信号语义) | ✅ 保持 Linux 兼容 | 生态兼容性 |
| 内核内部数据结构 | ⚠️ 优先用 QX 自己的方式 | 降低复杂度 |
| 内核内部算法 | 📋 借鉴 Linux 验证过的 | 避免重复造轮子 |
| Linux 特有功能 (KSM/THP/MGLRU) | ❌ 不引入，除非有明确需求 | 避免不必要的复杂度 |
| Linux 风格的 debug 标记 (0xDEADBEEF) | ⚠️ 改为 QX 特定标记 | 便于 crash dump 识别来源 |

## 三、与 ref-naming.md 的关系

- `ref-naming.md`: 定义 ABI 层面的兼容策略 (syscall 编号/路径/libc/工具链)
- 本文档: 定义内核实现层面的兼容策略 (内部实现/算法借鉴/复杂度控制)

两者共同构成 QueenX 的 Linux 兼容性完整立场。

## 四、交叉引用

- [ref-naming.md](./ref-naming.md) — 命名与 ABI 兼容立场
- [explain-framekernel.md](./explain-framekernel.md) — 框内核架构 (framework/services 分层)
- [linux-compat-maintenance.md](../plan/linux-compat-maintenance.md) — Linux 兼容性维护工程
