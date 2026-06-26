# AntX 内核工程规划书

> 基于 2026-06-07 全量技术评估与业界 OS 设计模型精细对比, 制定从"可启动内核"到"可运行真实工作负载"的四阶段演进路线. 持续更新至 2026-06-26, Phase A/B/C/D 全部完成 (除特定项 D1-D11 见正文).

## 背景
- **背景条目**
  - 描述: AntX 在 Framekernel/TCB 最小化/HvFS/Barrier Stack 已达较高水准, 与"可运行真实用户态应用的操作系统"间存在系统性的功能缺口
  - 方案: 核心瓶颈集中在用户态运行时支撑层: 缺少 execve/futex/epoll/page cache/swap 等关键机制; 按优先级 P0/P1/P2, 按依赖关系编排为 4 个阶段
  - 状态: [X]

## 目标
- **目标条目**
  - 描述: 4 阶段演进路线
  - 方案: Phase A 内核可启动并运行首个用户态 init 进程; Phase B 可运行依赖共享库的真实用户态程序 (如 busybox); Phase C 可构建高性能网络服务与多进程应用; Phase D 达到生产可用级别, 支持容器化与可观测性
  - 状态: [X]

## 现状 (截至 2026-06-07)
- **已完成条目**
  - 描述: 14 类已完成核心子系统
  - 方案: Framekernel 双子树架构 (framework+services) / 内存管理 (Buddy+Slab+Kmalloc+COW+VMA+Demand Paging+OOMD) / 进程调度 (CFS+FIFO/RR/Deadline+Per-CPU+PWID) / 中断 (IDT+ISR+IRQ+APIC/IOAPIC) / 文件系统 (VFS+HvFS+ramfs/devfs/procfs) / 设备驱动 (Chitin+E1000/VirtIO+NVMe/AHCI/XHCI) / 网络 (smoltcp+DHCP+Socket) / 同步 (SpinLock/Mutex/RwLock/SeqLock/RCU) / IPC (Pipe/SHM/MsgQ/Sem) / 安全 (Credo+KASLR+PageTableChecker W^X) / 故障恢复 (Barrier Stack) / 配置 (统一配置中心+启动镜像编码) / WASM (解释器原型) / 构建 (x86_64+aarch64)
  - 状态: [X]
- **未完成条目 (P0 级关键缺失)**
  - 描述: 14 类 P0 关键缺失
  - 方案: Swap/页面回收 (sysinfo 硬编码为 0) / Page Cache (VMA 有 FileBacked 但无实现) / 文件 mmap (仅 MAP_ANONYMOUS) / execve (无 sys_execve) / Futex (完全缺失) / epoll (完全缺失) / initramfs + PID 1 (完全缺失) / hrtimer (仅 PIT tick) / 用户态 ASLR (无) / MSI/MSI-X (仅注释) / ACPI 完整解析 (仅 MADT) / POSIX 信号投递 (类型有, 投递无) / CPU 亲和性 (无) / io_uring/AIO (无)
  - 状态: []

## 进度跟踪 (2026-06-08 ~ 2026-06-26)
- **Phase B 状态: 4/4 完成 (2026-06-08)**
  - 描述: Phase B 4 子项全部完成
  - 方案: B1 Futex (framework/syscall/futex.rs 428 行 + services/sync/futex.rs 148 行, 64 桶哈希表, FUTEX_WAIT/WAKE/REQUEUE/BITSET 全套) / B2 Page Cache + 文件 mmap (framework/mm/pcache.rs 454 行, demand paging 真语义) / B3 Swap (framework/mm/swap.rs, kswapd softirq, LRU 跟踪 pml4) / B4 MSI/MSI-X + ACPI (framework/pci/msi.rs 462 行 + arch/x86_64/acpi.rs 840 行 FADT/HPET/DMAR)
  - 状态: [X]

