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
| C6 Lockdep + ftrace | 部分完成 | 2026-06-09 | Lockdep 已完成 (见 P1), ftrace 未开始 | — |
| C7 KPTI + Seccomp | 未开始 | — | — | — |

**阶段标记**: Phase C 已完成 C1-C3, 剩余 C4-C7 待实施.

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
| inotify 文件事件通知 | 未开始 | — | — | — |
| sendfile / splice 零拷贝 | 未开始 | — | — | — |
| Resource Limits (rlimit) | 未开始 | — | — | — |
| 进程组/会话/控制终端 | 未开始 | — | — | — |
| Core Dump 生成 | 未开始 | — | — | — |
| 设备固件加载 | 未开始 | — | — | — |
| KGDB / ftrace | 未开始 | — | — | — |
| POSIX Timer | 未开始 | — | — | — |
| madvise / mlock | 未开始 | — | — | — |
| 用户态 Stack Canary | 未开始 | — | — | — |
| KPTI 实际页表隔离 | 未开始 | — | — | — |

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

---

## 五、决策记录索引

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

## 六、Backlog (TRACK-XXX)

> 由 `tools/track_todo.py` 自动维护. 完整列表见 [kernel-roadmap.md](./kernel-roadmap.md) §Backlog.

当前未关闭 TRACK 数量: 57

---

## 七、变更历史

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
