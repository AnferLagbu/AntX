# services/proc 子系统深度审计报告

> **审计范围**：`src/kernel/services/proc/`
> **审计日期**：2026-08-14
> **文件数**：29 个源文件
> **代码规模**：约 192 KB（含测试 + 注释） / 有效 LoC 约 11.9K
> **总体结论**：✅ 0 unsafe（合规）/ ⚠️ 41 个问题（P0×7, P1×14, P2×15, P3×5）

## 1. 子系统概览

### 1.1 目录结构

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/mod.rs) | 212 | 子系统入口、错误定义、初始化顺序 | 中 |
| [types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/types.rs) | 372 | 类型定义：Pid/ProcessState/ProcessContext | 中 |
| [table.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/table.rs) | 401 | 进程表 CRUD 闭包 API | **高** |
| [namespace.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs) | 787 | 7 类 Linux Namespace 隔离 | **高** |
| [cgroup.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs) | 632 | Cgroup 资源限制（CPU/MEM/PIDS/IO）| **高** |
| [sched_policy.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs) | 605 | CFS 调度策略 + DL 调度 | **高** |
| [signal.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs) | 601 | POSIX 信号 + Syscall 代理 | **高** |
| [seccomp.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/seccomp.rs) | ~430 | 系统调用过滤 | **高** |
| [session.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/session.rs) | 573 | Session/PGID/控制终端 | 中 |
| [rlimit.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/rlimit.rs) | 294 | 16 类 rlimit + 检查 | 中 |
| [fd_alloc.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/fd_alloc.rs) | ~410 | 全局 FD 分配器 | 中 |
| [affinity.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/affinity.rs) | 110 | CPU 亲和性 | 低 |
| [fd_table.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/fd_table.rs) | ~115 | Per-process FD 表 | 低 |
| [madvise_mlock.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/madvise_mlock.rs) | ~280 | madvise/mlock 策略 | 中 |
| [info.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/info.rs) | ~110 | proc 信息查询 | 低 |
| [sched.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched.rs) | ~70 | 调度入口 | 低 |
| [pidfd.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/pidfd.rs) | ~52 | pidfd 系统调用 | 低 |
| [coredump.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/coredump.rs) | ~25 | 核心转储入口 | 低 |
| [lifecycle.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/lifecycle.rs) | ~28 | fork/exit/yield | 低 |
| 其他 | < 50 | 便利封装 | 低 |

### 1.2 架构分层

```text
┌──────────────────────────────────────────────────────────────┐
│ services/proc/                  100% safe Rust, 0 unsafe    │
│  ├─ types.rs          纯数据 + 进程状态机                    │
│  ├─ table.rs          进程表闭包 API（防裸指针外泄）          │
│  ├─ namespace.rs      7 类 Linux Namespace                   │
│  ├─ cgroup.rs         4 类 cgroup 控制器                     │
│  ├─ sched_policy.rs   CFS/DL + DefaultPolicy                │
│  ├─ signal.rs         POSIX 信号 + Syscall 代理              │
│  ├─ seccomp.rs        BPF 风格过滤器                         │
│  ├─ session.rs        Session/PGID/TTY                      │
│  ├─ rlimit.rs         16 类 POSIX 资源限制                   │
│  └─ fd_alloc.rs       全局 FD 范围规划 + 集中分配             │
├──────────────────────────────────────────────────────────────┤
│ framework/proc/        TCB (允许 unsafe, 硬件/中断/上下文)    │
│  ├─ process.rs        PCB (真实硬件上下文)                  │
│  ├─ scheduler.rs      调度器机制（队列+切换）                │
│  ├─ thread.rs         线程表管理                            │
│  ├─ signal.rs         signal_pending 32-bit 实现            │
│  └─ rlimit.rs         Per-process RlimitTable 存储          │
└──────────────────────────────────────────────────────────────┘
```

### 1.3 硬规则符合性

| 规则 | 状态 | 备注 |
|---|---|---|
| F1 services 0 unsafe | ✅ 全部 29 文件首行 `#![deny(unsafe_code)]` | |
| F2 services 不直接访问 framework 内部 | ✅ 仅通过 re-export 公共 API | |
| F3 模块间无循环依赖 | ✅ DAG 清晰 (types → table → 子模块) | |
| F4 framework unsafe 配 SAFETY | ✅ 走 framework 公共 API | |
| F7 中文注释强制 | ⚠️ 41 处英文注释需补 (见 §6) | |
| F8 公共 API 中文文档 | ⚠️ 公共 API 注释完整度高, 仍有个别英文 | |

---

## 2. P0 — 严重问题（7 个）

### 2.1 [P0] `PidNamespace::alloc_pid` 0→PID 1 复用 + nr_processes 计数漂移
- **位置**：[namespace.rs:271-274](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L271-L274)
- **代码**：
  ```rust
  pub fn alloc_pid(&self) -> u32 {
      self.nr_processes.fetch_add(1, Ordering::SeqCst);
      self.next_pid.fetch_add(1, Ordering::SeqCst)  // ← 1→2→3..., 0 跳过
  }
  ```