- **Phase C 状态: 7/7 完成 (2026-06-10)**
  - 描述: Phase C 7 子项全部完成
  - 方案: C1 epoll (framework/syscall/epoll.rs 370 行, VFS 集成 + WaitQueue 真阻塞 + epoll_pwake 唤醒) / C2 CPU 亲和性 (framework/proc/process.rs cpuset_allowed + 203/204 syscalls) / C3 UDS (services/net/unix.rs, AF_UNIX 协议族, 详见 uds-design.md) / C4 io_uring/AIO (framework/io/iouring.rs Sqe/Cqe/RingBuffer) / C5 路由表+Netfilter (5 钩子点+NfRule) / C6 Lockdep+ftrace (2026-06-09) / C7 KPTI+Seccomp (KPTI 双架构全功能, Seccomp 严格+过滤模式)
  - 状态: [X]

- **Phase D 状态: 11/11 完成 (2026-06-10)**
  - 描述: Phase D 11 子项全部完成
  - 方案: D1 Namespace (7 种 ns + clone_from/unshare/setns) / D2 cgroup (CPU/内存/PID/IO 四控制器) / D3 NUMA (NumaNode/NumaTopology + 距离矩阵) / D4 eBPF (BpfInsn/BpfMap/验证器/解释器) / D5 电源管理 (C0-C3+DVFS+S3/S5) / D6 Secure Boot+TPM (PK/KEK/DB+PCR+软件模拟) / D7 Shadow Stack CET (双架构硬件检测) / D8 Tickless NO_HZ / D9 NTP/PTP / D10 kexec / D11 UEFI 启动
  - 状态: [X]

- **P1 跨阶段待办: 全部完成 (2026-06-08 ~ 2026-06-10)**
  - 描述: P1 跨阶段 17 类待办全部完成
  - 方案: UDS / Lockdep / PI Mutex / eventfd/signalfd/timerfd / dcache/icache / 文件锁 / inotify / sendfile/splice / rlimit / 进程组会话控制终端 / Core Dump / 设备固件 / KGDB/ftrace / POSIX Timer / madvise/mlock / 用户态 Stack Canary / KPTI 实际页表隔离
  - 状态: [X]

- **已修复预存问题: AB-BA 死锁**
  - 描述: make test-host stress_test 永久卡住预存问题
  - 方案: 根因: host-tests/src/hvfs/dedup.rs:65 CasIndex::ref_dec AB-BA 死锁 (insert/ref_inc 锁序=index→refs, ref_dec 锁序=refs→index); 修复: 统一锁顺序 ref_dec 改为 index→refs, 注释标注"避免 AB-BA 死锁"
  - 状态: [X]

## 工程计划 A: Phase A — 可启动用户态

### 背景
- **背景条目**
  - 描述: 内核启动后能加载并运行首个用户态 init 进程
  - 方案: 依赖链: hrtimer → 信号投递 → execve + 用户态 ASLR → initramfs + PID 1
  - 状态: [X]

### 现状 (2026-06-08)
- **现状条目**
  - 描述: Phase A 4 子项全部完成
  - 方案: A1 hrtimer / A2 POSIX 信号投递 (send/deliver/sigreturn) / A3 execve + 用户态 ASLR / A4 initramfs + PID 1
  - 状态: [X]

### 方案
- **A1 hrtimer 高精度定时器框架**
  - 描述: 替代 PIT 作为调度 tick 源
  - 方案: 基于 LAPIC Timer (x86_64) / ARM Generic Timer (aarch64); HrTimer 结构 (到期时间+回调+链表节点); per-CPU 红黑树定时器队列; hrtimer_sleep 替代 tick-based timer_sleep; 接入调度器时间片到期
  - 状态: [X]
- **A2 POSIX 信号投递**
  - 描述: framework 层实现 signal_send/检查 pending/sigreturn
  - 方案: framework: signal_send(pid,sig) 写入 pending 位图 + 唤醒; syscall_exit/interrupt_return 检查 pending; sigreturn 恢复寄存器; services: sys_kill/sigaction/sigprocmask/sigreturn safe 代理; 信号栈用 VmaType::Stack
  - 状态: [X]
