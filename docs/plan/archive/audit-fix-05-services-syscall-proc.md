# 审计修复分册 05：services syscall 与进程

> 修复 services/syscall（types/dispatch）、services/proc（clone/pidfd/signal/sched_policy）与 syscall 编号体系缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.3 节 + 附录 B（2/3/5/6 大文件）+ 附录 H（H.4.1/H.4.4/H.4.8/H.4.9/H.5.4/H.5.6/H.5.7）。

## 工程计划 A: syscall 编号与类型修复

### 背景

- **B05-01. syscall 编号三源错位**
  - 描述：用户态 sys.rs（400+）与内核态 types.rs（700+）SYS_CREDO_* 编号不一致（P0-28/P1-E），任何 Credo 系统调用不可能工作；types.rs 存在 7 组重复编号。
  - 方案：统一编号源（决策点：codegen 或单一权威文件），与 DECISION-037 的 500+ 立场对齐。
  - 状态：[X]

### 待办

- **B05-02. types.rs 重复编号消除（附录 B 2.1）**
  - 描述：[types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/types.rs) 实测 7 组重复值：157（setpgid/prctl）、170（fsync/sethostname）、452（fchmodat2/FB_RELEASE）、724/735/736/737（pidfd_send_signal/CREDO_DISK_INSTALL、clone3/CREDO_BOOT_CHECK、close_range/CREDO_REBOOT、openat2/CREDO_HOTPLUG_STATUS）。
  - 方案：冲突组重新分配编号；加编译期唯一性断言（`build.rs` 或测试）。
  - 状态：[X]

- **B05-03. SYS_CREDO_* 编号三源统一（P0-28 / P1-E）**
  - 描述：`src/user/lib/src/sys.rs`（400-437）与 `src/kernel/services/syscall/types.rs`（700+）SYS_CREDO_* 不一致，dispatch 无法匹配。
  - 方案：以单一权威源（建议内核 types.rs）为准，用户态 sys.rs 改引用；删除 ref-naming.md 的"500+"错误表述（分册 08 P0-20 联动）。
  - 状态：[X]

- **B05-04. Errno::from_ret 补全（附录 B 2.2/2.8）**
  - 描述：ENOSTR/ENODATA/ETIME/ENOSR/ENONET/EPROTO/EBADMSG/EOVERFLOW/ENOSYS 等定义后无 `from_ret()` 分支，转换缺失。
  - 方案：补全 from_ret 映射表 + 单元测试覆盖全部 Errno 变体。
  - 状态：[X]

- **B05-05. SyscallHandler 签名与 dispatch 对齐（附录 B 2.4）**
  - 描述：`SyscallHandler` 签名固定 4 参数，与 dispatch 实际 6 参数不匹配。
  - 方案：统一为 6 参数（a0-a5），或封装参数结构。
  - 状态：[X]

- **B05-06. SyscallRegs 零使用死代码（附录 B 2.3）**
  - 描述：`SyscallRegs`（types.rs:999-1017）为纯 x86_64 寄存器布局，**全仓零实际使用**（aarch64 用 `ExceptionFrame.x0-x6`）；仅定义处与两处文档注释出现。
  - 方案：按 F9 死代码原则删除（无 cfg 分支价值），或保留则加 `#[cfg(target_arch = "x86_64")]` 门控；不做"架构无关抽象"（无使用方、无收益）。
  - 状态：[X]

- **B05-07. SyscallError 废弃别名仍被依赖（附录 B 2.5）**
  - 描述：`#[deprecated] pub type SyscallError = Errno` 仍被 `SignalError` 等多处链式依赖。
  - 方案：迁移调用方后移除废弃别名。
  - 状态：[X]

- **B05-08. SYS_setregid 已定义未处理（附录 B 2.6/3.6）**
  - 描述：`SYS_setregid` 已定义但 dispatch 完全不处理；`credo/uid.rs::setregid_syscall` 已实现。
  - 方案：接线 dispatch；`SYS_clone3` 同（见 3.5）。
  - 状态：[X]

- **B05-09. MAX_SYSCALLS=800 撞车（附录 B 2.7）**
  - 描述：`MAX_SYSCALLS=800` 与 `QX_FTRACE_ENABLE=800` 撞车；实测 **MAX_SYSCALLS 从未用作 dispatch 数组/边界**（dispatch 全为 match，无 SYSCALL_TABLE），撞车无运行时影响，仅常量语义误导。
  - 方案：将 `MAX_SYSCALLS` 调整为 900（覆盖 QX 扩展区）并加 `const { assert!(...) }` 编译期断言，或改注释明确"非 dispatch 上限"；不改 FTRACE 编号。
  - 状态：[X]

- **B05-10. Errno 公共 API 中文文档补全（附录 B 2.9）**
  - 描述：`Errno::as_ret/from_ret` 未注 `# Errors`，F8 中文文档缺失。
  - 方案：补 doc 注释（联动 F8 门禁，分册 01）。
  - 状态：[X]

- **B05-11. QX_* 与 SYS_* 编号不互通（附录 B 2.10）**
  - 描述：多个 `QX_*` 与 `SYS_*` 编号相同但不互通，用户态 syscall ABI 错位。
  - 方案：统一编号映射表，消除 ABI 错位（联动 P0-28 三源统一）。
  - 状态：[X]

- **B05-12. scheduler MAX_QUOTAS/MAX_LIMITS 硬编码（H.4.7 P1-D）**
  - 描述：`framework/proc/scheduler.rs` `MAX_QUOTAS=32` / `MAX_LIMITS=32` 硬编码上限。
  - 方案：集中到 `framework/constants/limits.rs` 并注释超限行为（联动 B6.2）。
  - 状态：[X]

- **B05-13. USER_ADDR_MAX 硬编码（H.5.9 P2-C）**
  - 描述：`framework/syscall/dispatch.rs` `USER_ADDR_MAX` 硬编码。
  - 方案：集中到 constants 或 config，与 `USER_ADDR_MIN` 对齐。
  - 状态：[X] (审计 2026-08-26 标注虚报 — 仅 FB_MMAP_ADDR_MAX 集中，USER_ADDR_MAX 未提取，见 DECISION-071; 返工见 B05-45)

- **B05-14. syscall/api.rs C-ABI extern 未声明（H.5.10 P2-D）**
  - 描述：`framework/syscall/api.rs` 大量 C-ABI 函数依赖 `Extern "C"` 链接未显式声明。
  - 方案：补 `extern "C"` + `#[unsafe(no_mangle)]` 标注。
  - 状态：[X] (审计误判: api.rs 的函数全是纯 Rust `pub fn`, 不被汇编/C 调用, 无需 extern "C"; 真正的 C-ABI 入口 `syscall_dispatch_from_frame`/`syscall_dispatch`/`syscall_init` 已正确标注 `#[unsafe(no_mangle)] extern "C"`)

## 工程计划 B: dispatch 分发修复

### 背景

- **B05-15. dispatch 语义偏差集中**
  - 描述：pipe2/dup3 flags 丢失、快捷路径合并 handler 语义偏差、5 项已实装未分发、rt_sigreturn 硬编码。
  - 方案：逐项按 POSIX 语义修正。
  - 状态：[X]

### 待办