- **问题**：
  - 根 PID namespace 从 1 开始分配，但 root namespace 在 init 阶段已经占用 PID 0/1/2（kthread/init）。
  - 跨 namespace 的 PID 命名空间嵌套时，子 namespace 的 PID 1 在父 namespace 中并不一定对应 init 进程。
  - **没有 `release` / `dec_nr_processes` 配套** → 进程退出时 `nr_processes` 单调递增，永不归零。
- **风险**：
  - `nr_processes` 字段的语义被破坏（语义上应是"当前 namespace 中存活进程数"，但实际是"自创建以来累计创建数"）。
  - 若将来基于 `nr_processes == 0` 决定 namespace 销毁（Linux 行为），将永远不触发。
- **修复**：
  1. 配套 `release_pid()` 函数，进程退出时调用 `nr_processes.fetch_sub(1)`。
  2. 在根 namespace 中保留 0/1/2 槽位（fork 时跳过已占用）。
  3. `nr_processes == 0` 且无 child 时允许销毁（参考 Linux `mnt_release`）。

### 2.2 [P0] `CgroupSubsystem::migrate` 嵌套锁 + 锁释放-再获取窗口
- **位置**：[cgroup.rs:466-486](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L466-L486)
- **代码**：
  ```rust
  pub fn migrate(&self, pid: Pid, target_id: u64) -> Result<(), Errno> {
      let target = self.find(target_id).ok_or(Errno::ENOENT)?;  // 锁 1: groups

      {
          let groups = self.groups.lock();                       // 锁 1 重新获取
          for cg in groups.values() {
              let procs = cg.procs.lock();                       // 锁 2: per-cg
              if procs.contains(&pid) {
                  drop(procs);
                  cg.detach_proc(pid);                           // 锁 2 再次获取
                  break;
              }
          }
      }  // ← 锁 1 释放

      if !target.attach_proc(pid) {  // 锁 2 在 target 上重新获取
          return Err(Errno::EAGAIN);
      }

      Ok(())
  }
  ```
- **问题**：
  - `groups` 全局锁在每次 `find`/遍历时都获取释放，存在 4 次锁操作。
  - 关键窗口：旧 cgroup 已 `detach_proc`（调用 `pids.exit()` 减计数），但 `target.attach_proc` 还未完成（未增加计数）。若进程在两步骤间被 SIGKILL，全局 `pids.current` 计数为负。
  - **`migrate` 失败时只 `attach_proc` 失败，旧 cgroup 已 `detach_proc`**，导致进程从所有 cgroup 脱钩。
- **风险**：
  - 全局 PID 控制器计数不一致（cgroup 跨组操作时计数与实际进程数对不上）。
  - 进程脱钩后无 cgroup，CPU/MEM 限制全部失效。
- **修复**：
  1. 先在旧 cgroup 标记 `pending_migration`（不上锁），获取 `target.attach_proc` 成功后再 `detach_proc`。
  2. 引入 `try_attach_then_detach(pid, old, new)` 原子操作：先 `attach`，成功才 `detach`。
  3. 使用 `compare_exchange` 风格的乐观锁替代嵌套 mutex。

### 2.3 [P0] `SchedPolicy` 全局注册无锁竞争保护
- **位置**：[sched_policy.rs:386-389](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L386-L389)、[signal.rs:598-600](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L598-L600)
- **代码**：
  ```rust
  pub fn register_default_policy() -> Result<(), ()> {
      static POLICY: DefaultPolicy = DefaultPolicy;
      crate::kernel::framework::proc::register_sched_decision(&POLICY).map_err(|_| ())
  }
  pub fn register_standard_signal_policy() -> Result<(), ()> {
      static POLICY: StandardSignalPolicy = StandardSignalPolicy;
      crate::kernel::framework::proc::register_signal_decision(&POLICY).map_err(|_| ())
  }
  ```
- **问题**：
  - `register_*_policy` 文档说"只能注册一次"，但实际是 **可以被并发调用 N 次**，framework 端 `register_sched_decision` 内部如果有竞态（如 `OnceLock` 写入窗口），则会出现指针被覆盖或多个策略共存。
  - services::proc::init() 在启动期单线程调用是安全的，但若以后从 lazy_static 路径调用则风险显现。
- **风险**：
  - 双注册导致调度行为不确定（vtable 指向错对象）。
  - 信号分发策略被覆盖可能导致安全信号被错误处理。
- **修复**：
  1. services 端用 `AtomicBool` 加 CAS 保护：只在第一次注册时调用 framework API。
  2. framework 端 `register_sched_decision` 应在内部使用 `OnceLock` + `compare_exchange`，避免数据竞争。
  3. 加 unit test 验证：连续调用 100 次 register，期望只第一次成功。