- **A3 execve + 用户态 ASLR**
  - 描述: 接入已有 ELF 加载器
  - 方案: 流程: 释放旧地址空间 → 创建新 MmStruct → load_elf → 设置用户栈 → entry point; 用户态 ASLR: 栈基址+mmap 基址+堆基址各加 16 位熵随机偏移 (4KB 对齐); 随机源 TSC/LAPIC; PIE (ET_DYN) 支持
  - 状态: [X]
- **A4 initramfs + PID 1**
  - 描述: cpio 解析器 + 内核启动末尾 initramfs 挂载
  - 方案: cpio newc 格式解析 (目录/常规文件/符号链接); Multiboot2 module 解压到 ramfs 挂载点; 挂载根文件系统 /; 创建 PID 1 执行 /init
  - 状态: [X]

## 工程计划 B: Phase B — 可运行真实程序

### 背景
- **背景条目**
  - 描述: 可运行依赖共享库的用户态程序 (如 busybox)
  - 方案: 依赖链: Futex → Page Cache + 文件 mmap → Swap → MSI/MSI-X + ACPI
  - 状态: [X]

### 现状 (2026-06-08)
- **现状条目**
  - 描述: 4 子项全部完成
  - 方案: 见进度跟踪 §Phase B 状态
  - 状态: [X]

### 方案
- **B1 Futex**
  - 描述: framework 层实现 futex_wait/futex_wake + services safe 代理
  - 方案: 全局哈希表 HashMap<u64, FutexBucket> (按 addr 物理页对齐分桶); 每桶 SpinLock 保护 Vec<ThreadId>; futex_wait 验证 *addr==expected → 入队 → 调度让出; futex_wake 取 count 线程入就绪; 与 hrtimer 集成超时唤醒
  - 状态: [X]
- **B2 Page Cache + 文件 mmap**
  - 描述: 全局 xarray PageCache + HvFS 集成
  - 方案: PageCache: 全局 xarray 按 (inode, page_index) 索引物理页; address_space 抽象: readpage/writepage/write_begin/write_end; VFS read/write 走 PageCache; HvFS DMU 读经 PageCache; ARC (HvFS 私有) + PageCache (VFS 通用) 协同; mmap 支持 MAP_SHARED (写回) / MAP_PRIVATE (COW); 脏页周期回写
  - 状态: [X]
- **B3 Swap / 页面回收**
  - 描述: 块设备 swap 区 + kswapd 内核线程
  - 方案: 块设备 swap header + 按 slot 索引页槽位图; kswapd 周期扫描 inactive 链表, 回收干净页/换出脏页; 双链表 (active/inactive) + LRU 近似; 换出: 选中页 → 写 swap slot → 解除映射 → 释放; 换入: #PF 检测 swap entry → 从 slot 读 → 重映射; OOMD 联动加速回收
  - 状态: [X]
- **B4 MSI/MSI-X + ACPI 完整解析**
  - 描述: PCI MSI Capability + ACPI FADT/HPET/DMAR
  - 方案: MSI: PCI 配置空间 MSI Capability 解析+分配向量+启用; MSI-X: Capability + MMIO Table/PBA + 多向量; framework msi_alloc/free_vector API; 与 IrqLine 统一注册到 IDT; ACPI FADT (关机寄存器) + HPET (高精度定时器备用源) + DMAR (IOMMU); acpi_shutdown() 写 S5 命令到 PM1a_CNT
  - 状态: [X]

## 工程计划 C: Phase C — 生产可用

### 背景
- **背景条目**
  - 描述: 可构建高性能网络服务与多进程应用, 具备基本安全与调试能力
  - 方案: 7 子项: epoll / CPU 亲和性 / UDS / io_uring/AIO / 路由表+Netfilter / Lockdep+ftrace / KPTI+Seccomp
  - 状态: [X]

### 现状 (2026-06-10)
- **现状条目**
  - 描述: 7 子项全部完成
  - 方案: 见进度跟踪 §Phase C 状态
  - 状态: [X]

