# TCB 缩减工程规划书

> 本文档记录 AntX 内核 TCB (Trusted Computing Base) 缩减工程的完整方案、阶段划分与进度跟踪.
> 目标: 将 self TCB 占比从当前 54.1% 降至 30% 以下, 缩小安全审计面积, 强化 framekernel 架构合规性.
> 配合 [vfs-policy-extraction.md](./vfs-policy-extraction.md) (E6 已完成) 与 [maintenance-2026-06-11.md](./maintenance-2026-06-11.md) (I-01) 使用.

---

## 文档元信息

| 字段 | 值 |
|------|---|
| 起始日期 | 2026-06-16 |
| 当前 Self TCB | 54.1% (framework 115,456 LoC / 总 145,388 LoC, 不含 smoltcp) |
| 目标 Self TCB | < 30% |
| 需缩减 | ~35,000 effective LoC (framework → services) |
| 关联规范 | [AGENTS.md](../../AGENTS.md) — "能放 services/ 的别放 framework/" |
| 关联审计 | [maintenance-2026-06-11.md](./maintenance-2026-06-11.md) I-01 |
| 完成标记 | 每项完成后将 `[ ]` 改为 `[x]`, 补全完成记录 |

---

## 一、现状分析

### 1.1 TCB 度量 (2026-06-16 基线)

```
framework:     174,656 LoC (raw),    124,934 (effective)
services:       29,148 LoC (raw),     19,932 (effective)
smoltcp:        59,200 LoC (3rd-party, excluded from self-TCB)
self-fw:       115,456 LoC (raw, excluding smoltcp)
unsafe:          1,889 lines (framework),     0 (services)
TCB ratio:        86.2% (incl. smoltcp)
Self TCB:         54.1% (excl. smoltcp)
```

### 1.2 framework Top 10 模块 (按 LoC 降序)

| 模块 | LoC | unsafe | 提取潜力 | 说明 |
|------|-----|--------|---------|------|
| net | 64,196 | 92 | 低 | 含 smoltcp 59,200 (已排除 self-TCB); 自有 ~5K 中策略约 2K |
| driver | 16,363 | 205 | 极低 | 硬件机制, 必须留 framework |
| proc | 14,172 | 237 | **高** | 调度策略/信号策略/进程管理策略可大量提取 |
| mm | 11,715 | 310 | **高** | VMA 策略/PMM 策略/slab 策略可提取 |
| syscall | 10,054 | 141 | 中 | 分发机制留 framework, 个别 syscall 策略可提取 |
| sync | 5,160 | 111 | 低 | 锁原语是机制, 仅 lockdep 策略可少量提取 |
| tests | 6,334 | 31 | 无 | 测试代码不计 TCB |
| arch | 5,937 | 190 | 无 | 架构特定机制, 不可提取 |
| timer | 4,040 | 33 | 低 | 定时器机制, 仅调度策略可少量提取 |
| credo | 3,825 | 21 | 中 | 会话策略/授权策略可提取 |

### 1.3 已完成的提取 (累计 -10,400+ LoC)

| 批次 | 提取项 | 缩减 LoC | 日期 |
|------|--------|---------|------|
| D8 | FdTable → services/proc/fd_table.rs | -40 | 2026-06-11 |
| D9 | MemoryPressure → services/mm/memory_pressure.rs | -106 | 2026-06-11 |
| E6-1 | flock → services/fs/flock.rs | -700 | 2026-06-11 |
| E6-2 | inotify → services/fs/inotify.rs | -540 | 2026-06-11 |
| E6-3 | dcache → services/fs/dcache.rs | -846 | 2026-06-11 |
| E6-4 | FileSystem trait 分发 | -300 | 2026-06-11 |
| E6-5 | RamFS → services/fs/ramfs_core.rs | -1,629 | 2026-06-11 |
| E6-6 | HvFS → services/fs/hvfs/ | -6,154 | 2026-06-11 |
| E6-7 | DevFS → services/fs/devfs.rs | -282 | 2026-06-11 |
| E6-8 | ProcFS → services/fs/procfs_core.rs | -238 | 2026-06-11 |
| E6-9 | Chitin↔DevFS 桥接 + VFS 分发接入 | -200 | 2026-06-11 |

---

## 二、提取原则

### 2.1 判定标准: 什么放 services

| 放 services (策略) | 留 framework (机制) |
|-------------------|-------------------|
| 算法选择 (CFS 权重计算、buddy 阶数选择) | 硬件操作 (CR3 切换、页表写入、上下文切换) |
| 数据结构管理 (VMA 合并/拆分规则、调度队列组织) | unsafe 内存操作 (copy_from/to_user, 物理页操作) |
| 策略参数 (rlimit 阈值、调度时间片、OOM 评分) | 原子指令/内存屏障 |
| 协议逻辑 (信号投递规则、seccomp 过滤链) | 中断控制器编程 (APIC/GIC) |
| 格式解析 (ELF 验证、cpio 解包) | 寄存器读写、MMIO |

### 2.2 提取模式

1. **完整迁移**: 整个模块从 framework 移到 services, framework 仅 re-export (如 E6-5 RamFS)
2. **策略提取**: 模块一分为二 — 机制留在 framework, 策略函数移到 services, framework 调用 services 的策略函数
3. **代理增强**: 现有 services 代理层从"薄包装"升级为"策略主体", framework 对应代码缩减为 re-export + unsafe 边界

### 2.3 约束

