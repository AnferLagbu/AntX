# AntX 内核工程规划书

> 基于 2026-06-07 全量技术评估与业界 OS 设计模型精细对比, 制定从"可启动内核"到"可运行真实工作负载"的四阶段演进路线.

## 背景

AntX 内核在架构设计 (Framekernel)、安全抽象 (TCB 最小化)、文件系统深度 (HvFS) 和故障恢复 (Barrier Stack) 方面已达到较高水准, 但与"可运行真实用户态应用的操作系统"之间, 存在系统性的功能缺口. 核心瓶颈集中在**用户态运行时支撑层**: 缺少 execve/futex/epoll/page cache/swap 等关键机制, 导致内核当前无法启动用户态 init 进程、无法运行共享库程序、无法构建高性能 I/O 服务.

本规划将缺失功能按优先级分为 P0 (阻塞真实应用运行)、P1 (影响开发效率与系统完整性)、P2 (长期增强), 并按依赖关系编排为四个阶段.

## 目标

- Phase A: 内核可启动并运行首个用户态 init 进程
- Phase B: 可运行依赖共享库的真实用户态程序 (如 busybox)
- Phase C: 可构建高性能网络服务与多进程应用
- Phase D: 达到生产可用级别, 支持容器化与可观测性

## 现状

截至 2026-06-07, 已完成:

- Framekernel 双子树架构: framework (TCB) + services (safe Rust), CI 三层强制防线
- 内存管理: Buddy PMM + Slab + Kmalloc + COW + VMA + Demand Paging + OOMD + 内存压力检测
- 进程调度: CFS + FIFO/RR/Deadline + Per-CPU 队列 + PWID 配额
- 中断: IDT + ISR + IRQ + APIC/IOAPIC
- 文件系统: VFS + HvFS (SPA/DMU/ZAP/TXG/ZIL/ARC/RAID-Z) + ramfs/devfs/procfs
- 设备驱动: Chitin 框架 + E1000/VirtIO safe 迁移 + NVMe/AHCI/XHCI (framework 层)
- 网络: smoltcp 协议栈 + DHCP + Socket API 基础
- 同步: SpinLock/Mutex/RwLock/SeqLock/RCU + services 层 RAII 代理
- IPC: Pipe/SHM/MsgQ/Sem (1/4 完成 services 层迁移)
- 安全: Credo 能力系统 + KASLR + PageTableChecker (W^X)
- 故障恢复: Barrier Stack (UndoLog/Snapshot/BSR/BHR)
- 配置: 统一配置中心 + 启动镜像编码
- WASM: 解释器原型
- 构建: x86_64 主架构 + aarch64 进行中

未完成 (P0 级关键缺失):

- Swap / 页面回收: `sysinfo` 中 totalswap/freeswap 硬编码为 0
- Page Cache: VMA 有 FileBacked 类型但无实际页缓存实现
- 文件 mmap (MAP_SHARED/MAP_PRIVATE): sys_mmap 仅处理 MAP_ANONYMOUS
- execve: ELF 加载器存在但无 sys_execve 实现
- Futex: 完全缺失, 用户态互斥锁无法高效实现
- epoll: 完全缺失, 无法构建高性能网络服务
- initramfs + PID 1: 完全缺失, 无用户态入口
- hrtimer: 仅有 PIT tick 级定时, 无高精度定时器框架
- 用户态 ASLR: KASLR 完整但用户态地址空间无随机化
- MSI/MSI-X: 仅在注释中提及, 高速设备无法正常工作
- ACPI 完整解析: 仅解析 MADT, 无 FADT/HPET/DMAR
- POSIX 信号投递: services/proc/signal.rs 有类型定义但无投递机制
- CPU 亲和性: 完全缺失
- io_uring / AIO: 完全缺失

## 进度跟踪 (2026-06-08 更新)

> 本节记录 roadmap 中各子项的实际完成状态, 与 §方案 配合使用.

### Phase B 状态: 4/4 完成 ✅