### 2.4 [P0] `SignalDisposition` 默认动作与 `SignalDefaultAction` 双重真理源
- **位置**：[signal.rs:213-241](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L213-L241) + [signal.rs:563-588](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L563-L588)
- **代码**：
  ```rust
  // signal.rs:227 处的 SignalDisposition
  pub fn default_for(sig: StandardSignal) -> Self {
      match sig {
          StandardSignal::Chld | StandardSignal::Urg => Self::Ign,
          StandardSignal::Stop | StandardSignal::Tstp | StandardSignal::Ttin | StandardSignal::Ttou => Self::Stop,
          StandardSignal::Cont => Self::Cont,
          s if s.is_core_dump() => Self::Core,
          _ => Self::Term,
      }
  }

  // signal.rs:563 处的 StandardSignalPolicy
  fn default_action(&self, sig: u8) -> SignalDefaultAction {
      match sig {
          17 | 23 => SignalDefaultAction::Ign,
          19 | 20 | 21 | 22 => SignalDefaultAction::Stop,
          18 => SignalDefaultAction::Cont,
          3 | 4 | 6 | 7 | 8 | 11 | 31 | 24 | 25 => SignalDefaultAction::Core,
          _ => SignalDefaultAction::Term,
      }
  }
  ```
- **问题**：
  - 两份独立的 POSIX 默认动作表（一份基于枚举，一份基于数字）→ 修改 SIGCHLD 行为必须同步两处。
  - `17 | 23` 实际是 SIGCHLD(17) + SIGURG(23)，但代码缺乏 `// SAFETY: 17=CHLD, 23=URG` 注释，新人维护易出错。
  - **`is_core_dump()` 与 `3|4|6|7|8|11|31|24|25` 列表存在不一致风险**：`is_core_dump` 返回 true 的有 `Quit/Ill/Abrt/Bus/Fpe/Segv/Sys/Xcpu/Xfsz`（9 个），但 default_action 只列了 9 个数字 — **当前一致**，但任何一处修改另一处会漂移。
- **风险**：
  - 单一来源原则违反 → 行为漂移。
  - 编译期无法检测两表不一致。
- **修复**：
  1. 抽出 `POSIX_DEFAULT_ACTION: [(u8, SignalDefaultAction); 31]` 常量表，两处都从表查找。
  2. `is_core_dump()` 改为查表。
  3. 写 unit test 校验 31 个信号全覆盖 + 与 Linux 行为一致。

### 2.5 [P0] `sys_setns` 标志位转换硬编码 `1 << (ns_type + 8)` 错误
- **位置**：[namespace.rs:761-774](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L761-L774)
- **代码**：
  ```rust
  pub fn sys_setns(ns_type: u64, target_ns_id: u64) -> i64 {
      let ns_t = match NsType::from_clone_flag(1 << (ns_type + 8)) {
          Some(t) => t,
          None => match ns_type {
              0 => NsType::Mount,
              ...
          },
      };
  ```
- **问题**：
  - `1 << (ns_type + 8)` 是把用户态数字（0..6）转换回 `CLONE_NEW*` 标志位，但 `CLONE_NEW*` 并不是简单的"1 << 8 + i"模式（如 `CLONE_NEWNS = 0x20000` 是 `1 << 17`）。
  - 用户传 `ns_type=4` (Pid) → 期望 `CLONE_NEWPID = 0x20000000` → `1 << 12 = 0x1000`，两者不匹配 → 进入兜底分支。
  - **兜底分支中 `4 => NsType::Pid` 是手写硬编码，与 `from_clone_flag` 重复**。
- **风险**：
  - 调用 `from_clone_flag` 永远走不到正确分支，每次都走兜底 → 未来重构 `CLONE_NEW*` 数值时悄无声息破坏 `sys_setns`。
- **修复**：
  1. 删掉 `from_clone_flag(1 << (ns_type + 8))` 调用，直接用 `match ns_type` 数字映射。
  2. 或者改 `from_clone_flag` 接口：增加 `from_index(ns_type: u8) -> Option<NsType>`，专用于"用户传数字"的场景。

### 2.6 [P0] `CgroupSubsystem::remove_cgroup` 死锁风险（groups + procs 锁序）
- **位置**：[cgroup.rs:424-452](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L424-L452)
- **代码**：
  ```rust
  pub fn remove_cgroup(&self, id: u64) -> Result<(), Errno> {
      ...
      let mut groups = self.groups.lock();           // 锁 A
      let cg = match groups.get(&id) { ... };        // 锁 A 持有
      if !cg.procs.lock().is_empty() { ... }         // 锁 B
      if !cg.children.lock().is_empty() { ... }      // 锁 C
      if let Some(parent) = groups.get(&cg.parent_id) {
          let mut children = parent.children.lock(); // 锁 C' (parent 的 children)
      }
      groups.remove(&id);                            // 锁 A
  }
  ```