- services 层必须 `#![deny(unsafe_code)]`, 0 unsafe
- 提取后 framework 通过 re-export 保持 API 兼容, 调用方无需修改
- 每项提取必须通过: 双架构 0w0e + 三审计 + host-tests

---

## 三、阶段规划

### Phase T1: 进程策略提取 (预估 -4,500 LoC)

> **目标**: 将 proc 模块中调度策略、信号策略、进程管理策略提取到 services.
> **前置**: 无 (可与 T2 并行).

---

#### [x] T1-1: CFS 调度策略提取

**当前**: `framework/proc/cfs.rs` (438 行) + `framework/proc/scheduler.rs` 策略部分 (~600 行)
**提取内容**:
- CFS 权重计算 (prio_to_weight / prio_to_wmult 查找表)
- 时间片计算 (sched_period / slice_calc)
- vruntime 更新与比较策略
- 负载权重计算
- 调度统计策略
- CfsRunQueue / DlRunQueue 完整实现
- DeadlineParams (EDF + CBS)

**留在 framework**:
- 上下文切换汇编 (`switch.asm`)
- runqueue 锁操作
- CPU 队列管理 (`cpu_queue.rs`)
- 时钟中断回调触发调度
- scheduler.rs 机制部分 (后续可继续拆分)

**目标文件**: `services/proc/sched_policy.rs`
**预估缩减**: -800 LoC
**难度**: 中 (需拆分 scheduler.rs 机制/策略)
**验收**:
- [x] services/proc/sched_policy.rs `#![deny(unsafe_code)]`
- [x] framework re-export 保持 API 兼容
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/cfs.rs 438→19 行 (-419), services/proc/sched_policy.rs 新增 310 行
- TCB: 52.7% → 52.5% (proc 11,335→10,915 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: 原文件 0 unsafe, 纯策略代码直接迁移; scheduler.rs 策略部分待后续拆分

---

#### [ ] T1-2: 信号投递策略提取

**当前**: `framework/proc/signal.rs` (857 行)
**提取内容**:
- 信号掩码操作策略 (sigprocmask 语义)
- 信号优先级排序 (实时信号先于标准信号)
- 信号处理链查找 (sigaction 表管理)
- 核心转储信号判定策略 (哪些信号触发 coredump)
- 信号忽略/默认处理策略表

**留在 framework**:
- 信号栈帧构建 (unsafe: 写用户内存)
- sigreturn 恢复 (unsafe: 读用户内存)
- 中断返回路径信号检查

**目标文件**: `services/proc/signal_policy.rs`
**预估缩减**: -400 LoC
**难度**: 中
**验收**:
- [ ] services/proc/signal_policy.rs `#![deny(unsafe_code)]`
- [ ] framework re-export 保持 API 兼容
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T1-3: namespace 策略完整迁移

**当前**: `framework/proc/namespace.rs` (816 行) + `services/proc/namespace.rs` (代理)
**提取内容**: 将 namespace 完整实现从 framework 迁移到 services
- NamespaceSet 管理
- clone_from / unshare / setns 策略
- PID/Net/Mount/User/IPC/UTS/Cgroup 7 种 ns 的隔离规则

**留在 framework**: re-export
**目标文件**: `services/proc/namespace.rs` (升级为策略主体)
**预估缩减**: -750 LoC
**难度**: 中 (namespace 操作依赖 Process 结构, 需通过 safe API 访问)
**验收**:
- [x] services/proc/namespace.rs `#![deny(unsafe_code)]`
- [x] framework/proc/namespace.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/namespace.rs 816→20 行 (-796), services/proc/namespace.rs 25→762 行 (+737)
- TCB: 53.8% → 53.5% (proc 13,960→13,163 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过

---

#### [x] T1-4: cgroup 策略完整迁移

**当前**: `framework/proc/cgroup.rs` (768 行) + `services/proc/cgroup.rs` (代理)
**提取内容**: 将 cgroup 完整实现从 framework 迁移到 services
- CgroupRq / CgroupSubsystem 管理
- CPU/内存/PID/IO 四控制器策略
- fork 继承 / exit 清理策略

**留在 framework**: re-export
**目标文件**: `services/proc/cgroup.rs` (升级为策略主体)
**预估缩减**: -700 LoC
**难度**: 中
**验收**:
- [x] services/proc/cgroup.rs `#![deny(unsafe_code)]`
- [x] framework/proc/cgroup.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/cgroup.rs 768→20 行 (-748), services/proc/cgroup.rs 54→587 行 (+533)
- TCB: 53.2% → 52.9% (proc 12,564→11,815 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: klog_ffi! 替换为 framework::klog::serial_write_bytes (safe)

---

#### [x] T1-5: seccomp 策略完整迁移

**当前**: `framework/proc/seccomp.rs` (495 行) + `services/proc/seccomp.rs` (代理)
**提取内容**: 将 seccomp 完整实现从 framework 迁移到 services
- SeccompFilter 链式评估
- SeccompRule 匹配逻辑
- Strict 模式白名单
- Filter 模式 arg comparator

**留在 framework**: re-export
**目标文件**: `services/proc/seccomp.rs` (升级为策略主体)
**预估缩减**: -450 LoC
**难度**: 中
**验收**:
- [x] services/proc/seccomp.rs `#![deny(unsafe_code)]`
- [x] framework/proc/seccomp.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/seccomp.rs 495→16 行 (-479), services/proc/seccomp.rs 52→391 行 (+339)
- TCB: 52.9% → 52.7% (proc 11,815→11,335 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: 原文件 0 unsafe, 纯策略代码直接迁移

---

#### [x] T1-6: session 策略完整迁移

**当前**: `framework/proc/session.rs` (616 行) + `services/proc/session.rs` (代理)
**提取内容**: 将 session 完整实现从 framework 迁移到 services
- SessionManager 全部策略
- 进程组/会话/控制终端管理
- setsid/setpgid/getsid/getpgid 策略
- 前台进程组/终端信号广播

**留在 framework**: re-export
**目标文件**: `services/proc/session.rs` (升级为策略主体)
**预估缩减**: -550 LoC
**难度**: 中
**验收**:
- [x] services/proc/session.rs `#![deny(unsafe_code)]`
- [x] framework/proc/session.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/session.rs 616→18 行 (-598), services/proc/session.rs 90→552 行 (+462)
- TCB: 53.5% → 53.2% (proc 13,163→12,564 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: klog_ffi_info (unsafe) 替换为 framework::klog::serial_write_bytes (safe)

---

#### [SKIP] T1-7: posix_timer 策略完整迁移

**当前**: `framework/proc/posix_timer.rs` (652 行) + `services/proc/posix_timer.rs` (代理)
**提取内容**: 将 posix_timer 完整实现从 framework 迁移到 services
- TimerManager 全局表管理
- timer_create/settime/gettime/delete 策略
- 超时回调策略 (单次/周期)
- overrun 计数

**留在 framework**: re-export + hrtimer 回调注册 (中断上下文)
**目标文件**: `services/proc/posix_timer.rs` (升级为策略主体)
**预估缩减**: -500 LoC
**难度**: 中 (hrtimer 回调在中断上下文, 需拆分)
**跳过原因**: 含 unsafe 回调指针转换 (core::ptr::read_unaligned, callback ptr → fn) 与用户态指针操作, 策略与机制深度耦合, 不适合完整迁移
**验收**:
- [ ] services/proc/posix_timer.rs `#![deny(unsafe_code)]`
- [ ] framework/proc/posix_timer.rs 仅 re-export + ISR 桥接
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T1-8: rlimit 策略完整迁移

**当前**: `framework/proc/rlimit.rs` (308 行) + `services/proc/rlimit.rs` (代理)
**提取内容**: 将 rlimit 完整实现从 framework 迁移到 services
- RlimitTable 管理
- getrlimit/setrlimit 策略
- 特权检查策略
- 辅助检查 (nofile/as/nproc/memlock/stack)

**留在 framework**: re-export + syscall 入口 (含 unsafe 用户指针操作)
**目标文件**: `services/proc/rlimit.rs` (升级为策略主体)
**预估缩减**: -270 LoC
**难度**: 低
**验收**:
- [x] services/proc/rlimit.rs `#![deny(unsafe_code)]`
- [x] framework/proc/rlimit.rs 仅 re-export + syscall 入口
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/rlimit.rs 308→97 行 (-211), services/proc/rlimit.rs 55→259 行 (+204)
- TCB: 54.1% → 54.0% (proc 14,172→13,960 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过, host-tests 10/10

---

### Phase T2: 内存管理策略提取 (预估 -3,000 LoC)

> **目标**: 将 mm 模块中 VMA 策略、PMM 策略、slab 策略提取到 services.
> **前置**: 无 (可与 T1 并行).

---

#### [ ] T2-1: VMA 策略提取

**当前**: `framework/mm/vma.rs` (1,130 行)
**提取内容**:
- VMA 合并/拆分规则判定
- VmFlags 位集操作策略
- madvise 建议值路由 (24 种 advice → 动作映射)
- mlock/munlock 范围策略
- mprotect 权限变更策略
- mincore 驻留查询策略
- mremap 重映射策略

**留在 framework**:
- 页表操作 (map/unmap page)
- VMA 红黑树插入/删除 (指针操作)
- MmStruct 锁管理

**目标文件**: `services/mm/vma_policy.rs`
**预估缩减**: -500 LoC
**难度**: 中高 (VMA 操作与页表操作紧密耦合, 需仔细拆分)
**验收**:
- [ ] services/mm/vma_policy.rs `#![deny(unsafe_code)]`
- [ ] framework re-export 保持 API 兼容
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [ ] T2-2: PMM 策略提取

**当前**: `framework/mm/pmm.rs` (1,148 行)
**提取内容**:
- buddy 阶数选择策略 (向上取整到 2^n)
- 分配失败时的降级策略
- 碎片化评估策略
- 空闲页面回收阈值策略
- 内存水位线计算

**留在 framework**:
- buddy 位图操作 (bitmap set/clear/test)
- 物理页分配/释放 (unsafe: 页表操作)
- per-CPU 页缓存 (pcp) 管理
- 锁操作

**目标文件**: `services/mm/pmm_policy.rs`
**预估缩减**: -400 LoC
**难度**: 中高 (buddy 分配器机制/策略耦合较深)
**验收**:
- [ ] services/mm/pmm_policy.rs `#![deny(unsafe_code)]`
- [ ] framework re-export 保持 API 兼容
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [ ] T2-3: slab 策略提取

**当前**: `framework/mm/slab.rs` (1,044 行) + `framework/mm/kmalloc_slab.rs` (172 行)
**提取内容**:
- 缓存大小选择策略 (对象大小 → slab 阶数映射)
- 对象构造/析构回调策略
- 缓存着色 (coloring) 策略
- slab 缓存统计策略

**留在 framework**:
- slab 页面分配/释放
- 空闲链表操作 (unsafe: 指针操作)
- 锁操作

**目标文件**: `services/mm/slab_policy.rs`
**预估缩减**: -350 LoC
**难度**: 中
**验收**:
- [ ] services/mm/slab_policy.rs `#![deny(unsafe_code)]`
- [ ] framework re-export 保持 API 兼容
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [ ] T2-4: swap 策略完整迁移

**当前**: `framework/mm/swap.rs` (894 行) + `services/mm/swap.rs` (代理)
**提取内容**: 将 swap 完整实现从 framework 迁移到 services
- LRU 链表管理策略
- 页面选择策略 (kswapd 扫描逻辑)
- swap slot 分配策略
- 换出/换入策略

**留在 framework**: re-export + 页表操作 (swap entry 写入 PTE)
**目标文件**: `services/mm/swap.rs` (升级为策略主体)
**预估缩减**: -600 LoC
**难度**: 中高 (swap 与页表操作耦合)
**验收**:
- [ ] services/mm/swap.rs `#![deny(unsafe_code)]`
- [ ] framework/mm/swap.rs 仅 re-export + PTE 操作
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [SKIP] T2-5: pcache 策略完整迁移

**当前**: `framework/mm/pcache.rs` (457 行) + `services/mm/pcache.rs` (代理)
**提取内容**: 将 page cache 完整实现从 framework 迁移到 services
- PageCacheEntry 管理
- 查找/填充/预读策略
- 脏页追踪策略

**留在 framework**: re-export + 物理页操作
**目标文件**: `services/mm/pcache.rs` (升级为策略主体)
**预估缩减**: -350 LoC
**难度**: 中
**跳过原因**: 含 14 处 unsafe (UnsafeCell 裸指针操作、unsafe impl Send/Sync、pcache_copy_to_user 用户态拷贝、zeroed 初始化), 策略与机制深度耦合
**验收**:
- [ ] services/mm/pcache.rs `#![deny(unsafe_code)]`
- [ ] framework/mm/pcache.rs 仅 re-export
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T2-6: numa 策略完整迁移

**当前**: `framework/mm/numa.rs` (469 行) + `services/mm/numa.rs` (代理)
**提取内容**: 将 NUMA 完整实现从 framework 迁移到 services
- NumaTopology 管理
- 距离矩阵
- Mempolicy 策略 (bind/interleave/preferred)
- UMA 回退策略
- NUMA syscall (get_mempolicy/set_mempolicy/migrate_pages/getcpu)

**留在 framework**: re-export
**目标文件**: `services/mm/numa.rs` (升级为策略主体)
**预估缩减**: -400 LoC
**难度**: 中
**验收**:
- [x] services/mm/numa.rs `#![deny(unsafe_code)]`
- [x] framework/mm/numa.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/mm/numa.rs 469→15 行 (-454), services/mm/numa.rs 50→407 行 (+357)
- TCB: 52.5% → 52.3% (mm 11,698→11,260 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: klog_ffi! 替换为 framework::klog::serial_write_bytes (safe); #[no_mangle] 移除 (deny unsafe)

---

### Phase T3: 网络策略提取 (预估 -1,500 LoC)

> **目标**: 将 net 模块中策略代码提取到 services.
> **前置**: 无 (可与 T1/T2 并行, 但建议 T1/T2 优先).

---

#### [SKIP] T3-1: 网络初始化策略提取

**当前**: `framework/net/init.rs` (2,133 行)
**提取内容**:
- DHCP 配置策略 (超时/重试/参数)
- 接口配置策略 (IP/掩码/网关)
- 协议栈初始化顺序策略
- 网卡选择策略

**留在 framework**:
- smoltcp Interface 创建 (unsafe: MMIO)
- 网卡 DMA 缓冲区管理
- 中断注册

**目标文件**: `services/net/init_policy.rs`
**预估缩减**: -600 LoC
**难度**: 中 (init.rs 混合了硬件初始化和策略配置)
**跳过原因**: 含 55 处 unsafe (smoltcp Interface 创建、MMIO、DMA 缓冲区、中断注册), 策略与机制深度耦合, 无法安全分离
**验收**:
- [ ] services/net/init_policy.rs `#![deny(unsafe_code)]`
- [ ] framework re-export 保持 API 兼容
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T3-2: netfilter 策略完整迁移

**当前**: `framework/net/netfilter.rs` (439 行) + `services/net/netfilter.rs` (代理)
**提取内容**: 将 netfilter 完整实现从 framework 迁移到 services
- NfChain / NfRule 管理
- 规则匹配策略 (CIDR + 端口 + 协议)
- 钩子点注册/注销策略
- Verdict 判定策略

**留在 framework**: re-export
**目标文件**: `services/net/netfilter.rs` (升级为策略主体)
**预估缩减**: -380 LoC
**难度**: 中
**验收**:
- [x] services/net/netfilter.rs `#![deny(unsafe_code)]`
- [x] framework/net/netfilter.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/net/netfilter.rs 439→16 行 (-423), services/net/netfilter.rs 49→373 行 (+324)
- TCB: 51.8% → 51.7% (net 63,185→62,761 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: 原文件 0 unsafe, 纯策略代码直接迁移

---

#### [x] T3-3: route 策略完整迁移

**当前**: `framework/net/route.rs` (336 行) + `services/net/route.rs` (代理)
**提取内容**: 将路由表完整实现从 framework 迁移到 services
- RouteEntry 管理
- CIDR 最长前缀匹配策略
- 路由表 CRUD + syscall

**留在 framework**: smoltcp 同步 (sync_route_to_smoltcp / rebuild_smoltcp_routes) + re-export
**目标文件**: `services/net/route.rs` (升级为策略主体)
**预估缩减**: -300 LoC
**难度**: 低
**验收**:
- [x] services/net/route.rs `#![deny(unsafe_code)]`
- [x] framework/net/route.rs 仅 re-export + smoltcp 同步
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/net/route.rs 336→111 行 (-225), services/net/route.rs 32→192 行 (+160)
- TCB: 52.3% → 52.2%
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: smoltcp 同步逻辑留在 framework (依赖 raw::stack_mut); services 通过 framework API 委托同步

---

#### [x] T3-4: unix socket 策略完整迁移

**当前**: `framework/net/unix.rs` (805 行) + `services/net/unix.rs` (代理)
**提取内容**: 将 UDS 完整实现从 framework 迁移到 services
- 独立路径表管理
- 连接/监听策略
- 数据传输策略 (STREAM/DGRAM)
- 缓冲区管理策略

**留在 framework**: re-export
**目标文件**: `services/net/unix.rs` (升级为策略主体)
**预估缩减**: -700 LoC
**难度**: 中
**验收**:
- [x] services/net/unix.rs `#![deny(unsafe_code)]`
- [x] framework/net/unix.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/net/unix.rs 805→22 行 (-783), services/net/unix.rs 186→813 行 (+627)
- TCB: 52.2% → 51.8% (net 63,969→63,185 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: 原文件 0 unsafe, 纯策略代码直接迁移; 含完整单元测试

---

### Phase T4: 安全/调试策略提取 (预估 -2,000 LoC)

> **目标**: 将 credo/debug 模块中策略代码提取到 services.
> **前置**: T1 (进程策略) 部分完成.

---

#### [SKIP] T4-1: credo 会话策略提取

**当前**: `framework/credo/session.rs` (551 行) + `services/credo/sessions.rs` (代理)
**提取内容**: 将会话管理完整实现从 framework 迁移到 services
- Session 创建/销毁策略
- PWM 分配/回收策略
- 能力委托策略

**留在 framework**: re-export + PWM 硬件操作 (如有)
**目标文件**: `services/credo/sessions.rs` (升级为策略主体)
**预估缩减**: -500 LoC
**难度**: 中
**跳过原因**: session.rs 深度依赖 PROCESS_TABLE (framework::proc) 和 credo 子系统内部模块 (identity/engine/audit), 策略与机制深度耦合; services/credo/sessions.rs 已有独立 SessionTable 实现 (489 行), 两套会话模型职责不同 (per-process PwmContext vs 全局 SessionTable)
**验收**:
- [ ] services/credo/sessions.rs `#![deny(unsafe_code)]`
- [ ] framework/credo/session.rs 仅 re-export
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [SKIP] T4-2: credo 授权策略提取

**当前**: `framework/credo/identity.rs` (597 行) + `framework/credo/grant.rs` + `services/credo/identity.rs` + `services/credo/grants.rs`
**提取内容**: 将授权管理完整实现从 framework 迁移到 services
- Identity 管理策略
- Grant 授权/撤销策略
- 权限检查策略链

**留在 framework**: re-export + 密码学原语 (SHA256/Ed25519)
**目标文件**: `services/credo/identity.rs` + `services/credo/grants.rs` (升级为策略主体)
**预估缩减**: -550 LoC
**难度**: 中
**跳过原因**: identity.rs 含 3 处 unsafe (全局表裸指针访问 get_table_mut/addr_of!), grant.rs 含 2 处 unsafe (GRANT_RECORDS 裸指针访问), 策略与全局静态表机制深度耦合
**验收**:
- [ ] services/credo/identity.rs + grants.rs `#![deny(unsafe_code)]`
- [ ] framework/credo/identity.rs + grant.rs 仅 re-export
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [SKIP] T4-3: eBPF 验证器策略提取

**当前**: `framework/debug/ebpf.rs` (1,493 行) + `services/debug/ebpf.rs` (代理)
**提取内容**: 将 eBPF 验证器策略提取到 services
- BpfVerifier 有界循环检查
- 寄存器类型追踪
- 指针安全检查
- 程序复杂度限制策略

**留在 framework**: re-export + BpfInterpreter (热路径, 需高性能)
**目标文件**: `services/debug/ebpf.rs` (升级为策略主体)
**预估缩减**: -600 LoC
**难度**: 中高 (验证器与解释器有交叉)
**跳过原因**: 含 30 处 unsafe (BpfInterpreter 内存操作、用户态指针读写、bpf_map 操作), 验证器与解释器深度交叉, 无法安全分离
**验收**:
- [ ] services/debug/ebpf.rs `#![deny(unsafe_code)]`
- [ ] framework/debug/ebpf.rs 仅 re-export + 解释器
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T4-4: 电源管理策略提取

**当前**: `framework/driver/power.rs` (728 行) + `services/driver/power.rs` (代理)
**提取内容**: 将电源管理策略提取到 services
- C-state 选择策略
- DVFS governor 策略 (performance/powersave/ondemand)
- 通知器链管理策略
- syscall 分发策略

**留在 framework**: re-export + 硬件操作 (arch_halt/read_timestamp/arch_suspend_to_ram/arch_shutdown) + 全局实例
**目标文件**: `services/driver/power.rs` (升级为策略主体)
**预估缩减**: -400 LoC
**难度**: 中
**验收**:
- [x] services/driver/power.rs `#![deny(unsafe_code)]`
- [x] framework/driver/power.rs 仅 re-export + 硬件操作
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/driver/power.rs 728→177 行 (-551), services/driver/power.rs 37→560 行 (+523)
- TCB: 51.7% → 51.4% (driver 16,362→15,811 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: 7 处 unsafe (arch_halt/read_timestamp/arch_suspend_to_ram) 留在 framework; services 通过 select_cstate/suspend_prepare/sys_pm_dispatch 委托硬件操作

---

### Phase T5: syscall 策略提取 (预估 -1,500 LoC)

> **目标**: 将 syscall 模块中策略代码提取到 services.
> **前置**: T1/T2 部分完成 (proc/mm 策略已提取后 syscall 可调用 services).

---

#### [ ] T5-1: syscall 分发策略提取

**当前**: `framework/syscall/mod.rs` (3,974 行)
**提取内容**:
- syscall 号 → 处理函数映射表 (分发策略)
- 参数校验策略 (范围检查、权限检查)
- 返回值转换策略

**留在 framework**:
- 系统调用入口汇编 (syscall/sysret 指令)
- 用户指针操作 (UserReadPtr/UserWritePtr)
- 寄存器读写

**目标文件**: `services/syscall/dispatch.rs`
**预估缩减**: -600 LoC
**难度**: 高 (mod.rs 是最大单文件, 机制/策略深度混合)
**验收**:
- [ ] services/syscall/dispatch.rs `#![deny(unsafe_code)]`
- [ ] framework/syscall/mod.rs 仅保留入口 + unsafe 边界
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T5-2: linuxulator 策略完整迁移

**当前**: `framework/syscall/linuxulator.rs` (569 行)
**提取内容**: 将 linuxulator 完整实现从 framework 迁移到 services
- x86_64 + aarch64 syscall 号翻译表
- LinuxArgs 结构体适配
- is_rt_sigreturn / translate_syscall / translate_args

**留在 framework**: re-export
**目标文件**: `services/syscall/linuxulator.rs`
**预估缩减**: -550 LoC
**难度**: 低 (纯数据映射, 无 unsafe)
**验收**:
- [x] services/syscall/linuxulator.rs `#![deny(unsafe_code)]`
- [x] framework/syscall/linuxulator.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/syscall/linuxulator.rs 569→12 行 (-557), services/syscall/linuxulator.rs +470 行
- TCB: 54.0% → 53.8% (syscall 10,054→9,496 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过, host-tests 通过

---

#### [SKIP] T5-3: epoll 策略完整迁移

**当前**: `framework/syscall/epoll.rs` (512 行) + `services/sync/epoll.rs` (代理)
**提取内容**: 将 epoll 完整实现从 framework 迁移到 services
- EventPoll 管理
- 红黑树索引策略
- 就绪链表管理
- epoll_ctl/epoll_wait 策略

**留在 framework**: re-export + 中断唤醒路径
**目标文件**: `services/sync/epoll.rs` (升级为策略主体)
**预估缩减**: -400 LoC
**难度**: 中
**跳过原因**: 含 3 处 unsafe (core::ptr::read/write 用户态指针), 深度依赖 VFS/scheduler/eventfd/signalfd/timerfd 等 framework 内部模块, 策略与机制深度耦合
**验收**:
- [ ] services/sync/epoll.rs `#![deny(unsafe_code)]`
- [ ] framework/syscall/epoll.rs 仅 re-export
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T5-4: syscall/types.rs 纯类型迁移

**当前**: `framework/syscall/types.rs` (864 行)
**提取内容**: 将 syscall 类型定义完整迁移到 services
- 所有 syscall 编号常量 (SYS_*/QX_*)
- Errno 枚举及 Display/转换实现
- SyscallRegs 结构体
- SyscallHandler / SyscallResult 类型别名

**留在 framework**: re-export
**目标文件**: `services/syscall/types.rs`
**预估缩减**: -850 LoC
**难度**: 低 (纯数据定义, 0 unsafe)
**验收**:
- [x] services/syscall/types.rs `#![deny(unsafe_code)]`
- [x] framework/syscall/types.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/syscall/types.rs 864→9 行 (-855), services/syscall/types.rs +873 行
- TCB: 51.4% → 51.0% (syscall 9,496→8,632 LoC)
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: 纯类型定义, 0 unsafe; services/syscall/mod.rs Errno re-export 改为本地 types 模块

---

### Phase T6: IPC 策略提取 (预估 -800 LoC)

> **目标**: 将 IPC 模块中策略代码提取到 services.
> **前置**: T1 (进程策略) 部分完成.

---

#### [ ] T6-1: IPC 策略提取 (msgq/shm/sem)

**当前**: `framework/ipc/` (msgq.rs 432 行 + shm.rs + sem.rs + types.rs 483 行)
**提取内容**:
- 消息队列管理策略 (msgq 创建/发送/接收/销毁)
- 共享内存管理策略 (shm attach/detach/权限)
- 信号量策略 (sem 操作/undo)
- IPC 命名空间策略

**留在 framework**:
- copy_from/to_user (消息/数据拷贝)
- 页表操作 (shm 映射)
- 锁操作

**目标文件**: `services/ipc/` (新模块)
**预估缩减**: -800 LoC
**难度**: 中高 (IPC 与进程/内存管理交叉)
**验收**:
- [ ] services/ipc/*.rs `#![deny(unsafe_code)]`
- [ ] framework/ipc/ 仅保留 unsafe 边界
- [ ] 双架构 0w0e + 三审计 + host-tests

---

#### [x] T6-2: proc/types.rs 纯类型迁移

**当前**: `framework/proc/types.rs` (454 行)
**提取内容**: 将 proc 类型定义完整迁移到 services
- TaskState / SchedulingClass / ProcessGroup 等枚举
- TaskStruct 字段定义
- 信号/调度/资源相关类型

**留在 framework**: re-export
**目标文件**: `services/proc/types.rs`
**预估缩减**: -430 LoC
**难度**: 低 (纯类型定义, 0 unsafe)
**验收**:
- [x] services/proc/types.rs `#![deny(unsafe_code)]`
- [x] framework/proc/types.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/types.rs 454→9 行 (-432), services/proc/types.rs +440 行
- TCB: 51.0% → 50.8%
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: 纯类型定义, 0 unsafe; services/proc/mod.rs re-export 来源更新

---

#### [x] T6-3: ipc/types.rs 纯类型迁移

**当前**: `framework/ipc/types.rs` (325 行)
**提取内容**: 将 IPC 类型定义完整迁移到 services
- IpcId / IpcKey / IpcPerm 等类型
- 消息队列/共享内存/信号量相关常量与结构体

**留在 framework**: re-export
**目标文件**: `services/ipc/types.rs`
**预估缩减**: -310 LoC
**难度**: 低 (纯类型定义, 0 unsafe)
**验收**:
- [x] services/ipc/types.rs `#![deny(unsafe_code)]`
- [x] framework/ipc/types.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/ipc/types.rs 325→9 行 (-317), services/ipc/types.rs +323 行
- TCB: 50.8% → 50.5%
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: IpcId 重名冲突修复 (mod.rs 本地定义 → types re-export)

---

#### [SKIP] T6-4: fs/vfs/types.rs 纯类型迁移

**当前**: `framework/fs/vfs/types.rs`
**跳过原因**: FileSystem trait 被 framework/vfs/vfs.rs 直接使用, 迁移会产生反向依赖, 不符合 framekernel 分层

---

#### [x] T6-5: proc/fd_alloc.rs 全局 FD 分配器迁移

**当前**: `framework/proc/fd_alloc.rs` (331 行)
**提取内容**: 将全局 FD 分配器完整迁移到 services
- FdSubsystem / FdRange / FdPlan 类型
- FD 范围规划 + 分配/释放/反查策略

**留在 framework**: re-export
**目标文件**: `services/proc/fd_alloc.rs`
**预估缩减**: -310 LoC
**难度**: 低 (纯策略, 0 unsafe)
**验收**:
- [x] services/proc/fd_alloc.rs `#![deny(unsafe_code)]`
- [x] framework/proc/fd_alloc.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/proc/fd_alloc.rs 331→9 行 (-331), services/proc/fd_alloc.rs +331 行
- TCB: 50.5% → 50.4%
- 验证: x86_64/aarch64 0w0e, 三审计通过

---

#### [x] T6-6: barrier/reset/config.rs 恢复配置迁移

**当前**: `framework/barrier/reset/config.rs` (178 行)
**提取内容**: 将恢复配置完整迁移到 services
- RecoveryLayer / RecoveryResult / RecoveryConfig 类型
- 原子状态变量 (CURRENT_LAYER / RESET_IN_PROGRESS 等)
- 统计函数

**留在 framework**: re-export
**目标文件**: `services/barrier/reset_config.rs`
**预估缩减**: -170 LoC
**难度**: 低 (纯配置, 0 unsafe)
**验收**:
- [x] services/barrier/reset_config.rs `#![deny(unsafe_code)]`
- [x] framework/barrier/reset/config.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/barrier/reset/config.rs 178→9 行 (-178), services/barrier/reset_config.rs +178 行
- TCB: 50.5% → 50.4%
- 验证: x86_64/aarch64 0w0e, 三审计通过

---

#### [x] T6-7: credo/types.rs 纯类型迁移

**当前**: `framework/credo/types.rs` (454 行)
**提取内容**: 将 Credo 类型定义完整迁移到 services
- PWM 相关类型 (PwmId / PwmEntry / PwmError)
- 能力矩阵类型 (CapDomain / CapBits)
- 身份条目 / 审计类型
- raw::bytes_to_str (from_utf8_unchecked → from_utf8, 消除唯一 unsafe)

**留在 framework**: re-export
**目标文件**: `services/credo/types.rs`
**预估缩减**: -440 LoC
**难度**: 低 (纯类型定义, 1 unsafe → safe)
**验收**:
- [x] services/credo/types.rs `#![deny(unsafe_code)]`
- [x] framework/credo/types.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/credo/types.rs 454→9 行 (-454), services/credo/types.rs +458 行
- TCB: 50.4% → 50.1%
- 验证: x86_64/aarch64 0w0e, 三审计通过
- 备注: raw::bytes_to_str 从 from_utf8_unchecked 改为 from_utf8().unwrap_or(""), 消除唯一 unsafe

---

#### [x] T6-8: credo/capability.rs + sha256.rs 迁移

**当前**: `framework/credo/capability.rs` (54 行) + `framework/credo/sha256.rs` (186 行)
**提取内容**:
- capability: 16 域能力位常量 + VIABLE_FLOOR 数组
- sha256: SHA-256 哈希纯算法实现 (0 unsafe)

**留在 framework**: re-export
**目标文件**: `services/credo/capability.rs` + `services/credo/sha256.rs`
**预估缩减**: -230 LoC
**难度**: 低 (纯常量 + 纯算法, 0 unsafe)
**验收**:
- [x] services/credo/capability.rs `#![deny(unsafe_code)]`
- [x] services/credo/sha256.rs `#![deny(unsafe_code)]`
- [x] framework/credo/capability.rs 仅 re-export
- [x] framework/credo/sha256.rs 仅 re-export
- [x] 双架构 0w0e + 三审计 + host-tests

**完成记录**:
- 日期: 2026-06-16
- 改动: framework/credo/capability.rs 54→9 行 (-54), framework/credo/sha256.rs 186→9 行 (-186)
- TCB: 50.1% → 50.0%
- 验证: x86_64/aarch64 0w0e, 三审计通过

---

## 四、进度总表

| Phase | 名称 | 任务数 | 预估缩减 LoC | 状态 |
|-------|------|--------|-------------|------|
| T1 | 进程策略提取 | 8 | -4,500 | **进行中** (6/8 完成, 1 SKIP) |
| T2 | 内存管理策略提取 | 6 | -3,000 | **进行中** (1/6 完成, 1 SKIP) |
| T3 | 网络策略提取 | 4 | -1,500 | **完成** (3/4 完成, 1 SKIP) |
| T4 | 安全/调试策略提取 | 4 | -2,000 | **进行中** (1/4 完成, 3 SKIP) |
| T5 | syscall 策略提取 | 3 | -1,500 | **完成** (2/3 完成, 1 SKIP) |
| T6 | IPC/类型/配置策略提取 | 8 | -2,800 | **进行中** (6/8 完成, 1 SKIP) |
| **合计** | | **33** | **-15,300** | **19 完成, 6 SKIP, 8 待做** |

### 当前 Self TCB: 50.0% (2026-06-16)

| 阶段 | framework (eff.) | services (eff.) | Self TCB | 实际 |
|------|-----------------|-----------------|----------|------|
| 基线 | 124,934 | 19,932 | 54.1% | 54.1% |
| T1 完成后 | ~120,434 | ~24,432 | ~49.5% | 52.5% (部分) |
| T2 完成后 | ~117,434 | ~27,432 | ~46.5% | — |
| T3 完成后 | ~115,934 | ~28,932 | ~45.5% | — |
| T4 完成后 | ~113,934 | ~30,932 | ~43.5% | — |
| T5 完成后 | ~112,334 | ~32,532 | ~42.0% | — |
| T6 完成后 | ~111,534 | ~33,332 | ~41.5% | — |
| **当前** | — | — | — | **50.0%** |

> **注**: 以上为保守估算. 实际 TCB 下降取决于策略/机制拆分比例.
> 达到 30% 目标需要约 35,000 effective LoC 迁移, 当前 6 个 Phase 覆盖 ~13,300 LoC.
> 后续需追加: driver 策略 (NVMe/AHCI 命令队列策略)、sync 策略 (lockdep 图算法)、
> 以及更激进的 proc/api.rs 拆分.

---

## 五、执行约定

### 5.1 每项提取的标准流程

1. **分析**: 读取源文件, 标记机制行 (unsafe/硬件操作) 与策略行 (纯逻辑/数据结构)
2. **创建分支**: `git checkout -b tcb/TX-Y-描述`
3. **实现提取**: 策略代码移到 services, framework 转为 re-export
4. **验证**:
   ```bash
   make build ARCH=x86_64 && make build ARCH=aarch64
   ./scripts/audit_services_boundary.py
   ./scripts/audit_safety_coverage.py
   ./scripts/audit_deadlock_matrix.py
   python3 scripts/audit_tcb_ratio.py
   make test-host
   ```
5. **更新文档**: 在本文档对应项将 `[ ]` 改为 `[x]`, 补全完成记录
6. **提交**: 格式 `tcb(TX-Y): 简述`

### 5.2 完成记录格式

每项完成后追加:

```
**完成记录**:
- 日期: YYYY-MM-DD
- 分支: tcb/TX-Y-描述
- 改动: framework -X LoC, services +Y LoC
- TCB: XX.X% → YY.Y%
- 验证: 双架构 0w0e, 三审计通过, host-tests N/N
```

### 5.3 优先级建议

1. **T1-8 rlimit** — 最简单, 验证提取流程
2. **T1-3 namespace** — 已有代理, 升级为策略主体
3. **T1-6 session** — 同上
4. **T5-2 linuxulator** — 纯数据, 无 unsafe
5. **T3-3 route** — 最简单的网络提取
6. 其余按依赖关系和难度递进

---

## 变更历史

- 2026-06-16: 初始版本, 6 Phase / 26 项任务, 预估 -13,300 LoC
- 2026-06-16: T1-1/3/4/5/6/8 完成, T1-7 SKIP; T2-6 完成, T2-5 SKIP; T3-2/3/4 完成, T3-1 SKIP; T4-4 完成, T4-1/2/3 SKIP; T5-2/4 完成, T5-3 SKIP; T6-2/3/5/6/7/8 完成, T6-4 SKIP. Self TCB: 54.1% → 50.0%
