# QueenX 工程进度跟踪

> 本文档记录 QueenX 内核项目所有工程阶段的实际完成状态与关键产出. 配合 kernel-roadmap.md (规划) 与 CHANGELOG.md (变更日志) 使用. 2026-06-26 同步.

## 工程计划 A: 主线工程 Phase A-D

### 背景
- **背景条目**
  - 描述: Phase A-D 主线工程 4 阶段进度跟踪
  - 方案: 阶段 A 可启动用户态; 阶段 B 可运行真实程序; 阶段 C 生产可用; 阶段 D 企业级
  - 状态: [X]

### 现状 (2026-06-10)
- **现状条目**
  - 描述: Phase A/B/C 全部完成, Phase D 部分完成
  - 方案: Phase A 4/4 (2026-06-08) + Phase B 4/4 (2026-06-08) + Phase C 7/7 (2026-06-10) + Phase D 11/11 (2026-06-10)
  - 状态: [X]

### 方案
- **Phase A 可启动用户态 (已完成 2026-06-08)**
  - 描述: 4 子项全部完成
  - 方案: A1 hrtimer (LAPIC Timer / ARM Generic Timer 替代 PIT, per-CPU 红黑树定时器队列, hrtimer_sleep, 双架构 0 error, host test 通过) / A2 POSIX 信号投递 (signal_send, pending 位图, syscall_exit/interrupt_return 检查, sigreturn, pgid 支持, TRACK-315B7C 修复, 4 个新测试) / A3 execve + 用户态 ASLR (sys_execve, ELF 加载器集成, 16 位熵随机偏移, PIE 支持) / A4 initramfs + PID 1 (cpio newc 解析器, vfs_symlink, init_launch_status 状态机, services/init.rs, 6 个测试, 总 259)
  - 状态: [X]

- **Phase B 可运行真实程序 (已完成 2026-06-08)**
  - 描述: 4 子项全部完成
  - 方案: B1 Futex (framework/syscall/futex.rs 428 行 + services/sync/futex.rs 148 行, 64 桶哈希表, FUTEX_WAIT/WAKE/REQUEUE/BITSET) / B2 Page Cache + 文件 mmap (framework/mm/pcache.rs 454 行, MAP_SHARED/PRIVATE/ANONYMOUS, demand paging 真语义, VMA file_pwm 桥接, vfs_pread_inode + prewarm_file_pages, 7 个 pwm 测试) / B3 Swap / 页面回收 (framework/mm/swap.rs, SwapEntry, swap_out_to_pte, kswapd softirq, LRU 跟踪 pml4, 0 dead_code, KSWAPD_TICK_INTERVAL=100) / B4 MSI/MSI-X + ACPI (framework/pci/msi.rs 462 行 + arch/x86_64/acpi.rs 840 行, FADT/HPET/DMAR 全套, msi_alloc_vector/enable, msix_enable/mask/unmask, acpi_shutdown)
  - 状态: [X]

- **Phase C 生产可用 (已完成 7/7 2026-06-10)**
  - 描述: 7 子项全部完成
  - 方案: C1 epoll (framework/syscall/epoll.rs 370 行, VFS 集成, WaitQueue 真阻塞, epoll_pwake, 3 TODO 全清) / C2 CPU 亲和性 (cpuset_allowed, sys_sched_setaffinity/getaffinity, services/proc/sched.rs, host test 172/172) / C3 UDS (详见 uds-design.md) / C4 io_uring/AIO (IoOpCode + Sqe/Cqe + RingBuffer + IoUring + URING_TABLE, sys_io_uring_setup/enter/register/submit_sqe, services/io/iouring.rs 0 unsafe, Read/Write 待 VFS fd 表集成) / C5 路由表+Netfilter (RouteEntry + KERNEL_ROUTE_TABLE + CIDR 最长前缀匹配 + sync_route_to_smoltcp, NfHook 5 钩子点 + NfRule + NfChain) / C6 Lockdep+ftrace (见 P1) / C7 KPTI+Seccomp (KPTI 双架构全功能 + Seccomp framework/proc/seccomp.rs Strict/Filter 模式)
  - 状态: [X]
  - 详情: 详见 [uds-design.md](uds-design.md)