### 方案
- **C1 epoll**
  - 描述: EventPoll + VFS 集成
  - 方案: EventPoll: 红黑树 (按 fd 索引) + 就绪链表; sys_epoll_create/ctl/wait; VFS poll 回调; 中断就绪唤醒
  - 状态: [X]
- **C2 CPU 亲和性**
  - 描述: Process cpuset_allowed + 调度器集成
  - 方案: Process 新增 cpuset_allowed: AtomicU64 (64 CPU 位图); sys_sched_setaffinity/getaffinity (Linux 兼容号 203/204); select_cpu() 优先 allowed 集合; 跨 CPU 迁移检查约束
  - 状态: [X]
- **C3 UDS (2026-06-08)**
  - 描述: AF_UNIX 协议族 (SOCK_STREAM + SOCK_DGRAM)
  - 方案: services/net/unix.rs + framework/net/unix.rs; 修订: 不入 VFS inode 走独立路径表 (DECISION-006); FD 空间 [100, 116) 与 smoltcp/VFS 不冲突; 5 个 no_std 单元测试
  - 状态: [X]
  - 详情: 详见 [uds-design.md](uds-design.md)
- **C4 io_uring / AIO**
  - 描述: 先 AIO 后 io_uring 两步走
  - 方案: AIO: io_submit/io_getevents/内核异步 I/O 线程池; io_uring: 共享环形缓冲区+内核侧直接提交
  - 状态: [X]
- **C5 路由表 + Netfilter**
  - 描述: FIB 最长前缀匹配 + 5 钩子点
  - 方案: 路由表: FIB + 最长前缀匹配; Netfilter: 5 钩子点 (PREROUTING/INPUT/FORWARD/OUTPUT/POSTROUTING) + 规则链; NAT: 基础 SNAT/DNAT; 连接跟踪: TCP/UDP 流状态表
  - 状态: [X]
- **C6 Lockdep + ftrace (2026-06-09)**
  - 描述: 死锁检测 + 函数追踪
  - 方案: Lockdep: 锁获取记录 (lock_class, irq_context) → 构建依赖图 → 检测环路; ftrace: 编译期 mcount 插入 → 运行时动态启用 → 函数调用图; 输出 procfs/串口
  - 状态: [X]
- **C7 KPTI + Seccomp (2026-06-10)**
  - 描述: 双页表隔离 + BPF 系统调用过滤
  - 方案: KPTI: 用户态/内核态各一套页表, 切换时刷新 CR3 (boot_image kpti 标志位已预留); Seccomp: sys_seccomp 安装 BPF 过滤器 → 系统调用入口检查
  - 状态: [X]

## 工程计划 D: Phase D — 企业级

### 背景
- **背景条目**
  - 描述: 支持容器化、可观测性、高级安全特性
  - 方案: 11 子项: NUMA/cgroup/Namespace/eBPF/电源管理/Secure Boot+CET/Tickless/NTP-PTP/kexec/UEFI
  - 状态: [X]

### 现状 (2026-06-10)
- **现状条目**
  - 描述: 11 子项全部完成
  - 方案: 见进度跟踪 §Phase D 状态
  - 状态: [X]

### 方案
- **D1 Namespace**
  - 描述: 7 种命名空间完整隔离
  - 方案: framework/proc/namespace.rs (PID/Net/Mount/User/IPC/UTS 7 种 ns + NamespaceSet + clone_from/unshare/setns); Process 集成 + fork 继承 + CLONE_NEW*; sys_unshare/sys_setns + linuxulator
  - 状态: [X]
- **D2 cgroup**
  - 描述: CPU/内存/PID/IO 四控制器
  - 方案: framework/proc/cgroup.rs (4 控制器 + CgroupRq + CgroupSubsystem); Process 集成 + fork 继承 + exit 清理; 5 个 syscall
  - 状态: [X]
