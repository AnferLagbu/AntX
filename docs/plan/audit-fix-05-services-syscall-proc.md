# 审计修复分册 05：services syscall 与进程

> 修复 services/syscall（types/dispatch）、services/proc（clone/pidfd/signal/sched_policy）与 syscall 编号体系缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.3 节 + 附录 B（2/3/5/6 大文件）+ 附录 H（H.4.1/H.4.4/H.4.8/H.4.9/H.5.4/H.5.6/H.5.7）。

## 工程计划 A: syscall 编号与类型修复

### 背景

- **syscall 编号三源错位**
  - 描述：用户态 sys.rs（400+）与内核态 types.rs（700+）SYS_CREDO_* 编号不一致（P0-28/P1-E），任何 Credo 系统调用不可能工作；types.rs 存在 7 组重复编号。
  - 方案：统一编号源（决策点：codegen 或单一权威文件），与 DECISION-037 的 500+ 立场对齐。
  - 状态：[]

### 待办

- **types.rs 重复编号消除（附录 B 2.1）**
  - 描述：[types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/types.rs) 实测 7 组重复值：157（setpgid/prctl）、170（fsync/sethostname）、452（fchmodat2/FB_RELEASE）、724/735/736/737（pidfd_send_signal/CREDO_DISK_INSTALL、clone3/CREDO_BOOT_CHECK、close_range/CREDO_REBOOT、openat2/CREDO_HOTPLUG_STATUS）。
  - 方案：冲突组重新分配编号；加编译期唯一性断言（`build.rs` 或测试）。
  - 状态：[]

- **SYS_CREDO_* 编号三源统一（P0-28 / P1-E）**
  - 描述：`src/user/lib/src/sys.rs`（400-437）与 `src/kernel/services/syscall/types.rs`（700+）SYS_CREDO_* 不一致，dispatch 无法匹配。
  - 方案：以单一权威源（建议内核 types.rs）为准，用户态 sys.rs 改引用；删除 ref-naming.md 的"500+"错误表述（分册 08 P0-20 联动）。
  - 状态：[]

- **Errno::from_ret 补全（附录 B 2.2/2.8）**
  - 描述：ENOSTR/ENODATA/ETIME/ENOSR/ENONET/EPROTO/EBADMSG/EOVERFLOW/ENOSYS 等定义后无 `from_ret()` 分支，转换缺失。
  - 方案：补全 from_ret 映射表 + 单元测试覆盖全部 Errno 变体。
  - 状态：[]

- **SyscallHandler 签名与 dispatch 对齐（附录 B 2.4）**
  - 描述：`SyscallHandler` 签名固定 4 参数，与 dispatch 实际 6 参数不匹配。
  - 方案：统一为 6 参数（a0-a5），或封装参数结构。
  - 状态：[]

## 工程计划 B: dispatch 分发修复

### 背景

- **dispatch 语义偏差集中**
  - 描述：pipe2/dup3 flags 丢失、快捷路径合并 handler 语义偏差、5 项已实装未分发、rt_sigreturn 硬编码。
  - 方案：逐项按 POSIX 语义修正。
  - 状态：[]

### 待办