- **Phase D 企业级 (已完成 11/11 2026-06-10)**
  - 描述: 11 子项全部完成
  - 方案: D1 Namespace (7 种 ns + clone_from/unshare/setns) / D2 cgroup (CPU/内存/PID/IO 四控制器) / D3 NUMA (NumaNode + 距离矩阵) / D4 eBPF (BpfInsn/BpfMap/验证器/解释器) / D5 电源管理 (C0-C3 + DVFS) / D6 Secure Boot+TPM (PK/KEK/DB + PCR) / D7 Shadow Stack CET / D8 Tickless NO_HZ / D9 NTP/PTP / D10 kexec / D11 UEFI 启动
  - 状态: [X]

## 工程计划 B: P1 级跨阶段插班任务

### 背景
- **背景条目**
  - 描述: 17 个 P1 跨阶段插班任务
  - 方案: UDS / PI Mutex / Lockdep / eventfd/signalfd/timerfd / QueenX 原生 syscall 编号体系 / linuxulator 独立模块 / dcache/icache / 文件锁 / inotify / sendfile/splice / rlimit / 进程组会话控制终端 / Core Dump / 设备固件 / KGDB/ftrace / POSIX Timer / madvise/mlock / 用户态 Stack Canary
  - 状态: [X]

### 现状 (2026-06-10)
- **现状条目**
  - 描述: 全部 17 类 P1 任务完成
  - 方案: 详见各项关键产出
  - 状态: [X]

### 方案
- **Unix Domain Socket**
  - 描述: 详见 C3
  - 方案: 详见 uds-design.md
  - 状态: [X]
- **Priority Inheritance Mutex**
  - 描述: framework/sync/pi_mutex.rs + services/sync/pi_mutex.rs
  - 方案: 直接捐赠, 多等待者取 max, 回调钩子; 8 个 no_std 测试
  - 状态: [X]
  - 详情: 详见 [pi-mutex-design.md](pi-mutex-design.md)
- **Lockdep 死锁检测**
  - 描述: framework/sync/lockdep.rs 620+ 行
  - 方案: LockClassId/LockKind/LockDepMap/HeldLockStack + AB-BA 环检测 + 中断上下文睡眠锁检测 + 递归锁检测; 集成到 SpinLock/Mutex/RwLock/IrqSpinLock/PiMutex (named() 构造器 + acquire/release 钩子); 双架构 0 error/0 warning, 三审计通过, clippy 0 新 warning
  - 状态: [X]
- **eventfd / signalfd / timerfd**
  - 描述: framework/syscall/{eventfd,signalfd,timerfd}.rs + services/sync/{eventfd,signalfd,timerfd}.rs
  - 方案: epoll 集成, FD 空间 [200,256), 7 个 syscall; 双架构 0 error, clippy 0 新 warning, 三审计通过, 9 个内核测试
  - 状态: [X]
- **QueenX 原生 syscall 编号体系**
  - 描述: types.rs QX_* 常量 (500-800+)
  - 方案: linuxulator.rs 双架构翻译层 (x86_64 + aarch64), syscall_dispatch 改用 QX_* 分发, api.rs 导出 QX_* + 保留 SYS_*; 双架构 0 error/0 warning, 三审计通过, CI 4/4
  - 状态: [X]
- **linuxulator 独立模块**
  - 描述: framework/syscall/linuxulator.rs (替代 translate.rs)
  - 方案: 编号翻译 + 参数转换接口 (LinuxArgs), 预留 at 系列路径拼接/结构体适配; 双架构 0 error/0 warning, 三审计通过
  - 状态: [X]