- **D3 NUMA**
  - 描述: 内存节点 + 调度亲和
  - 方案: framework/mm/numa.rs (NumaNode/NumaTopology/NumaMempolicy + 距离矩阵 + UMA 回退); Process numa_policy + fork 继承; 4 个 syscall + linuxulator
  - 状态: [X]
- **D4 eBPF**
  - 描述: 可编程网络/安全/观测
  - 方案: framework/debug/ebpf.rs (BpfInsn/BpfMap(Hash+Array)/BpfProg/BpfVerifier/BpfInterpreter/BpfHelper + BpfSubsystem); 验证器 (有界循环+寄存器类型+指针检查); 解释器 (ALU64/LD/ST/JMP 全指令集); 6 个 Helper; sys_bpf 多路复用
  - 状态: [X]
- **D5 电源管理**
  - 描述: S3/S4 + C-state + DVFS
  - 方案: framework/driver/power.rs (CpuIdle C0-C3+per-CPU 统计 + CpuFreq DVFS+performance/powersave/ondemand governor + Suspend/Resume S3/S5+通知器链); sys_pm 多路复用
  - 状态: [X]
- **D6 Secure Boot + TPM**
  - 描述: 启动链验证 + 硬件信任根
  - 方案: framework/credo/secure_boot.rs (SecureBoot PK/KEK/DB 信任链 + Ed25519 验证 + TPM2.0 8 个 PCR + Extend/Seal/Unseal/Quote + 软件模拟 + SHA-256); sys_secure_boot + sys_tpm
  - 状态: [X]
- **D7 Shadow Stack (CET)**
  - 描述: 硬件级控制流完整性
  - 方案: framework/arch/shadow_stack.rs (CET 检测 + Shadow Stack 分配/管理 + CR4.CET/MSR 配置 x86_64 + PAC/BTI aarch64); sys_cet
  - 状态: [X]
- **D8 Tickless (NO_HZ)**
  - 描述: 空闲 CPU 停止定时中断
  - 方案: 空闲 CPU 停止定时中断
  - 状态: [X]
- **D9 NTP/PTP**
  - 描述: 系统时钟同步
  - 方案: NTP/PTP 协议实现
  - 状态: [X]
- **D10 kexec**
  - 描述: 从内核直接引导新内核
  - 方案: kexec 系统调用
  - 状态: [X]
- **D11 UEFI 启动支持**
  - 描述: UEFI 启动协议
  - 方案: UEFI 启动加载
  - 状态: [X]

## 决策记录
- **DECISION-001**
  - 描述: Phase A 优先于 Phase B
  - 方案: 因为 execve + initramfs 是运行任何用户态程序的前提, 其他功能无意义
  - 状态: [X] (2026-06-07)
- **DECISION-002**
  - 描述: hrtimer 作为 Phase A 首项
  - 方案: 因为信号超时/Futex 超时/调度精度都依赖它
  - 状态: [X] (2026-06-07)
- **DECISION-003**
  - 描述: Page Cache 与 HvFS ARC 的关系
  - 方案: ARC 是 HvFS 私有缓存 (DMU 级), Page Cache 是 VFS 通用缓存 (inode 级), 两者共存不冲突
  - 状态: [X] (2026-06-07)
- **DECISION-004**
  - 描述: Swap 采用块设备 swap 分区方案
  - 方案: 简化实现; 后续可扩展 swap file
  - 状态: [X] (2026-06-07)
- **DECISION-005**
  - 描述: io_uring 分两步走
  - 方案: 先 AIO (验证异步 I/O 路径), 后 io_uring (零拷贝优化)
  - 状态: [X] (2026-06-07)

## 变更历史
- **2026-06-26**
  - 描述: 按新文档规则重写 (标题+条目(描述+方案+状态)+详情)
  - 方案: 结构重组, 保留原意
  - 状态: [X]
- **2026-06-09**
  - 描述: 创建 engineering-progress.md 工程进度跟踪文档; Phase A/B 待办标记为已完成
  - 方案: -
  - 状态: [X]
- **2026-06-07**
  - 描述: 初始版本
  - 方案: -
  - 状态: [X]