| 子项 | 状态 | 关键产出 | 验证 |
|---|---|---|---|
| B1 Futex | ✅ 完成 | framework/syscall/futex.rs (428 行) + services/sync/futex.rs (148 行) | 64 桶哈希表, FUTEX_WAIT/WAKE/REQUEUE/BITSET 全套 |
| B2 Page Cache + 文件 mmap | ✅ 完成 | framework/mm/pcache.rs (454 行) + syscall/mmap.rs MAP_SHARED/PRIVATE/ANONYMOUS | demand paging 真语义, 走 pcache_lookup+get+fill |
| B3 Swap | ✅ 完成 | framework/mm/swap.rs (SwapEntry, swap_out_to_pte, kswapd softirq) + lib.rs swap_init/kswapd_init | 0 dead_code, LRU 跟踪 pml4, 周期唤醒 KSWAPD_TICK_INTERVAL=100 |
| B4 MSI/MSI-X + ACPI | ✅ 完成 | framework/pci/msi.rs (462 行) + arch/x86_64/acpi.rs (840 行) FADT/HPET/DMAR 全套 | msi_alloc_vector/enable, msix_enable/mask/unmask, acpi_shutdown |

### Phase C 状态: 2/7 完成 (C1-C2 完成, C3-C7 待实施)

| 子项 | 状态 | 关键产出 | 验证 |
|---|---|---|---|
| C1 epoll | ✅ 完成 | framework/syscall/epoll.rs (370 行) + VFS 集成 | 完整集成 VFS poll + WaitQueue 真阻塞 + epoll_pwake 唤醒 (3 TODO 全清) |
| C2 CPU 亲和性 | ✅ 完成 | framework/proc/process.rs cpuset_allowed + scheduler is_cpu_allowed/select_cpu_for + syscall mod.rs sys_sched_setaffinity/getaffinity (Linux 兼容号 203/204) + services/proc/sched.rs (新文件) | 双架构 0 error 0 warning, 3 审计 0 issue, host test 172/172 |
| C3 Unix Domain Socket | ⬜ 待实施 | — | Domain::Unix 未实现; 无 sockaddr_un; 无 AF_UNIX 协议族 |
| C4 io_uring / AIO | ⬜ 待实施 | — | 整个项目无 io_uring/io_submit/io_getevents 痕迹 |
| C5 路由表 + Netfilter | ⬜ 待实施 | — | 无 routing_table/FIB/NF_INET/NAT/conntrack |
| C6 Lockdep + ftrace | ⬜ 待实施 | — | 无 lockdep (lock_class/irq_context) / ftrace (mcount) |
| C7 KPTI + Seccomp | ⬜ 待实施 | — | config/caps.rs 仅有 kpti 标志; 无双页表切换; 无 sys_seccomp |

### 已修复预存问题

| 问题 | 位置 | 根因 | 修复 |
|---|---|---|---|
| `make test-host` stress_test 永久卡住 | host-tests/src/hvfs/dedup.rs:65 `CasIndex::ref_dec` | **AB-BA 死锁**: `insert`/`ref_inc` 锁序 = `index → refs`, `ref_dec` 锁序 = `refs → index` | 统一锁顺序: `ref_dec` 改为 `index → refs`, 注释标注"避免 AB-BA 死锁" |

## 方案

### Phase A — 可启动用户态

目标: 内核启动后能加载并运行首个用户态 init 进程.

依赖链: hrtimer → 信号投递 → execve + 用户态 ASLR → initramfs + PID 1

#### A1. hrtimer 高精度定时器框架

- 基于 LAPIC Timer (x86_64) / ARM Generic Timer (aarch64) 替代 PIT 作为调度 tick 源
- 实现 `HrTimer` 结构: 到期时间 + 回调 + 链表节点
- 实现 per-CPU 红黑树定时器队列 (到期时间排序)
- 实现 `hrtimer_sleep` 替代当前 tick-based `timer_sleep`
- 接入调度器: 时间片到期由 hrtimer 回调触发 `need_reschedule`
- framework 层实现 (允许 unsafe), services 层暴露 `timer_create`/`timer_delete` safe API

#### A2. POSIX 信号投递

- framework 层: 实现 `signal_send(pid, sig)` — 查找目标进程, 写入 pending 位图, 唤醒目标
- framework 层: 在 `syscall_exit` / `interrupt_return` 路径检查 pending 信号
- framework 层: 实现 `sigreturn` — 恢复信号处理前的寄存器状态
- services 层: `sys_kill` / `sys_sigaction` / `sys_sigprocmask` / `sys_sigreturn` safe 代理
- 信号栈: 在进程 VMA 中分配 `VmaType::Stack` 作为 altstack