- **dcache / icache**
  - 描述: framework/fs/vfs/dcache.rs 880+ 行
  - 方案: DCache (Robin Hood 开放寻址哈希) + ICache, 正缓存/负缓存/按 parent_ino 失效; services/fs/dcache.rs 安全封装; RamFs resolve_path 集成 dcache 快速路径; create_file/mkdir/unlink/link/symlink 失效父目录 dcache; write/truncate 失效 icache; 修复预存问题: Mutex/RwLock/PiMutex ?Sized 字段顺序 (data 移至末尾); 双架构 0 error/0 warning, 三审计通过, CI 4/4, host-tests 通过
  - 状态: [X]
- **文件锁 (flock / POSIX locks)**
  - 描述: framework/fs/vfs/flock.rs 730+ 行
  - 方案: FlockTable (64 条目) + PosixLockTable (64 条目), flock (LOCK_SH/LOCK_EX/LOCK_UN/LOCK_NB) + POSIX record locks (F_SETLK/F_GETLK/F_RDLCK/F_WRLCK/F_UNLCK), 字节范围冲突检测, 锁升级/降级; services/fs/flock.rs 安全封装; QX_FLOCK=590 syscall + fcntl F_SETLK/F_GETLK 扩展; 修复预存问题: QX_SETREGID 编号冲突 (合并到 QX_SETREUID), QX_FLOCK 插入导致 580-599 段重编号
  - 状态: [X]
- **inotify 文件事件通知**
  - 描述: framework/fs/vfs/inotify.rs 590+ 行
  - 方案: InotifyInstance/WatchEntry/InotifyEvent, 环形事件队列, sys_inotify_init1/add_watch/rm_watch/read, inotify_notify 事件分发; services/fs/inotify.rs 安全封装; FD 空间 [260,268); QX_INOTIFY_INIT1=640/QX_INOTIFY_ADD_WATCH=641/QX_INOTIFY_RM_WATCH=642 syscall; linuxulator x86_64(288/29/30) + aarch64(26/27/28) 映射; VFS 集成: vfs_open(IN_CREATE+IN_OPEN), vfs_unlink(IN_DELETE+IN_DELETE_SELF), vfs_mkdir(IN_CREATE+IN_ISDIR), vfs_write(IN_MODIFY), vfs_truncate(IN_MODIFY), vfs_close(IN_CLOSE_WRITE/IN_CLOSE_NOWRITE); sys_read/sys_close inotify fd 路径; epoll_pwake 集成
  - 状态: [X]
- **sendfile / splice 零拷贝**
  - 描述: framework/syscall/sendfile.rs 300+ 行
  - 方案: sys_sendfile (file→file/pipe 内核缓冲区中转, 8KB bounce buffer, offset 读写), sys_splice (pipe↔file 中转, SPLICE_F_* flags, 至少一端必须 pipe); ipc/pipe.rs 新增 is_pipe_fd 公开接口; services/fs/sendfile.rs 安全封装; QX_SENDFILE=650/QX_SPLICE=651 syscall; linuxulator x86_64(40/76) + aarch64(71/76) 映射
  - 状态: [X]
- **Resource Limits (rlimit)**
  - 描述: framework/proc/rlimit.rs 290+ 行
  - 方案: RlimitTable (16 条目, Rlimit{cur,max}), sys_getrlimit/sys_setrlimit per-process 读写, setrlimit 特权检查 (pid=1 可提高 hard limit), fork 继承 rlimit; 辅助检查: check_nofile_exceeded/check_as_exceeded/check_nproc_exceeded/get_stack_limit/get_nofile_limit; Process 新增 rlimit_table 字段; services/proc/rlimit.rs 安全封装; QX_GETRLIMIT=702/QX_SETRLIMIT=703 syscall; linuxulator x86_64(97/160) + aarch64(163/164) 映射
  - 状态: [X]