- **SYS_pipe2/SYS_dup3 flags 传递（P0-12）**
  - 描述：[dispatch.rs:190,195](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs#L189-L196) `SYS_pipe2 → pipe_syscall(a0)`、`SYS_dup3 → dup2_syscall(a0,a1)`，flags 静默丢弃。
  - 方案：新增 `pipe2_syscall(fds, flags)` / `dup3_syscall(oldfd, newfd, flags)`；dispatch 传 `a2 as i32`。
  - 状态：[]

- **快捷路径合并 handler 语义偏差（H.4.9 P1-F）**
  - 描述：dispatch 大量"快捷路径"合并 handler（如 fchown/chown、fchmod/fchmodat、pipe/pipe2），语义偏差风险。
  - 方案：逐组核对 POSIX 语义差异，分离不兼容 handler（fchown 应只取 fd）。
  - 状态：[]

- **5 项已实装未 dispatch（H.5.4 P1-G）**
  - 描述：`SYS_setregid`/`SYS_clone3`/`SYS_clone` 等已实装函数未分发（报告 R2 表 A）。
  - 方案：按 `audit_unwired_pub_fn.py` 表 A（5 项）逐一接线 dispatch。
  - 状态：[]

- **rt_sigreturn 硬编码 sysno（H.5.6 P1-I）**
  - 描述：dispatch 中 `rt_sigreturn` 处理硬编码 sysno。
  - 方案：改用常量引用。
  - 状态：[]

- **dispatch 参数传递仅支持 x86_64（H.5.7 P1-J）**
  - 描述：syscall 入口参数传递仅支持 x86_64，aarch64 未覆盖。
  - 方案：实现 aarch64 参数传递路径（或确认 exception.rs 独立路径并修正声明）。
  - 状态：[]

- **dispatch_other 直调 framework::syscall::api（附录 B 3.2）**
  - 描述：`dispatch_other` 直接调用 `framework::syscall::api::*`，违反 F2 黑名单。
  - 方案：改走 framework 顶层 re-export（分册 01 修复 F2 门禁后验证）。
  - 状态：[]

- **name_to_handle_at/open_by_handle_at 错误吞咽（附录 B 3.1）**
  - 描述：使用 `unwrap_or_else(Errno::as_ret)` 错误吞咽 + 静默 ENOSYS。
  - 方案：改为显式错误传播。
  - 状态：[]

## 工程计划 C: 进程子系统修复

### 背景

- **proc 层安全/语义缺陷**
  - 描述：clone 运算符优先级 bug、pidfd 返回 PID、sched boost_priority 死代码、signal 范围校验缺失。
  - 方案：按安全缺陷优先修复。
  - 状态：[]

### 待办

- **clone_syscall 运算符优先级 Bug（P0-11）**
  - 描述：[clone.rs:41](file:///home/anfer/Code/QueenX/src/kernel/services/proc/clone.rs#L41) `flags & CLONE_SIGHAND == 0` 因优先级等效 `flags & 0`，CLONE_VM+CLONE_THREAD→CLONE_SIGHAND 约束**恒不触发**。
  - 方案：加括号 `(flags & CLONE_SIGHAND) == 0`；补 clone flags 校验 host-tests。
  - 状态：[]

- **pidfd_open 返回 PID 作为 fd（P0-10）**
  - 描述：[pidfd.rs:28](file:///home/anfer/Code/QueenX/src/kernel/services/proc/pidfd.rs#L28) `Ok(pid as usize)`，pid=1 与 stdin 冲突、重复调用同 fd、攻击者任意信号注入。
  - 方案：通过 `fd_alloc::alloc_fd` 分配真实 fd，维护 pidfd→pid 映射表；pidfd_getfd 依赖 OpenFile 系统（ISSUE-SRC-026）。
  - 状态：[]

- **sched_policy boost_priority 死代码（附录 B 5.1）**
  - 描述：`CfsRunQueue::boost_priority` 存在但无人调用，v2.0 §F7.1/F7.2 修复未触达。
  - 方案：接线到调度路径或按 F9 原则删除（联动分册 09 死代码治理）。
  - 状态：[]

- **signal 范围校验与 RT 信号支持（附录 B 6.2/6.3）**
  - 描述：`kill_syscall` 缺 pid 极端值校验；RT 信号（32..=64）可设 handler 但内核基础设施仅 32-bit。
  - 方案：补 pid 范围校验；RT 信号支持明确 fail-closed 或扩展实现。
  - 状态：[]

- **SYS_exit_group 与 SYS_exit 共享 handler（H.4.4 P1-A）**
  - 描述：线程组语义违反，`exit_group` 应结束整个线程组。
  - 方案：分离 handler，exit_group 遍历线程组终止。
  - 状态：[]

### 验证门槛

- **syscall 回归**
  - 描述：编号修复后跑 syscall host-tests + 用户态 smoke（若可启动）。
  - 方案：`make test-host`。
  - 状态：[]

- **proc 回归**
  - 描述：clone/pidfd/signal 修复后跑 proc 相关 host-tests。
  - 方案：`make test-host`。
  - 状态：[]

### 决策记录

- **DECISION-050**
  - 描述：syscall 编号统一采用"内核 types.rs 单一权威源"方案，用户态 sys.rs 改引用。
  - 方案：后续 codegen（DECISION-H25）以 types.rs 为输入；消除 400/500/700 三源错位。
  - 状态：[]