#### A3. execve + 用户态 ASLR

- 接入已有 ELF 加载器: `sys_execve(path, argv, envp)`
- 流程: 释放旧地址空间 → 创建新 MmStruct → load_elf → 设置用户栈 → 设置 entry point
- 用户态 ASLR: execve 时对栈基址、mmap 基址、堆基址各加随机偏移 (16 位熵, 对齐 4KB)
- 随机源: 读取 TSC 或 LAPIC 计数器作为种子
- PIE (ET_DYN) 支持: ELF 加载器识别 ET_DYN 类型, 在随机基址加载

#### A4. initramfs + PID 1

- 实现 cpio 格式解析器 (newc 格式, 仅目录/常规文件/符号链接)
- 内核启动末尾: 将 Multiboot2 module (initramfs cpio 归档) 解压到 ramfs 挂载点
- 挂载为根文件系统 `/`
- 创建 PID 1 进程, 执行 `/init` (来自 initramfs)
- init 进程负责后续用户态初始化 (挂载 devfs/procfs, 启动 shell 等)

### Phase B — 可运行真实程序

目标: 可运行依赖共享库的用户态程序 (如 busybox), 支持文件 I/O 与内存换出.

依赖链: Futex → Page Cache + 文件 mmap → Swap → MSI/MSI-X + ACPI

#### B1. Futex

- framework 层: 实现 `futex_wait(addr, expected, timeout)` / `futex_wake(addr, count)`
- 核心数据结构: 全局哈希表 `HashMap<u64, FutexBucket>` (按 addr 物理页对齐分桶)
- 每桶: SpinLock 保护的等待队列 (Vec<ThreadId>)
- `futex_wait`: 验证 *addr == expected → 加入等待队列 → 调度让出
- `futex_wake`: 从等待队列取出 count 个线程 → 加入就绪队列
- services 层: `sys_futex` safe 代理
- 与 hrtimer 集成: 支持超时唤醒

#### B2. Page Cache + 文件 mmap

- 实现 `PageCache` 结构: 全局 xarray (稀疏数组) 按 (inode, page_index) 索引物理页
- 实现 `address_space` 抽象: `readpage`/`writepage`/`write_begin`/`write_end` 回调
- VFS 层: `vfs_read`/`vfs_write` 经过 Page Cache
- HvFS 集成: DMU 读取经过 Page Cache, ARC 与 Page Cache 协同 (ARC 作为 HvFS 私有缓存, Page Cache 作为 VFS 通用缓存)
- 文件 mmap: `sys_mmap` 支持 MAP_SHARED (写回文件) / MAP_PRIVATE (COW)
- 脏页回写: 周期性内核线程扫描脏页, 调用 `writepage` 回写

#### B3. Swap / 页面回收

- 实现 swap 区抽象: 块设备上的 swap header + 按 slot 索引的页槽位图
- 实现 `kswapd` 内核线程: 周期性扫描 inactive 链表, 回收干净页 / 换出脏页
- 页面替换策略: 双链表 (active/inactive) + LRU 近似 (类似 Linux)
- 换出流程: 选中页 → 写入 swap slot → 解除映射 → 释放物理页
- 换入流程: #PF 检测 swap entry → 从 swap slot 读取 → 重新映射
- 与 OOMD 联动: 内存压力升级时 kswapd 加速回收

#### B4. MSI/MSI-X + ACPI 完整解析

- MSI: PCI 配置空间 MSI Capability 解析 + 分配向量 + 启用
- MSI-X: MSI-X Capability + MMIO Table/PBA + 多向量分配
- framework 层: `msi_alloc_vector()` / `msi_free_vector()` API
- 与 IrqLine 统一: MSI 向量注册到 IDT, 复用 IRQ 分发框架
- ACPI: 解析 FADT (关机寄存器) + HPET (高精度定时器备用源) + DMAR (IOMMU)
- 实现 `acpi_shutdown()`: 写 S5 命令到 PM1a_CNT