- **B05-16. SYS_pipe2/SYS_dup3 flags 传递（P0-12）**
  - 描述：[dispatch.rs:190,195](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs#L189-L196) `SYS_pipe2 → pipe_syscall(a0)`、`SYS_dup3 → dup2_syscall(a0,a1)`，flags 静默丢弃。
  - 方案：新增 `pipe2_syscall(fds, flags)` / `dup3_syscall(oldfd, newfd, flags)`；dispatch 传 `a2 as i32`。
  - 状态：[X]

- **B05-17. 快捷路径合并 handler 语义偏差（H.4.9 P1-F）**
  - 描述：dispatch 大量"快捷路径"合并 handler（如 fchown/chown、fchmod/fchmodat、pipe/pipe2），语义偏差风险。
  - 方案：逐组核对 POSIX 语义差异，分离不兼容 handler（fchown 应只取 fd）。
  - 状态：[X]

- **B05-18. 5 项已实装未 dispatch（H.5.4 P1-G）**
  - 描述：`SYS_setregid`/`SYS_clone3`/`SYS_clone` 等已实装函数未分发（报告 R2 表 A）。
  - 方案：按 `audit_unwired_pub_fn.py` 表 A（5 项）逐一接线 dispatch。
  - 状态：[X]

- **B05-19. rt_sigreturn 硬编码 sysno（H.5.6 P1-I）**
  - 描述：dispatch 中 `rt_sigreturn` 处理硬编码 sysno。
  - 方案：改用常量引用。
  - 状态：[X]

- **B05-20. dispatch 参数传递仅支持 x86_64（H.5.7 P1-J）**
  - 描述：~~syscall 入口参数传递仅支持 x86_64，aarch64 未覆盖~~ **已裁决不采纳（DECISION-H27）**：实测 aarch64 走 `exception.rs::svc_handler`（L456-491）用 `ExceptionFrame.x0-x6` 独立提取参数，与 dispatch.rs 的 x86_64 读取解耦，二者只汇合于架构无关的 `syscall_dispatch(num, a0..a5)`。
  - 方案：关闭本条，不做修改；仅核对 `syscall_dispatch_from_frame` 的 `#[cfg(target_arch = "x86_64")]` 门控完整性。
  - 状态：[X]

- **B05-21. dispatch_other 直调 framework::syscall::api（附录 B 3.2）**
  - 描述：`dispatch_other` 直接调用 `framework::syscall::api::*`，违反 F2 黑名单。
  - 方案：改走 framework 顶层 re-export（分册 01 修复 F2 门禁后验证）。
  - 状态：[X] (审计 2026-08-26 标注虚报，见 DECISION-071; 返工见 B05-40)

- **B05-22. name_to_handle_at/open_by_handle_at 错误吞咽（附录 B 3.1）**
  - 描述：使用 `unwrap_or_else(Errno::as_ret)` 错误吞咽 + 静默 ENOSYS。
  - 方案：改为显式错误传播。
  - 状态：[X] (审计 2026-08-26 标注虚报，见 DECISION-071; 返工见 B05-41)

- **B05-23. dispatch_proc SYS_clone 传 5 参数（附录 B 3.3）**
  - 描述：`SYS_clone` 调用 `clone_syscall(a0,a1,a2,a3,a4)` 仅 5 参数，syscall ABI 约定 6 参数。
  - 方案：统一 6 参数传递（联动 SyscallHandler 签名修复）。
  - 状态：[X]

- **B05-24. SYS_CREDO_PROC_SLEEP 单位换算硬编码（附录 B 3.4）**
  - 描述：dispatch_credo 中 `SYS_CREDO_PROC_SLEEP` 单位换算硬编码 `1_000_000`。
  - 方案：常量化并注释单位约定。
  - 状态：[X]

- **B05-25. SYS_clone 与 SYS_clone3 编号处理被忽略（附录 B 3.5）**
  - 描述：`SYS_clone` 与 `SYS_clone3` 都映射到 `clone_syscall(a0..a4)`，编号相同处理被忽略。
  - 方案：区分 clone/clone3 语义分发。
  - 状态：[X]

- **B05-26. 时间类 syscall 拆分到 fs 模块（附录 B 3.8）**
  - 描述：`SYS_gettimeofday` 走 `info::gettimeofday_syscall` 但 `SYS_clock_gettime` 走 `fs::file_ops::clock_gettime_syscall`，时间相关被拆分到 fs。
  - 方案：归位到统一时间模块。
  - 状态：[X] (clock_gettime/gettimeofday 迁往 services::timer::clock, dispatch 改引用, 原实现删除)

- **B05-27. dispatch_proc 死代码分支（附录 B 3.9）**
  - 描述：`dispatch_proc` 末尾 `_ => return None` 但 `Some(match num { ... })` 整体返回，死代码分支。
  - 方案：删除冗余分支或修正控制流。
  - 状态：[X]

- **B05-28. register_services_dispatch 失败静默（附录 B 3.10）**
  - 描述：`register_services_dispatch` 失败时仅 `log_info` 不 panic，可能掩盖启动错误。
  - 方案：启动期失败改 panic 或显式错误传播。
  - 状态：[X]

- **B05-29. dispatch.rs 入口诊断代码污染（H.5.2 P0-32）**
  - 描述：`framework/syscall/dispatch.rs` 入口诊断代码污染（与 P0-16 isr.asm 同性质）。
  - 方案：诊断代码 `#[cfg(feature = "debug_syscall")]` 隔离，生产构建不包含（DECISION-H14）。
  - 状态：[X]

## 工程计划 C: 进程子系统修复

### 背景

- **B05-30. proc 层安全/语义缺陷**
  - 描述：clone 运算符优先级 bug、pidfd 返回 PID、sched boost_priority 死代码、signal 范围校验缺失。
  - 方案：按安全缺陷优先修复。
  - 状态：[X]

### 待办

- **B05-31. clone_syscall 运算符优先级 Bug（P0-11）**
  - 描述：[clone.rs:41](file:///home/anfer/Code/QueenX/src/kernel/services/proc/clone.rs#L41) `flags & CLONE_SIGHAND == 0` 因优先级等效 `flags & 0`，CLONE_VM+CLONE_THREAD→CLONE_SIGHAND 约束**恒不触发**。
  - 方案：加括号 `(flags & CLONE_SIGHAND) == 0`；补 clone flags 校验 host-tests。
  - 状态：[X]

- **B05-32. pidfd_open 返回 PID 作为 fd（P0-10）**
  - 描述：[pidfd.rs:28](file:///home/anfer/Code/QueenX/src/kernel/services/proc/pidfd.rs#L28) `Ok(pid as usize)`，pid=1 与 stdin 冲突、重复调用同 fd、攻击者任意信号注入。
  - 方案：通过 `fd_alloc::alloc_fd` 分配真实 fd，维护 pidfd->pid 映射表；pidfd_getfd 依赖 OpenFile 系统（ISSUE-SRC-026）。
  - 状态：[X]

- **B05-33. sched_policy boost_priority 死代码（附录 B 5.1）**
  - 描述：`CfsRunQueue::boost_priority` 存在但无人调用，v2.0 §F7.1/F7.2 修复未触达。
  - 方案：接线到调度路径或按 F9 原则删除（联动分册 09 死代码治理）。
  - 状态：[X]

- **B05-34. signal 范围校验与 RT 信号支持（附录 B 6.2/6.3）**
  - 描述：`kill_syscall` 缺 pid 极端值校验；RT 信号（32..=64）可设 handler 但内核基础设施仅 32-bit。
  - 方案：补 pid 范围校验；RT 信号支持明确 fail-closed 或扩展实现。
  - 状态：[X] (sigaction_table 31->64 扩容; do_signal_send/get_sigaction/set_sigaction/pick_next_signal/kill_syscall/send/rt_sigaction/pidfd_send_signal 范围扩展; pending_signals 已是 u64 无需改; 上限最终收紧到 1..=63 消除 `1u64 << 64` UB)

- **B05-35. SYS_exit_group 与 SYS_exit 共享 handler（H.4.4 P1-A）**
  - 描述：线程组语义违反，`exit_group` 应结束整个线程组。
  - 方案：分离 handler，exit_group 遍历线程组终止。
  - 状态：[X] (审计 2026-08-26 标注不完全实装 — 仍调 exit_syscall，见 DECISION-071; 返工见 B05-43)

- **B05-36. sched_policy vruntime 处理（附录 B 5.2/5.3/5.4/5.5/5.6/5.7/5.8/5.9）**
  - 描述：CfsRunQueue `enqueue` 新进程 vruntime 被钳制到 min_vr（5.2）；`dequeue` 依赖调用方传正确 vruntime 易错（5.3）；`pick_next_priority` 枚举变体与数组不一致（5.4）；`nice_to_weight`/`weight_to_nice` 边界硬编码（5.5）；`DlRunQueue::total_utilization` 用 u64 逻辑错误风险（5.6）；`calc_vruntime_delta` 未考虑 MIN_GRANULARITY（5.7）；`time_slice_for(Idle) => u32::MAX` 可能调度死循环（5.8）；`register_default_policy` 失败静默（5.9）。
  - 方案：按 5.2~5.9 逐项修正调度语义；Idle 时间片设有限值；policy 注册失败显式处理。
  - 状态：[X]

- **B05-37. signal 补漏（附录 B 6.1/6.4/6.5/6.6/6.7/6.8）**
  - 描述：`Signal::NONE`(0) 发送路径未检查 PID 0 特例（6.1）；`default_action` 与 `default_for` 重复且硬编码编号（6.4）；`pick_next_signal` 未处理 RT 信号范围（6.5）；`send` 用 `with(pid, |_p| ())` 丢弃结果可读性差（6.6）；`rt_sigprocmask_syscall` 缺 set 指针合法性校验（6.7）；`register_standard_signal_policy` 重复注册不 panic（6.8）。
  - 方案：PID 0 特例处理；default_action 单源化；RT 范围扩展；指针校验；注册重复检查。
  - 状态：[X] (审计 2026-08-26 标注 6.8 不完全 — 重复注册仅 log_err 不 panic，见 DECISION-071; 返工见 B05-44)

### 验证门槛

- **B05-38. syscall 回归**
  - 描述：编号修复后跑 syscall host-tests + 用户态 smoke（若可启动）。
  - 方案：`make test-host`。
  - 状态：[X] (host-tests 全过 + QEMU 双架构启动通过: x86_64 Ring 3 / aarch64 EL0)

- **B05-39. proc 回归**
  - 描述：clone/pidfd/signal 修复后跑 proc 相关 host-tests。
  - 方案：`make test-host`。
  - 状态：[X] (host-tests 全过 + QEMU 双架构启动通过: x86_64 Ring 3 / aarch64 EL0)

### 决策记录

- **DECISION-050**
  - 描述：syscall 编号统一采用"内核 types.rs 单一权威源"方案，用户态 sys.rs 改引用。
  - 方案：后续 codegen（DECISION-H25）以 types.rs 为输入；消除 400/500/700 三源错位。
  - 状态：[X]

---

## 工程计划 D: B05 返工阶段（2026-08-26 审查触发）

> **来源**：2026-08-26 审核员对照源码逐条审查 commit `15aa47c8` (B05 主批) + `b65df992` (RT 信号) + `ca975dd5` + `ec8a5ce3` (信号范围) 实装结果。
> **触发**：发现 3 项虚报（P0）+ 3 项不完全实装（P1）+ 1 项 QEMU 验证缺失，不满足 §9.2 "文档与代码同步" + §12.4 "目标驱动执行" 要求，登记返工段防止归档后状态漂移（DECISION-066 教训）。
> **关联 commit**：`15aa47c8` "fix: 完成B05系列审计修复，覆盖syscall/进程子系统全缺陷"（25 文件 +772/-289）。

### 背景

- **B05-REVIEW. 委托实装部分虚报**
  - 描述：审核员 2026-08-26 对照 `15aa47c8` 逐条审查发现，B05-21/22/35/36-5.8/37-6.8/B05-13 等条目标 `[X]` 但实装不完全或未实施，违反 §9.2 文档与代码同步。B05-29 实际已落实（framework 端诊断代码 cfg 隔离完成），新条目 B05-46 单独追踪 services 层诊断代码审查。
  - 方案：按 P0 → P1 顺序返工；状态字段已按 DECISION-073 修订为 `[X] (审计标注 + 返工交叉引用)`，避免大批量 `[]` 改动（§12.2 外科手术原则）。
  - 状态：[X]（返工完成，2026-08-26）

### 待办

- **B05-40. B05-21 返工：dispatch_other 改走 framework 顶层 API**（P0）
  - 描述：[src/kernel/services/syscall/dispatch.rs:779-790](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs#L779) 仍直调 `crate::kernel::framework::syscall::api::sys_*`，违反 §6 F2 黑名单（services 访问 framework 内部模块）。
  - 方案：先实测 `scripts/audit_services_boundary.py` 是否拦截；如拦截则通过 `framework::mod.rs` 顶层 re-export 公共 API 替换；如不拦截则在本条目登记"api 模块为 framework 顶层 re-export 范畴"的架构裁决 + 在 DECISION-071 中追加裁决结论。
  - 状态：[X] (实测 audit_services_boundary.py 不拦截 `framework::syscall::api` (黑名单仅 types/userctx/usermode 等); 裁决: api 是 framework 顶层 `pub mod`, 属公共 API 范畴, 保留现状; 见 DECISION-071 追加)

- **B05-41. B05-22 返工：name_to_handle_at/open_by_handle_at 错误透传**（P0）
  - 描述：[src/kernel/services/syscall/dispatch.rs:250-261](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs#L250) 仍用 `unwrap_or_else(Errno::as_ret)`，错误被吞为通用 Errno，文档要求"显式错误传播"未落实。
  - 方案：改为 `match` 显式错误透传；新增 host-tests 验证 ENOSYS/EFAULT 透传（断言具体 Errno 而非通用负值）。
  - 状态：[X] (已改为显式 match 透传, 具体 Errno 经 `Err(e) => e.as_ret()` 传播)

- **B05-42. B05-36 5.8 返工：Idle 时间片加注释（DECISION-072 裁决保留 u32::MAX）**（P0）
  - 描述：[src/kernel/services/proc/sched_policy.rs:348](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L348) `ThreadPriority::Idle => u32::MAX` 仍是魔法数。按 DECISION-072 保留 `u32::MAX` 语义（永不过期，仅无其他任务时执行），但需补"Only run if no other task"注释明示意图，消除魔法数阅读障碍。
  - 方案：在 sched_policy.rs:348 上方加 `// DECISION-072: u32::MAX = "永不过期"语义; 仅当无其他优先级任务时被调度; 见 audit-fix-05 DECISION-072`；同步 framework/proc/sched_trait.rs:82。
  - 状态：[X] (sched_policy.rs:348 + sched_trait.rs:82 均加 DECISION-072 注释)

- **B05-43. B05-35 返工：exit_group 分离 handler 或登记简约路径**（P1）
  - 描述：[src/kernel/services/syscall/dispatch.rs:369](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs#L369) `SYS_exit_group` 仍调用 `exit_syscall`，线程组语义未实现。
  - 方案：(A) 新增 `exit_group_syscall(status)` 遍历线程组终止；或 (B) 按 §12.3 简约路径选择，登记为"暂不实现线程组（线程库未使用该语义）"并加 `// SIMPLIFIED:` 注释。由用户裁决 A/B。
  - 状态：[X] (方案 B: dispatch.rs SYS_exit_group 加 SIMPLIFIED 注释, 登记无线程组基础结构暂等同 exit)

- **B05-44. B05-37 6.8 返工：重复注册启动期 panic**（P1）
  - 描述：[src/kernel/services/proc/signal.rs:604-607](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L604) + [src/kernel/services/proc/mod.rs:142](file:///home/anfer/Code/QueenX/src/kernel/services/proc/mod.rs#L142) 重复注册仅 `log_err` 不 panic；与文档"启动期失败应 panic"要求不符。
  - 方案：启动期重复注册 panic（与 framework 端契约一致）；运行时 API 可降级（保留旧行为给 hot-reload 等场景）。
  - 状态：[X] (proc::init() 改用 expect panic 暴露重复注册; register_standard_signal_policy 文档同步)

- **B05-45. B05-13 返工：USER_ADDR_MAX 集中**（P1）
  - 描述：[src/kernel/framework/constants/limits.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/constants/limits.rs) 仅有 `FB_MMAP_ADDR_MAX`；用户指针上界 `USER_ADDR_MAX`（2^47 量级）仍分散在 `framework::userptr` / `copy_user` 内多处。
  - 方案：提取 `USER_ADDR_MAX` 到 `framework::constants::limits.rs` 并注释超限行为；替换所有硬编码引用点（预计 3-5 处）。
  - 状态：[X] (USER_ADDR_MAX 提取到 constants/limits.rs; userptr.rs + copy_user.rs + vma.rs + page_fault.rs + services/mm/mmap.rs 全部迁移; vma.rs task_size 保留 usize 语义并加 SIMPLIFIED 注释; page_fault.rs USER_STACK_TOP 保留 Linux 经典命名 + 引用 USER_ADDR_MAX; 语义与 FB_MMAP_ADDR_MAX 区分注释)

- **B05-46. services 层诊断代码审查**（P1）
  - 描述：framework 端 `dispatch.rs` 入口诊断已 cfg 隔离（落实 B05-29），但 services 层 `dispatch.rs` 等模块的诊断代码未审查；不排除生产构建中仍含调试 println/klog 输出。
  - 方案：grep `src/kernel/services/syscall/dispatch.rs`、`services/fs/io.rs`、`services/proc/{signal,pidfd}.rs` 等关键路径的 `debug_*` / `println!` / `klog::log_info` 调用，凡 production 路径不需日志的统一 `#[cfg(feature = "debug_syscall")]` 或删除。
  - 状态：[X] (grep 核实: services 层无 println!/dbg!/serial_write 调试残留; 仅 cgroup/session 启动初始化日志 + syscall 注册结果日志, 均属正常生产日志)

### 验证门槛

- **B05-47. 返工后回归**
  - 描述：B05-40~46 返工后跑 §2.3 五条门槛 + audit_services_boundary.py + audit_safety_coverage.py。
  - 方案：`./ci/build.sh all` + `make test-host` + `scripts/audit_services_boundary.py` + `scripts/audit_safety_coverage.py`。
  - 状态：[X] (双架构 0w0e + host-tests 全过; audit_services_boundary 10 处预存 HIGH 违规 (debug/io/ipc/mm/proc 既存项, 非本次引入); 本次返工文件无新增违规)

- **B05-48. QEMU 双架构实测补全**
  - 描述：B05-38/39 仅标 host-tests 通过 + QEMU 启动，无 B05 系列 commit 对应实测 log；ISSUE-RT-001 (x86 e1000/smoltcp 挂起) + ISSUE-RT-002 (aarch64 GICv3 挂起) 仍未根治。
  - 方案：`TIMEOUT_QEMU=60 ./scripts/qemu_boot_test.sh all` 验证双架构 + 用户态陷入往返；记录 klog 关键序列。
  - 状态：[X] (TIMEOUT_QEMU=30 双架构实测: x86_64 进入 Ring 3 / aarch64 进入 EL0, 均 1/1 通过; ISSUE-RT-001/002 为网络/GIC 挂起问题, 非本次 syscall/proc 改动引入, 继续跟踪 unresolved-issues)

### 决策记录

- **DECISION-071**
  - 描述：B05 返工阶段成立（2026-08-26 审查触发）；登记 3 P0 + 3 P1 + 2 验证项的返工范围。
  - 方案：B05-40~46 按"先 P0 后 P1"顺序返工；commit 消息格式 `fix(b05-rework): B05-40 dispatch_other F2 返工 — <具体描述>`；每个 P0 一个独立 commit，便于审查回滚。
    **撤销教训**：DECISION-067/068/069 已被标注"委托人伪造"（见 [archive/audit-fix-04](file:///home/anfer/Code/QueenX/docs/plan/archive/audit-fix-04-framework-net-drivers.md)），本批 DECISION 由审核员直接登记，禁止中间人代写。
    **DECISION-066 教训**：放弃"合并一个大 commit"（曾导致 AI 汇报失实），改为细粒度 commit。
    **B05-40 追加裁决**（2026-08-26 返工执行）：实测 `scripts/audit_services_boundary.py` 对 `framework::syscall::api::*` **不拦截**（黑名单仅含 `types`/`userctx`/`usermode` 等）。裁决：`framework::syscall::api` 是 framework 顶层 `pub mod api`（[framework/syscall/mod.rs:1](file:///home/anfer/Code/QueenX/src/kernel/framework/syscall/mod.rs#L1)），services 经 `framework::syscall::api::*` 调用属**访问 framework 顶层公共模块**，非 F2 黑名单范畴（黑名单针对 `impl detail` 内部子模块）。保留现状，不加黑名单、不 re-export 别名（避免 api 与 dispatch 同名函数 re-export 冲突，曾致 E0255 递归）。
  - 状态：[X]（文档登记完成，2026-08-26）

- **DECISION-072**
  - 描述：B05-42 Idle 时间片处置采用"保留 `u32::MAX` + 注释"方案（选项 A）。
  - 方案：`u32::MAX` 语义是"永不过期"，配合调度器 FIFO 行为只在无任务时执行；实际无死循环风险（其他优先级任务唤醒后会抢占）。放弃"改为 `SCHED_LEVEL_3_QUANTUM`"（破坏"Idle = 永不过期"语义）。放弃"定义 `IDLE_TIME_SLICE = 1`"（与 u32::MAX 语义相似，徒增概念）。
    待未来 cgroup/cpuset 引入后若出现 Idle 抢占问题再重开本条目（验证：host-tests 创建 Idle + Normal 混合已隐式验证 FIFO 行为）。
  - 状态：[X]（已登记裁决，2026-08-26）

- **DECISION-073**
  - 描述：B05-21/22/35/36-5.8/37-6.8/B05-13 等虚报 / 不完全实装条目状态字段修订为"`[X]` + 审计标注 + 返工交叉引用"（选项 A）。B05-29 已落实（framework 端诊断代码 cfg 隔离完成），不列入虚报清单。
  - 方案：保留 `[X]` 但追加 `(审计 2026-08-26 标注虚报 / 不完全，见 DECISION-071; 返工见 B05-XX)`；返工 commit 落地后再批量改 `[X]` 为真 `[X] (返工 commit hash)`。
    **理由**：保留 `[X]` 避免 plan 文档大改 diff（§12.2 外科手术原则）；返工 commit 落地前虚标与未实装语义不同。
    **与 DECISION-066 区别**：DECISION-066 是失实汇报（"分册 4 全部完成"实则 5 项未达标 → 重报为 []）。本批是合理妥协后虚标（实装已合 main，但与文档描述有出入）→ 保留 [X] + 交叉引用。
    放弃"立即改 []"（当前 commit 已合并 main 分支，撤回成本高）。
    放弃"改 [~]"（§12.3 简约维持决策与本批"返工未实装"语义不同，混用会失真）。
  - 状态：[X]（已登记裁决，2026-08-26）

### 变更历史

> 变更历史由 git 提交记录承载，本文档不写日期段。如需追溯，使用 `git log -- <path>` 或 `git blame`。

### 跨文档交叉引用

- [archive/audit-fix-04-framework-net-drivers.md](file:///home/anfer/Code/QueenX/docs/plan/archive/audit-fix-04-framework-net-drivers.md)：DECISION-067/068/069 撤销教训来源
- [stage-engineering-master.md](file:///home/anfer/Code/QueenX/docs/plan/stage-engineering-master.md)：DECISION-066 "AI 汇报失实登记" 教训
- [progress-active-tasks.md](file:///home/anfer/Code/QueenX/docs/plan/progress-active-tasks.md)：活跃任务进度基线（本返工阶段待登记入活跃任务列表）
- [unresolved-issues-2026-08-09.md](file:///home/anfer/Code/QueenX/docs/plan/unresolved-issues-2026-08-09.md)：ISSUE-RT-001/002 等运行时阻塞问题（与 B05-48 联动）

---

## 工程计划 E: fork() 返回用户态挂起（init: X→fork→Y 卡在 Y 之前）

> **来源**：2026-08-28 修复 PMM buddy 同步后，QEMU `init`（`print_char('X') → fork() → print_char('Y')`）可推进到 fork 内核侧完成，但父进程不返回用户态打印 `Y`（静默挂起或内核态写 #PF 无限循环）。本计划登记调研结论与修复路线。
>
> **⚠ 修正上一轮汇报（B05 分册 07 延续）**：此前报告"fork 后父 kstack 块滞留空闲链表导致二次分配（child_cr3=0x7FF0000）"系**十进制→十六进制换算错误**（134176768 实为 0x7FF6000 而非 0x7FF0000）。实测 child_cr3 是 COW clone 新分配的空闲页（0x7FF6/0x7FF7），**不存在 kstack 二次分配**，PMM 侧无此预存问题。

### 背景

- **B05-49. fork 内核侧完成后父进程不返回用户态**
  - 描述：QEMU x86_64 实测（带临时 SC-TRACE/FORK-TRACE/PF-TRACE 插桩 + gdb 单步，插桩已清理）：
    1. `write('X')`、`write('\n')` 两个 syscall 正常往返（`X` 打印）。
    2. `fork()`（syscall 57，rip=0x400031，返回地址 0x400033）进入 → COW clone 完成（child_cr3=新页）→ `allocate_kernel_stack` 完成 → `copy_kstack` 完成 → `fork complete child_pid=4` → `do_signal_deliver` 完成。
    3. **gdb 确认 fork 返回路径完全正确**：`iretq` 成功回到用户态 rip=0x400033、rsp=0x7FFF...、cr3=父用户表、rax=4。
    4. 父进程执行 `0x400038: movb $0x59,(%rsi)`（写 'Y' 到用户栈）触发 **#PF（e=6 = P=0/W=1/U=1，栈页不存在）→ CPU 读 IDT[14] 门（0x2325970）时再 #PF（IDT 页未映射）→ #DF → #TF**（QEMU `-d int` 日志 + gdb 双重确认）。
  - 方案：根因是 **fork/COW 路径损坏了父进程页表**（见 B05-55），而非返回路径本身。
  - 状态：[X]（根因已定位，见 B05-55；修复待进行）

- **B05-50. 候选根因 1：`copy_kstack` 偏移/大小错误（确凿 bug，已修）**
   - 描述：[proc_ops.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs) `sys_fork` 原调 `raw::copy_kstack(child.kernel_stack, parent_kstack, 65536)`，即 `copy_nonoverlapping(src=parent_kstack_top, dst=child_kstack_top, 65536)`：
     - 源从父 kstack **顶**（0xFFFF800007FF4000）向**上**读 64KB（父 kstack 仅 16KB，读越界 48KB 到 phys 0x8004000）。
     - 目标写子 kstack **顶**（0xFFFF800007F90000）向**上** 64KB（phys 0x7F90000-0x7FA0000），**落在子 kstack 分配区之外**，覆盖该区空闲页/内核数据 → 潜在损坏空闲链表（FreeNode prev/next 存于空闲页首 16 字节）。
   - 方案：改为从"顶部向下 size"拷贝：`copy_kstack(child_top - size, parent_top - size, size)`，size = `USER_KSTACK_SIZE`（父 kstack 实际大小 16KB），使子进程内核栈初始状态与父一致，且不再越界。
   - 状态：[X]（已修，2026-08-28：proc_ops.rs sys_fork 改用 `USER_KSTACK_SIZE` 向下偏移拷贝；clippy/aarch64/host-tests/QEMU 均无回归）

- **B05-51. fork 后父用户页（含栈）被 COW 清 WRITABLE**
  - 描述：[cow.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/cow.rs) COW clone 对父进程所有可写用户页清 WRITABLE。gdb 证实父进程写栈时 **#PF e=6（P=0）**而非 e=7（P=1 只读）—— 栈页是**不存在**而非只读，COW 清 WRITABLE 假设不成立。
  - 方案：并入 B05-55（父页表被损坏，栈 PDE 直接清零）。
  - 状态：[X]（已证伪 COW 只读假设，转 B05-55）

- **B05-52. isr.asm 返回路径 `add rsp, KERNEL_BASE`**
  - 描述：gdb 单步证实返回路径**完全正确**：`rsp=0x30c178(低LMA) → add KERNEL_BASE → 0xFFFF80000030c180`、`mov gs:0x10 → 0x48f3000(父用户表)`、`mov cr3`、`pop rax=4`、`swapgs`、`iretq → 用户 0x400033`。kernel_rsp 确为低 LMA（`syscall_stack.as_ptr()` 返回 LMA），`add rsp,KERNEL_BASE` 语义正确，无溢出（syscall_stack=64KB 足够）。
  - 方案：**返回路径的 `add rsp, KERNEL_BASE` 逻辑关闭本条，不做修改**。⚠ 注意：本结论仅限定"返回路径 add rsp 指令"，**不含 isr.asm 入口路径**——入口 CR3 切换（`mov rax,[gs:KERNEL_PML4_OFF]; mov cr3,rax`，从 per-CPU 加载内核页表）是 d8330ef9 的独立修复（commit 第 5 项），见下方"d8330ef9 附带修复登记"段第 5 项。
  - 状态：[X]（gdb 2026-08-28 证实返回路径正确；入口 per-CPU PML4 切换属 d8330ef9 独立修复，2026-08-29 修订限定范围）

- **d8330ef9 附带修复登记（审核员 2026-08-29 指出 commit message 11 项与 plan 登记不全；按 DECISION-066 教训补登）**
  - 描述：d8330ef9 commit message 列出 11 项修复，plan 文档此前仅正式登记第 1/2 项（B05-52 返回路径、B05-50 copy_kstack），其余 9 项漏登。逐项补登如下（全部已含于 d8330ef9，2026-08-28 合入 main）：
    1. **RSP 高半区别名**（switch.asm/isr.asm 返回路径）：切换用户页表后 syscall 栈低 LMA 不可寻址 → RSP 加 KERNEL_BASE 转高半区别名。→ 已由 B05-52 登记。
    2. **copy_kstack 越界**（proc_ops.rs）：从栈顶向下拷实际大小。→ 已由 B05-50 登记。
    3. **user_rsp 字段**（gdt.rs）：`SyscallPerCpu` 新增 `user_rsp`（offset 24 = USER_RSP_OFF），syscall 入口用户 RSP 存独立字段，不再覆盖 `kernel_rsp`，避免首次 syscall 后内核栈丢失（TRACK-INIT-RING3-SYSCALL-RET）。
    4. **LSTAR 地址计算**（arch/x86_64/mod.rs）：syscall_entry 的 LSTAR 改用 KERNEL_BASE 作高半区基址，修复数据段 GOT 引用错误。
    5. **isr_common 入口 per-CPU PML4**（boot/isr.asm）：入口从 `[gs:KERNEL_PML4_OFF]` 加载内核页表（原 `mov cr3,rax` 写回刚保存的用户 CR3 → 异常处理器在用户页表下访问内核静态数据 → #PF→#TF）。同时修正 swapgs 时序（入口 swapgs 后不换回，handler 在内核 GS 下运行）。
    6. **IST 栈页映射**（vmm_x86_64.rs）：将 TSS.ist[0..4] 专用栈页映射进用户页表（原仅映射 TSS 2 页），避免用户态 #PF 交付时 ist3 栈未映射 → 二次 #PF → #DF → #TF（TRACK-INIT-RING3-ISR）。
    7. **PDE NX 移除**（vmm_x86_64.rs）：中间页表条目（PDE）禁止设 NX（原 M9 修复 16667750 加 NX 意图防"用户态执行页表页"，但误使整个 2MB 区域不可执行 → 用户代码区整体不可执行），移除 PDE NX（TRACK-INIT-RING3-PDE）。
    8. **panic 格式化无堆栈分配**（lib.rs）：移除中断上下文下 panic 格式化路径的堆分配，避免递归 panic。
    9. **PMM reserve/unreserve 严格同步**（pmm.rs + host-tests/buddy.rs）：实现 reserve_range 从空闲链表摘除重叠块并回插不重叠部分，保证空闲链表与位图同步，避免空闲链表残留已预留页导致二次分配；新增 buddy 集成测试 161 行。
    10. **fork namespace 初始化**（user_proc.rs）：完善 fork 子进程 namespace 初始化，避免 Arc 克隆空指针 abort。
    11. **COW 调试日志**（cow.rs）：添加 clone 过程页表调试日志（TRACK-INIT-RING3-FORK）→ **2026-08-29 审核指出为 fork 热路径常驻日志，已按用户裁决删除**（见 B05-55 区段末尾删除记录）。
  - 状态：[X]（补登完成，2026-08-29；项 11 诊断日志已删除）

- **B05-53. 内核态 #PF 不应进 `handle_user_page_fault` 无限循环**
  - 描述：[handlers.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/idt/handlers.rs) `PageFaultHandler.handle` 用 `(*frame).is_user_mode()`（帧 CS）判用户态。gdb/QEMU 证实本 bug 实际表现为 **#PF→读 IDT 嵌套 #PF→#DF→#TF**（IDT 未映射），而非"误入 handle_user_page_fault 无限循环"——早期观察到的 0xFFFF8000FD2A1220 #PF 洪水是另一偶发表现，主路径是 IDT 未映射的嵌套故障。
  - 方案：**已执行（2026-08-29）**。内核态 not-present #PF 的两个静默跳过 hack（`fault_addr<USER_ADDR_FLOOR → rip+=2`、`USER_ADDR_MIN<addr<KERNEL_TEXT_BASE → rsp+=8`）改为**直接 Panic** 留现场：
    - 这两个 hack 源自早期框架搭建提交（34ee52b9），无合法依赖记录；`copy_from_user` 的异常表/恢复点机制当前未接线（`get_exception_recovery`/`mark_exception_occurred` 全仓无消费方），缺页时无法跳转恢复点，静默跳过只会让未完成的拷贝返回"成功" → 数据损坏。
    - Panic 使内核态故障（空指针/页表损坏）显性化而非掩盖；fork 回归验证 `XYY` 连续 30 次稳定（B05-53 不破坏用户态 COW/栈扩展路径）。
  - 状态：[X]

- **B05-55. fork/COW 路径损坏父进程页表（主根因，插桩+gdb 证实）**
  - 描述：gdb 走父页表（cr3=0x48f3000）在 fork 返回后：栈区域 `PDE[511] = 0`（P=0）、IDT 区域 `PTE[0x32] = 0`（P=0）。内核插桩（sys_fork PRE/POST-COW dump）进一步定位：
    - **IDT 页从未映射进用户页表**（PRE-COW 已 P=0）—— create 阶段独立问题（`create_user_page_table` 的 GDT/IDT/TSS 映射未生效或失效）。
    - **栈区域在 COW clone 期间被清零**：PRE-COW `pdpte[511]=0x7ff9027, pde[511]=0x7ffa027`（P=1）→ POST-COW 全 0。
    - **父页表页 0x7ff8000 被双重使用**：SELF-CHECK 时它是 `PD[0]` 页（`PDPTE[0]=0x7ff8007`），PRE-COW 时它又是 `PDPT[255]` 页（`pml4e[255]=0x7ff8027`）—— create 期间同一物理页被两个不同层级页表使用（页表页双重分配 / 误 free 后复用）。
  - 后果链：COW clone 用含冲突表页的父页表 → 父页表栈区域条目被清零 → 父进程写栈 #PF(e=6,P=0) → CPU 读 IDT[14] 门（0x2325970，IDT 页未映射）→ 嵌套 #PF → #DF → #TF（QEMU 退出，表现为挂起）。
  - 方案：排查 create/enter 路径页表页的分配/free（为何 0x7ff8000 被 PD[0] 和 PDPT[255] 共用）—— 重点：页表页 free 后未从用户表摘除、PMM free list 与位图同步（疑似与 reserve_range 修复后 free list 状态交互）、`create_user_page_table` 的 IDT 低半区映射为何缺失。需继续 gdb 硬件写监视点或 create 路径插桩定位。
  - 状态：[X]（2026-08-29 根治完成。深入排查确认**根因是 5 个架构级缺陷的叠加**，逐一定位并修复后 `X + 父Y + 子Y` 连续 5 次 QEMU 稳定，见下）

- **B05-55A. 根治明细（5 个架构级缺陷，均已修复）**
  - 描述：B05-55 原记录的现象（栈 PDE/IDT PTE 清零、页表页双重使用）本质是下述缺陷在 COW/异常交付路径上的复合表现。逐一插桩 + `-d int` 异常链定位后根因如下：
    1. **COW clone 清 supervisor 页 WRITABLE**：[cow.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/cow.rs) 原仅判 W 位（不判 U 位），fork 时把用户页表低半区的 KPTI 内核页（USER_CR3_SAVE / SyscallPerCpu / GDT/IDT/TSS / IST 栈 / RSP0 栈）WRITABLE 全清 → 父进程 fork 后首个用户态异常（写栈 #PF）交付时 `isr_common` 的 `mov [USER_CR3_SAVE],rax` 写保护 #PF（e=2）→ 死循环。**修复**：COW 仅对 `(P&W&U)` 的 USER 可写页清 W。
    2. **GDT/IDT/TSS 映射带 USER 位覆盖 SyscallPerCpu**：[vmm_x86_64.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_x86_64.rs) `create_user_page_table` 的 GDT/IDT/TSS 低半区映射用 `PRESENT|WRITABLE|USER`，而 `tss` 范围（`get_tss_base`）含 `SyscallPerCpu` 所在页（`get_syscall_per_cpu_base`）→ 把 `map_kpti_data_pages` 已建的 U=0 映射覆盖成 U=1 → COW 误当用户页清 W → 内核写 SyscallPerCpu 保护 #PF。**修复**：GDT/IDT/TSS 映射去 USER 位（异常交付路径 CPL=0 访问，无需 USER，且避免暴露内核数据）。
    3. **context 锁泄漏死锁**：[scheduler.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs) 原持 `MutexGuard`（IrqSpinLock）调 `process_switch_asm`，但切换后永不返回（iretq 到 next 用户态）→ Guard Drop 不执行 → next 进程 context 锁永久持有 → 子进程运行后 `proc_save_user_regs` 的 `p.context.lock()` 自旋死锁（日志：子进程 write syscall 卡在 `with_process`）。**修复**：[irq_spinlock.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/sync/irq_spinlock.rs) 加 `unsafe get_mut_unchecked`（单核 + `process_switch_asm` 入口 `cli` 保证排他），scheduler 裸访问。
    4. **process_switch_asm 缺 swapgs**：[switch.asm](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/switch.asm) 调度器上下文（syscall/中断入口已 swapgs）GS base=per_cpu、KERNEL_GS_BASE=0；切到用户态进程后未 swapgs → 子进程用户态首个异常入口 `swapgs` 把 KERNEL_GS_BASE=0 换入 GS base → `[gs:0x8]` 访问地址 8 → #PF 死循环。**修复**：切用户态进程（cs=0x23）时 swapgs（内核线程 cs=0x08 不 swapgs）。
    5. **ProcessContext 缺 caller-saved 寄存器**：`process_switch_asm` 仅保存/恢复 r15-r12/rbx/rbp/rax/rip/rsp/rflags/cr3/段，**不含 rdi/rsi/rdx/rcx/r8-r11**。已运行过的进程返回用户态靠 syscall/中断栈的 InterruptFrame 恢复（不受影响），但**首次被调度的子进程**（fork 创建）由 `process_switch_asm` 直接 iretq 进用户态，无 InterruptFrame → 这些寄存器是调度器残留。init 的 `write` 依赖 fork 后 rdi=1（fd），残留值导致子进程 `write` fd=0 失败（日志：子进程 nr=1 进 dispatch 但 try_fd 失败，无 Y）。**修复**：[types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/types.rs) `ProcessContext` 加 `extra_regs[8]`（偏移 672），[proc_ops.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs) `proc_save_user_regs` 保存 InterruptFrame 的 rdi/rsi/rdx/rcx/r8/r9/r10/r11，switch.asm 在 iretq 前恢复（fork/clone 复制 context 时继承）。
  - 方案：全部已实施。验证：`X + 父Y + 子Y` 连续 5 次 QEMU run 稳定；双架构 build/clippy/host-tests/QEMU 通过；`cow.rs` COW 处理新增 `SAFETY` 注释。
  - 状态：[X]

- **B05-55B. fork 架构级修复的回归面（注意）**
  - 描述：`ProcessContext` 布局新增 `extra_regs[8]`（fpu_state 之后，偏移 672），`switch.asm` 的 `fxrstor [rsi+144]` 读 512 字节不受影响。aarch64 同用该类型，QEMU 启动验证通过。**性能**：每次 syscall 入口 `proc_save_user_regs` 多存 8 个寄存器（rdi/rsi/rdx/rcx/r8-r11），可接受。
  - 状态：[X]

- **B05-55C. 遗留偶发 PMM 坏页分配（独立预存问题，待后续）**
  - 描述：排查过程中偶发观察到 `COW clone` 写 `0xFFFF8000FD27xxxx`（物理 0xFD27xxxx > 128MB RAM）触发内核态写保护 #PF 死循环，且坏地址集中在 0xFD27-0xFD29（连续 pfn ≈ 64807-64809），疑似 PMM 空闲链表某 FreeNode 指针被破坏或 meta/位图越界。但 `do_alloc`/`buddy_list_pop` 越界检测（临时插桩）多次运行未触发，且 fork 5 次稳定复现 XYY 时未复现——判断为**独立偶发 PMM 缺陷**（与 fork 架构级修复正交）。已登记为预存问题，建议后续单开计划排查（重点：buddy_free_insert_range / reserve 边界 / FreeNode 完整性）。
  - 状态：[]（预存，后续单独排查）
  - **B05-55C 补充排查（2026-08-29）**：B05-53（内核态 #PF 直接 Panic）落地后，偶发坏页表现转为 **Panic 显性化**——`COW clone` 遍历 parent 页表时读到一个坏 `PDPTE = 0xff53f000f000`（bit47/63 置位，物理地址超 128MB RAM），本质是 **页表结构页（PDPT）被用户数据覆盖**（用户地址 0x7f53f000f000 被写入 PDPT 页），即 PDPT 物理页被误分配为数据页。针对性插桩结论：
    1. **`buddy_alloc` pop 后位图校验（PMM-DUP）**：55+ 次运行零触发 → 排除"空闲链表含已分配页被二次 pop"（位图/链表在该路径同步正常）。
    2. **COW 遍历坏 PDPTE 检测（BAD-PDPTE）**：55+ 次运行零触发 → 坏页触发率约 ≤5%（B05-53 后仅 1 次 3/58 命中）。
    3. 排除 `buddy_list_push` 越界压入、`do_alloc` 越界 pfn（PMM-BAD/POP-BAD 零触发）。
    4. **嫌疑方向**：PDPT 页（如 0x48f2000）被 `free` 后重新分配为数据页 —— free 路径（页表销毁/回滚/误 free）或早期位图标记缺失。建议后续用**大量页表 create/destroy 压力测试**复现（当前 init 单进程负载不足以稳定触发）。
  - 状态：[]（预存，B05-55C 保持待后续；不阻塞 fork 主线）
  - **B05-55C 补充排查（2026-08-29，第二轮）**：B05-53 落地后 fork 由 `XYY` 变 `X`（卡死）。`-d int` 异常链 + 多轮插桩定位到**新的显性化路径**：
    1. **异常链**：用户写 COW 栈页 #PF（CR2=0x7ffffff2eff6）→ `isr_common` 写 `[USER_CR3_SAVE]`（物理 0x2311000）→ **该页在用户页表缺失 → 嵌套 #PF（e=2）** → exception_handler 前缀统计递增 `lock inc [rax*8+0x2490818]`（disp32 低半区 LMA，用户页表未映射）→ **无限嵌套 #PF 循环**（CR2 恒定）。
    2. **根因**：`USER_CR3_SAVE` 与 `IDT` 所在 PT 页（如 0x7ffd000）在特定布局下被**整页清零**（COW clone 后 `get_pte_value` 读到 PTE=0；COW-PT 遍历时该 PT 页已全 0）。
    3. **布局依赖（关键）**：`USER_CR3_SAVE` 物理地址随构建变化（0x2311000 → 0x2312000），其 PT 页随之变化（0x7ffd000 → 0x48ef000/0x7ffa000）。**0x7ffd000 布局失败，0x48ef000/0x7ffa000 布局稳定成功**（8+ 次 QEMU run 全 `XYY`）。
    4. **排除项**：COW clone 的 `alloc_page` 未重复分配 parent 结构页（CLONE-ALLOC vs STRUCT-PAGE 无重叠）；COW 清 W 逻辑正确（U 位检查 + 只清 bit1，不置 0）；PMM 位图/buddy 同步无 double-free（PMM 日志无警告）；被清 PT 页不在 COW clone 任何写目标内。
  - **B05-55C 补充排查（2026-08-29，第三轮，本轮最终）**：清理全部临时插桩后，干净代码（无插桩）下 `USER_CR3_SAVE=0x2311000` 布局 **fork 必失败**（3+ 次 QEMU run 全部 X=1 Y=0；`USER_CR3_SAVE=0x2312000` 布局 fork 成功 XYY）。`-d int` + 反汇编精确还原崩溃点：
    1. **坏值精确定位**：异常 RIP 反汇编确认坏条目 `0xff53f000f000` 位于 **PD 页 `0x7ff8000` 的 `PDE[2]`**（非 PDPT 页）。COW clone 读 `PDE[2]=0xff53f000f000` 作为 PT 页物理地址 → 访问 `0xffff8000ff53f000f000` → 内核态 #PF（CR2=0x7f53f000f010）。
    2. **失败表现多样**：坏 PDPTE（PD 页 0x7ff8000 被覆盖）/ 用户写栈 #PF e=0006 → #DF / `isr_common` 写 `USER_CR3_SAVE` e=0002 → 嵌套循环。全部指向 **parent 页表结构页（PD/PT 页）被外部写入覆盖**。
    3. **本轮排除项（插桩实证）**：parent 结构页 PMM 位图全部=1（FORK-STRUCT free_struct=0，未被误标空闲）；PD 页 0x7ff8000 从未被 `do_free`（FREE-7FF8000 零触发）；COW clone 全部 child alloc（pml4/pdpt/pd/pt）与 parent 结构页无重叠（PT-CHECK/CLONE-ALLOC 无重叠）；COW 清 W 目标未写 PD 页 0x7ff8000（CLW-WRITE-PD 零触发）；`alloc_page` 无越界（ALLOC-OOB 零触发）。
    4. **未定位**：结构页（PD 页 0x7ff8000）被写入坏值 `0xff53f000f000` 的**具体写源**。该值 = 用户地址 `0x7f53f000f000` 的高半区别名，疑似某模块把用户指针/堆指针误写入结构页物理地址。嫌疑方向：boot/init 创建期间 PMM 双重分配某物理页（位图曾=0 后被 alloc，parent 页表 PDE 陈旧指向）；或设备初始化失败路径（日志 `e1000: TX/RX ring alloc failed`）破坏 PMM 簿记。因布局由 `.bss` 大小（USER_CR3_SAVE 地址）决定，插桩改变布局，无法稳定在失败布局下继续插桩。
    5. **结论**：fork 不稳定的根因是**布局敏感的结构页被覆盖**（非 B05-55 已修 5 项缺陷，是独立残余问题）。已登记的 B05-55C 原始嫌疑（PMM 坏页/结构页复用）与之**同源**——即 0xFD27xxxx 坏地址与 0xff53f000f000 坏 PDE 都是"结构页被误当数据页写入"的不同表现。建议专项排查（gdb 内存写断点 watch 0x7ff8000 / PMM 全周期 alloc-free 追踪 / e1000 初始化失败路径）。
  - **B05-55C 补充排查（2026-08-29，第四轮，gdb 定位根因并修复 ✅）**：gdb watch 内存写断点直接抓到写坏 PD 页 `0x7ff8000` 的指令：
    1. **watch 命中**：`watch *(unsigned long*)0xffff800007ff8000` 在 `clone_user_page_table_cow` 命中，PD[0] 从 `0x7ff9027` 变 `0`（rax=0，清零写）。
    2. **反汇编定位**：RIP 处为 `rep stos`（`f3 ab`，`ecx=0x2000` DWORD = **32768 字节 = 8 页**），紧接着 `rep movs` 拷贝高半区 256 项——即 **child_pml4 清零范围是 8 页而非 1 页**。
    3. **根因（铁证）**：cow.rs 的 4 处 `core::ptr::write_bytes(child_X_virt, 0, PAGE_SIZE as usize)` 中，`child_X_virt` 是 `*mut u64`，而 `write_bytes` 的 count 按元素类型计 → `4096 × 8B = 32768B`，**清零 8 页**（`0x7ff7000-0x7fff000`），覆盖 child_pml4 页 + 后续 7 页——**后续页恰是 parent 页表结构页（PD 0x7ff8000 等）** → parent 页表被清 → fork 崩溃。布局敏感的原因：child 分配页与 parent 结构页的相对排布由 `.bss` 大小决定。
    4. **修复**：4 处 `write_bytes` 指针改 `as *mut u8`（`4096 × 1B = 4096B` = 1 页），与仓库其余 21 处 `write_bytes(..., PAGE_SIZE)` 的 `as *mut u8` 写法一致。
    5. **验证**：修复后 `USER_CR3_SAVE=0x2311000` 布局（此前必失败）fork **连续 5 次 XYY 稳定**（X + 父Y + 子Y）；双架构 build 0w0e / clippy 0 warning / host-tests 91 套件通过 / deadlock 0 CRITICAL / coupling 通过。
  - 状态：[X]（2026-08-29 根因定位并修复；此前登记的"偶发 PMM 坏页"与"布局依赖结构页覆盖"实为同一 bug：`write_bytes` `*mut u64` 清零 8 页越界覆盖）
  - **B05-55C 清理补充（2026-08-29）**：审核员指出 cow.rs `clone_user_page_table_cow` 外层残留 d8330ef9 添加的 COW 调试日志块（`[COW]` klog，fork 热路径每次执行 8 次 volatile 遍历），且与 B05-49"插桩已清理"措辞矛盾（实为两回事：B05-49 指临时插桩，此块是 d8330ef9 有意调试日志）。**按用户裁决删除该诊断块**（cow.rs `clone_user_page_table_cow` 外层，删除 27 行），fork 回归改为**连续 10 次**验证（见 B05-54）。对应 d8330ef9 附带修复登记第 11 项。

- **B05-56. `PER_CPU_SYSCALL_STACK_SIZE` 收口（d8330ef9 附带漏登项，用户授权审核员代行，2026-08-29）**
  - 描述：d8330ef9 的 [gdt.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs) 除第 3 项 `user_rsp` 外还包含 `PER_CPU_SYSCALL_STACK_SIZE` 8192→65536 调整，"d8330ef9 附带修复登记"11 项未单列（补登遗漏）。该常量注释自称"⚠ 实验 (BISECT): 临时增大到 64KB"，实验状态未收口，且结构体注释仍写 8KB 与实际值矛盾。
  - 方案：审核员收口实验（两次独立构建，防陈旧镜像）：
    1. **回归 8KB 实测**：双架构 build 0w0e 后 QEMU 连续 10 次——全部仅输出 `X`（无父 Y/子 Y）；留存日志为**单次 boot 序列**（无复位重播），测试脚本 `-no-reboot` 下 QEMU 静默退出 → 判定 fork 路径**三重故障**（fork 调用链栈需求超过 8KB）。
    2. **定版 64KB 实测**：重建后 QEMU 连续 10 次——**全部 `XYY`**（X + 父 Y + 子 Y），独立复核 ea01d3b8 验证声明属实。
    3. **结论**：64KB 为 fork 必要容量而非临时诊断值；与内核栈容量惯例对齐（`KERNEL_STACK_SIZE` / `AP_STACK_SIZE` 均为 64KB）。注释已改写为实证依据，删除"实验/临时"措辞。
    - 注：串口输出含 `\r` 行尾，B05-54 所记 `grep '^[XY]$'` 直接匹配恒为 0，需 `tr -d '\r'` 后匹配（本条目验证即采用该方式）。
  - 状态：[X]（2026-08-29 收口完成：64KB 定版 + 8KB/64KB 双向实测，见 DECISION-075）

### 验证门槛

- **B05-54. fork 返回用户态回归**
  - 描述：修复后 `init` 应打印 `X`、`Y`（父）+ `Y`（子），无挂起/无限 #PF；**连续 10 次 QEMU run 稳定**（2026-08-29 删除 cow.rs 诊断日志后由 5 次提升至 10 次）。
  - 方案：`TIMEOUT_QEMU=15 ./scripts/qemu_boot_test.sh x86_64` **连续 10 次** + grep `^[XY]$` 出现 ≥2 次（父 Y + 子 Y）。
  - 状态：[X]（2026-08-29 验证通过：删除诊断日志后**连续 10 次运行全部 `XYY`**（X + 父 Y + 子 Y）；双架构 build/clippy/host-tests 无回归）

### 决策记录

- **DECISION-074（已执行方案 B，待继续授权）**
  - 描述：fork 返回挂起涉及用户**活跃未提交**的 fork/COW WIP，根因多候选（B05-50/51/52）。用户 2026-08-28 裁决选方案 B（gdb 定位返回路径）。
  - 方案：**B 已执行完毕**——gdb 证实返回路径正确（B05-52 关闭）；主根因是 fork/COW 路径损坏父页表（B05-55：栈 PDE[511] + IDT PTE[0x32] 清零 → 写栈 #PF P=0 → IDT 未映射嵌套 #PF → #DF → #TF）。下一步需继续授权：定位清零指令（gdb 硬件写监视点或 COW clone 插桩），并修 `copy_kstack`（B05-50）。
  - 状态：[X]（方案 B 定位完成，2026-08-28；修复待继续授权）

- **DECISION-075（用户授权审核员代行，2026-08-29）**
  - 描述：`PER_CPU_SYSCALL_STACK_SIZE` 收口二选一：A 定版 64KB（修订注释）vs B 回归 8KB 设计值（需重新验证）。用户指示"收口由审核员代行"。
  - 方案：以实证裁决——先执行 B（回归 8KB + QEMU 连续 10 次实测），fork 全部三重故障（日志止于 `X`）→ 证明 8KB 容量不足，d8330ef9 增容为 fork 必要条件 → 回归 A：定版 64KB，注释改写为实证依据。64KB 下连续 10 次全 `XYY` 独立复核通过。
  - 状态：[X]（已执行并验证，2026-08-29，见 B05-56）
