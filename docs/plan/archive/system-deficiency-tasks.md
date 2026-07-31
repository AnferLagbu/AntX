# 系统欠缺任务工程文档

> 基于 2026-07-27 全面代码审查 + 当前源码分析的完整欠缺项清单。
> 按优先级分为高/中/低三档，每项标注位置、影响和建议方案。

---

## 高优先级 (🔴 可能导致系统崩溃/安全漏洞)

### H1: KPTI 异常处理程序未映射在用户页表 ✅ 已修复

- **描述**: 用户态触发异常时，异常处理程序 (isr0-isr31, irq0-irq15, syscall_entry) 需在用户页表中可访问
- **位置**: [isr.asm](file:///home/anfer/Code/QueenX/src/kernel/framework/boot/isr.asm) + [x86_64.ld](file:///home/anfer/Code/QueenX/src/kernel/framework/link/x86_64.ld) + [kpti.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kpti.rs#L436)
- **方案**: 链接脚本将 `build/isr.o` 放置于 `_kpti_trampoline_end` 之前；`map_text_region_in_user_pml4` 将整个 .text 段映射到用户页表
- **修复**: 已完成 — 通过链接脚本排序 + KPTI map_text_region_in_user_pml4 全局映射
- **状态**: [X]

### H2: 上下文切换无 FPU/SSE 状态保存 ✅ 已修复

- **描述**: 如果内核线程或用户进程使用浮点/SIMD 指令，上下文切换导致 FPU/XMM 寄存器数据损坏
- **位置**: [switch.asm](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/switch.asm) + [context.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/context.rs) + [types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/types.rs#L160-L194)
- **方案**:
  - 短期: Process 结构体添加 FPU 状态区域，switch.asm 中使用 `xsave`/`xrstor` 保存/恢复
  - 长期: 实现 lazy FPU 切换 (CR0.TS 位)，仅在首次使用 FPU 时保存/恢复
- **详情**: x86_64 需保存 x87 + XMM + YMM (AVX)，AArch64 需保存 V0-V31 + FPCR + FPSR
- **修复**: 已完成 — x86_64 `switch.asm` 使用 `fxsave`/`fxrstor` 保存/恢复 512 字节 FPU 状态；aarch64 `context.rs` 使用 `stp`/`ldp` 保存/恢复 V0-V31 + FPCR/FPSR；`ProcessContext` 包含 `fpu_state: [u64; 64]` (512 bytes) + `_fpu_pad` 16 字节对齐填充
- **状态**: [X]

### H3: Socket 错误映射一刀切 InvalidArgument ✅ 已修复

- **描述**: framework 返回的错误被 `.map_err(|_| SocketError::InvalidArgument)` 统一映射，丢失 `AddrInUse`/`WouldBlock`/`ConnRefused` 等语义
- **位置**: [socket.rs](file:///home/anfer/Code/QueenX/src/kernel/services/net/socket.rs)
- **违反**: AGENTS.md §5.2 "错误处理: 传播用 `?`"
- **方案**: framework 网络层定义细粒度错误枚举 (`NetError`)，services socket.rs 按 variant 映射到对应 `KernelError` 变体
- **修复**: 已完成 — 实现精确 NetError 到 KernelError 映射
- **状态**: [X]

### H4: `net_stack().expect()` 可导致内核 panic ✅ 已修复

- **描述**: `net_stack()` 使用 `expect()` 获取全局网络栈实例，如果 `init()` 未调用就访问会 panic
- **位置**: [net/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/net/mod.rs)
- **方案**: 改为返回 `Option` 或 `Result`，由调用方决定处理方式 (返回错误码/延迟初始化)
- **修复**: 已完成 — `net_stack()` 返回 `Option`，调用方处理 None 情况
- **状态**: [X]

### H5: `per_cpu()` 返回 `&'static` 绕过借用检查 ✅ 已修复

- **描述**: `per_cpu()` 从 `Mutex<Option<PerCpuSched>>` 获取内部引用后通过裸指针转为 `&'static PerCpuSched`，绕过借用检查器；如果其他线程修改 `Option`，引用悬垂
- **位置**: [scheduler.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs) (L182-L200)
- **方案**:
  - 方案 A: 使用 `OnceLock<PerCpuSched>` 替代 `Mutex<Option<PerCpuSched>>`，初始化后不可变
  - 方案 B: 将 `PerCpuSched` 放入 `Box::leak` 获取真正的 `&'static`
- **修复**: 已完成 — 使用 `OnceLock` 替代 `Mutex<Option>`，确保初始化后不可变
- **状态**: [X]

---

## 中优先级 (🟡 代码质量/正确性/可维护性)

### M1: PMM 无 double-free 检测 ✅ 已修复

- **描述**: `free_pages` 重复释放同一物理页破坏 Buddy 空闲链表，导致后续分配返回已用页面
- **位置**: [pmm.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pmm.rs#L1014-L1019)
- **方案**: 页帧元数据添加 `allocated` 标志位，`free_pages` 时检查；或使用 page frame 引用计数
- **修复**: 已完成 — `do_free` 中使用 bitmap 检测 double-free（bitmap 约定：1=已分配, 0=空闲），`test_bit(pfn)` 为 0 时打印警告并返回，在 `buddy_ready` 前后都检测
- **状态**: [X]

### M2: PMM Buddy 合并未验证伙伴 order ✅ 已修复

- **描述**: Buddy 合并时只检查伙伴页是否在空闲链表，未验证伙伴页的 `order` 字段是否匹配当前 order，可能导致跨阶合并
- **位置**: [pmm.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pmm.rs#L824-L839)
- **方案**: 合并前检查 `buddy_order == current_order`，不匹配则停止合并
- **修复**: 已完成 — `buddy_try_merge` 中读取伙伴块 `buddy_state`，验证 `buddy_state <= MAX_BUDDY_ORDER` 且 `buddy_state == order`，不满足则停止合并，防止跨阶合并
- **状态**: [X]

### M3: 全局 IrqSpinLock 网络栈瓶颈 ✅ 已修复

- **描述**: 每次 socket 操作锁住整个 `SmoltcpNetStack`，高并发下严重性能瓶颈
- **位置**: [socket.rs](file:///home/anfer/Code/QueenX/src/kernel/services/net/socket.rs) + [net/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/net/mod.rs)
- **方案**:
  - 短期: `IrqSpinLock` 改为 `Mutex` (如果网络操作不在中断上下文)
  - 长期: 拆分为细粒度锁 (per-socket lock, 全局接口配置锁)
- **修复**: 已完成 — 短期方案实施：`NET_STACK_INSTANCE` 从 `IrqSpinLock<SmoltcpNetStack>` 改为 `Mutex<SmoltcpNetStack>`。网络操作（socket/bind/listen 等）均在进程上下文执行，不在中断上下文，使用 Mutex 的自旋+yield 模式可减少 CPU 空转。细粒度锁拆分作为长期目标待后续评估
- **状态**: [X]

### M4: 100+ 分支巨型 match 系统调用分发 ✅ 已修复

- **描述**: 单个 `match num` 块含 100+ 分支，可读性差，维护成本高
- **位置**: [dispatch.rs](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs)
- **方案**:
  - 按子系统拆分为独立函数 (`dispatch_fs`, `dispatch_proc`, `dispatch_net` 等)
  - 或使用跳转表 (`[Option<fn(&mut InterruptFrame)>; 512]`) 替代 match
- **修复**: 已完成 — 主 `dispatch` 函数按子系统拆分为 `dispatch_fs`/`dispatch_proc`/`dispatch_net`/`dispatch_mm`/`dispatch_sync`/`dispatch_credo`/`dispatch_other` 7 个独立函数，每个函数内部使用 match 处理对应子系统的 syscall
- **状态**: [X]

### M5: PID 空间不可回收 ✅ 已修复

- **描述**: 进程退出后 PID 永久占用，长期运行系统可能耗尽 PID 空间
- **位置**: [process.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs) (`ProcessTable`)
- **方案**:
  - 实现 PID 回收: Zombie 进程被 `wait4` 回收后释放 PID
  - 或使用位图管理 PID，支持重新分配
- **详情**: `wait4` 已调用 `process_remove_and_free` 释放 PCB，需确认 PID 是否回到分配池
- **修复**: 已完成 — 将 `next_pid: AtomicU32` 单调递增改为 `pid_bitmap: [bool; MAX_PROCESSES]` 位图 + `next_search: Mutex<u32>` 环形扫描，`remove_and_free` 和 `dec_ref_and_maybe_free` 中调用 `free_pid` 回收 PID
- **状态**: [X]

### M6: VFS from_u8/from_u32 静默吞非法输入 ✅ 已修复

- **描述**: `VfsFileType::from_u8` / `VfsSeekWhence::from_u32` 对非法值返回默认值，不返回错误
- **位置**: [vfs_types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs)
- **方案**: 改为返回 `Option<Self>` 或 `KernelResult<Self>`，调用方显式处理非法输入
- **修复**: 已完成 — `VfsFileType::from_u8` 和 `VfsSeekWhence::from_u32` 改为返回 `Option<Self>`，更新调用方：`ramfs.rs` 比较 `Some(VfsFileType::Dir)`，`api.rs::vfs_seek` 返回 `InvalidArgument`，`epoll.rs` 返回 0 (无事件)，测试断言更新为 `Some(...)` / `is_none()`
- **状态**: [X]

### M7: Process + UserProcess 双结构同步负担 ✅ 已修复

- **描述**: 进程信息分散在两个结构体中，需要同步维护；新增字段时容易遗漏
- **位置**: [process.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs) + [user_proc.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs)
- **方案**:
  - 短期: `UserProcess` 只保留用户态特有字段 (页表/栈)，通用字段统一放 `Process`
  - 长期: 合并为单一 `Process`，用户态信息用 `Option<UserContext>` 字段
- **修复**: 已完成 — 删除 UserProcess 6 个镜像字段 (pid/pwm/cr3/kernel_stack/user_stack/state) + 3 个同步方法 (sync_from_process/sync_to_process/check_sync)，UserProcRef 所有共享字段访问器委托到 Process 权威字段，消除双向同步负担
- **状态**: [X]

### M8: mount_fs 错误映射粗糙 ✅ 已修复

- **描述**: `mount_fs` 所有非零 `rc` → `KernelError::Io`，丢失具体错误语义
- **位置**: [fs/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/mod.rs)
- **方案**: 根据 framework 返回码细分到对应 `KernelError` 变体
- **修复**: 已完成 — framework 层统一返回负 errno 格式（`KernelError::as_i32()`），services 层使用 `KernelError::from_i32(-rc)` 精确映射，保留具体错误语义（如 `NotSupported`/`Io`/`Busy` 等）
- **状态**: [X]

### M9: 中间页表页权限过宽 ✅ 已修复

- **描述**: 新分配的 PDPT/PD/PT 页默认映射为 `PRESENT|WRITABLE`，无 `NO_EXECUTE` 位；如果 PML4 入口被意外映射为 USER，用户态可执行中间页表页中的代码
- **位置**: [vmm_x86_64.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_x86_64.rs)
- **方案**: 中间页表页添加 `NO_EXECUTE` 位 (bit 63)，减少攻击面
- **修复**: 已完成 — 在 `get_or_create_table_entry` 函数中，新创建的中间页表页标志位从 `PRESENT | WRITABLE` 改为 `PRESENT | WRITABLE | NX`，防止用户态执行页表页代码
- **状态**: [X]

---

## 低优先级 (🟢 改进建议)

### L1: error.rs 缺 `#![deny(unsafe_code)]` ✅ 已修复

- **描述**: 虽父模块 `services/mod.rs` 有此声明 (编译期覆盖)，但违反每个文件头部声明的惯例和 AGENTS.md §6 F1 的字面要求
- **位置**: [error.rs](file:///home/anfer/Code/QueenX/src/kernel/services/error.rs)
- **方案**: 文件头部添加 `#![deny(unsafe_code)]`
- **修复**: 已完成 — 在 error.rs 文件头部添加 `#![deny(unsafe_code)]` 声明，符合 AGENTS.md §6 F1 要求
- **状态**: [X]

### L2: CFS vruntime 溢出未处理 ✅ 已修复

- **描述**: `vruntime` 为 `u64`，理论上可溢出回绕，导致调度顺序错误
- **位置**: [cfs.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cfs.rs)
- **方案**: 使用 `checked_add` 或定期归零最小 vruntime (与 Linux CFS `min_vruntime` 一致)
- **修复**: 已完成 — 在 scheduler.rs 中使用 `saturating_add` 替代裸 `+` 操作，防止 vruntime 和 sum_exec_runtime 溢出
- **状态**: [X]

### L3: dispatch.rs 注释中被注释掉的 DEBUG 代码 ✅ 已修复

- **描述**: 被注释掉的 `klog_info!` 调试代码，违反代码整洁原则
- **位置**: [dispatch.rs](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs) (约 L58-61)
- **方案**: 删除被注释掉的 DEBUG 代码
- **修复**: 已完成 — 检查确认文件中无被注释掉的 DEBUG 代码，代码整洁符合规范
- **状态**: [X]

### L4: FileSystem trait 26 方法过多 ✅ 已修复

- **描述**: 实现方需实现全部 26 个方法 (即使大部分返回 NotSupported)，增加实现负担
- **位置**: [vfs_types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs)
- **方案**: 拆分为核心 trait (open/read/write/close/mkdir/readdir/stat) + 可选 extension trait
- **修复**: 已完成 — FileSystem trait 拆分为核心方法（必须实现）+ 扩展方法（默认返回 NotSupported），减少实现负担
- **状态**: [X]

### L5: proc/mod.rs 注释路径错误 ✅ 已修复

- **描述**: 注释中写 `kernel::crate::kernel::framework::proc::types`，路径格式有误
- **位置**: [proc/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/mod.rs) (约 L6)
- **方案**: 修正为 `crate::kernel::framework::proc::types`
- **修复**: 已完成 — 修正注释中的路径错误，从 `kernel::crate::kernel::framework::proc::types` 改为 `crate::kernel::framework::proc::types`
- **状态**: [X]

### L6: Mutex 实为自旋锁，缺少文档说明 ✅ 已修复

- **描述**: 当前 `Mutex` 本质是自旋锁 (持锁期间 CPU 空转)，与名称暗示的"睡眠锁"不符
- **位置**: [spinlock.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/sync/spinlock.rs) (Mutex 定义)
- **方案**:
  - 短期: `Mutex` 文档注释中明确说明当前实现是自旋等待
  - 长期: 实现真正的睡眠锁 (等待队列 + 调度器 yield)
- **修复**: 已完成 — 更新 mutex.rs 文档注释，明确说明当前实现是"自旋 + yield"混合模式，非真正睡眠锁，并标注未来改进方向
- **状态**: [X]

---

## 系统调用覆盖

### 已迁移 (services 层, ~60 个)

文件 I/O (`open`/`close`/`read`/`write`/`lseek`/`pread64`/`pwrite64`/`readv`/`writev`)、文件系统操作 (`mkdir`/`rmdir`/`chmod`/`unlink`/`rename`/`symlink`/`*at`)、统计 (`stat`/`fstat`/`lstat`/`statx`)、进程 (`clone`/`exit`/`getpid`/`gettid`/`yield`/`nanosleep`)、IPC (`pipe`/`shmget`/`shmat`/`shmdt`/`msgget`/`msgsnd`/`msgrcv`/`semget`/`semop`)、信号 (`tgkill`/`setrlimit`)、内存 (`brk`)、文件控制 (`fcntl`/`ioctl`/`sendfile`/`splice`)、WASI (`fd_*`)

### 框架层已有 (~10 个)

`execve`、`wait4`、`epoll_create`/`epoll_ctl`/`epoll_wait`、`mmap`、`set_tid_address`、`set_robust_list`

### 框架层存根 (返回 ENOSYS)

| syscall | 描述 | 位置 |
|---------|------|------|
| `mremap` | 重新映射内存区域 | 框架层存根 |
| `mprotect` | 修改内存保护属性 | 框架层存根 |
| `munmap` | 取消内存映射 | 框架层存根 |
| `io_uring_setup`/`enter`/`register` | 异步 I/O 引擎 | 框架层存根 |
| `rt_sigreturn` | 信号返回帧恢复 | 框架层存根 (aarch64 预留分支) |
| `credo_disk_install` | Credo 磁盘安装 (kernel_test 模式下) | 框架层存根 |
| Socket 系列 (net feature 关闭时) | `socket`/`bind`/`listen`/`accept`/`connect`/`sendto`/`recvfrom` | 框架层 ENOSYS |

### 关键缺口

| 类别 | 缺项 | 影响 |
|------|------|------|
| 内存管理 | `mmap`/`munmap`/`mprotect` 框架层已有完整实现 | `brk` 可行，但动态加载器需 mmap |
| 进程管理 | `execve`/`wait4` 框架层已有实现 | 用户进程完整生命周期已可闭环 |
| io_uring | 全部存根 | 高性能异步 I/O 不可用 |
| 信号投递 | `rt_sigreturn` 框架层存根 | 用户态信号处理器返回不可用 |
| 信号 | 发送/注册/屏蔽基础设施已有 | 信号投递到用户态 handler 不可用 |

---

## 远期规划 (非当前阶段)

| 项目 | 描述 | 预估工作量 |
|------|------|-----------|
| F1 | mdBook 文档体系 (5 部分：handbook/services/architecture/rfcs/contributing) | 2 周 |
| F2 | RISC-V 架构支持 (Sv39 + OpenSBI + PLIC + 调度切换) | 6-8 周 |
| F3 | TDX 机密计算 (CPUID 检测 + tdcall 封装 + attest + 内存加密) | 4-6 周 |
| F4 | NFS 网络文件共享 (客户端/服务器，services 层 safe Rust 实现) | 6-8 周 |

- **详情**: [future-roadmap.md](file:///home/anfer/Code/QueenX/docs/plan/future-roadmap.md)

---

## 已修复问题 (历史追溯)

| # | 问题 | 修复 | 日期 |
|---|------|------|------|
| F1 | RSP0 栈页使用 `map_page_in_table` 触发 KPTI 安全门控 | 改用 `map_kernel_page_in_table` | 2026-07-27 |
| F2 | 内核栈物理地址未 identity mapping，CR3 切换后 #PF | 添加 identity mapping | 2026-07-27 |
| F3 | iretq 帧所在页未映射在用户页表 | 映射 `(kstack-40)` 所在页 | 2026-07-27 |
| F4 | 用户态执行 IO 指令触发 #GP | 移除 enter_user_asm 中用户态诊断 out | 2026-07-27 |
| F5 | 初始 RSP 指向 guard page (未映射) | 修正为 `stack_virt + GUARD + SIZE - 8` | 2026-07-27 |
| F6 | isr_common 诊断代码在 KPTI CS 检查前 push | KPTI CS 检查移至 push 前 | 2026-07-27 |
| F7 | syscall_entry 入口处修改 RAX | 入口处绝对禁止修改通用寄存器 | 2026-07-27 |
| F8 | PerCpuGdt 缺少 `#[repr(C)]`，GDT 条目被栈 push 覆盖 | 添加 `#[repr(C)]` | 2026-07-29 |
| F9 | aarch64 GICv3 初始化挂死 (gicr_read WAKER) | QEMU 添加 `-machine virt,gic-version=3` | 2026-07-29 |
| F10 | aarch64 `msr daif` 指令挂起 | 改用 `msr daifclr, #2` | 2026-07-29 |
| F11 | aarch64 UART/GIC 地址在 TTBR0 切换后不可访问 | 迁移至 TTBR1 高半区地址 | 2026-07-29 |
| F12 | aarch64 TTBR1_EL1 指向 L0 表，T1SZ=16 下遍历故障 | TTBR1_EL1 直接指向 L1 表 | 2026-07-29 |
| F13 | aarch64 删除冗余 TTBR1_L0 4KB 死表 | T1SZ=16 硬件从 level 1 开始，无需 L0 | 2026-07-29 |

---

## 统计

| 维度 | 总数 | 已完成 | 待修复 | 完成率 |
|------|------|--------|--------|--------|
| 高优先级 | 5 | 5 (H1-H5) | 0 | 100% |
| 中优先级 | 9 | 9 (M1-M9) | 0 | 100% |
| 低优先级 | 6 | 6 (L1-L6) | 0 | 100% |
| **合计** | **20** | **20** | **0** | **100%** |
| 已修复 (历史) | 13 | 13 | 0 | 100% |

**推荐修复顺序**: ~~H2 (FPU 保存)~~ → ~~M1 (double-free)~~ → ~~M2 (buddy order)~~ → ~~M3 (网络锁)~~ → ~~M4 (syscall 分发)~~

**更新日期**: 2026-07-31

---

> 审查日期: 2026-07-27；本文创建: 2026-07-29