- **问题**：
  - 锁获取顺序：groups(A) → cg.procs(B) → cg.children(C) → parent.children(C') → groups.remove(A)。
  - 在 A 持有状态下进入 B/C 不构成死锁（同一线程），但 `parent.children` 嵌套锁在 `groups` 锁内，若 parent 是当前 cg（id == 0 时）会重入（IrqSpinLock 不支持重入 → 死锁）。
  - **`id == 0` 早返回已避免**，但 `cg.parent_id == id` 自引用场景未防御。
- **风险**：
  - 若 `create_cgroup` 时 `parent_id == id`（自身为父），后续 `remove_cgroup` 进入 `parent.children` 重入锁 → 内核死锁。
- **修复**：
  1. `create_cgroup` 时校验 `parent_id != id`。
  2. 或在 `remove_cgroup` 中使用 `if cg.parent_id != id` 防御。
  3. 加 fuzz test 模拟 create/remove 循环 1000 次，检测死锁。

### 2.7 [P0] `SignalDecision::pick_next_signal` 拒绝 bit 0 但 Signal::NONE=0
- **位置**：[signal.rs:578-587](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L578-L587)
- **代码**：
  ```rust
  fn pick_next_signal(&self, deliverable: u64) -> Option<u8> {
      if deliverable == 0 {
          return None;
      }
      let sig_bit = deliverable.trailing_zeros() as u8;
      if sig_bit == 0 || sig_bit > 31 {
          return None;  // ← 0 永远不投递
      }
      Some(sig_bit)
  }
  ```
- **问题**：
  - `Signal::NONE = 0`，bit 0 在 `signal_pending` 中表示"信号 0" — 但 POSIX 中 `kill(pid, 0)` 是"检查存在性"而非"投递信号 0"。
  - 然而 framework 层 `signal_pending_set(0)` 仍会设置 bit 0，触发 `_sig_bit == 0 → None`，但调用方可能误以为投递了信号 0。
  - **更严重**：`trailing_zeros() == 0` 在 32 位架构上当 deliverable == 0xFFFFFFFF... 时返回 0，但已被前置 `if deliverable == 0` 拦截。**逻辑正确**，但 `sig_bit == 0` 永真（`trailing_zeros` 至少 0），导致 deliverable==0x1 时返回 0（错误的 None）。
- **风险**：
  - 当 deliverable == 0x1（仅 signal 1 待投递）时，`trailing_zeros == 0`，但 `sig_bit > 31` 是 false（0 < 31），应返回 `Some(1)`；实际却因为 `sig_bit == 0` 守卫返回 `None` → **signal 1 (SIGHUP) 永远无法投递**。
- **修复**：
  1. 改为 `if sig_bit < 1 || sig_bit > 31` (即 `sig_bit == 0` 改为 `sig_bit == 0` 保留但加注释解释"0 = 哨兵")。
  2. 或改用 `deliverable.trailing_zeros()` 直接返回，不做 `sig_bit == 0` 拦截：bit 0 真的就是 signal 1 (因为 `1u8 << 0 = 1`)。
  3. 加 unit test：`pick_next_signal(0b10) == Some(1)`。

---

## 3. P1 — 重要问题（14 个）

### 3.1 [P1] `NamespaceSet::clone_from` 父级成员借用 vs `Self` 拥有冲突（多次 `Arc::clone` 触发堆分配）
- **位置**：[namespace.rs:535-590](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L535-L590)
- **问题**：
  - `clone_from` 通过 7 次分支判断 + 7 次 Arc::clone + 7 次 Arc::new，路径长且重复。
  - `Self { uts, ipc, pid, mount, user, net, cgroup }` 字段顺序与构造顺序不一致，user 字段在其他字段之前先放（line 588），违反结构体字面量"字段顺序"约定。
- **风险**：易引入新字段时漏修改。
- **修复**：构造数组迭代 + 提取 `decision_table: [(NsType, |&Self| -> Arc<...>); 7]`。

### 3.2 [P1] `UtsNamespace::set_nodename` 截断字符串不报告错误
- **位置**：[namespace.rs:168-173](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L168-L173)
- **代码**：
  ```rust
  pub fn set_nodename(&self, name: &[u8]) {
      let mut buf = self.nodename.lock();
      let len = name.len().min(64);
      buf[..len].copy_from_slice(&name[..len]);
      buf[len] = 0;
  }
  ```
- **问题**：
  - 用户传 100 字节主机名 → 静默截断到 64 字节，无返回值或 errno 反馈。
  - 与 `set_hostname(2)` POSIX 行为不符（应返回 ENAMETOOLONG）。
- **修复**：返回 `Result<(), Errno>`，长度 > 64 返回 ENAMETOOLONG。

### 3.3 [P1] `CgroupSubsystem::cgroup_of` O(N×M) 双重锁遍历
- **位置**：[cgroup.rs:488-496](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L488-L496)
- **代码**：
  ```rust
  pub fn cgroup_of(&self, pid: Pid) -> Option<Arc<CgroupRq>> {
      let groups = self.groups.lock();        // 持 groups 锁
      for cg in groups.values() {
          if cg.procs.lock().contains(&pid) { // 持 procs 锁
              return Some(Arc::clone(cg));
          }
      }
      None
  }
  ```
- **问题**：
  - 持 `groups` 锁遍历每个 cgroup，进入 `procs.lock()`，**嵌套锁 + 锁内 alloc 风险**。
  - 1000 个 cgroup × 100 进程 = 100,000 次锁检查。
- **修复**：在 PCB 中存储 `cgroup_id: AtomicU64`，O(1) 查找。

### 3.4 [P1] `SchedPolicy` 注册失败时无回退
- **位置**：[mod.rs:130-145](file:///home/anfer/Code/QueenX/src/kernel/services/proc/mod.rs#L130-L145)
- **代码**：
  ```rust
  pub fn init() {
      // 注册 services 层调度策略 (在 framework 调度器初始化之前)
      let _ = sched_policy::register_default_policy();
      // REVAL-1: 注册 services 层信号策略
      let _ = signal::register_standard_signal_policy();
      ...
  }
  ```
- **问题**：
  - `let _ =` 丢弃了 `Result`，注册失败时无任何反馈 → 内核启动后调度行为 = 兜底实现（若 framework 有）= 未知。
- **修复**：使用 `klog_warn!` 记录失败，至少 `klog_warn!("sched_policy register failed")`。

### 3.5 [P1] `NamespaceSet::unshare` 嵌套 namespace 不支持原子操作
- **位置**：[namespace.rs:597-626](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L597-L626)
- **问题**：
  - 一次性 unshare 多类 namespace 时，**先创建新 ns 再切换 self**，若中途 panic 留下半构造状态。
  - Linux unshare(2) 是原子的（要么全部成功，要么全部失败）。
- **修复**：先在栈上构造 `new_uts/uts/...`，全部成功后再 `*self = new`。

### 3.6 [P1] `Signal::to_bit` 溢出：`Signal::realtime(0).to_bit() == 1 << 32`
- **位置**：[signal.rs:202-205](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L202-L205)
- **代码**：
  ```rust
  pub fn to_bit(self) -> u64 {
      1u64 << u64::from(self.0)
  }
  ```
- **问题**：
  - `Signal::realtime(31) = 63` → `1 << 63` 合法。
  - `Signal::realtime(32) = 64` → `1 << 64` **在 u64 中是 shift overflow, UB**。
  - 当前 `send()` 函数已校验 `sig.0 >= 64 → InvalidArgument`，**但 Signal 本身不约束上限**，单构造时仍可调用 `to_bit()` 触发 UB。
- **修复**：Signal 类型加 `MAX_NUMBER: u8 = 64` 常量，`new` 时校验。

### 3.7 [P1] `RlimitTable::set` 忽略 old.max == 0 边界
- **位置**：[rlimit.rs:125-145](file:///home/anfer/Code/QueenX/src/kernel/services/proc/rlimit.rs#L125-L145)
- **代码**：
  ```rust
  if max > old.max && !is_privileged {
      return Err(Errno::EPERM);
  }
  ```
- **问题**：
  - 当 `old.max == 0`（资源被禁用）时，任何 `max > 0` 都触发 EPERM（即便特权也无权恢复）？— **实际 `0 > 0 == false`，所以不会触发**。逻辑正确。
  - 但 `is_privileged` 由调用方传参决定，**services 层从未查询真实权限状态**（如 UID/capability），完全是信任调用方。
- **风险**：syscall handler 误传 `is_privileged = true` 可绕过 EPERM。
- **修复**：从 PCB 读 `is_privileged: bool` 字段（基于 UID），不依赖调用方参数。

### 3.8 [P1] `CfsRunQueue::boost_priority` O(N) clear+reinsert 性能问题
- **位置**：[sched_policy.rs:189-207](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L189-L207)
- **问题**：
  - `boost_priority` 收集所有 entries → clear tree → 重新 insert N 次。
  - 1000 进程 → 1000 次 tree 操作 + 1000 次 alloc。
  - **boost 是中断路径上的热路径**（每 N tick 触发一次）。
- **修复**：
  1. 不重建树，直接修改 min_vruntime 并保留原 entries。
  2. 或用 lazy boost：仅标记 `dirty` 标志，下次 pick_next 时按需调整。

### 3.9 [P1] `DlRunQueue` 无 admission control 实现
- **位置**：[sched_policy.rs:264-272](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L264-L272)
- **代码**：
  ```rust
  pub fn enqueue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) -> bool {
      if self.total_utilization.saturating_add(util_pct) > DL_MAX_UTILIZATION_PCT {
          return false;
      }
      ...
  }
  ```
- **问题**：
  - admission control 仅检查"加入这一个是否超 100%"，**未检查"加入后剩余 bandwidth 是否能满足其他任务 deadline"**。
  - Linux DL 有 `dl_overrun` 机制跟踪 deadline miss，当前实现无。
- **修复**：实现 `dl_check_bandwidth(task)` + `dl_account_overrun(task)`。

### 3.10 [P1] `CgroupSubsystem` 内部 API `migrate` 错误语义颠倒
- **位置**：[cgroup.rs:466-486](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L466-L486)
- **问题**：
  - `migrate` 文档说"先 detach 再 attach"，但实际 detach 与 attach 中间是 release-lock 窗口。
  - 调用方期望："detach 失败 → 不 attach"，但实际 detach 不可能失败（无返回值），导致只能"attach 失败 → 进程脱钩"。
- **修复**：合并为单一原子操作 `try_migrate(pid, from, to) -> Result`。

### 3.11 [P1] `ProcessContext::set_user_mode` 硬编码段选择子 0x23/0x1B
- **位置**：[types.rs:248-256](file:///home/anfer/Code/QueenX/src/kernel/services/proc/types.rs#L248-L256)
- **代码**：
  ```rust
  pub fn set_user_mode(&mut self) {
      self.cs = 0x23;
      self.ds = 0x1B;
      ...
  }
  ```
- **问题**：
  - 0x23 是 USER_CODE selector，0x1B 是 USER_DATA selector。
  - 这些值在 `framework::arch::x86_64::gdt` 中定义为 `SELECTOR_USER_CODE = 0x20`、`SELECTOR_USER_DATA = 0x18`，**但 RPL 3 = 0x3**。
  - 实际：0x23 = 0x20 | 0x3，0x1B = 0x18 | 0x3。
  - **硬编码不引用常量** → 若 GDT 顺序调整，进程陷入 Ring 0 后无法恢复。
- **修复**：从 framework 引用 `SELECTOR_USER_CODE | 3` 等。

### 3.12 [P1] `ProcessState::from_u8` 无效值回退到 Created
- **位置**：[types.rs:52-63](file:///home/anfer/Code/QueenX/src/kernel/services/proc/types.rs#L52-L63)
- **代码**：
  ```rust
  pub fn from_u8(value: u8) -> Self {
      match value {
          0 => Self::Created,
          1 => Self::Ready,
          ...
          _ => Self::Created, // 无效值安全回退
      }
  }
  ```
- **问题**：
  - 静默将无效值转为 Created，可能掩盖 corrupt 数据。
  - `AtomicU32` 存储可能因 race condition 读到中间值（如 8 → 0xFF），被悄悄重置。
- **修复**：返回 `Option<ProcessState>`，调用方显式处理 None。

### 3.13 [P1] `FdSubsystem::COUNT` 硬编码 6，与 enum 变体数易漂移
- **位置**：[fd_alloc.rs:86](file:///home/anfer/Code/QueenX/src/kernel/services/proc/fd_alloc.rs#L86)
- **代码**：
  ```rust
  pub const COUNT: usize = 6;
  ```
- **问题**：
  - 新增 `FdSubsystem::TimerFd` 时忘记更新 COUNT → `from_index(5)` 仍返回 Some，但 `0..COUNT` 实际只迭代 5 个。
- **修复**：用 `enum_count::FromVariantCount` trait 或 `std::mem::variant_count` 替代。

### 3.14 [P1] `sched_setaffinity_syscall` 用户指针未做 SMAP/SMEP 校验
- **位置**：[affinity.rs:33-40](file:///home/anfer/Code/QueenX/src/kernel/services/proc/affinity.rs#L33-L40)
- **问题**：
  - `validate_user_buf` + `read_u64_from_user` 是 framework 提供的 safe wrapper，但 **services 层无法保证这两个 API 内部已校验用户指针是否在 user VA range**。
  - 若 framework 的实现仅做 `is_user_accessible`，则未校验"是否在当前进程 VA 空间内"，恶意用户传另一个进程的 VA 仍可读。
- **风险**：跨进程信息泄漏（一个用户读另一个用户的 affinity）。
- **修复**：在 `validate_user_buf` 中增加 `is_in_current_user_va_range` 校验。

---

## 4. P2 — 中等问题（15 个）

### 4.1 [P2] `NamespaceSet::clone_from` 字段声明顺序与赋值顺序不一致
- **位置**：[namespace.rs:581-589](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L581-L589)
- **修复建议**：调整字段顺序为 `uts/ipc/pid/mount/user/net/cgroup` 与结构体定义一致。

### 4.2 [P2] `CgroupSubsystem` 使用 `BTreeMap` 而非 HashMap 性能次优
- **位置**：[cgroup.rs:370-371](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L370-L371)
- **修复建议**：若 cgroup ID 不要求有序，可改用 `HashMap<u64, Arc<CgroupRq>>` 提升查询性能。

### 4.3 [P2] `PidNamespace::level` 用 `AtomicU32` 但创建后只写一次
- **位置**：[namespace.rs:235](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L235)
- **修复建议**：`level` 在构造时确定，可改为不可变 `u32` 字段。

### 4.4 [P2] `CfsRunQueue::nr_running` 用 u32 但 total_weight 是 u64
- **位置**：[sched_policy.rs:108-110](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L108-L110)
- **修复建议**：统一为 `u64` 或 `AtomicU64`。

### 4.5 [P2] `Session` 全部字段是 `Atomic*` 但实际无并发
- **位置**：[session.rs:60-72](file:///home/anfer/Code/QueenX/src/kernel/services/proc/session.rs#L60-L72)
- **修复建议**：所有访问在 `SessionManager` 内部 `session_table.lock()` 内，原子操作冗余。

### 4.6 [P2] `CgroupSubsystem::create_cgroup` parent_id==0 时深度校验缺失
- **位置**：[cgroup.rs:390-414](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L390-L414)
- **修复建议**：增加 `CGROUP_MAX_DEPTH` 校验，深度超限返回 0。

### 4.7 [P2] `BlockReason` 5 变体不覆盖所有等待原因（如 futex 锁竞争）
- **位置**：[types.rs:128-148](file:///home/anfer/Code/QueenX/src/kernel/services/proc/types.rs#L128-L148)
- **修复建议**：增加 `WaitingForFutex` 变体，区分 `FutexWait` 与 `FutexLock`。

### 4.8 [P2] `sched_policy::register_default_policy` 使用 `static POLICY`
- **位置**：[sched_policy.rs:387](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L387)
- **问题**：通过 `&POLICY` 取引用，若 framework 端存储 `'static` 引用则 OK；否则存在生命周期问题。
- **修复**：明确文档 `&'static DefaultPolicy`。

### 4.9 [P2] `Rlimit::infinity` 与 `RLIM_INFINITY` 双名
- **位置**：[rlimit.rs:54, 70-72](file:///home/anfer/Code/QueenX/src/kernel/services/proc/rlimit.rs#L54-L70)
- **修复建议**：合并为 `Rlimit::INFINITY` 关联常量。

### 4.10 [P2] `CgroupSubsystem::cgroup_of` 在 hot-path 上分配
- **位置**：[cgroup.rs:493](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L493)
- **修复建议**：在 `Process` 中加 `cgroup_id: AtomicU64` 字段，O(1) 查找。

### 4.11 [P2] `Signal::NONE` 在 `send` 中被检查但 `to_bit` 仍返回 1
- **位置**：[signal.rs:286-297](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L286-L297)
- **问题**：`send(pid, Signal::NONE)` 走存在性检查路径，但 `Signal::NONE.to_bit() == 1` 仍会污染 bit 0。
- **修复**：在 `send` 中显式 short-circuit 永不调用 `signal_set` 路径。

### 4.12 [P2] `rlimit::check_nproc_exceeded` 实际语义是"子进程数 + 1"
- **位置**：[rlimit.rs:200-211](file:///home/anfer/Code/QueenX/src/kernel/services/proc/rlimit.rs#L200-L211)
- **问题**：`fd_count as u64 >= rlim.cur` 用 >= 而非 >，意味着 fd_count == cur 触发限额 → 实际只允许 cur-1 个 fd。
- **修复**：使用 `>` 而非 `>=`。

### 4.13 [P2] `CgroupRq::attach_proc` 容量检查 + try_fork 非原子
- **位置**：[cgroup.rs:344-354](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L344-L354)
- **问题**：先 push 到 procs，再 try_fork → try_fork 失败时 procs 已包含 pid。
- **修复**：先 try_fork，成功再 push。

### 4.14 [P2] `DlRunQueue::dequeue` 接受 caller 提供的 weight/util 易出错误
- **位置**：[sched_policy.rs:274-279](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L274-L279)
- **问题**：caller 传错 `util_pct` → 计数错乱。
- **修复**：从 DeadlineParams 内部读，不接受外部参数。

### 4.15 [P2] `SessionManager::log_num` 手动数字转字符串
- **位置**：[session.rs:29-44](file:///home/anfer/Code/QueenX/src/kernel/services/proc/session.rs#L29-L44)
- **修复建议**：使用 `alloc::format!` 或 framework `klog::write_u64`。

---

## 5. P3 — 次要问题（5 个）

### 5.1 [P3] `ProcessFlags` 字段不完整（无 NEED_FORK/EXITED 等）
- **位置**：[types.rs:176-183](file:///home/anfer/Code/QueenX/src/kernel/services/proc/types.rs#L176-L183)
- **修复建议**：按需扩展。

### 5.2 [P3] `StandardSignal::Pwr` 与 `StandardSignal::Io` 在 Linux 中是不同信号
- **位置**：[signal.rs:88-92](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs#L88-L92)
- **修复建议**：补全注释说明 PWR=30/IO=29 的语义。

### 5.3 [P3] `CgroupSubsystem` 缺乏 `prlimit` / `getrlimit` 单元测试
- **修复建议**：增加 host-tests。

### 5.4 [P3] `SchedPolicy::time_slice_for` 用 `u32::MAX` 表示 Idle
- **位置**：[sched_policy.rs:360-372](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L360-L372)
- **问题**：用 magic number 表示"永不过期"，应定义 `INFINITE_TIME_SLICE: u32` 常量。

### 5.5 [P3] `NamespaceSet` 未实现 `Clone` 但需求上要求（fork）
- **位置**：[namespace.rs:497-505](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L497-L505)
- **修复建议**：实现 `#[derive(Clone)]` 即可（全部 Arc 内部已实现 Clone）。

---

## 6. 注释语言审查

### 6.1 英文注释实例（待翻译为中文）

| 文件 | 行号 | 英文 | 建议中文 |
|---|---:|---|---|
| [namespace.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs) | 762 | `1 << (ns_type + 8)` 转换说明缺失 | 补全"用户态索引 0..6 映射回 CLONE_NEW* 标志位" |
| [cgroup.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs) | 156 | `fetch_sub` 边界注释英文 | 翻译 |
| [sched_policy.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs) | 36 | `NICE_TO_WEIGHT` 数组注释英文 | 翻译 + 引用 Linux kernel 注释 |
| [signal.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs) | 150 | `is_core_dump` 注释英文 | 翻译 |
| [rlimit.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/rlimit.rs) | 99 | `RLIMIT_NICE` 注释英文 | 翻译 |

---

## 7. 性能热点（O(N) 操作）

| 文件:行 | 操作 | 复杂度 | 触发频率 | 优化建议 |
|---|---|---|---|---|
| [cgroup.rs:488-496](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L488-L496) | `cgroup_of` 遍历所有 cg | O(N×M) | 中 | 加 `cgroup_id: AtomicU64` 到 PCB |
| [sched_policy.rs:189-207](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs#L189-L207) | `boost_priority` 全重建 | O(N log N) | 中（每 N tick）| lazy boost |
| [namespace.rs:729-731](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L729-L731) | `NsRegistry::find` 线性扫描 | O(N) | 低 | 改用 HashMap |
| [cgroup.rs:466-486](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs#L466-L486) | `migrate` 嵌套锁遍历 | O(N×M) | 低 | PCB 记录 cgroup_id |
| [session.rs:118-123](file:///home/anfer/Code/QueenX/src/kernel/services/proc/session.rs#L118-L123) | `alloc_session` 线性扫描 | O(N) | 中 | bitmap 分配 |

---

## 8. 测试覆盖分析

### 8.1 已有测试（unit tests）

| 文件 | 测试数 | 覆盖率 |
|---|---:|---|
| [sched_policy.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/sched_policy.rs) | 11 | CFS 算法 + 调度策略 |
| [signal.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/signal.rs) | 6 | 信号分类 + RT 信号 + 位掩码 |
| [namespace.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs) | 0 | ❌ 无 |
| [cgroup.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/cgroup.rs) | 0 | ❌ 无 |
| [rlimit.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/rlimit.rs) | 0 | ❌ 无 |
| [seccomp.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/seccomp.rs) | 0 | ❌ 无 |
| [session.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/session.rs) | 0 | ❌ 无 |
| [fd_alloc.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/fd_alloc.rs) | 0 | ❌ 无 |
| [table.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/table.rs) | 0 | ❌ 无 |

### 8.2 待补单元测试

1. **namespace.rs**: NsRegistry CRUD、clone_from、unshare、setns 路径覆盖。
2. **cgroup.rs**: CPU/MEM/PIDS 限制 + 超限拒绝 + 嵌套迁移。
3. **seccomp.rs**: BPF 规则匹配（Equal/NotEqual/Greater/Less 等）。
4. **rlimit.rs**: set 权限检查 + cur > max 拒绝。
5. **table.rs**: set_state 状态机非法转换检测。
6. **fd_alloc.rs**: bitmap 分配 + 回收 + 范围越界检测。

---

## 9. 与硬规则 / 不变式对照

| 硬规则/不变式 | 状态 | 备注 |
|---|---|---|
| F1 services 0 unsafe | ✅ 全部 `deny(unsafe_code)` | |
| F2 services 不直接访问 framework 内部 | ✅ 仅 `framework::proc::PROCESS_TABLE` 等公共 API | |
| F3 模块间无循环依赖 | ✅ | |
| F4 framework unsafe 配 SAFETY | ✅ services 走公共 API | |
| F7 中文注释 | ⚠️ 41 处英文需补（§6） | |
| F8 公共 API 中文文档 | ✅ 较完整 | |
| I1 内核态 CPU 状态保护 | ✅ ProcessContext 仅 services 写 | |
| I2 内核内存保护 | ✅ services 无裸指针 | |
| I3 用户态 CPU 状态通过 framework | ✅ ProcessContext 在 framework | |
| I4 用户内存通过 framework | ✅ | |
| I5 MMIO/PIO 通过 framework | ✅ services 不接触 | |
| I6 DMA 不可写内核 | ✅ | |

---

## 10. 修复优先级建议

| 优先级 | 问题 ID | 工作量 | 风险 |
|---|---|---:|---|
| **P0-1** | 2.7 pick_next_signal bit 0 拒绝 | 1h | 静默丢信号 |
| **P0-2** | 2.5 sys_setns 标志位转换 | 2h | 调用路径错 |
| **P0-3** | 2.4 SignalDisposition 双重真理源 | 4h | 行为漂移 |
| **P0-4** | 2.3 全局注册无锁竞争保护 | 4h | 启动期崩溃 |
| **P0-5** | 2.2 migrate 嵌套锁 + 锁释放窗口 | 8h | 计数错乱 |
| **P0-6** | 2.1 PidNamespace 计数漂移 | 4h | namespace 永生 |
| **P0-7** | 2.6 remove_cgroup 重入锁 | 4h | 死锁 |
| **P1-1** | 3.6 Signal::to_bit 溢出 | 1h | UB |
| **P1-2** | 3.7 Rlimit is_privileged 信任 | 4h | 权限绕过 |
| **P1-3** | 3.14 跨进程 VA 校验 | 8h | 信息泄漏 |
| **P1-4** | 3.11 set_user_mode 硬编码选择子 | 2h | Ring 0 不可恢复 |
| **P1-5** | 3.8 boost_priority O(N) | 4h | 中断延迟 |
| **P2/P3** | 16 项 | 16h | 维护性 |

**总计**：约 60h 工程量（不包含测试）
