# AntX 工程进度跟踪

> 本文档记录 AntX/QueenX 内核项目所有工程阶段的实际完成状态与关键产出.
> 每完成一次工程, 对应阶段标记更新, 并在变更历史中追加条目.
> 配合 [kernel-roadmap.md](./kernel-roadmap.md) (规划) 与 [CHANGELOG.md](../../CHANGELOG.md) (变更日志) 使用.

---

## 一、主线工程 (Roadmap Phase A-D)

### Phase A: 可启动用户态 — 状态: 已完成

| 子项 | 状态 | 完成日期 | 关键产出 | 验证 |
|------|------|----------|----------|------|
| A1 hrtimer | 已完成 | 2026-06-08 | LAPIC Timer / ARM Generic Timer 替代 PIT, per-CPU 红黑树定时器队列, hrtimer_sleep | 双架构 0 error, host test 通过 |
| A2 POSIX 信号投递 | 已完成 | 2026-06-08 | signal_send, pending 位图, syscall_exit/interrupt_return 检查, sigreturn, pgid 支持 (kill 四种 pid 语义) | TRACK-315B7C 修复, 4 个新测试 |
| A3 execve + 用户态 ASLR | 已完成 | 2026-06-08 | sys_execve, ELF 加载器集成, 16 位熵随机偏移, PIE (ET_DYN) 支持 | 双架构编译通过 |
| A4 initramfs + PID 1 | 已完成 | 2026-06-08 | cpio newc 解析器, vfs_symlink, init_launch_status (AtomicU32 状态机), services/init.rs | 6 个测试, 总测试 259 |

**阶段标记**: Phase A 于 2026-06-08 全部完成, 内核可启动并运行首个用户态 init 进程.

---

### Phase B: 可运行真实程序 — 状态: 已完成

| 子项 | 状态 | 完成日期 | 关键产出 | 验证 |
|------|------|----------|----------|------|
| B1 Futex | 已完成 | 2026-06-08 | framework/syscall/futex.rs (428 行), services/sync/futex.rs (148 行), 64 桶哈希表, FUTEX_WAIT/WAKE/REQUEUE/BITSET | 双架构 0 error |
| B2 Page Cache + 文件 mmap | 已完成 | 2026-06-08 | framework/mm/pcache.rs (454 行), MAP_SHARED/PRIVATE/ANONYMOUS, demand paging 真语义, VMA file_pwm 桥接 | vfs_pread_inode + prewarm_file_pages, 7 个 pwm 测试 |
| B3 Swap / 页面回收 | 已完成 | 2026-06-08 | framework/mm/swap.rs, SwapEntry, swap_out_to_pte, kswapd softirq, LRU 跟踪 | 0 dead_code, KSWAPD_TICK_INTERVAL=100 |
| B4 MSI/MSI-X + ACPI | 已完成 | 2026-06-08 | framework/pci/msi.rs (462 行), arch/x86_64/acpi.rs (840 行), FADT/HPET/DMAR 全套 | msi_alloc_vector/enable, msix_enable/mask/unmask, acpi_shutdown |

**阶段标记**: Phase B 于 2026-06-08 全部完成, 内核可运行依赖共享库的用户态程序.

---

### Phase C: 生产可用 — 状态: 进行中 (3/7 完成)