### Phase C — 生产可用

目标: 可构建高性能网络服务与多进程应用, 具备基本安全与调试能力.

#### C1. epoll

- 实现 `EventPoll` 结构: 红黑树 (按 fd 索引) + 就绪链表
- `sys_epoll_create`: 分配 eventpoll 实例
- `sys_epoll_ctl`: 注册/修改/删除监控项
- `sys_epoll_wait`: 遍历就绪链表, 无就绪则阻塞等待
- 与 VFS 集成: 文件/Socket/pipe 的 poll 操作回调
- 与中断集成: 数据就绪时唤醒等待的 epoll 实例

#### C2. CPU 亲和性

- `Process` 结构新增 `cpuset_allowed: AtomicU64` (64 CPU 位图)
- `sys_sched_setaffinity` / `sys_sched_getaffinity`
- 调度器: `select_cpu()` 优先选择 allowed 集合内的 CPU
- 负载均衡: 跨 CPU 迁移时检查亲和性约束

#### C3. Unix Domain Socket ✓ (2026-06-08)

- 实现 `AF_UNIX` 协议族: 流式 (SOCK_STREAM) + 数据报 (SOCK_DGRAM)
- 地址格式: `sockaddr_un` (路径名)
- 数据传输: 内核缓冲区直拷贝 (同地址空间无 IPC 开销)
- ~~与 VFS 集成: bind 创建文件系统入口, connect 查找~~ → **修订**: UDS 不入 VFS inode, 走独立路径表 (DECISION-006)
- FD 空间 `[100, 116)` 与 smoltcp / VFS 不冲突
- 5 个 no_std 单元测试, 详见 [uds-design.md](uds-design.md)

#### C4. io_uring / AIO

- 先实现 AIO: `io_submit` + `io_getevents` + 内核异步 I/O 线程池
- 后续升级 io_uring: 共享环形缓冲区 + 内核侧直接提交, 避免系统调用

#### C5. 路由表 + Netfilter

- 路由表: FIB (Forwarding Information Base) + 最长前缀匹配
- Netfilter: 5 个钩子点 (PREROUTING/INPUT/FORWARD/OUTPUT/POSTROUTING) + 规则链
- NAT: 基础 SNAT/DNAT (端口地址转换)
- 连接跟踪: TCP/UDP 流状态表

#### C6. Lockdep + ftrace

- Lockdep: 锁获取时记录 (lock_class, irq_context) → 构建依赖图 → 检测环路
- ftrace: 编译期插入 `mcount` 调用点 → 运行时动态启用/禁用 → 函数调用图追踪
- 输出: 通过 procfs 或串口

#### C7. KPTI 实现 + Seccomp

- KPTI: 用户态/内核态各一套页表, 切换时刷新 CR3 (boot_image kpti 标志位已预留)
- Seccomp: `sys_seccomp` 安装 BPF 过滤器 → 系统调用入口检查允许/拒绝/陷阱

### Phase D — 企业级

目标: 支持容器化、可观测性、高级安全特性.

- NUMA 感知: 内存节点 + 调度亲和
- cgroup: CPU/内存/IO/PID 控制器
- Namespace: PID/Net/Mount/User/IPC/UTS 完整隔离
- eBPF: 可编程网络/安全/观测
- 电源管理: S3 挂起/S4 休眠/C-state/DVFS
- Secure Boot + TPM: 启动链验证 + 硬件信任根
- Shadow Stack (CET): 硬件级控制流完整性
- Tickless (NO_HZ): 空闲 CPU 停止定时中断
- NTP/PTP: 系统时钟同步
- kexec: 从内核直接引导新内核
- UEFI 启动支持

## 待办

### Phase A — 已完成 (2026-06-08)
- [x] A1: hrtimer 高精度定时器框架
- [x] A2: POSIX 信号投递 (send/deliver/sigreturn)
- [x] A3: execve + 用户态 ASLR
- [x] A4: initramfs + PID 1

### Phase B — 已完成 (2026-06-08)
- [x] B1: Futex (wait/wake/requeue)
- [x] B2: Page Cache + 文件 mmap (MAP_SHARED/MAP_PRIVATE)
- [x] B3: Swap / 页面回收 (kswapd + LRU)
- [x] B4: MSI/MSI-X + ACPI 完整解析