- **进程组/会话/控制终端**
  - 描述: framework/proc/session.rs 617 行
  - 方案: Session + SessionManager (create/create_with_sid/destroy/set_controlling_terminal/release_controlling_terminal/set_foreground_pgid/get_foreground_pgid), proc_setsid/getsid/setpgid/getpgid/init_pgid, sys_tiocsctty/tcsetpgrp/tcgetpgrp, signal_foreground_pgid (终端信号广播), session_leader_exit (SIGHUP+SIGCONT+释放控制终端); services/proc/session.rs 安全封装 (setsid/getsid/setpgid/getpgid/tcsetpgrp/tcgetpgrp); fork 继承 session_id+pgid; QX_TCGETPGRP=538/QX_TCSETPGRP=539 syscall; scheduler.exit 调用 session_leader_exit
  - 状态: [X]
- **Core Dump 生成**
  - 描述: framework/proc/coredump.rs 700+ 行
  - 方案: Elf64Ehdr/Elf64Phdr/PrStatus/CoreSiginfo 完整 ELF 结构体定义, coredump_allowed/coredump_limit 查询, do_coredump (RLIMIT_CORE 检查→内存段收集→ELF 头/程序头/PrStatus/Siginfo note 段写入), write_note_prstatus/write_note_siginfo/write_ehdr/write_phdrs/write_segments/write_memory_segment, copy_from_user_safe (简化版), 双架构 regs 数组 (x86_64 27 u64 / aarch64 34 u64); do_signal_default_action 增加 frame_addr 参数, Core 类信号触发 do_coredump; services/proc/coredump.rs 安全封装
  - 状态: [X]
- **设备固件加载**
  - 描述: framework/chitin/firmware.rs
  - 方案: FirmwareBlob/FirmwareInfo (16 MiB 上限), FNV-1a name hash, devtree_attach_firmware/devtree_get_firmware/devtree_detach_firmware (ChitinNode.firmware 字段); framework/chitin/mod.rs 注册 pub mod firmware; framework/chitin/devtree.rs 调整 DevTree/DEV_TREE 为 pub(crate) 以供同模块树访问; framework/syscall/types.rs 分配 730-733 (QX_FW_LOAD/GET/GET_INFO/DETACH); framework/syscall/firmware.rs: sys_fw_load/get/get_info/detach; framework/syscall/mod.rs 注册 4 个 dispatch; framework/syscall/linuxulator.rs 双架构映射 (x86_64 175/313, aarch64 271/314); services/driver/firmware.rs 安全代理
  - 状态: [X]
- **KGDB / ftrace**
  - 描述: framework/debug/{mod,ringbuf,ftrace,kgdb,api}.rs
  - 方案: ringbuf: SPSC 环形缓冲区 (CAP 2 的幂, push/pop/peek); ftrace: TraceEvent (48 字节 POD, ts + name_hash + 4 u64 args), FtraceState (4 KiB 主 ring), fnv1a_32 名称 hash, trace_event! 宏 (0-4 args); kgdb: KgdbSerial trait, KgdbRegs (x86_64 18 u64 / aarch64 33 u64), GDB RSP 子集 ('?'/'g'/'G'/'m'/'M'/'c'/'s'/'k'/'Z'/'z'); services/debug/mod.rs (0 unsafe); 修复预存问题: kgdb.rs 2 处缺 SAFETY 注释; framework/syscall/types.rs 分配 800-804; framework/syscall/ftrace_kgdb.rs: sys_ftrace_enable/disable/read/stat/kgdb_enter
  - 状态: [X]