| 子项 | 状态 | 完成日期 | 关键产出 | 验证 |
|------|------|----------|----------|------|
| C1 epoll | 已完成 | 2026-06-08 | framework/syscall/epoll.rs (370 行), VFS 集成, WaitQueue 真阻塞, epoll_pwake | 3 TODO 全清 |
| C2 CPU 亲和性 | 已完成 | 2026-06-08 | cpuset_allowed, sys_sched_setaffinity/getaffinity, services/proc/sched.rs | host test 172/172 |
| C3 Unix Domain Socket | 已完成 | 2026-06-08 | framework/net/unix.rs + services/net/unix.rs, SOCK_STREAM + SOCK_DGRAM, 独立路径表, FD [100,116) | 5 个 no_std 测试, 详见 [uds-design.md](uds-design.md) |
| C4 io_uring / AIO | 未开始 | — | — | — |
| C5 路由表 + Netfilter | 未开始 | — | — | — |
| C6 Lockdep + ftrace | 已完成 | 2026-06-09 | Lockdep 已完成 (见 P1), ftrace 已完成 (见 P1) | 双架构 0 error, 三审计通过, CI 4/4 |
| C7 KPTI + Seccomp | 部分完成 (KPTI x86_64 全功能 + aarch64 全功能) | 2026-06-10 | x86_64: USER_PML4 分配 + 复制 KERNEL_PML4[256..512] 内核高半区 + 清 USER 位 + kpti_enter_kernel / kpti_exit_to_user CR3 切换原语 + 公共 API; vmm::Vmm::init 集成; 汇编 entry/exit trampoline CR3 切换 (syscall/IRQ/exception); trampoline 页 RO+NX 化; PCID/INVPCID 优化 (CR4.PCIDE + PCID 编码 CR3 + invpcid 刷新); aarch64: kpti_aarch64.rs 全功能 (trampoline TTBR1 + kpti_enter/exit + kpti_init + #[no_mangle] 全局变量供汇编读取); exception.rs handle_el0_sync/handle_el0_irq 入口 adrp+ldr KERNEL_TTBR1 切换 + eret 出口 adrp+ldr TRAMP_TTBR1 切换 + handle_svc 出口切换; vmm_aarch64.rs init 末尾调用 kpti_init; enter_user/return_to_user eret 前 TTBR1 切换; 双架构 0/0 + clippy 0 + 三审计 + CI 4/4; **未完成**: Seccomp | KPTI 双架构全功能就绪, Seccomp 待实施 |

**阶段标记**: Phase C 已完成 C1-C3, C6, 剩余 C4/C5/C7 待实施.

---

### Phase D: 企业级 — 状态: 未开始 (子项部分预研)

| 子项 | 状态 | 完成日期 | 关键产出 | 验证 |
|------|------|----------|----------|------|
| D1 网络栈收尾 | 部分完成 | — | 13 safe API (services/net/socket.rs), 12 socket syscall (framework/syscall), 需补 DNS + 端到端测试 | 待补全 |
| D2 HiveFS 端到端测试 | 未开始 | — | 已有 17 module (SPA/DMU/ZAP/TXG/ZIL/ARC/RAID-Z), 缺 e2e 测试 | — |
| D3 axsh (用户态 Shell) | 已完成 | 2026-06-08 | 31 个内置命令, 管道支持, 文件操作/系统管理 | 缺单元测试 (D6) |
| D4 elfld.so + queenx_libc.musl | 未开始 | — | — | — |
| D5 linuxulator | 未开始 | — | — | — |
| D6 axsh 单元测试 | 未开始 | — | — | — |

**阶段标记**: Phase D 尚未正式开始, D1/D3 有前期产出.

---

## 二、P1 级跨阶段插班任务

| 子项 | 状态 | 完成日期 | 关键产出 | 验证 |
|------|------|----------|----------|------|
| Unix Domain Socket | 已完成 | 2026-06-08 | 同 C3 | 同 C3 |
| Priority Inheritance Mutex | 已完成 | 2026-06-08 | framework/sync/pi_mutex.rs + services/sync/pi_mutex.rs, 直接捐赠, 多等待者取 max, 回调钩子 | 8 个 no_std 测试, 详见 [pi-mutex-design.md](pi-mutex-design.md) |
| Lockdep 死锁检测 | 已完成 | 2026-06-09 | framework/sync/lockdep.rs (620+ 行): LockClassId/LockKind/LockDepMap/HeldLockStack, AB-BA 环检测 + 中断上下文睡眠锁检测 + 递归锁检测; services/sync/lockdep.rs 安全封装; 集成到 SpinLock/Mutex/RwLock/IrqSpinLock/PiMutex (named() 构造器 + acquire/release 钩子) | 双架构 0 error/0 warning, 三审计通过, clippy 0 新 warning |
| eventfd / signalfd / timerfd | 已完成 | 2026-06-09 | framework/syscall/eventfd.rs + signalfd.rs + timerfd.rs, services/sync/eventfd.rs + signalfd.rs + timerfd.rs, epoll 集成, FD 空间 [200,256), 7 个 syscall (eventfd/eventfd2/signalfd/signalfd4/timerfd_create/settime/gettime) | 双架构 0 error, clippy 0 新 warning, 三审计通过, 9 个内核测试 |
| QueenX 原生 syscall 编号体系 | 已完成 | 2026-06-09 | types.rs QX_* 常量 (500-800+), linuxulator.rs 双架构翻译层 (x86_64 + aarch64), syscall_dispatch 改用 QX_* 分发, api.rs 导出 QX_* + 保留 SYS_* | 双架构 0 error/0 warning, 三审计通过, CI 4/4 |
| linuxulator 独立模块 | 已完成 | 2026-06-09 | framework/syscall/linuxulator.rs (替代 translate.rs), 编号翻译 + 参数转换接口 (LinuxArgs), 预留 at 系列路径拼接/结构体适配 | 双架构 0 error/0 warning, 三审计通过 |
| dcache / icache | 已完成 | 2026-06-09 | framework/fs/vfs/dcache.rs (880+ 行): DCache (Robin Hood 开放寻址哈希) + ICache, 正缓存/负缓存/按 parent_ino 失效; services/fs/dcache.rs 安全封装; RamFs resolve_path 集成 dcache 快速路径 (Hit/Negative/Miss); create_file/mkdir/unlink/link/symlink 失效父目录 dcache; write/truncate 失效 icache; 修复预存问题: Mutex/RwLock/PiMutex ?Sized 字段顺序 (data 移至末尾) | 双架构 0 error/0 warning, 三审计通过, CI 4/4, host-tests 通过 |
| 文件锁 (flock / POSIX locks) | 已完成 | 2026-06-09 | framework/fs/vfs/flock.rs (730+ 行): FlockTable (64 条目) + PosixLockTable (64 条目), flock (LOCK_SH/LOCK_EX/LOCK_UN/LOCK_NB) + POSIX record locks (F_SETLK/F_GETLK/F_RDLCK/F_WRLCK/F_UNLCK), 字节范围冲突检测, 锁升级/降级; services/fs/flock.rs 安全封装; QX_FLOCK=590 syscall + fcntl F_SETLK/F_GETLK 扩展; sys_close 释放 flock_release_fd, process_exit 释放 flock_release_pid + posix_lock_release_pid, vfs_unlink 释放 posix_lock_release_inode; linuxulator x86_64(73) + aarch64(32) 映射; 修复预存问题: QX_SETREGID 编号冲突 (合并到 QX_SETREUID), QX_FLOCK 插入导致 580-599 段重编号 | 双架构 0 error/0 warning, 三审计通过, CI 4/4, host-tests 通过 |
| inotify 文件事件通知 | 已完成 | 2026-06-09 | framework/fs/vfs/inotify.rs (590+ 行): InotifyInstance/WatchEntry/InotifyEvent, 环形事件队列, sys_inotify_init1/add_watch/rm_watch/read, inotify_notify 事件分发, inotify_release/inotify_fd_readable; services/fs/inotify.rs 安全封装; FD 空间 [260,268); QX_INOTIFY_INIT1=640/QX_INOTIFY_ADD_WATCH=641/QX_INOTIFY_RM_WATCH=642 syscall; linuxulator x86_64(288/29/30) + aarch64(26/27/28) 映射; VFS 集成: vfs_open(IN_CREATE+IN_OPEN), vfs_unlink(IN_DELETE+IN_DELETE_SELF), vfs_mkdir(IN_CREATE+IN_ISDIR), vfs_write(IN_MODIFY), vfs_truncate(IN_MODIFY), vfs_close(IN_CLOSE_WRITE/IN_CLOSE_NOWRITE); sys_read/sys_close inotify fd 路径; epoll_pwake 集成 | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过, CI 4/4 |
| sendfile / splice 零拷贝 | 已完成 | 2026-06-09 | framework/syscall/sendfile.rs (300+ 行): sys_sendfile (file→file/pipe 内核缓冲区中转, 8KB bounce buffer, offset 读写), sys_splice (pipe↔file 中转, SPLICE_F_* flags, 至少一端必须 pipe); ipc/pipe.rs 新增 is_pipe_fd 公开接口; services/fs/sendfile.rs 安全封装; QX_SENDFILE=650/QX_SPLICE=651 syscall; linuxulator x86_64(40/76) + aarch64(71/76) 映射 | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过, CI 4/4 |
| Resource Limits (rlimit) | 已完成 | 2026-06-09 | framework/proc/rlimit.rs (290+ 行): RlimitTable (16 条目, Rlimit{cur,max}), sys_getrlimit/sys_setrlimit per-process 读写, setrlimit 特权检查 (pid=1 可提高 hard limit), fork 继承 rlimit; 辅助检查: check_nofile_exceeded/check_as_exceeded/check_nproc_exceeded/get_stack_limit/get_nofile_limit; Process 结构体新增 rlimit_table 字段; services/proc/rlimit.rs 安全封装 (re-export + 委托); QX_GETRLIMIT=702/QX_SETRLIMIT=703 syscall; linuxulator x86_64(97/160) + aarch64(163/164) 映射 | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过, CI 4/4 |
| 进程组/会话/控制终端 | 已完成 | 2026-06-09 | framework/proc/session.rs (617 行): Session 结构体 (session_id/terminal/foreground_pgid), SessionManager (create/create_with_sid/destroy/set_controlling_terminal/release_controlling_terminal/set_foreground_pgid/get_foreground_pgid), proc_setsid (进程组长检查+新会话+新进程组), proc_getsid (读取 session_id), proc_setpgid (会话一致性+子进程检查+进程组存在性), proc_getpgid, proc_init_pgid, sys_tiocsctty (会话 leader 设置控制终端), sys_tcsetpgrp/sys_tcgetpgrp (前台进程组), signal_foreground_pgid (终端信号广播), session_leader_exit (SIGHUP+SIGCONT+释放控制终端); services/proc/session.rs 安全封装 (setsid/getsid/setpgid/getpgid/tcsetpgrp/tcgetpgrp); fork 继承 session_id+pgid; sys_getpgid 委托 session 模块; QX_TCGETPGRP=538/QX_TCSETPGRP=539 syscall; scheduler.exit 调用 session_leader_exit | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过, CI 4/4 |
| Core Dump 生成 | 已完成 | 2026-06-09 | framework/proc/coredump.rs (700+ 行): Elf64Ehdr/Elf64Phdr/PrStatus/CoreSiginfo 完整 ELF 结构体定义, coredump_allowed/coredump_limit 查询, do_coredump (RLIMIT_CORE 检查→内存段收集→ELF 头/程序头/PrStatus/Siginfo note 段写入), write_note_prstatus/write_note_siginfo/write_ehdr/write_phdrs/write_segments/write_memory_segment, copy_from_user_safe (简化版,后续接 exception table), 双架构 regs 数组 (x86_64 27 u64 / aarch64 34 u64); do_signal_default_action 增加 frame_addr 参数,Core 类信号触发 do_coredump; services/proc/coredump.rs 安全封装 (coredump_allowed/coredump_limit) | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过, CI 4/4 |
| 设备固件加载 | 已完成 | 2026-06-09 | framework/chitin/firmware.rs: FirmwareBlob/FirmwareInfo (16 MiB 上限), FNV-1a name hash, devtree_attach_firmware/devtree_get_firmware/devtree_detach_firmware (ChitinNode.firmware 字段); framework/chitin/mod.rs 注册 pub mod firmware; framework/chitin/devtree.rs 调整 DevTree/DEV_TREE 为 pub(crate) 以供同模块树访问; framework/syscall/types.rs 分配 730-733 (QX_FW_LOAD/GET/GET_INFO/DETACH); framework/syscall/firmware.rs: sys_fw_load (vfs_open/vfs_read 读路径, NUL 结尾校验, UTF-8 校验, 16 MiB 截断), sys_fw_get (offset + 长度拷贝到用户态, 8 MiB 缓冲), sys_fw_get_info (FirmwareInfo POD 写入), sys_fw_detach; framework/syscall/mod.rs 注册 4 个 dispatch; framework/syscall/linuxulator.rs 双架构映射 (x86_64 175/313, aarch64 271/314); services/driver/firmware.rs 安全代理 (firmware_request, firmware_name_hash) | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过, CI 4/4 |
| KGDB / ftrace | 已完成 | 2026-06-09 | framework/debug/mod.rs 整合 ringbuf/ftrace/kgdb/api; framework/debug/ringbuf.rs: SPSC 环形缓冲区 (CAP 2 的幂, push/pop/peek, 容量不足覆盖最旧); framework/debug/ftrace.rs: TraceEvent (48 字节 POD, ts + name_hash + 4 u64 args), FtraceState (4 KiB 主 ring, enabled/event_count/overflow_count, MAX_TRACE_POINTS 登记表, record/pop/register_point/record_named), fnv1a_32 名称 hash, trace_event! 宏 (0-4 args); framework/debug/kgdb.rs: KgdbSerial trait, KgdbRegs (x86_64 18 u64 / aarch64 33 u64), GDB RSP 子集 ('?'/'g'/'G'/'m'/'M'/'c'/'s'/'k'/'Z'/'z'), 串口函数指针分发 (read_dispatch/write_dispatch), kgdb_loop/breakpoint/break_now/serial_ready/active; framework/debug/api.rs: debug_init/ftrace_enable/disable/is_enabled/event_count/overflow_count/pop_event/register_point/kgdb_break_now; framework/syscall/types.rs 分配 800-804 (QX_FTRACE_ENABLE/DISABLE/READ/STAT/KGDB_ENTER); framework/syscall/ftrace_kgdb.rs: sys_ftrace_enable/disable/read (TraceEvent POD 写入, 48 字节)/stat ([u64;2] 写入)/kgdb_enter (kgdb_serial_ready 检查, ENODEV 兜底); framework/syscall/mod.rs 注册 5 个 dispatch; framework/syscall/api.rs 重导出 QX_FTRACE_*/QX_KGDB_ENTER; services/debug/mod.rs (0 unsafe): ftrace_enable/disable/is_enabled/event_count/overflow_count/read_event/register, kgdb_enter/is_active/serial_ready, debug_init; services/mod.rs 注册 debug 子模块; 修复预存问题: kgdb.rs 2 处缺 SAFETY 注释 (unsafe fn 配 // SAFETY:, unsafe { 移至调用点) | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过 (services 0 unsafe + SAFETY 100%), CI 4/4 |
| POSIX Timer | 已完成 | 2026-06-09 | framework/proc/posix_timer.rs: TimerManager 全局表 (MAX_POSIX_TIMERS=32 槽位), PosixTimerSlot 嵌入 HrTimer (中断上下文回调), Sigevent/Itimerspec #[repr(C)] POD; sys_timer_create (clockid 校验 + sigevent 解析 + SIGEV_NONE/SIGNAL), sys_timer_settime (旧值输出 + disarm + 绝对/相对时间 + hrtimer_start), sys_timer_gettime, sys_timer_delete, sys_timer_getoverrun, sys_clock_getres (1ms 分辨率), posix_timer_release_pid (进程退出清理), callback (单次 atomic+发信号, 周期 forward); framework/proc/mod.rs 导出 posix_timer::*; framework/syscall/types.rs 分配 740-745 (QX_TIMER_CREATE/SETTIME/GETTIME/DELETE/GETOVERRUN/CLOCK_GETRES); framework/syscall/posix_timer.rs 6 个 sys_ 包装; framework/syscall/mod.rs 6 个 dispatch; framework/syscall/api.rs 重导出; framework/syscall/linuxulator.rs 双架构映射: x86_64 (222/223/224/226/227/229) + aarch64 (107/108/109/110/111/114); services/proc/posix_timer.rs (0 unsafe, #![deny(unsafe_code)]); services/proc/mod.rs 注册; slot.armed/expiry_count 改为 AtomicBool/AtomicU64 避开 `&T as *mut T` 不安全转换 (中断上下文持 mut slot) | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过 (services 0 unsafe + SAFETY 100%), CI 4/4 |
| madvise / mlock | 已完成 | 2026-06-09 | framework/mm/vma.rs: VmFlags 位集 (MLOCKED/LOCKED_FUTURE/LOCKED_ONFAULT/MADV_DONTNEED/MADV_PAGEOUT/MADV_DONTFORK/MADV_DODUMP/MADV_HUGEPAGE/...); Vma 新增 vm_flags 字段; MmStruct 新增 locked_vm (AtomicUsize) + mlock_all_flags (AtomicU32) 字段; MmStruct::madvise_range (24 种 advice 路由 + VMA 拆分/合并 + PAGEOUT/DONTNEED 触发 swap 路径) / mlock_range (VMA 标记 MLOCKED + locked_vm 累计 + 同步 set_page_locked) / munlock_range (解除锁定 + 计数回收) / mlock_all (MCL_CURRENT/FUTURE/ONFAULT) / munlock_all / mincore_range (page table 走页查询驻留) / mprotect/mremap 保留 vm_flags 修正; framework/mm/swap.rs: LruEntry 新增 locked 字段, get_victim 跳过 locked, set_page_locked/is_page_locked 公共 API; services/mm/swap.rs 同步封装; framework/proc/rlimit.rs: get_memlock_limit/check_memlock_exceeded (RLIMIT_MEMLOCK); framework/proc/madvise_mlock.rs (新): sys_madvise/sys_mlock/sys_munlock/sys_mlockall/sys_munlockall/sys_mincore, 6 个 sys_ 全部带地址页对齐 + 用户指针 check_user_buf + advice 范围校验 + 错误返回 -EINVAL/-EFAULT/-ENOMEM; framework/syscall/types.rs 分配 760-765 (QX_MADVISE/MLOCK/MUNLOCK/MLOCKALL/MUNLOCKALL/MINCORE); framework/syscall/madvise_mlock.rs (新): 6 个 sys_ 薄封装适配 4-arg 寄存器约定; framework/syscall/mod.rs 注册 6 个 dispatch; framework/syscall/linuxulator.rs 双架构映射: x86_64 madvise=28/mincore=27/mlock=149/munlock=150/mlockall=151/munlockall=152, aarch64 madvise=233/mincore=232/mlock=228/munlock=229/mlockall=230/munlockall=231; services/proc/madvise_mlock.rs (新, 0 unsafe, #![deny(unsafe_code)]): Advice 强类型枚举 (Normal/Random/Sequential/WillNeed/DontNeed/Free/Remove/DontFork/DoFork/Mergeable/Unmergeable/HugePage/NoHugePage/DontDump/DoDump/WipeOnFork/KeepOnFork/Cold/PageOut) + MlockAllFlags 位集 + MlockError 错误 + madvise/mlock/munlock/mlockall/munlockall/mincore 用户态 API; services/proc/mod.rs 注册; **架构约束说明**: CURRENT_MM 为全局静态指针, MmStruct 在 execve 时被 set_current_mm 注入, 所有后续 fork 共享同一 MmStruct → fork 路径自动继承 mlock_all_flags + locked_vm (locked_vm 共享), process_exit 无需释放 (MmStruct 生命周期由 execve 边界管理); 修复预存问题: vma.rs Vma 不实现 Copy 导致拆分前缀/后缀两处 moved value 错误, 改用 vmas[i+1].start/.end 取值; vma.rs munlock_all 多余 `use Errno` 引用; madvise_mlock.rs 2 个未用 import 清理 | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过 (services 0 unsafe + SAFETY 100%), CI 4/4 |
| 用户态 Stack Canary | 已完成 (双架构完整实现) | 2026-06-10 | framework/proc/canary.rs: LFSR-64 熵池 (CAS 推进) + PER_PROC_SEED + generate_canary (低字节强制 0, Linux/glibc 兼容) + 双架构统一实现: get_random_bytes 用 copy_to_user 写用户内存, write_canary_to_user 写 8 字节 canary, process_get_current_canary 读 Process::stack_canary; framework/syscall/canary.rs: sys_getrandom (buf,len,flags, max 256) + sys_get_canary (写 8 字节到用户 buffer); framework/syscall/types.rs 分配 746-747 (QX_GETRANDOM/QX_GET_CANARY); framework/syscall/linuxulator.rs 双架构映射 (x86_64 getrandom=318, aarch64 getrandom=278); framework/syscall/mod.rs + api.rs 注册 dispatch; Process::stack_canary 字段 + Process::new 调 generate_canary; api.rs fork 路径继承父进程 canary; services/proc/canary.rs (0 unsafe) getrandom/get_canary/get_canary_u64 三件套; services/proc/mod.rs 注册; **LLVM 22 bug 已修复**: copy_user.rs 将 asm label 逻辑拆分到 #[inline(never)] setup_recovery/teardown_recovery, aarch64 用 adr (PC-relative) 替代 mov+label 规避 movz/movk fixup bug; canary.rs 移除架构分流, 双架构走同一实现路径 | 双架构 0 error/0 warning, clippy 0 新 warning, 三审计通过 (services 0 unsafe + SAFETY 100%), CI 4/4 |
| KPTI 实际页表隔离 | 已完成 (双架构全功能) | 2026-06-10 | framework/mm/kpti.rs (x86_64): USER_PML4 分配 + 复制 KERNEL_PML4[256..512] 内核高半区 + 清 USER 位 + kpti_enter_kernel / kpti_exit_to_user CR3 切换原语 + 公共 API; vmm::Vmm::init 集成; 汇编 entry/exit trampoline CR3 切换 (syscall/IRQ/exception); trampoline 页 RO+NX 化; PCID/INVPCID 优化 (CR4.PCIDE + PCID 编码 CR3 + invpcid 刷新); framework/mm/kpti_aarch64.rs: 全功能 (trampoline TTBR1 + kpti_enter/exit + kpti_init + #[no_mangle] 全局变量供汇编读取); exception.rs handle_el0_sync/handle_el0_irq 入口 adrp+ldr KERNEL_TTBR1 切换 + eret 出口 adrp+ldr TRAMP_TTBR1 切换 + handle_svc 出口切换; vmm_aarch64.rs init 末尾调用 kpti_init; enter_user/return_to_user eret 前 TTBR1 切换; 双架构 0/0 + clippy 0 + 三审计通过 + CI 4/4 | KPTI 双架构全功能就绪 |

---

## 三、Services 层迁移工程

> 将 syscall handler 从 framework 迁移到 services 层, 实现 0 unsafe.

| 批次 | 状态 | 完成日期 | 迁移内容 | 测试数 |
|------|------|----------|----------|--------|
| Phase B (B10-B20) | 已完成 | 2026-06-08 | 49 个 syscall (open/close, stat, link/symlink, mount/umount, rename, timer 等), 13 个新 services 模块 | 220 host-side |
| Storage 子系统 | 已完成 | 2026-06-08 | services/storage/ (disk_list/info/format/partition), 6 个 credo_disk_* syscall, has_capability 认证 | 10 测试, 总 246 |
| mmap/mprotect/mremap | 已完成 | 2026-06-08 | services/mm/mmap.rs, mprotect.rs, mremap.rs, MAP_SHARED/PRIVATE/ANONYMOUS/FIXED | — |
| ELF 加载 + execve | 已完成 | 2026-06-08 | services/proc/elf.rs, execve.rs, 7 个 API, 错误码封装 | — |
| sysfs | 已完成 | 2026-06-08 | services/fs/sysfs.rs, 6 个节点, SysfsValue enum | — |

**当前 services 层状态**: 0 unsafe 行, 边界审计通过.

---

## 四、已修复预存问题

| 问题 | 位置 | 根因 | 修复日期 |
|------|------|------|----------|
| stress_test 永久卡住 | host-tests/src/hvfs/dedup.rs:65 | AB-BA 死锁 (index→refs vs refs→index) | 2026-06-08 |
| signal kill 四种 pid 语义缺失 | services/proc/signal.rs | 缺 pgid 字段与 do_signal_send_extended | 2026-06-08 |
| Mutex/RwLock/PiMutex ?Sized 编译失败 | framework/sync/mutex.rs, rwlock.rs, pi_mutex.rs | UnsafeCell<T> 不是结构体最后字段, ?Sized T 无法编译 | 2026-06-09 |
| 31 个 clippy warning | framework + services 多文件 | manual_range_contains (15), redundant_closure (4), manual_is_multiple_of (4), identical_if_blocks (2), ptr_eq (1), needless_return (1), while_let_loop (1), slow_vector_init (1), should_implement_trait (1), manual_range_contains (Range 1) | 2026-06-09 |
| aarch64 clippy deny: absurd_extreme_comparisons | framework/driver/virtio/net.rs:525 | KERNEL_BASE 在 aarch64 上为 0, phys >= 0 恒为 true | 2026-06-09 |
| kgdb.rs 缺 SAFETY 注释 (2 处) | framework/debug/kgdb.rs:73, 379 | unsafe fn kgdb_set_serial 仅有 # Safety doc comment, 审计仅识别 // SAFETY: 行内注释; unsafe { 块的 SAFETY 注释写在块内部而非前置 | 2026-06-09 |

---

## 五、已知未解决问题 (待跟进)

> 列出当前已确认但尚未修复的问题, 按 TRACK-XXX 跟踪. 每完成一项, 删除对应条目, 迁移到 §四.

### 5.1 [TRACK-081BC6 / F0ED2E / FA2B11] aarch64 LLVM 22 codegen bug 致 P1 #14 三个函数 stub 化 — 已关闭

**状态**: 已修复 (2026-06-10). copy_user.rs 将 asm label 逻辑拆分到 `#[inline(never)]` 的 `setup_recovery`/`teardown_recovery` 函数; aarch64 用 `adr` (PC-relative 单指令) 替代 `mov`+label, 规避 `movz/movk` fixup bug; canary.rs 移除架构分流, 双架构走同一实现路径.

**跟踪 ID**: 三个 (每个函数一个, 由 `tools/track_todo.py` 自动生成)
- `TRACK-081BC6` — `src/kernel/framework/proc/canary.rs:27` (`get_random_bytes`) — **已关闭**
- `TRACK-F0ED2E` — `src/kernel/framework/proc/canary.rs:40` (`write_canary_to_user`) — **已关闭**
- `TRACK-FA2B11` — `src/kernel/framework/proc/canary.rs:54` (`process_get_current_canary`) — **已关闭**

**位置**:
- 关联 doc: `src/kernel/framework/syscall/canary.rs` 顶层"已知问题"注释
- 关联 doc: `docs/plan/kernel-roadmap.md` §Backlog 末尾三行

**优先级**: P1 #14 收尾, 高
**报告日期**: 2026-06-09
**影响范围**: aarch64 架构 (x86_64 不受影响)
**症状**: 用户态启动时若调用 `QX_GETRANDOM` (syscall 746) 或 `QX_GET_CANARY` (syscall 747), 内核侧实际不写用户 buffer, 用户态拿到 0 / 空填充; 完整进程间 stack canary 隔离失效 (所有进程共享熵池生成的同一 canary).

#### 5.1.1 错误信息

```
error: could not compile `queenx` (lib) due to 1 previous error
  |
  = note: due to invalid fixup for movz/movk instruction
  = note: define the .ll file with `-C llvm-args=--save-temps`
```

具体 LLVM IR 错误涉及 `mov x8, 8f` 这类带 label 的内联汇编在 aarch64 后端被错误地编码为 `movz/movk` 立即数寻址.

#### 5.1.2 根因

工具链: **rustc 1.97 nightly + LLVM 22**, 已知 aarch64 codegen 缺陷.

触发条件 (同时满足):
1. 涉及 aarch64 后端
2. inline asm 中含 label (例如 `8:` / `1:`)
3. 该函数被 inline 进入一个**极大**的函数 (例如 `Process::new`, dispatch 宏展开的 `syscall_dispatch`)
4. inline 链路上含 `copy_to_user` / `PROCESS_TABLE.with_process` 闭包

**x86_64 端不受影响** — `mov x8, 8f` 是 aarch64 指令编码问题.

#### 5.1.3 排查过程 (按时间顺序)

| 尝试 | 修改 | 结果 |
|------|------|------|
| 1 | 给 `Process::new` 加 `#[inline(never)]` | 失败 — 编译仍报错 |
| 2 | 给 `sys_get_canary` / `sys_getrandom` 加 `#[inline(never)]` | 失败 — 编译仍报错 |
| 3 | 移除 aarch64 端 `read_arch_timestamp` 的 inline asm, 退化为 `PER_PROC_SEED` 原子读; 给 `read_arch_timestamp` / `generate_canary` 加 `#[inline(never)]` | 失败 — 编译仍报错 |
| 4 | stub 化 `get_random_bytes` (返回 0) | 失败 — 编译仍报错 (说明不是 `get_random_bytes` 自身) |
| 5 | 同步 stub 化 `write_canary_to_user` + `process_get_current_canary` | 成功! 编译通过, 0 warning, 0 error |

#### 5.1.4 当前 stub 行为

```rust
// framework/proc/canary.rs
pub fn get_random_bytes(_buf: u64, _len: usize) -> usize {
    0  // 真实: 检查 check_user_buf + 生成随机字节 + copy_to_user
}

pub fn write_canary_to_user(_buf: u64, _len: usize) -> i64 {
    0  // 真实: check_user_buf(buf, 8) + process_get_current_canary + 8 字节 LE 写
}

pub fn process_get_current_canary() -> u64 {
    generate_canary()  // 真实: PROCESS_TABLE.with_process(pid, |p| p.stack_canary.load(...))
}
```

`sys_getrandom` 因底层 stub 永远返回 0, `sys_get_canary` 永远返回 0 (不写用户 buffer).

#### 5.1.5 修复建议 (接手人参考)

**方案 D 已实施: x86_64 / aarch64 架构分流**

当前状态: `framework/proc/canary.rs` 已用 `#[cfg(target_arch)]` 分流:
- x86_64 走真实路径 (`copy_to_user` + `PROCESS_TABLE.with_process`)
- aarch64 保持 stub

x86_64 端功能完整, 后续 aarch64 根因修复方案:

**方案 C (推荐后续迭代): 将 `copy_to_user` exception table 机制从 `asm!` 迁移到 `global_asm!`**

参考 Linux `__arch_copy_to_user` 模式: 将含 label 的 inline asm 移到独立汇编文件或
`global_asm!`, Rust 侧只做 FFI 调用. 这是从根本上规避 LLVM aarch64 codegen bug
的方式, 改动面涉及 `copy_to_user` / `copy_from_user` / `clear_user` 等五个函数.

**方案 A: 拆分函数 + 中间结构传递**

将每个真实函数拆分为"检查 + 中间数据"和"写入"两个函数, 中间通过普通 `u64` 数组或 `Result<u64, ()>` 传递, 避免闭包/inline asm 同时进入大函数.

```rust
#[inline(never)]
pub fn process_get_current_canary() -> u64 {
    let pid = process_get_current_pid();
    // 拆出 read_step 函数, 让内联只到此处终止
    read_stack_canary_step(pid)
}

#[inline(never)]
fn read_stack_canary_step(pid: Pid) -> u64 {
    PROCESS_TABLE.with_process(pid, |p| p.stack_canary.load(Ordering::Acquire)).unwrap_or(0)
}
```

**方案 B: 升级工具链**

将 rustc 升级至 stable 1.85+ 或 LLVM 19-, 等待 nightly 修复:
```bash
rustup default stable
# 或固定 LLVM 19: RUSTC_BOOTSTRAP=1 cargo +nightly build -Z llvm-args=...
```

**方案 C: 禁用内联汇编 (退而求其次)**

将 `copy_to_user` 内部所有含 label 的 inline asm 改为普通汇编或 `core::arch::global_asm!`, 避开 LLVM aarch64 后端 bug.

**方案 D: x86_64 / aarch64 分流实现**

仅在 aarch64 端 stub 化 (x86_64 走真实路径), 用 `#[cfg(target_arch = "x86_64")]` 分流, x86_64 编译验证 aarch64 编进 stub 即可.

#### 5.1.6 验证步骤 (修复后)

1. `cd src/rust && cargo build --release --target aarch64-unknown-none 2>&1 | tail -3` → 期望 0 warning, 0 error
2. `cargo build --release --target x86_64-unknown-none 2>&1 | tail -3` → 期望 0 warning, 0 error
3. `cargo clippy --release --target aarch64-unknown-none --lib 2>&1 | tail -5` → 期望 0 警告
4. `python3 scripts/audit_services_boundary.py` / `audit_safety_coverage.py` / `audit_deadlock_matrix.py` → 期望全部通过
5. QEMU 启动测试 (full 模式): `ci/audit.sh full` 跑 QEMU 双架构真实启动
6. 单元测试: `cd host-tests && cargo test` 期望 0 失败
7. 编写 user-mode 集成测试: 用户态程序 `syscall(QX_GETRANDOM, buf, 32, 0)` 检查 `buf` 不全为 0; `syscall(QX_GET_CANARY, &c, 8)` 检查 `c` 与熵池常量不同

#### 5.1.7 修复完成后操作清单

- [x] 删除 `framework/proc/canary.rs` 中 `TODO(TRACK-*)` 注释及架构分流
- [x] 删除 `framework/syscall/canary.rs` 顶层"已知问题"段落
- [x] 在 §四 追加修复记录
- [x] 在 §七 变更历史追加"P1 #14 aarch64 完整实现完成"
- [x] 重跑双架构编译 + clippy + 三审计 + CI → 全部通过

### 5.2 [TRACK-KPTI-TRAMPOLINE] KPTI 汇编 entry/exit trampoline 集成 — 已完成

**状态**: 双架构 KPTI 全功能就绪. x86_64 (trampoline CR3 切换 + RO+NX + PCID/INVPCID) + aarch64 (trampoline TTBR1 切换 + 异常入口/出口集成 + enter_user/return_to_user 集成).

**已完成项** (2026-06-10):

1. **x86_64 汇编 entry/exit trampoline**: `isr.asm` 中 syscall/IRQ/exception 入口已集成 CR3 切换 (通过 per-CPU `SyscallPerCpu.kernel_pml4/user_pml4` 字段).
2. **PCID/INVPCID 优化**: `kpti.rs` 中实现 CR4.PCIDE 启用 + PCID 编码 CR3 + `invpcid` 指令刷新; 汇编中 `mov cr3` 携带 PCID, 避免全局 TLB 刷新.
3. **aarch64 TTBR0/TTBR1 全功能**: `kpti_aarch64.rs` 提供 `kpti_init` (创建 trampoline TTBR1) + `kpti_enter_kernel` / `kpti_exit_to_user` (TTBR1 切换) + 公共 API + `#[no_mangle]` 全局变量供汇编直接 `adrp+ldr` 读取.
4. **aarch64 异常入口汇编集成**: `exception.rs` 的 `handle_el0_sync` / `handle_el0_irq` 入口插入 `adrp+ldr KERNEL_TTBR1` + `msr ttbr1_el1` 切换; `handle_svc` / 非 SVC sync / IRQ 出口插入 `adrp+ldr TRAMP_TTBR1` + `msr ttbr1_el1` 切换; `cbz` 跳过零值 (KPTI 未激活时无副作用).
5. **aarch64 VMM init 集成**: `vmm_aarch64.rs` `init()` 末尾调用 `kpti_init(current_l0)`, 在 TTBR1_EL1 设置完成后自动创建 trampoline 页表.
6. **aarch64 enter_user/return_to_user 集成**: `arch/aarch64/mod.rs` 的 `enter_user` 和 `return_to_user` 在 `eret` 前调用 `kpti_trampoline_ttbr1_or_kernel` 切换 TTBR1.
7. **trampoline 页 RO+NX 化**: x86_64 `kpti_init` 遍历 USER_PML4, 将 .text 页设为 RO+NX.

**未完成项**:

1. **Seccomp**: C7 的另一半, 未开始.

**位置**:
- x86_64: `src/kernel/framework/mm/kpti.rs` + `src/kernel/framework/boot/isr.asm` + `src/kernel/framework/arch/x86_64/gdt.rs`
- aarch64: `src/kernel/framework/mm/kpti_aarch64.rs` + `src/kernel/framework/arch/aarch64/exception.rs` + `src/kernel/framework/arch/aarch64/mod.rs` + `src/kernel/framework/mm/vmm_aarch64.rs`

**优先级**: 低 (双架构 KPTI 全功能就绪; Seccomp 待实施)

---

## 六、决策记录索引

| 编号 | 内容 | 日期 | 关联文档 |
|------|------|------|----------|
| DECISION-001 | Phase A 优先于 Phase B | 2026-06-07 | kernel-roadmap.md |
| DECISION-002 | hrtimer 作为 Phase A 首项 | 2026-06-07 | kernel-roadmap.md |
| DECISION-003 | ARC 是 HvFS 私有缓存, Page Cache 是 VFS 通用缓存 | 2026-06-07 | kernel-roadmap.md |
| DECISION-004 | Swap 采用块设备 swap 分区方案 | 2026-06-07 | kernel-roadmap.md |
| DECISION-005 | io_uring 分两步走: 先 AIO 后 io_uring | 2026-06-07 | kernel-roadmap.md |
| DECISION-006 | UDS 不入 VFS inode, 走独立路径表 | 2026-06-08 | uds-design.md |
| DECISION-007 | UDS SOCK_DGRAM 单消息排队 | 2026-06-08 | uds-design.md |
| DECISION-008 | UDS 阻塞语义 v1 退化为 EAGAIN | 2026-06-08 | uds-design.md |
| DECISION-009 | PI Mutex v1 只支持直接捐赠 | 2026-06-08 | pi-mutex-design.md |
| DECISION-010 | PI Mutex 不直接修改 Process, 通过回调通知 | 2026-06-08 | pi-mutex-design.md |
| DECISION-011 | PI Mutex 等待策略 v1 自旋+yield | 2026-06-08 | pi-mutex-design.md |
| DECISION-012 | QueenX 自有 syscall 编号为规范, Linux 兼容层为翻译; 0-299 保留 linuxulator, 500+ 为 QX_* 原生 | 2026-06-09 | queenx-naming-standpoint.md |

---

## 七、Backlog (TRACK-XXX)

> 由 `tools/track_todo.py` 自动维护. 完整列表见 [kernel-roadmap.md](./kernel-roadmap.md) §Backlog.

当前未关闭 TRACK 数量: 59 (含 P1 #14 后续 TRACK-081BC6 / F0ED2E / FA2B11)

---

## 八、变更历史

| 日期 | 变更 | 阶段标记 |
|------|------|----------|
| 2026-06-08 | Phase A 全部完成 (A1-A4) | Phase A: 已完成 |
| 2026-06-08 | Phase B 全部完成 (B1-B4) | Phase B: 已完成 |
| 2026-06-08 | Phase C 完成 C1-C3 | Phase C: 进行中 (3/7) |
| 2026-06-09 | P1 eventfd/signalfd/timerfd 完成, 与 epoll (C1) 配合实现高效事件驱动 | P1: eventfd/signalfd/timerfd 已完成 |
| 2026-06-08 | P1 #3 PI Mutex 完成 | P1: 2/17 完成 |
| 2026-06-08 | Services 层迁移完成 (49 syscall + storage + mmap + ELF + sysfs) | Services: 0 unsafe |
| 2026-06-08 | 预存问题修复 (AB-BA 死锁 + signal pgid) | — |
| 2026-06-09 | 创建工程进度跟踪文档 | — |
| 2026-06-09 | P1 linuxulator 独立模块完成 (替代 translate.rs, 扩展为完整 Linux ABI 兼容层框架) | P1: linuxulator 已完成 |
| 2026-06-09 | P1 dcache/icache 完成: Robin Hood 哈希缓存 + RamFs resolve_path 集成 + 6 处失效点; 修复 Mutex/RwLock/PiMutex ?Sized 编译错误 | P1: dcache/icache 已完成 |
| 2026-06-09 | P1 inotify 文件事件通知完成: 3 个 syscall (init1/add_watch/rm_watch) + read 路径, FD 空间 [260,268), VFS 6 处事件触发, epoll 集成, linuxulator 双架构映射 | P1: inotify 已完成 |
| 2026-06-09 | P1 sendfile/splice 零拷贝完成: sendfile (file→file/pipe) + splice (pipe↔file), 8KB bounce buffer 中转, QX_SENDFILE=650/QX_SPLICE=651, linuxulator 双架构映射 | P1: sendfile/splice 已完成 |
| 2026-06-09 | P1 rlimit 完成: per-process RlimitTable (16 条目), getrlimit/setrlimit, fork 继承, 特权检查, 辅助检查函数 (NOFILE/AS/NPROC/STACK), QX_GETRLIMIT=702/QX_SETRLIMIT=703, linuxulator 双架构映射 | P1: rlimit 已完成 |
| 2026-06-09 | P1 KGDB / ftrace 完成: ringbuf (SPSC 4 KiB ring) + ftrace (TraceEvent/FtraceState/trace_event! 宏) + KGDB (KgdbSerial trait/GDB RSP 子集/双架构寄存器), 5 个 syscall (QX_FTRACE_ENABLE=800/DISABLE/READ/STAT/KGDB_ENTER=804), services/debug 安全封装; 修复预存问题: kgdb.rs 2 处缺 SAFETY 注释 | P1: KGDB / ftrace 已完成; Phase C: C6 已完成 |
| 2026-06-09 | P1 POSIX Timer 完成: TimerManager 全局表 (MAX_POSIX_TIMERS=32) + PosixTimerSlot 嵌入 HrTimer (中断上下文 callback); sys_timer_create/settime/gettime/delete/getoverrun/clock_getres; callback 单次模式 atomic+信号, 周期模式 forward; 6 个 syscall (QX_TIMER_CREATE=740..QX_CLOCK_GETRES=745); Linuxulator 双架构映射 (x86_64 222/223/224/226/227/229, aarch64 107/108/109/110/111/114); services/proc/posix_timer.rs 0 unsafe; slot.armed/expiry_count 改为 AtomicBool/AtomicU64 避开 `&T as *mut T` 不安全转换 | P1: POSIX Timer 已完成; services 0 unsafe, SAFETY 100% |
| 2026-06-09 | P1 用户态 Stack Canary 完成: framework/proc/canary.rs (LFSR-64 熵池 + per-proc 种子 + low-byte 0 兼容) + Process::stack_canary 字段 + Process::new 初始化 + fork 继承; framework/syscall/canary.rs sys_getrandom (Linux 兼容 318/278) + sys_get_canary (QX 扩展 747, 写 8 字节到用户 buffer); services/proc/canary.rs 0 unsafe 封装; **aarch64 codegen 修复**: 移除 read_arch_timestamp inline asm 退化为 per-proc 种子 (规避 rustc 1.97 nightly + LLVM 22 的 `invalid fixup for movz/movk` bug); get_random_bytes/write_canary_to_user/process_get_current_canary 暂 stub 化, 后续 aarch64 编译稳定后再扩展 | P1: 用户态 Stack Canary 已完成; services 0 unsafe, SAFETY 100% |
| 2026-06-09 | P1 #14 收尾: aarch64 LLVM 22 codegen bug 导致 canary.rs 三个函数无法完整实现, 暂以 stub + TRACK-081BC6/F0ED2E/FA2B11 形式记录; 在 §五.1 完整记录问题 (错误信息/根因/5 次排查过程/4 套修复方案/7 步验证/收尾清单), docs/plan/kernel-roadmap.md §Backlog 末尾三行登记 | P1 #14 已知问题已转交, services 0 unsafe, SAFETY 100% |
| 2026-06-09 | P1 #15 madvise/mlock 完成: VmFlags 位集 (MLOCKED/FUTURE/ONFAULT/MADV_DONTNEED/MADV_PAGEOUT/DONTFORK/DODUMP/HUGEPAGE/...) + MmStruct::locked_vm/mlock_all_flags 字段 + madvise_range/mlock_range/munlock_range/mlock_all/munlock_all/mincore_range; LruEntry::locked + set_page_locked/is_page_locked (LRU 跳过锁定页); rlimit get_memlock_limit/check_memlock_exceeded; framework/proc/madvise_mlock.rs (6 个 sys_ 入口) + framework/syscall/madvise_mlock.rs (薄封装) + framework/syscall/types.rs 分配 760-765 + linuxulator 双架构映射 (x86_64: madvise=28/mincore=27/mlock=149/munlock=150/mlockall=151/munlockall=152, aarch64: madvise=233/mincore=232/mlock=228/munlock=229/mlockall=230/munlockall=231); services/proc/madvise_mlock.rs (0 unsafe, #![deny(unsafe_code)]) Advice 强类型 + MlockAllFlags 位集 + MlockError; **架构约束**: CURRENT_MM 为全局静态, MmStruct 在 execve 时注入, fork 共享同一 MmStruct → 自动继承 mlock_all_flags + locked_vm, process_exit 无需释放 (MmStruct 生命周期由 execve 边界管理); 修复预存问题: vma.rs Vma 不 Copy 导致拆分前缀/后缀两处 moved value (用 vmas[i+1].start/.end 取值) + munlock_all 多余 use Errno + madvise_mlock.rs 2 个未用 import | P1 #15 已完成; services 0 unsafe, SAFETY 100% |
| 2026-06-09 | P1 C7 KPTI 骨架完成: framework/mm/kpti.rs (x86_64) 提供 USER_PML4 分配 + 复制 KERNEL_PML4[256..512] 内核高半区并清 USER 位 + kpti_enter_kernel / kpti_exit_to_user CR3 切换原语 + 公共 API (kpti_init / is_active / user_pml4 / kernel_pml4 / user_pml4_or_kernel); vmm::Vmm::init 自动检测 + 集成 kpti_init; 双架构 0/0 + clippy 0 + 三审计通过 + CI 4/4; **未完成 (在 §五.2 TRACK-KPTI-TRAMPOLINE 登记)**: 汇编 entry/exit trampoline 集成 + PCID/INVPCID 优化 + aarch64 TTBR0/TTBR1 + trampoline RO 化 | KPTI 数据结构就绪, trampoline 集成待跟进 |
| 2026-06-10 | P1 C7 KPTI 全功能完成 (x86_64) + aarch64 框架: x86_64 — isr.asm syscall/IRQ/exception 入口集成 CR3 切换 (per-CPU SyscallPerCpu.kernel_pml4/user_pml4); gdt.rs gdt_set_kpti_pml4 设置双 PML4 字段; linker script .kpti_trampoline section 包含 isr.o; trampoline 页 RO+NX 化 (遍历 USER_PML4 .text 页清除 W+NX 位); PCID/INVPCID 优化 (CR4.PCIDE + PCID_KERNEL/PCID_USER + invpcid 刷新 + CR3 低 12 位编码 PCID); aarch64 — kpti_aarch64.rs 框架 (trampoline TTBR1 + kpti_enter_kernel/kpti_exit_to_user + kpti_init); Bug #1 架构分流: canary.rs x86_64 完整实现 + aarch64 stub; 双架构 0/0 + clippy 0 + 三审计 + CI 4/4 | KPTI x86_64 全功能就绪, aarch64 框架待集成 |
| 2026-06-10 | P1 C7 KPTI aarch64 全功能集成: kpti_aarch64.rs KERNEL_TTBR1/TRAMP_TTBR1 加 #[no_mangle] 供汇编 adrp+ldr 直接读取; exception.rs handle_el0_sync/handle_el0_irq 入口插入 adrp+ldr KERNEL_TTBR1 → msr ttbr1_el1 (cbz 跳过零值) + eret 出口插入 adrp+ldr TRAMP_TTBR1 → msr ttbr1_el1; handle_svc 出口同样插入 TRAMP_TTBR1 切换; vmm_aarch64.rs init() 末尾调用 kpti_init(current_l0); arch/aarch64/mod.rs enter_user/return_to_user eret 前调用 kpti_trampoline_ttbr1_or_kernel 切换 TTBR1; 双架构 0/0 + clippy 0 + 三审计 + CI 4/4 | KPTI 双架构全功能就绪, TRACK-KPTI-TRAMPOLINE 关闭 |
| 2026-06-10 | P1 #14 aarch64 Stack Canary 完整实现: copy_user.rs 将 asm label 逻辑拆分到 #[inline(never)] setup_recovery/teardown_recovery 函数; aarch64 用 `adr` (PC-relative 单指令) 替代 `mov`+label 规避 LLVM 22 `movz/movk` fixup bug; canary.rs 移除架构分流, 双架构统一实现; TRACK-081BC6/F0ED2E/FA2B11 关闭; 双架构 0/0 + clippy 0 + 三审计 + CI 4/4 | P1 #14 双架构完整实现, TRACK 三项关闭 |