### Phase C
- [x] C1: epoll
- [x] C2: CPU 亲和性
- [x] C3: Unix Domain Socket (2026-06-08, FD 100-115, 独立路径表, 详见 [uds-design.md](uds-design.md))
- [ ] C4: io_uring / AIO
- [ ] C5: 路由表 + Netfilter
- [ ] C6: Lockdep + ftrace
- [ ] C7: KPTI + Seccomp

### Phase D
- [ ] NUMA 感知
- [ ] cgroup 控制器
- [ ] Namespace 完整隔离
- [ ] eBPF
- [ ] 电源管理
- [ ] Secure Boot + TPM
- [ ] Shadow Stack (CET)
- [ ] Tickless (NO_HZ)
- [ ] NTP/PTP 时钟同步
- [ ] kexec
- [ ] UEFI 启动

## P1 级待办 (跨阶段, 按需穿插)

- [x] Unix Domain Socket (可提前到 Phase B) → 2026-06-08 完成 (Phase C.3)
- [ ] Lockdep 死锁检测
- [x] Priority Inheritance Mutex → 2026-06-08 完成 (P1 #3)
- [ ] eventfd / signalfd / timerfd
- [ ] dcache / icache
- [ ] 文件锁 (flock / POSIX locks)
- [ ] inotify 文件事件通知
- [ ] sendfile / splice 零拷贝
- [ ] Resource Limits (rlimit) 完整实现
- [ ] 进程组/会话/控制终端
- [ ] Core Dump 生成
- [ ] 设备固件加载
- [ ] KGDB / ftrace
- [ ] POSIX Timer
- [ ] madvise / mlock
- [ ] 用户态 Stack Canary
- [ ] KPTI 实际页表隔离

## 决策记录

- DECISION-001: Phase A 优先于 Phase B, 因为 execve + initramfs 是运行任何用户态程序的前提, 其他功能无意义
- DECISION-002: hrtimer 作为 Phase A 首项, 因为信号超时/Futex 超时/调度精度都依赖它
- DECISION-003: Page Cache 与 HvFS ARC 的关系: ARC 是 HvFS 私有缓存 (DMU 级), Page Cache 是 VFS 通用缓存 (inode 级), 两者共存不冲突
- DECISION-004: Swap 采用块设备 swap 分区方案 (非 swap file), 简化实现; 后续可扩展 swap file
- DECISION-005: io_uring 分两步走: 先 AIO (验证异步 I/O 路径), 后 io_uring (零拷贝优化)

## 变更历史

- 2026-06-09: 创建 [engineering-progress.md](engineering-progress.md) 工程进度跟踪文档; Phase A/B 待办标记为已完成
- 2026-06-07: 初始版本

## Backlog: 过期 TODO 跟踪

> 由 `tools/track_todo.py` 自动维护. 每条 `TRACK-XXX` 唯一对应一处未完成项.
> 修复后删除对应行, 并清掉源码中 `TODO(TRACK-XXX)` 标记.

- [TRACK-CCB422] `src/kernel/framework/dma_buf.rs:181` TODO
- [TRACK-D64319] `src/kernel/framework/dma_buf.rs:206` TODO
- [TRACK-315B7C] `src/kernel/services/proc/signal.rs:424` TODO
- [TRACK-5B3EBC] `src/kernel/services/mm/mmap.rs:46` TODO
- [TRACK-2CED20] `src/kernel/framework/idt/mod.rs:247` TODO
- [TRACK-F38D98] `src/kernel/framework/idt/idt.rs:557` TODO
- [TRACK-57C7C9] `src/kernel/framework/idt/idt.rs:740` TODO
- [TRACK-B2082D] `src/kernel/framework/idt/idt.rs:826` TODO
- [TRACK-8F40F4] `src/kernel/framework/idt/idt.rs:837` TODO
- [TRACK-D0E338] `src/kernel/framework/idt/handlers.rs:340` TODO
- [TRACK-2B4902] `src/kernel/framework/idt/handlers.rs:389` TODO
- [TRACK-A99EBB] `src/kernel/framework/dma/engine.rs:365` TODO
- [TRACK-82FEA0] `src/kernel/framework/mm/vmm_aarch64.rs:384` TODO
- [TRACK-A589E3] `src/kernel/framework/mm/vmm_aarch64.rs:386` TODO
- [TRACK-A7DE25] `src/kernel/framework/mm/pcache.rs:137` TODO
- [TRACK-8C5FFB] `src/kernel/framework/ipc/scheduler_integration.rs:89` TODO
- [TRACK-48CC21] `src/kernel/framework/ipc/signal.rs:63` TODO
- [TRACK-614BD5] `src/kernel/framework/ipc/signal.rs:82` TODO
- [TRACK-F806F4] `src/kernel/framework/ipc/signal.rs:99` TODO
- [TRACK-3A9016] `src/kernel/framework/ipc/signal.rs:108` TODO
- [TRACK-21BAF1] `src/kernel/framework/ipc/sem.rs:80` TODO
- [TRACK-077F14] `src/kernel/framework/syscall/mmap.rs:158` TODO
- [TRACK-90BFB0] `src/kernel/framework/syscall/types.rs:45` TODO
- [TRACK-8B3C91] `src/kernel/framework/syscall/types.rs:59` TODO
- [TRACK-6564B9] `src/kernel/framework/syscall/types.rs:62` TODO
- [TRACK-0FF0F0] `src/kernel/framework/syscall/types.rs:85` TODO
- [TRACK-B62489] `src/kernel/framework/syscall/types.rs:121` TODO
- [TRACK-CFB870] `src/kernel/framework/syscall/types.rs:124` TODO
- [TRACK-C3720B] `src/kernel/framework/syscall/types.rs:132` TODO
- [TRACK-1475D8] `src/kernel/framework/syscall/types.rs:145` TODO
- [TRACK-B29335] `src/kernel/framework/syscall/mod.rs:2569` TODO
- [TRACK-9CD1ED] `src/kernel/framework/syscall/epoll.rs:251` TODO
- [TRACK-2C209B] `src/kernel/framework/syscall/epoll.rs:277` TODO
- [TRACK-81D068] `src/kernel/framework/syscall/epoll.rs:306` TODO
- [TRACK-FA10A1] `src/kernel/framework/syscall/clone.rs:166` TODO
- [TRACK-4D8B74] `src/kernel/framework/timer/mod.rs:163` TODO
- [TRACK-CDB9E5] `src/kernel/framework/timer/sleep.rs:201` TODO
- [TRACK-2F4B39] `src/kernel/framework/proc/api.rs:976` TODO
- [TRACK-558BA7] `src/kernel/framework/driver/usb/mod.rs:36` TODO
- [TRACK-AE516E] `src/kernel/framework/driver/usb/mod.rs:37` TODO
- [TRACK-832FCE] `src/kernel/framework/driver/usb/mod.rs:38` TODO
- [TRACK-688EA7] `src/kernel/framework/driver/usb/xhci.rs:544` TODO
- [TRACK-2E0EB0] `src/kernel/framework/driver/usb/xhci.rs:553` TODO
- [TRACK-1F75C1] `src/kernel/framework/driver/usb/xhci.rs:558` TODO
- [TRACK-599EDA] `src/kernel/framework/driver/display/dp.rs:218` TODO
- [TRACK-B61830] `src/kernel/framework/driver/display/dp.rs:229` TODO
- [TRACK-9B691E] `src/kernel/framework/driver/display/dp.rs:252` TODO
- [TRACK-0350FE] `src/kernel/framework/driver/display/dp.rs:308` TODO
- [TRACK-3C1169] `src/kernel/framework/driver/display/dp.rs:319` TODO
- [TRACK-CD5DA5] `src/kernel/framework/driver/display/hdmi.rs:437` TODO
- [TRACK-7CCB60] `src/kernel/framework/driver/display/hdmi.rs:451` TODO
- [TRACK-1BDEF6] `src/kernel/framework/driver/display/hdmi.rs:498` TODO
- [TRACK-162CB0] `src/kernel/framework/driver/virtio/blk.rs:205` TODO
- [TRACK-26731B] `src/kernel/framework/arch/x86_64/smp_init.rs:189` TODO