- **POSIX Timer**
  - 描述: framework/proc/posix_timer.rs
  - 方案: TimerManager 全局表 (MAX_POSIX_TIMERS=32 槽位), PosixTimerSlot 嵌入 HrTimer, Sigevent/Itimerspec POD; sys_timer_create (clockid 校验 + sigevent 解析 + SIGEV_NONE/SIGNAL), sys_timer_settime/gettime/delete/getoverrun, sys_clock_getres (1ms), posix_timer_release_pid, callback (单次 atomic+发信号, 周期 forward); framework/proc/mod.rs 导出 posix_timer::*; framework/syscall/types.rs 分配 740-745; framework/syscall/posix_timer.rs 6 个 sys_ 包装; services/proc/posix_timer.rs (0 unsafe, #![deny(unsafe_code)]); slot.armed/expiry_count 改为 AtomicBool/AtomicU64 避开 `&T as *mut T` 不安全转换
  - 状态: [X]
- **madvise / mlock**
  - 描述: framework/mm/vma.rs + framework/mm/swap.rs + framework/proc/madvise_mlock.rs (新)
  - 方案: VmFlags 位集 (MLOCKED/LOCKED_FUTURE/LOCKED_ONFAULT/MADV_DONTNEED/MADV_PAGEOUT/MADV_DONTFORK/MADV_DODUMP/MADV_HUGEPAGE/...); Vma 新增 vm_flags 字段; MmStruct 新增 locked_vm + mlock_all_flags; MmStruct::madvise_range (24 种 advice 路由 + VMA 拆分/合并 + PAGEOUT/DONTNEED 触发 swap 路径) / mlock_range / munlock_range / mlock_all / munlock_all / mincore_range / mprotect/mremap 保留 vm_flags 修正; framework/mm/swap.rs: LruEntry 新增 locked 字段, get_victim 跳过 locked, set_page_locked/is_page_locked 公共 API; services/mm/swap.rs 同步封装; framework/proc/rlimit.rs: get_memlock_limit/check_memlock_exceeded (RLIMIT_MEMLOCK); framework/proc/madvise_mlock.rs (新): sys_madvise/mlock/munlock/mlockall/munlockall/mincore, 6 个 sys_; services/proc/madvise_mlock.rs (新, 0 unsafe, #![deny(unsafe_code)]); 修复预存问题: vma.rs Vma 不实现 Copy 导致拆分前缀/后缀两处 moved value 错误, 改用 vmas[i+1].start/.end 取值; vma.rs munlock_all 多余 `use Errno` 引用; madvise_mlock.rs 2 个未用 import 清理
  - 状态: [X]
- **用户态 Stack Canary**
  - 描述: framework/proc/canary.rs + framework/syscall/canary.rs
  - 方案: LFSR-64 熵池 (CAS 推进) + PER_PROC_SEED + generate_canary (低字节强制 0, Linux/glibc 兼容) + 双架构统一实现: get_random_bytes 用 copy_to_user 写用户内存, write_canary_to_user 写 8 字节 canary, process_get_current_canary 读 Process::stack_canary; framework/syscall/canary.rs: sys_getrandom (buf,len,flags, max 256) + sys_get_canary (写 8 字节到用户 buffer); framework/syscall/types.rs 分配 746-747 (QX_GETRANDOM/QX_GET_CANARY); framework/syscall/linuxulator.rs 双架构映射 (x86_64 getrandom=318, aarch64 getrandom=278); framework/syscall/mod.rs + api.rs 注册 dispatch; Process::stack_canary 字段 + Process::new 调 generate_canary; api.rs fork 路径继承父进程 canary; services/proc/canary.rs (0 unsafe) getrandom/get_canary/get_canary_u64 三件套; **LLVM 22 bug 已修复**: copy_user.rs 将 asm label 逻辑拆分到 #[inline(never)] setup_recovery/teardown_recovery, aarch64 用 adr (PC-relative) 替代 mov+label 规避 movz/movk fixup bug; canary.rs 移除架构分流, 双架构走同一实现路径
  - 状态: [X]

## 变更历史
- **2026-06-26**
  - 描述: 按新文档规则重写 (标题+条目(描述+方案+状态)+详情)
  - 方案: 结构重组, 保留原意
  - 状态: [X]
- **2026-06-25**
  - 描述: 同步 Phase D 11/11 完成
  - 方案: -
  - 状态: [X]
- **2026-06-10**
  - 描述: 同步 Phase C 7/7 完成
  - 方案: -
  - 状态: [X]
- **2026-06-08**
  - 描述: 同步 Phase A/B 完成
  - 方案: -
  - 状态: [X]
- **2026-06-07**
  - 描述: 初始版本
  - 方案: -
  - 状态: [X]
