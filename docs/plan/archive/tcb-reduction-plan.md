# AntX TCB 缩减计划

> 本文档记录 AntX/QueenX 项目 TCB 缩减的完整方案、累计进度、判定标准与阶段规划. 创建于 2026-06-10, 2026-06-26 归档重写. 累计 TCB 收益 -10,400+ LoC.

## 现状度量
- **TCB 占比基线 (2026-06-10)**
  - 描述: TCB 度量
  - 方案: framework 181,693 行 / services 17,683 行 / TCB 占比 129.7% / 目标 < 30%; 详见 framekernel-compliance.md 现状度量
  - 状态: [X]
- **已完成的提取 (累计 -10,400+ LoC)**
  - 描述: 11 批次提取
  - 方案: D8 FdTable → services/proc/fd_table.rs -40 / D9 MemoryPressure → services/mm/memory_pressure.rs -106 / E6-1 flock → services/fs/flock.rs -700 / E6-2 inotify → services/fs/inotify.rs -540 / E6-3 dcache → services/fs/dcache.rs -846 / E6-4 FileSystem trait 分发 -300 / E6-5 RamFS → services/fs/ramfs_core.rs -1629 / E6-6 HvFS → services/fs/hvfs/ -6154 / E6-7 DevFS → services/fs/devfs.rs -282 / E6-8 ProcFS → services/fs/procfs_core.rs -238 / E6-9 Chitin↔DevFS 桥接 + VFS 分发接入 -200
  - 状态: [X]

## 提取原则
- **判定标准: 什么放 services**
  - 描述: 5 类放 services vs 留 framework
  - 方案: 放 services (策略): 算法选择 (CFS 权重/buddy 阶数) / 数据结构管理 (VMA 合并/调度队列) / 策略参数 (rlimit/时间片/OOM 评分) / 协议逻辑 (信号投递/seccomp 过滤链) / 格式解析 (ELF 验证/cpio 解包); 留 framework (机制): 硬件操作 (CR3 切换/页表写入/上下文切换) / unsafe 内存操作 (copy_from/to_user/物理页操作) / 原子指令/内存屏障 / 中断控制器编程 (APIC/GIC) / 寄存器读写/MMIO
  - 状态: [X]
- **提取模式**
  - 描述: 3 类提取模式
  - 方案: (1) 完整迁移: 整个模块从 framework 移到 services, framework 仅 re-export (如 E6-5 RamFS); (2) 策略提取: 模块一分为二 — 机制留在 framework, 策略函数移到 services, framework 调用 services 的策略函数; (3) 代理增强: 现有 services 代理层从"薄包装"升级为"策略主体", framework 对应代码缩减为 re-export + unsafe 边界
  - 状态: [X]
- **约束**
  - 描述: 3 项硬约束
  - 方案: services 层必须 #![deny(unsafe_code)] 0 unsafe; 提取后 framework 通过 re-export 保持 API 兼容调用方无需修改; 每项提取必须通过双架构 0w0e + 三审计 + host-tests
  - 状态: [X]

## 阶段规划
- **Phase T1 进程策略提取 (预估 -4,500 LoC)**
  - 描述: 进程策略提取
  - 方案: 包括进程调度策略 (CFS 权重 + MLFQ 周期) / 进程表管理 (生命周期/状态机) / 进程组会话 (PGID/SID/控制终端); 已通过工程纪律性强化专项 (D-01~D-06/T-01~T-05/L-01~L-03) 大部分完成
  - 状态: [X]
- **Phase T2 内存策略提取 (预估 -3,000 LoC)**
  - 描述: 内存策略提取
  - 方案: 包括 VMA 合并/拆分规则 / 页面回收策略 (kswapd LRU) / OOM 评分; 详见 maintenance-cycle-2026-06-19.md REVAL-3 (T2-5 pcache 策略迁移评估后 SKIP, 深度 unsafe 耦合)
  - 状态: [X]
- **Phase T3 同步策略提取 (预估 -1,500 LoC)**
  - 描述: 同步策略提取
  - 方案: 包括锁顺序策略 / 中断上下文睡眠锁检查 / 递归锁检测; 已通过 T-01~T-05 + 工程纪律性专项完成
  - 状态: [X]
- **Phase T4 文件系统策略提取 (预估 -8,000 LoC)**
  - 描述: 文件系统策略提取
  - 方案: 已通过 E6 VFS 策略提取工程完成 (flock/inotify/dcache/RamFS/HvFS/DevFS/ProcFS, 累计 -10,235 LoC, 详见 vfs-policy-extraction.md)
  - 状态: [X]
- **Phase T5 中断/驱动策略提取 (预估 -1,500 LoC)**
  - 描述: 中断/驱动策略提取
  - 方案: 包括中断路由策略 / 设备驱动注册策略; T-04 IrqDecision trait 已完成; 后续 Chitin 注册回调 + DevFS 桥接已通过 E6-9b 完成
  - 状态: [X]
- **Phase T6 凭据/安全策略提取 (预估 -800 LoC)**
  - 描述: 凭据/安全策略提取
  - 方案: 包括 Credo 会话策略 / 授权策略; REVAL-5 (T4-1/T4-2/T4-3 credo/eBPF) 评估后 SKIP (深度 unsafe 耦合); 后续可考虑更激进架构重构
  - 状态: []

## 阶段目标
- **最终目标**
  - 描述: TCB 缩减最终目标
  - 方案: framework LoC 181,693 → < 60,000 (↓ 67%); services LoC 17,683 → > 140,000 (↑ 690%); TCB 占比 129.7% → < 30%; framework unsafe 行 1,848 → < 500
  - 状态: [X]

## 关联文档
- **引用清单**
  - 描述: 7 个关联文档
  - 方案: maintenance-cycle-2026-06-19.md (维护周期主文档) / framekernel-compliance.md (框内核合规工程书) / vfs-policy-extraction.md (VFS 策略提取详细记录) / engineering-discipline.md (工程纪律性专项) / engineering-progress.md (主线工程进度) / kernel-roadmap.md (Phase A-D 路线图) / guide-dev.md (架构详解)
  - 状态: [X]

