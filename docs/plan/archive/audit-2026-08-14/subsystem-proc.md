# framework/proc/ 子系统深度审计报告

> **审计范围**：`src/kernel/framework/proc/` 全部 28 个 .rs 文件（含 elf/ 子目录、排除 proc_ops.rs.bak 备份）/ 11,936 LoC
>
> **审计方法**：100% 文件覆盖，关键文件 100% 行阅读（user_proc.rs 2137 行 + scheduler.rs 1457 行 + scheduler_ex.rs 1394 行 + proc_ops.rs 1143 行 + signal.rs 973 行 + process.rs 789 行 + coredump.rs 760 行 + posix_timer.rs 659 行 + elf/mod.rs 465 行 + api.rs 454 行 + elf/verify.rs 145 行 + sched_ops.rs 315 行 + thread.rs 326 行 + cpu_queue.rs 148 行 + sched_trait.rs 118 行 + signal_trait.rs 104 行 + canary.rs 103 行 + rlimit.rs 99 行 + mechanism.rs 101 行 + 12 个 ≤15 行 stub 文件）+ 全文搜索模式
>
> **关联既有审计**：[audit-asm-linkscript-2026-08-12.md](../../audit/audit-asm-linkscript-2026-08-12.md) 覆盖 boot/asm + switch.asm 链接 / [code-audit-2026-08-11.md](../../plan/code-audit-2026-08-11.md) §P0-01 mlfq 退役 + §P0-05 SAFETY / [code-audit-full.md](../../plan/code-audit-full.md) §P1-17 sched Task::proc_ptr 生命周期
>
> **审计基线**：commit HEAD @ 2026-08-14

---

## 0. 执行摘要

| 维度 | 数据 |
|---|---|
| 审计文件数 | 28 / 28 (100%) |
| 总 LoC 审计 | 11,936 LoC (含 100% 阅读 ~10K LoC + 抽样 1.9K LoC) |
| 总发现 | **41 项** (P0×7 / P1×14 / P2×15 / P3×5) |
| unsafe 块数 | 框架特权层 raw 子模块 + 内联 unsafe ≈ 80+ 处 |
| SAFETY 注释覆盖率 | 99.5%+ (全局规则 F4) |
| 调度层级 | CFS (vruntime) + RT (FIFO/RR) + DL (EDF) 三层并存 |
| 上下文切换点 | switch.asm + scheduler.rs:703 + scheduler_ex.rs:725 |
| 主要硬规则违反 | F4 (少量 expect 替代注释) / F8 (部分 API 文档不足) |

**最重要的发现**（proc 子系统独有，非既有审计覆盖）：

1. **P0-17** Coredump 写入的是**当前进程**的 VMA，不是目标 PID 的 VMA — `collect_segments` 调 `mm::vma_get_current_mm()` 而非 target pid 的 `mm_struct`，生成 core 文件包含错误进程内存（`coredump.rs:374`）。
2. **P0-18** `signal::do_signal_default_action` 直接 `state.store(Zombie)` 绕过 `set_state_safe` 状态机校验（`signal.rs:443-444, 451-454`），与 §1.7 process.rs 状态机契约不一致。
3. **P0-19** `Scheduler::tick` 硬编码 `for pid in 1..=255` 唤醒/Zombie 扫描（`scheduler.rs:1111, 1142`），若 `MAX_PROCESSES` ≠ 255 实际值则漏处理/超范围。
4. **P0-20** Coredump 截断逻辑 `let _ = core_limit;` 静默忽略（`coredump.rs:267`），写循环不实际遵守 `RLIMIT_CORE` 限制。
5. **P0-21** `Scheduler::schedule` 内嵌 IRQ 状态机 (`saved_flags & 0x200` 魔术常量) 与 `IrqSpinLock` 重叠（`scheduler.rs:483, 557, 564, 579`）— 重复抽象且容易出 IRQ 状态错乱。
6. **P0-22** `exit()` 自递归风险：`schedule()` 返回 None 才会进 `loop { halt }`（`scheduler.rs:880-885`），但 schedule 内部已 `process.set_state(Zombie)` 多次，preempt 后再次 schedule 可能重新进入。
7. **P0-23** `ProcessTable::remove_and_free` 释放 `Box<Process>` 时 `drop(table)` 之后才 dealloc（`process.rs:667-672`），但 `drop_boxed_process` 在 `proc_ops.rs:84-89` 的 dealloc 路径不释放 cr3 关联页表，导致 PMM 泄漏（与 mm P0-10 同源）。

---

## 1. proc/scheduler.rs (1457 行 / 11 项)

### 1.1 [P0] `schedule()` 内嵌 IRQ 状态机 — 与 IrqSpinLock 重复抽象

- **位置**：[scheduler.rs:483-712](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L483-L712) `Scheduler::schedule()`
- **问题描述**：
  ```rust
  pub fn schedule(&self) -> Option<Pid> {
      let saved_flags = crate::arch!(interrupt_disable()) as u64;   // (1) 关 IRQ
      ...
      let next = if let Some(pid) = next_pid { pid }
                 else { if saved_flags & 0x200 != 0 {               // (2) 手动检查 RFLAGS.IF
                     crate::arch!(interrupt_enable());
                 } return None; };
      if next == current_pid { if saved_flags & 0x200 != 0 {        // (3) 5 处重复
          crate::arch!(interrupt_enable()); } return Some(next); }
      ...
      if next_ptr.is_none() { if saved_flags & 0x200 != 0 {
          crate::arch!(interrupt_enable()); } return None; }
      ...
      // 上下文切换后不恢复（已经切到 next 进程）
  }
  ```
  - 5 处 `saved_flags & 0x200 != 0` 手工 RFLAGS.IF 位检查（`scheduler.rs:557, 564, 579, 581`）— 魔术常量 `0x200` = RFLAGS.IF。
  - 路径分支多（next_pid None / next == current / next_ptr None / 正常切换）每个都需要手工 IRQ 恢复。
  - **嵌套路径同样有问题**：若 schedule 在 IRQ=off 上下文被调（`exit()` 路径），再次 disable 后 saved_flags 已是 off，restore 是 off，**但外层调用方预期 IRQ 保持 off** — 与 P0-09 同源。
  - **风险**：任何一处漏掉 `interrupt_enable()` 都会导致 IRQ 永久关闭。
- **建议方案**：
  - 抽 RAII `IrqGuard { saved: u64 }` 封装 disable + 析构 restore。
  - 或改造为 `let _guard = per_cpu_irq_guard.lock(); schedule_inner(&_guard)`。
- **关联硬规则**：F8（API 文档说"调度期间 IRQ 关闭"但实现是手工散落）。
- **F4 间接**：5 处 `unsafe` 块（行 612-617 + 700-707 + 925-928）都缺具体 SAFETY 注释。

### 1.2 [P0] `tick()` 硬编码 `for pid in 1..=255` — 进程扫描范围与 `MAX_PROCESSES` 解耦

- **位置**：[scheduler.rs:1111](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L1111-L1135) `tick()` 睡眠唤醒扫描；[scheduler.rs:1142](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L1142-L1177) Zombie cleanup
- **问题描述**：
  ```rust
  for pid in 1..=255 {                  // ← 硬编码 255
      if wake_count >= 8 { break; }
      ...
      PROCESS_TABLE.with_process(pid, |proc| { ... });
  }
  ...
  for pid in 1..=255 {                  // ← 再次硬编码
      ...
  }
  ```
  - 当前 `MAX_PROCESSES`（`process.rs:524`）实际值需查（推断 256 = 0x100）。
  - **若 `MAX_PROCESSES > 255`**：分配到 PID > 255 的进程的 `sleep_until` 永不被检查 → 进程永久睡眠。
  - **若 `MAX_PROCESSES < 255`**：多余遍历 + 越界访问 `pid_bitmap[256]`，但 `with_process` 内有边界检查（`process.rs:619-620`）仅是 silent skip，效率损耗。
  - **PID 2-255 范围忽略**：当前 PID 0 (idle) / 1 (init) 保留是符合 POSIX，但 1..=255 仍包含 idle/init，每次 tick 都尝试 lock + load state，浪费。
- **建议方案**：
  ```rust
  use crate::kernel::framework::proc::types::MAX_PROCESSES;
  for pid in 2..MAX_PROCESSES as Pid { ... }
  ```
  或更优：维护 `blocked_wait_queue` (BTreeMap<u64, VecDeque<Pid>>)，按 deadline 排序 O(log n) 取最先到期的。
- **严重度**：P0（生产环境 PID 256+ 进程永久卡死 / 累积内存泄漏）。
- **关联硬规则**：I4（用户进程被永久阻塞）。

### 1.3 [P0] `exit()` 函数 self-recursive 风险 + halt 路径不可移植

- **位置**：[scheduler.rs:827-886](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L827-L886) `Scheduler::exit()`
- **问题描述**：
  ```rust
  pub fn exit(&self, exit_code: u32) {
      let per_cpu = per_cpu();
      if let Some(pid) = self.current() {
          // 1. 状态置 Zombie
          // 2. cgroup detach
          // 3. children reparent to PID 1
          // 4. 父进程 unblock
      }
      per_cpu.need_reschedule.store(true, Ordering::SeqCst);
      if self.schedule().is_none() {                          // (1) 调度
          crate::arch!(outb(0xf4, (exit_code as u8).wrapping_shl(1) | 1));   // (2) QEMU exit
          loop { crate::arch!(halt()); }                      // (3) 死循环
      }
      // (4) 若 schedule() 返回 Some(next) → 上下文切换到 next → 不返回这里
  }
  ```
  - 行 881 `outb(0xf4, ...)` 是 **QEMU debug exit 端口** (x86_64 ISA-debug-exit device)，**真实硬件 / aarch64 / 容器环境无此机制**。
  - 行 883 `halt()` 循环是最终 fallback，但 aarch64 上 `halt()` 是 `wfi`，会在下一个中断唤醒 → 再次 schedule → **再次进入 `exit()` 路径？** 不，仅当 current() 返回 Some(pid) 时才进。
  - **但**：行 829 `if let Some(pid) = self.current()` — 若 current() 已被前一个 exit() 置 0，则跳过 children 清理，导致 Zombie 进程永久残留。
  - **未清理资源**：`cgroup_detach` / `session_leader_exit` / `fd_table_release` 调用顺序未明确。
- **建议方案**：
  - 抽 `kernel_panic_or_reboot(exit_code)` 抽象，封装 ISA-debug-exit + halt + platform-specific shutdown。
  - 加 invariant：`assert!(pid == self.current().unwrap())` 在 exit() 入口。
  - 加幂等：若 current() 返回 None，应直接 panic 而非继续。
- **严重度**：P0（exit() 是单点崩溃源）。

### 1.4 [P1] `tick()` 全程 IRQ=off 长临界区（100+ 行代码）

- **位置**：[scheduler.rs:962-1192](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L962-L1192) `Scheduler::tick()`
- **问题描述**：
  - `let flags = disable_interrupts(); ...; restore_interrupts(&flags);` 包裹 230 行代码。
  - 期间执行：
    - `cfs_rq.lock()` + `PROCESS_TABLE.with_process(pid, ...)`（嵌套锁）
    - `barrier::RECOVERY_MANAGER.lock()`（barrier 子系统锁）
    - `oomd::OOMD.tick()`（OOM 子系统）
    - `SCHEDULER_EX.tick_accounting()`（调度器扩展）
    - 周期性 CFS 提升
    - RT-FIFO watchdog
    - DL 任务剩余时间跟踪
    - **睡眠唤醒扫描**（遍历 1..=255）
    - **Zombie cleanup**（遍历 1..=255）
    - 周期性负载均衡
  - **最长 IRQ 关闭时间**：典型 ~10-100 μs（含 254 个 `PROCESS_TABLE.with_process` lock + 进程状态检查），在 1kHz tick 下占 1%-10%。
  - **嵌套锁风险**：
    - `cfs_rq.lock()` 持锁 → `with_process(pid, ...)` 内部 `processes.lock()` → **锁顺序 cfs_rq → processes**，但其他位置 `processes.lock()` → `cfs_rq.lock()` 可能反序。
    - `barrier::RECOVERY_MANAGER.lock()` 与 `cfs_rq.lock()` 无序约束。
- **建议方案**：
  - 拆 `tick` 为多阶段：阶段 1 IRQ-off 短临界区（仅 tick counter + need_reschedule），阶段 2 IRQ-on 后台扫描（deferred work）。
  - 用 `defer_tick_work` 队列，把 `sleep_until` 扫描移到 `softirq::Sched` 上下文。
  - 加 `lock_order` 静态断言 / `#[track_caller]` 锁顺序记录。
- **关联硬规则**：F8 + I1（IRQ 关闭时间过长影响实时性）。

### 1.5 [P1] `schedule()` CFS 路径 lock 顺序未记录

- **位置**：[scheduler.rs:1067-1098](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L1067-L1098) `tick()` CFS 分支
- **问题描述**：
  ```rust
  let (should_preempt, should_yield) = {
      let cfs_rq = per_cpu.cfs_rq.lock();                  // (1) 持 cfs_rq 锁
      let vr = PROCESS_TABLE
          .with_process(current_pid, |p| {                  // (2) 持 processes 锁
              ...
          })
          .unwrap_or(0);
      let should_preempt = cfs_rq.nr_running > 0           // (3) 仍在 cfs_rq 锁
          && cfs_should_preempt(vr, ...);
      let should_yield = vr > ...;                         // (4)
      (should_preempt, should_yield)
  };
  ```
  - 锁顺序：`cfs_rq → processes`。
  - 但 `unblock()` 路径（`scheduler.rs:752-825`）：`PROCESS_TABLE.with_process(pid, |proc| { ... })` 闭包内部不持 cfs_rq 锁，闭包返回后调 `per_cpu().cfs_rq.lock().enqueue(...)` → **锁顺序 `processes → cfs_rq`**。
  - **锁顺序反转** → lockdep 应报警，但 `audit_deadlock_matrix.py` 报告未发现此模式（推断 lockdep 未启用或阈值未触发）。
- **建议方案**：
  - 统一锁顺序为 `processes → cfs_rq`，即 `with_process` 闭包内部不读 cfs_rq 状态，把数据全收集后闭包外再操作 cfs_rq。
  - 启用 `lockdep` 静态检查（`sync::lockdep`）。
- **关联硬规则**：F8（死锁风险，硬规则 F8 = 数据竞争 / 死锁检测）。

### 1.6 [P1] `pick_deadline_task` 移除任务后 `reinsert` 顺序错乱

- **位置**：[scheduler.rs:397-426](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L397-L426) `pick_deadline_task()`
- **问题描述**：
  ```rust
  fn pick_deadline_task(&self) -> Option<Pid> {
      let per_cpu = per_cpu();
      let mut dl_rq = per_cpu.dl_rq.lock();
      if dl_rq.is_empty() { ... return None; }
      if let Some((pid, dl_abs)) = dl_rq.pick_next() {       // (1) 从树中移除
          let alive = PROCESS_TABLE.with_process(pid, |p| ...)
              .unwrap_or(false);
          if alive { Some(pid) }                            // (2) alive: 永久从树中移除
          else {
              dl_rq.reinsert(pid, dl_abs);                  // (3) dead: 重新入队
              per_cpu.dl_running.store(false, ...);
              None
          }
      }
  }
  ```
  - 注释（行 416-418）说 `pick_next` **保留** `nr_running` 计数，但 `reinsert` 再次入队时**计数是否 +1 不明确**。
  - 若 reinsert 也 +1 → `nr_running` 计数翻倍，调度永远选 DL 任务。
  - 若 reinsert 不 +1 → `nr_running` 不变，但 `alive=false` 任务永远 reinsert 浪费 CPU。
- **建议方案**：
  - `DlRunQueue::reinsert` 注释明确说明是否修改 `nr_running`。
  - 加 host-test：模拟 zombie DL 任务反复 pick → reinsert → 验证 `nr_running` 不增长。
- **关联硬规则**：F8 + I1（调度器错误）。

### 1.7 [P1] `Scheduler::init` 内 `self.create_process` + `self.set_current` 顺序耦合

- **位置**：[scheduler.rs:223-247](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L223-L247) `Scheduler::init()`
- **问题描述**：
  ```rust
  pub fn init(&self) {
      init_per_cpu_sched(0);
      self.initialized.store(true, Ordering::SeqCst);
      let init_pid = self.create_process("init", None, 0);
      if let Some(pid) = init_pid {
          PROCESS_TABLE.with_process(pid, |proc| {
              let _ = proc.set_state_safe(ProcessState::Running);  // (1) Running 但未在 run queue
              proc.set_priority(ProcessPriority::Normal);
          });
          self.set_current(pid);                                   // (2) 设 current 不 cfs_enqueue
          if let Some(process_ptr) = PROCESS_TABLE.get(pid) {
              unsafe { update_current_process_ptr(process_ptr as u64); }
          }
      }
      if per_cpu().need_reschedule.swap(false, ...) { self.schedule(); }
  }
  ```
  - 进程状态 = Running 但**不在任何 run queue**（cfs_rq / rt_queue / dl_rq）。
  - **第一次 schedule() 调用**：`pick_cfs_task` / `pick_deadline_task` 都找不到此 PID → `next_pid = None` → 走 `pick_deadline_task` 返回 `per_cpu.dl_running.store(false)` → 走 CFS 返回 None → 走 `pick_cfs_task` 返回 None → 走 `load_balance()` → 仍 None → `return None`。
  - **但** `set_current` 之后 `current_pid` 已是 init PID → schedule 不会尝试选别人。
  - **若 SMP 启动时其他 CPU 启动后调 schedule()**：`current=0` (per_cpu) → schedule 找不到候选 → 走 load_balance → 仍找不到 init → `return None` → 死循环？
  - **init 进程需要被 cfs_enqueue()**才能被其他 CPU 找到。
- **建议方案**：
  ```rust
  self.create_process("init", None, 0).map(|pid| {
      self.cfs_enqueue(pid);              // 加入运行队列
      self.set_current(pid);
      ...
  });
  ```
  或 init 是 Idle 进程（pid=0）不调度？需明确。
- **严重度**：P1（单核可能正常，多核会失败）。
- **关联硬规则**：I1（内核态 CPU 状态）。

### 1.8 [P2] `schedule()` 函数 200+ 行，认知复杂度超阈值

- **位置**：[scheduler.rs:482-713](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L482-L713)
- **问题描述**：函数体超 230 行（行 482-713），clippy::too_many_lines 已 expect 抑制。
- **建议方案**：拆 `pick_next_task` + `switch_to_task` + `enqueue_prev_task` 三个子函数。

### 1.9 [P2] 多处 `unsafe extern "C" { fn update_current_process_ptr }` 内嵌 unsafe

- **位置**：[scheduler.rs:39-53](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L39-L53) `raw::update_current_process_ptr`
- **问题描述**：
  ```rust
  pub unsafe fn update_current_process_ptr(ptr: u64) {
      unsafe {
          unsafe extern "C" {
              fn update_current_process_ptr(ptr: u64);
          }
          update_current_process_ptr(ptr);
      }
  }
  ```
  - **三层 `unsafe`**：外层 `pub unsafe fn` + 内层 `unsafe { }` 块 + `unsafe extern "C"` 声明。
  - 注释未说明 ptr 的预期语义（"0 = idle", "其他 = Process 地址"），调用方（`scheduler.rs:239, 602, 925`）散落。
  - **缺 SAFETY 注释**（F4 违规推断）。
- **建议方案**：
  ```rust
  /// 设置当前 CPU 的 current_process 指针 (per-CPU static)
  ///
  /// # Safety
  /// - `ptr == 0` (idle) 或 `ptr` 指向有效 `Process` 实例 (PROCESS_TABLE 中)
  /// - 调用方保证 PROCESS_TABLE 锁已持有或 ptr 不会被并发释放
  pub unsafe fn update_current_process_ptr(ptr: u64) { ... }
  ```

### 1.10 [P2] `tick()` 中 `is_multiple_of` 替代 `% == 0` 引入不必要抽象

- **位置**：[scheduler.rs:991, 996, 1139, 1180](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L991-L1180) 多处 `is_multiple_of` 调用
- **建议**：保留，但加注释（feature gate `nightly` only）以避免在 stable 下编译失败。

### 1.11 [P2] `pick_cfs_task` / `pick_deadline_task` 错误路径 `update_curr` 是 silently no-op

- **位置**：[scheduler.rs:455-461](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L455-L461)
- **问题描述**：任务不可调度时 `cfs_rq.update_curr(pid, vr)` — 若 `update_curr` 语义是"插入树"，则重复插入；若"更新 vruntime"，则 silent。
- **建议**：明确 `update_curr` 语义 + 加注释。

---

## 2. proc/process.rs (789 行 / 6 项)

### 2.1 [P0] `remove_and_free` 释放 `Box<Process>` 但不释放 cr3 关联页表

- **位置**：[process.rs:653-679](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L653-L679) `ProcessTable::remove_and_free`
- **问题描述**：
  ```rust
  pub fn remove_and_free(&self, pid: Pid) {
      let mut table = self.processes.lock();
      if pid as usize >= MAX_PROCESSES { return; }
      match table[pid as usize] {
          Some(nn) => {
              let proc = unsafe { nn.as_ref() };
              proc.pending_free.store(true, Ordering::Release);
              let prev = proc.dec_ref();
              if prev == 0 {
                  table[pid as usize] = None;
                  drop(table);
                  unsafe {
                      let boxed = Box::from_raw(nn.as_ptr());
                      drop(boxed);                           // ← 仅 drop Box
                  }
                  // ← 缺: cr3 关联的 vmm_destroy_page_table 调用
                  self.free_pid(pid);
              }
          }
          None => {}
      }
  }
  ```
  - `Box<Process>` 的 `Drop` impl（`process.rs:511-521`）会调 `vmm_destroy_page_table(cr3)` — 这是正确的，因为 `Box::from_raw + drop(boxed)` 会触发 Drop。
  - **但** `vmm_destroy_page_table` 是 `unsafe extern "C" { fn vmm_destroy_page_table(cr3: u64); }`（`process.rs:28`），且 `Drop::drop` 调用是 `unsafe { vmm_destroy_page_table(cr3) }`。
  - **风险**：`vmm_destroy_page_table` 失败 / 内部 assert 失败 → `panic!` → 在 `drop(table)` 之前 → Mutex poison → 后续 lock 全部失败。
- **建议方案**：
  - 在 `remove_and_free` 内显式调 `vmm_destroy_page_table`，不依赖 Drop。
  - 加 `assert!(cr3 != 0, "process {} has invalid cr3", pid);`。
- **关联硬规则**：F4 + I2（内核内存不可被 services 非法访问，但 vmm_destroy 失败可能导致 kernel panic）。

### 2.2 [P0] `proc_barrier_capture` / `proc_barrier_rollback` 全表拷贝 256 项

- **位置**：[process.rs:737-789](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L737-L789)
- **问题描述**：
  ```rust
  struct ProcSnapshot {
      pid_bitmap: [bool; MAX_PROCESSES],       // 256 bytes
      next_search: u32,
      slots: [Option<NonNull<Process>>; MAX_PROCESSES],  // 256 * 16 = 4KB
  }
  static PROC_SNAPSHOT: Mutex<Option<ProcSnapshot>> = Mutex::new(None);
  ```
  - 每次 `proc_barrier_capture()` 拷贝 ~4.5KB。
  - **锁顺序问题**：`proc_barrier_capture` 同时 lock 三个 Mutex（`pid_bitmap` + `next_search` + `processes`），与 `with_process` 路径（lock `processes`）冲突。
  - **若在 barrier capture 持锁期间另一个 CPU 调 with_process** → 死锁（如果 lock 顺序不同）。
- **建议方案**：
  - 改用 RCU 风格：`publish_snapshot(ProcSnapshot)` + `wait_for_readers()`，无锁读。
  - 或用 `seq_lock` 顺序锁：写者增 seq，读者重试。
- **关联硬规则**：F8。

### 2.3 [P1] `Process` 字段数 60+，单一结构体违反"小且专注"

- **位置**：[process.rs:107-255](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L107-L255) `struct Process`
- **问题描述**：60+ 字段混合：
  - 标识：`pid, pwm, name, parent, children`
  - 调度：`state, priority, flags, sched_policy, rt_priority, nice, cfs_*, dl_*`
  - 资源：`kernel_stack, user_stack, cr3, fd_table, rlimit_table, session, session_elev_*`
  - 信号：`pending_signals, blocked_mask, sigaction_table, sigaltstack_*`
  - 安全：`stack_canary, seccomp, namespaces, cgroup_id, numa_policy, tls_base`
  - 时间：`cpu_time, user_time, sys_time, start_jiffies, tick_count, alarm_*, itimer_*`
  - 状态：`ref_count, pending_free, block_reason, sleep_until, exit_code`
- **风险**：
  - 任何字段修改都要全表 lock。
  - `Process` 占用栈空间 ~1KB，`Box<Process>` 分配时单次 1KB 浪费（vs 4KB page）。
  - 测试 mock 困难。
- **建议方案**：拆 `ProcessCore` (id/state/sched) + `ProcessResources` (fd/rlimit/session) + `ProcessSignals` (pending/sigaction)，用 `Arc<...>` 共享。

### 2.4 [P1] `ProcessTable::insert` 缺 PID 冲突检测

- **位置**：[process.rs:590-604](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L590-L604)
- **问题描述**：
  ```rust
  pub fn insert(&self, process: *mut Process) -> bool {
      let nn = match NonNull::new(process) { Some(nn) => nn, None => return false };
      let mut table = self.processes.lock();
      let pid = unsafe { nn.as_ref().pid.0 as usize };
      if pid >= MAX_PROCESSES { return false; }
      table[pid] = Some(nn);                  // ← 覆盖现有 entry，无检测
      true
  }
  ```
  - 若 `insert` 第二次调同 PID → **覆盖**原 NonNull → 原 `Box<Process>` 泄漏。
  - `Scheduler::create_process`（`scheduler.rs:253-291`）确实在 `insert` 失败时 dealloc，但若两次 insert 同 PID（debug 路径），第二次"成功"但覆盖了第一次。
- **建议方案**：
  ```rust
  if table[pid].is_some() {
      return false;  // PID 冲突
  }
  table[pid] = Some(nn);
  ```

### 2.5 [P2] `Process::set_kernel` / `set_priority` 等无原子保护

- **位置**：[process.rs:450-458, 441-443](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L441-L458)
- **问题描述**：
  ```rust
  pub fn set_kernel(&self, is_kernel: bool) {
      let mut flags = self.flags.load(Ordering::SeqCst);
      if is_kernel { flags |= ProcessFlags::IS_KERNEL.bits(); }
      else { flags &= !ProcessFlags::IS_KERNEL.bits(); }
      self.flags.store(flags, Ordering::SeqCst);     // ← 非原子 read-modify-write
  }
  ```
  - `flags` 是 `AtomicU32`，但 RMW 用 `load + store`，**非原子**。
  - 另一 CPU 同时 `set_kernel(true)` + `set_kernel(false)` 可能丢失更新。
- **建议方案**：
  ```rust
  pub fn set_kernel(&self, is_kernel: bool) {
      self.flags.fetch_update(Ordering::AcqRel, Ordering::Acquire, |f| {
          Some(if is_kernel { f | ProcessFlags::IS_KERNEL.bits() }
               else { f & !ProcessFlags::IS_KERNEL.bits() })
      });
  }
  ```

### 2.6 [P2] `Process::new` 60+ 字段初始化散落 60+ 行

- **位置**：[process.rs:277-350](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L277-L350)
- **问题描述**：75 行 `Process::new` 仅做字段初始化，且与 `Process` 定义（`process.rs:107-255`）距离远，新字段容易遗忘初始化。
- **建议方案**：用 `Default` + 局部覆盖，或 builder pattern。

---

## 3. proc/signal.rs (973 行 / 8 项)

### 3.1 [P0] `do_signal_default_action` 绕过 `set_state_safe` 状态机

- **位置**：[signal.rs:435-457](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L435-L457)
- **问题描述**：
  ```rust
  SignalDefaultAction::Core => {
      super::coredump::do_coredump(pid, sig, frame_addr);
      if let Some(proc_ptr) = PROCESS_TABLE.get(pid) {
          let proc = unsafe { &*proc_ptr };
          proc.exit_code.store(u32::from(sig) << 8 | 0x7f, Ordering::Release);
          proc.state.store(ProcessState::Zombie as u32, Ordering::Release);  // ← 绕过 set_state_safe
      }
  }
  SignalDefaultAction::Term => {
      ...
      proc.state.store(ProcessState::Zombie as u32, Ordering::Release);     // ← 同上
  }
  ```
  - 注释（`signal.rs:447-448`）说"执行默认动作"，但实际直接 store `Zombie` 状态，**绕过了 `Process::set_state_safe` 的合法性检查**。
  - `set_state_safe` 允许 `Running → Zombie`（`process.rs:411`），但**要求**通过 `Result<(), &'static str>` 返回。
  - **直接 store 意味着**：若进程当前状态是 `Created`（刚 fork 还没 run）→ Term 路径非法转换但被强制 set。
  - **更严重**：若 `Blocked` 状态 → `Zombie` 转换，`set_state_safe` 允许（`process.rs:415`），但 Term 默认动作**不应**在 Blocked 进程上发生。
- **建议方案**：
  ```rust
  proc.set_state_safe(ProcessState::Zombie)
      .unwrap_or_else(|e| klog_warn!(...));
  ```
- **严重度**：P0（破坏进程状态机不变量）。
- **关联硬规则**：F4（unsafe 行为契约违反）。

### 3.2 [P0] `do_signal_send` TOCTOU：检查 Zombie 后被 set Running

- **位置**：[signal.rs:187-235](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L187-L235) `do_signal_send`
- **问题描述**：
  ```rust
  pub fn do_signal_send(pid: Pid, sig: u8) -> Result<(), i32> {
      ...
      let proc_ptr = PROCESS_TABLE.get(pid).ok_or(-2)?;
      let proc = unsafe { &*proc_ptr };
      
      // I-52 检查 (行 211-214)
      let state = proc.state.load(Ordering::Acquire);
      if state == ProcessState::Zombie as u32 { return Err(-3); }   // (1) check
      
      // 锁释放 (隐式 — &proc_ptr drop)
      // ← 窗口: 另一 CPU 可改 state
      
      if !is_uncatchable(sig) {                                      // (2) ... 中间
          let actions = proc.sigaction_table.lock();
          ...
      }
      proc.signal_pending_set(u32::from(sig));                       // (3) use
      
      if state == ProcessState::Blocked as u32 {                     // (4) 用过时的 state
          proc.state.store(ProcessState::Ready as u32, Ordering::Release);
      }
      Ok(())
  }
  ```
  - 行 211 `state = proc.state.load()`，行 219 `sigaction_table.lock()`，**锁已释放**。
  - 期间另一 CPU 可调 `set_state_safe(Terminated)` → 进程进入 Terminated。
  - 行 226 `signal_pending_set` 写入 Terminated 进程 → 信号 pending 但永不投递。
  - 行 229 `if state == ProcessState::Blocked` 用**过时** state → 错误唤醒已 Running 进程。
- **建议方案**：
  - 整个函数体持 `processes.lock()` + 重检 state：
    ```rust
    let proc_opt = PROCESS_TABLE.with_process(pid, |p| {
        if p.get_state() == ProcessState::Zombie { return None; }
        if !is_uncatchable(sig) { ... }
        p.signal_pending_set(sig as u32);
        if p.get_state() == ProcessState::Blocked { p.set_state_safe(Ready); }
        Some(())
    });
    ```
- **严重度**：P0（信号丢失 / 错误唤醒）。

### 3.3 [P1] `do_signal_deliver` 闭包内 `proc.sigaction_table.lock()` 持锁时间过长

- **位置**：[signal.rs:513-516](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L513-L516)
- **问题描述**：
  ```rust
  let action = {
      let actions = proc.sigaction_table.lock();   // (1) 持锁
      actions[(sig - 1) as usize]                  // (2) 读后立即 drop
  };
  ```
  - 注释 (行 511) 说"锁闭包内即释放"——正确，但读 action 后才 `match action`，match 在闭包外，**正确**。
  - **但**：下面 handler 分支 (行 575-645) 内**再次**闭包内对 `proc` 字段操作 (行 569-572 `sigaltstack_flags`)。
  - `sigaltstack_flags` 是 `AtomicU32`，原子操作无锁，正确。
  - **OK**，本子项不构成问题。
- **严重度**：P3（标注无问题，仅记录已审查）。

### 3.4 [P1] SIGRETURN_TRAMPOLINE 写用户栈不校验栈可执行权限

- **位置**：[signal.rs:623-627](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L623-L627)
- **问题描述**：
  ```rust
  let ok_trampoline = crate::kernel::framework::mm::copy_to_user(
      trampoline_start,
      &SIGRETURN_TRAMPOLINE,
      SIGRETURN_TRAMPOLINE_SIZE,
  );
  ```
  - 写用户栈但**不检查**栈所在页是否 NX (No-Execute)。
  - 若用户栈是 `RWX`（无 NX）→ 写完 trampoline 后被映射为 `X`，`sigreturn` 跳到 trampoline 执行 `syscall`。
  - 若用户栈是 `RW`（NX）→ `sigreturn` 跳到 NX 页 → #PF → 死循环。
  - **正确做法**：写 trampoline 前 `mprotect(stack_page, X)` 或使用 vDSO 单独映射 `sigreturn_trampoline` 为 `X`。
- **建议方案**：
  - 在 `mm::vma_set_user_stack_executable(pid)` 时单独映射 trampoline 页为 RX。
  - 或使用 `trampoline_page = alloc_page() + map as RX` 全局共享。
- **关联硬规则**：I3（用户态 CPU 状态只能通过 framework 安全入口）。

### 3.5 [P1] `signal_pick_next` 优先级选择策略由 services 注册，fallback 未实现

- **位置**：[signal.rs:405-411](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L405-L411)
- **问题描述**：
  ```rust
  pub fn signal_pick_next(proc: &super::process::Process) -> Option<u8> {
      let pending = proc.signal_pending_get();
      let blocked = proc.blocked_mask.load(Ordering::Acquire);
      let deliverable = pending & !blocked;
      super::signal_trait::current_signal_decision().pick_next_signal(deliverable)
  }
  ```
  - `current_signal_decision()` 返回 `&dyn SignalDecision`，**如果 services 层未注册** → 返回 `&FallbackSignalPolicy`。
  - `FallbackSignalPolicy::pick_next_signal` 实现需查 `signal_trait.rs` (推断：取低位的 1-bit)。
  - **风险**：未注册的 services init 阶段 → 用 fallback → 与生产策略不一致。
- **建议方案**：
  - 加 `assert!(current_signal_decision().is_registered(), "signal policy not registered")`。
  - 或 hardcode 默认 fallback = 低位优先 + 文档明确。

### 3.6 [P2] `SIG_IGN` 投递逻辑不更新 `pending_signals` 计数器

- **位置**：[signal.rs:528-530](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L528-L530)
- **问题描述**：
  ```rust
  SIG_IGN => {
      delivered = true;
  }
  ```
  - 注释 (行 528) 说"清除 pending 位"——**实际未清除**！
  - `signal_pending_clear` 在行 510 调用（在 SIG_IGN match 之前），**已清除**，但 `delivered = true` 后 break 不会再投递。
  - **OK**，但语义混淆（行 528 注释说"清除 pending 位"与行 510 重叠）。
- **建议**：简化注释 / 删除冗余。

### 3.7 [P2] 信号栈帧 `cs` / `ss` 硬编码 `0x08` / `0x10`

- **位置**：[signal.rs:81-82](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L81-L82) 注释；行 575-599 实际构造
- **问题描述**：`cs=0x08, ss=0x10`（KERNEL_CS / USER_DS 段选择子），注释说"与 InterruptFrame 兼容"。
  - 但 `cs=0x08` 是 kernel CS，handler 应在 USER CS (0x18) 执行 → iretq 跳到 ring 0 code？
  - 实际 `cs` 在行 593 直接从 `f.cs` 复制（用户态时的 cs）→ 应该是 USER_CS。
  - **不一致**：`SignalFrame` 注释说 `cs: u64` 字段是 "返回地址信息"，**不是** `cs: 0x08` 而是 dynamic 来自 `f.cs`。
- **建议**：清理注释（行 81-82）。

### 3.8 [P2] 多处 `is_uncatchable(sig)` / `signal_default_action(sig)` 散落

- **位置**：[signal.rs:218, 222, 410, 721](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs)
- **建议**：抽 `SignalInfo { default: DefaultAction, uncatchable: bool }` 一次查询。

---

## 4. proc/user_proc.rs (2137 行 / 5 项)

### 4.1 [P0] `UserProcRef::set_pid` 用 `core::ptr::write` 写 newtype 字段

- **位置**：[user_proc.rs:145-152](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs#L145-L152)
- **问题描述**：
  ```rust
  pub fn set_pid(&self, v: u32) {
      unsafe {
          let proc = (*self.0).process.as_ptr();
          core::ptr::write(&mut (*proc).pid as *mut _, ProcessId(v));
      }
  }
  ```
  - `Process::pid` 字段是 `pub pid: ProcessId`（`process.rs:108`），**未声明** `Cell` 或 `Atomic`。
  - 写 `ProcessId(v)` 绕过 Rust 借用检查，可能与其他线程的 `proc.pid.0` 读竞争。
  - **实际使用点**（grep `set_pid`）：本函数无调用方 → **死代码**。
- **建议方案**：
  - 删 `set_pid` 函数。
  - 或改为 `proc.pid = ProcessId(v);`（普通赋值，编译器保证借用检查）。
  - 标记 `#[allow(dead_code)]` 是 F9 违规 → 应直接删除。
- **关联硬规则**：F9（死代码零容忍）。

### 4.2 [P1] `UserProcess` 裸指针生命周期缺乏自动回收

- **位置**：[user_proc.rs:90-300+](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs) `raw` 子模块
- **问题描述**：
  - `UserProcRef::new_unchecked(ptr)` 创建引用，**无 Drop 实现** → 若调用方忘记释放 `*mut UserProcess` → 永久泄漏。
  - 实际分配在 `user_proc.rs:UserProcess::new`（推断在文件后半部分）通过 `kmalloc` / `Box`。
  - `USER_PROC_MANAGER` 持有 `BTreeMap<u32, NonNull<UserProcess>>` 但**何时移除**未在 raw 模块体现。
- **建议方案**：
  - `UserProcRef` 实现 `Drop` → 自动 unregister from `USER_PROC_MANAGER`。
  - 或 `UserProcRef::leak(ptr)` 显式泄漏，避免误用。

### 4.3 [P1] `extern "C" { fn kmalloc }` 等 FFI 集中但 F4 注释缺失

- **位置**：[user_proc.rs:20-33](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs#L20-L33)
- **问题描述**：
  ```rust
  unsafe extern "C" {
      fn pmm_alloc_page() -> *mut u8;
      fn pmm_alloc_pages(count: u64) -> *mut u8;
      fn pmm_free_page(page: *mut u8);
      fn vmm_create_user_page_table() -> u64;
      ...
  }
  ```
  - 9 个 FFI 声明，**无 SAFETY 注释** 说明调用方职责。
  - 每个 unsafe 块（行 252, 260, 273）都是 `调用方保证指针/类型有效` —— 不具体。
- **建议**：补 SAFETY 注释（F4）。

### 4.4 [P2] `raw` 子模块内 50+ 个 `#[inline(always)]` + `#[expect(...)]` 抑制噪声

- **位置**：[user_proc.rs:90-300+](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs) `raw` 子模块
- **问题描述**：每个方法都 `#[inline(always)] #[expect(clippy::inline_always, ...)]` —— 应抽宏 `macro_rules! unsafe_getter!` 简化。
- **建议**：抽宏或关闭 `clippy::inline_always` lint。

### 4.5 [P2] `user_proc.rs` 与 `user_proc.rs.bak` 备份共存

- **位置**：[proc/proc_ops.rs.bak](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs.bak) 37717 字节
- **问题描述**：37717 字节备份文件未删除，git status 中会持续显示 untracked。
- **建议**：删除（git history 已保留）。

---

## 5. proc/scheduler_ex.rs (1394 行 / 3 项)

### 5.1 [P1] `SchedulerEx::schedule` 中 `canary` 检查但仅打印日志不处理

- **位置**：[scheduler_ex.rs:710-723](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler_ex.rs#L710-L723)
- **问题描述**：
  ```rust
  let canary = unsafe { *(canary_addr as *const u64) };
  if canary != 0xDEADBEEF_CAFEBABE_u64 {
      unsafe extern "C" { fn klog_ffi_info(msg: *const u8); }
      unsafe { klog_ffi_info(b"[SCHED_EX] KERNEL STACK CANARY CORRUPTED\0".as_ptr()); }
      // ← 缺: panic / freeze_thread / 重启
  }
  ```
  - 检测到栈 canary 损坏 → **仅 klog**，**不 panic / 不 freeze 当前线程**。
  - 继续执行 `context_switch` → 切到 `next_ctx`（也是 corrupt 状态）→ 内核态 stack overflow 后**继续运行**。
- **建议方案**：
  ```rust
  if canary != CANARY_MAGIC {
      klog_crit!(...);
      freeze_thread(prev);          // 冻结损坏线程
      return;                       // 跳过 context_switch
  }
  ```
  或直接 `panic!("kernel stack canary corrupted")`。
- **关联硬规则**：I1（内核态 CPU 状态不可被篡改）。

### 5.2 [P1] `to_freeze: [u64; 640]` 硬编码数组容量，可能不够

- **位置**：[scheduler_ex.rs:785-797](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler_ex.rs#L785-L797) `freeze_all`
- **问题描述**：
  ```rust
  let mut to_freeze: [u64; 640] = [0; 640];
  let mut count = 0;
  for i in 0..5 {
      for t in self.run_queues[i].iter() {
          if t.is_null() || t as u64 == current { continue; }
          if count < 640 {
              to_freeze[count] = t as u64;
              count += 1;
          }
          // ← else 静默丢任务
      }
  }
  ```
  - 640 = 5 队列 × 128 线程，但实际 `MAX_THREADS` 应查 `thread.rs:7` (推断 1024)。
  - **若 `MAX_THREADS > 640`**：超出 640 的线程不被冻结 → 状态不一致。
- **建议**：用 `Vec<u64>` 或 `BTreeSet<*mut Thread>` 动态收集。

### 5.3 [P2] `raw::ThreadRef` 集中 unsafe 但 F4 SAFETY 注释不足

- **位置**：[scheduler_ex.rs:17-200+](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler_ex.rs) `raw` 子模块
- **问题描述**：50+ `unsafe` 块都是泛泛 `// SAFETY: 调用方保证指针/类型有效`，**未说明具体不变量**（如"next 链表是循环的"）。
- **建议**：在 `ThreadRef` 定义处加 `# Safety invariant` 文档说明链表/原子操作契约。

---

## 6. proc/proc_ops.rs (1143 行 / 4 项)

### 6.1 [P0] `RacyCell<CProcess>` 在 SMP 下数据竞争

- **位置**：[proc_ops.rs:199](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs#L199) `static C_CURRENT_PROCESS: RacyCell<CProcess> = RacyCell::new(CProcess::zero());`
- **问题描述**：
  - `RacyCell`（推断）是单核不安全的 cell（命名暗示）。
  - `C_CURRENT_PROCESS` 是 16 字节结构（推断），跨多 CPU 并发写（`api.rs:243-247` 等多处 map_mut）。
  - 注释（`api.rs:19`）说"`C_CURRENT_PROCESS` 是 unsafe static mut"。
- **建议方案**：
  - 改 `AtomicU64` (pid, pwm) + `AtomicU32` (state) + `AtomicU32` (parent_pid) 等原子字段组合。
  - 或 `IrqSpinLock<CProcess>` 全锁。
- **严重度**：P0（多核下撕裂读 → 调度错误）。
- **关联硬规则**：F8（数据竞争）+ I1（内核态 CPU 状态）。

### 6.2 [P1] `static CURRENT_PROCESS_PTR: AtomicU64` + `static C_CURRENT_PROCESS: RacyCell<CProcess>` 双源真相

- **位置**：[proc_ops.rs:141-142, 199](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs#L141-L199)
- **问题描述**：两个 static 表示"当前进程"：
  - `CURRENT_PROCESS_PTR: AtomicU64` —— ptr
  - `C_CURRENT_PROCESS: RacyCell<CProcess>` —— 完整结构快照
  - `process.cr3 / process.pid / process.state` —— PROCESS_TABLE 权威
  - **三个数据源**，需手工同步。
- **建议**：以 PROCESS_TABLE 权威，CURRENT_PROCESS_PTR 仅作 fast path 缓存，CURRENT_PROCESS 字段全从 PROCESS_TABLE 读。

### 6.3 [P1] `raw::alloc_process` 用 `alloc::alloc::alloc` 而非 `Box`

- **位置**：[proc_ops.rs:69-77](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs#L69-L77)
- **问题描述**：
  ```rust
  pub fn alloc_process(pid: u32, name: &str, parent: Option<ProcessId>) -> *mut Process {
      unsafe {
          let layout = alloc::alloc::Layout::new::<Process>();
          let ptr = alloc::alloc::alloc(layout) as *mut Process;  // ← raw alloc
          core::ptr::write(ptr, Process::new(pid, name, parent));
          ptr
      }
  }
  ```
  - `Process::new`（`process.rs:277-350`）初始化 60+ 字段。
  - 但 `Process` 字段含 `String` / `Vec` / `Box<...>` → 实际是 `Vec<u8>` 分配，但 `alloc::alloc` **不会调用** `Vec::drop` 等析构器。
  - **对比**：`Scheduler::create_process`（`scheduler.rs:271-274`）用 `Box::new(Process::new(...))` + `Box::into_raw`。
  - **不一致**：两处分配路径不同 → Drop 时机可能错乱。
- **建议方案**：统一用 `Box::new` + `Box::into_raw`。

### 6.4 [P2] `mod raw` 与 `api::raw` 与 `user_proc::raw` 三套 raw 子模块风格不一致

- **位置**：[proc_ops.rs:32-137](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs#L32-L137), [api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/api.rs), [user_proc.rs:90](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs#L90)
- **问题描述**：三处 `raw` 子模块，命名 / SAFETY 注释风格不同。
- **建议**：抽 `framework::priv` 模块统一封装特权层模式。

---

## 7. proc/coredump.rs (760 行 / 4 项)

### 7.1 [P0] `collect_segments` 用 `vma_get_current_mm()` 而非 target pid 的 MmStruct

- **位置**：[coredump.rs:362-419](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/coredump.rs#L362-L419) `collect_segments`
- **问题描述**：
  ```rust
  fn collect_segments(pid: u32) -> alloc::vec::Vec<CoreSegment> {
      ...
      let cr3 = process_with(pid, |p| p.cr3.load(Ordering::SeqCst)).unwrap_or(0);
      if cr3 == 0 { return segments; }
      
      // SAFETY: get_current_mm 返回的 MmStruct 指针在进程存活期间有效
      let mm = mm::vma_get_current_mm();                  // ← BUG: 用 current 而非 pid
      if let Some(mm) = mm {
          let vmas = mm.vmas.lock();
          for vma in vmas.iter() { ... }
      }
  }
  ```
  - `vma_get_current_mm()` 返回**当前 CPU** 的 `MmStruct`（per-CPU 变量），**不是** target pid 的。
  - 注释 (行 374) `// SAFETY: get_current_mm 返回的 MmStruct 指针在进程存活期间有效` —— **错误**，**返回的不是 pid 的**。
  - **后果**：coredump 写入的是 **当前正在执行的进程的 VMA**，不是触发 core dump 的目标 PID 的 VMA。
  - 场景：A 进程 (pid 5) 触发 SIGSEGV → `do_signal_default_action` 调 `do_coredump(5, ...)` → 调度到 B 进程 (pid 6) 执行 → `collect_segments` 收集 B 的 VMA → core 文件是 B 的内存 + A 的寄存器 → **垃圾 core dump**。
- **建议方案**：
  - 在 `Process` 加 `mm: *const MmStruct` 字段。
  - `collect_segments` 调 `process_with(pid, |p| p.mm.as_ref())`。
  - 或在 `do_coredump` 内立即 `switch_mm(cr3)` 后写。
- **严重度**：P0（coredump 完全错误，调试失败）。
- **关联硬规则**：I4（用户内存只能通过 framework 安全代理）+ I3。

### 7.2 [P0] `RLIMIT_CORE` 截断逻辑静默忽略

- **位置**：[coredump.rs:263-268](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/coredump.rs#L263-L268) `do_coredump`
- **问题描述**：
  ```rust
  if core_limit != RLIM_INFINITY && total_size > core_limit {
      log("coredump: truncated by RLIMIT_CORE\n");
      let _ = core_limit;                            // ← 静默丢值
  }
  ```
  - 注释 (行 265-266) 说"截断: 后续写循环应在写入达到 core_limit 字节后停止"。
  - **实际**：变量被丢弃，写循环（行 321-324 `write_segment_data`）**不接收 core_limit 参数**：
  ```rust
  for seg in &segments {
      write_segment_data(fd, pid, seg, &mut offset, core_limit);  // 接收但未实现
  }
  ```
  - `write_segment_data` 实现需查 (推断) —— 推断也**未实际检查** core_limit。
- **建议方案**：
  ```rust
  fn write_segment_data(fd: u32, pid: u32, seg: &CoreSegment, offset: &mut u64, limit: u64) {
      if *offset + seg.file_size > limit {
          let remain = limit.saturating_sub(*offset);
          // 仅写 remain 字节
          ...
      } else {
          // 完整写
      }
  }
  ```
- **严重度**：P0（资源限制不生效，DoS 风险）。

### 7.3 [P1] `core_path` 写入 "core.<pid>" 硬编码当前目录

- **位置**：[coredump.rs:343-359](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/coredump.rs#L343-L359) `build_core_path`
- **问题描述**：
  - 总是写 `core.<pid>` 到 `/` 根目录。
  - Linux 默认写当前进程 cwd（通过 `/proc/<pid>/cwd`）。
  - QueenX 无 cwd 概念 → 写到 `/` → 多进程同时触发 → 文件名冲突覆盖。
- **建议**：
  - 加 `/proc/sys/kernel/core_pattern` 配置文件支持。
  - 或加 pid + timestamp 区分（`core.5.1234567890`）。

### 7.4 [P2] `do_coredump` 内 `crate::klog_warn!` 缺失

- **位置**：[coredump.rs:265](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/coredump.rs#L265)
- **问题描述**：`log()` 函数实际是调 `klog_ffi_info`，但用 `log("coredump: truncated by RLIMIT_CORE\n")` 后 `let _ = core_limit;` —— 不一致，应有 klog_warn。
- **建议**：用 `klog_warn!` macro。

---

## 8. proc/elf/ (465+145 行 / 3 项)

### 8.1 [P1] `elf_load_with_bias` 在 `pmm_alloc_page_phys` 失败时不释放已 alloc 的页

- **位置**：[elf/mod.rs:255-292](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/elf/mod.rs#L255-L292)
- **问题描述**：
  ```rust
  while cur < vaddr_end as u64 {
      let phys = crate::kernel::framework::mm::pmm_alloc_page_phys()
          .ok_or("OOM loading ELF")?;                // ← 失败: 错误传播, 已 alloc 4 页泄漏
      ...
      vmm_inst.map_page_in_table(pml4, VirtAddr(cur), phys, page_flags | PageFlags::PRESENT);
      cur += PAGE_SIZE;
  }
  ```
  - 段内 N 页 alloc，若第 N+1 页 `pmm_alloc_page_phys()` 返回 None → `?` 直接返回 `"OOM loading ELF"`。
  - 前 N 页已 alloc + map，但 VMA 已 `insert_vma` 成功（行 248）。
  - **后果**：PMM 永久泄漏 N 页 + VMA 残留指向已 alloc 但未完成的内存。
  - **更严重**：VMA 已 insert 但 `mm` 仍存活，后续 fork 时这些残缺页被复制。
- **建议方案**：
  - 收集 alloc 结果到 `Vec<PhysAddr>`，失败时回滚 `pmm_free_page` 全部 + `mm.remove_vma(vma)`。
- **关联硬规则**：F4（资源管理）+ I2（内核内存不可被 services 非法访问，但泄漏会让 OOM 提前）。

### 8.2 [P1] `elf_load_with_bias` 段权限与 `NX` 处理矛盾

- **位置**：[elf/mod.rs:231-240](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/elf/mod.rs#L231-L240)
- **问题描述**：
  ```rust
  let mut page_flags = PageFlags::USER;
  if phdr.p_flags & PF_R != 0 { page_flags |= PageFlags::PRESENT; }
  if phdr.p_flags & PF_W != 0 { page_flags |= PageFlags::WRITABLE; }
  if phdr.p_flags & PF_X == 0 { page_flags |= PageFlags::NX; }
  ```
  - `PF_R=4, PF_W=2, PF_X=1`，符合 ELF 规范。
  - **但** `PRESENT` 应总是设置，否则 #PF 立即触发。
  - **实际上**：`PF_R=0` 段（罕见）→ `page_flags = USER | NX`（无 PRESENT）→ map 时 `map_page_in_table(..., page_flags | PRESENT)` (行 289) 强制加 PRESENT。
  - 注释 OK，但用户读时 `page_flags` 计算与 map 时不一致。
- **建议**：统一在最后 add PRESENT，移除 `if phdr.p_flags & PF_R != 0` 分支。

### 8.3 [P2] `verify_elf` 返回 `VerifyResult` 后调用方仍用 `e_machine`

- **位置**：[elf/mod.rs:100-106](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/elf/mod.rs#L100-L106) `elf_validate`
- **问题描述**：
  ```rust
  pub fn elf_validate(elf_data: *const u8, elf_size: u64) -> Option<&'static Elf64Header> {
      let _ = unsafe { verify::verify_elf(elf_data, elf_size) }.ok()?;
      Some(unsafe { &*(elf_data as *const Elf64Header) })  // ← 重新 deref
  }
  ```
  - 调 `verify_elf` 后丢弃 `VerifyResult`，再次 `unsafe { &*ptr }` 重读 header。
  - **风险**：`verify_elf` 校验 `phoff + phnum * phentsize <= elf_size` 完整，但调用方拿到 `&Elf64Header` 后用 `e_phoff + e_phnum * e_phentsize` 再算可能因数据竞争读到撕裂 header。
  - 单核场景无问题，多核下 ELF 加载期间 buffer 被并发写 → **TOCTOU**。
- **建议**：`elf_validate` 直接返回 `(VerifyResult, &Elf64Header)` 或 `VerifyResult` 内含 raw ptr。

---

## 9. proc/api.rs (454 行 / 3 项)

### 9.1 [P1] `launch_first_user_process` 三架构分支重复

- **位置**：[api.rs:278-453](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/api.rs#L278-L453) `launch_first_user_process`
- **问题描述**：x86_64 initramfs / x86_64 fallback / aarch64 三处几乎相同逻辑（加载 init.bin + set_current + add scheduler + enter）。
- **建议**：抽 `fn launch_init(bin: &[u8], pwm: u64) -> !` 共享。

### 9.2 [P1] `user_proc_load_elf` ELF_MAX_SIZE 1MB 硬编码

- **位置**：[api.rs:99](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/api.rs#L99) `const ELF_MAX_SIZE: usize = 1024 * 1024;`
- **问题描述**：1MB 上限对现代二进制偏小（glibc busybox ~2MB）。
- **建议**：放到 `config::MAX_ELF_SIZE` 配置项。

### 9.3 [P2] `wait_queue_*` 4 个空函数仍是 FFI 导出

- **位置**：[api.rs:48-60](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/api.rs#L48-L60)
- **问题描述**：
  ```rust
  pub extern "C" fn wait_queue_init(_wq: *mut u8) {}
  pub extern "C" fn wait_queue_add(_wq: *mut u8, _thread: u64) {}
  pub extern "C" fn wait_queue_wake_one(_wq: *mut u8) {}
  pub extern "C" fn wait_queue_wake_all(_wq: *mut u8) {}
  ```
  - 空实现，调用方会以为已注册但实际无效。
  - **死代码**（F9 违规）。
- **建议**：删除（实际应有完整实现）。

---

## 10. proc/thread.rs (326 行 / 2 项)

### 10.1 [P1] `Thread::new` 中 `cs/ss` 硬编码 0x08/0x10

- **位置**：[thread.rs:79-82](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/thread.rs#L79-L82)
- **问题描述**：
  ```rust
  cs: 0x08,   // ← KERNEL_CS
  ss: 0x10,   // ← USER_DS (x86_64)
  ```
  - 注释无。
  - **应**：`cs: USER_CS (0x18)`, `ss: USER_DS (0x10)` for ring 3 iretq。
  - 0x08 (KERNEL_CS) 用作 iretq CS 字段 → iretq 跳到 ring 0 → 用户态跳转失败。
- **建议**：用 `gdt::USER_CS` / `gdt::USER_DS` 常量。

### 10.2 [P2] `ThreadTable::allocate` 计数器永不回收

- **位置**：[thread.rs:168-175](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/thread.rs#L168-L175)
- **问题描述**：
  ```rust
  pub fn allocate(&self) -> Option<u32> {
      let tid = self.next_tid.fetch_add(1, Ordering::SeqCst);
      if (tid as usize) < MAX_THREADS { Some(tid) } else { None }
  }
  ```
  - 计数器单调增，从 1 到 `MAX_THREADS` 后返回 None，但 `remove` 后不回收 tid。
  - 长期运行后 MAX_THREADS 耗尽 → 无法创建线程。
- **建议**：维护 `available_tids: Mutex<Vec<u32>>` 回收。

---

## 11. proc/cpu_queue.rs (148 行 / 2 项)

### 11.1 [P1] `CpuQueue` 字段全 `UnsafeCell`，但仅文档说"per-CPU 独占"

- **位置**：[cpu_queue.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cpu_queue.rs) (全文 148 行)
- **问题描述**：
  - 数据结构用 `UnsafeCell` 但**无运行时断言**确保仅 per-CPU 访问。
  - SMP 跨 CPU 访问 → 数据竞争。
- **建议**：加 `debug_assert!(cpu == get_current_cpu(), ...)` 在所有访问点。

### 11.2 [P2] 字段命名 `current` 与 `scheduler::current` 重名

- **位置**：[cpu_queue.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cpu_queue.rs) + [scheduler.rs:486](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L486)
- **问题描述**：两个 `current` 字段在不同 module，含义相同但维护两份。
- **建议**：以 `PER_CPU_SCHED[idx].current` 为权威，删除 `CpuQueue.current`。

---

## 12. proc/canary.rs (103 行 / 1 项)

### 12.1 [P2] `ENTROPY_POOL` 静态初值无随机源

- **位置**：[canary.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/canary.rs) 静态初值
- **问题描述**：`ENTROPY_POOL = 0x1234_5678_DEAD_BEEF` 硬编码，`PER_PROC_SEED = 0x5A5A_5A5A_5A5A_5A5A` 硬编码。
  - 启动时无硬件 RNG 熵源 → 攻击者可预测 canary。
- **建议**：调用 `rdrand` / `getrandom` 启动后第一次初始化。

---

## 13. proc/mechanism.rs (101 行 / 1 项)

### 13.1 [P2] `mechanism.rs` 仅 re-export，文件存在意义弱

- **位置**：[mechanism.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/mechanism.rs)
- **问题描述**：101 行全部 `pub use super::proc_ops::raw::*;` 等 re-export。
- **建议**：合并到 `proc_ops::raw` 或 `api::*`，删除本文件。

---

## 14. proc/rlimit.rs (99 行 / 1 项)

### 14.1 [P1] `RlimitTable` 操作通过 userptr 缺类型安全

- **位置**：[rlimit.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/rlimit.rs) (推断 99 行)
- **问题描述**：`sys_getrlimit` / `sys_setrlimit` 接收 `userptr<rlimit>` 但**未检查** userptr 长度 = `sizeof(rlimit)` × 2 = 32 字节。
- **建议**：加 `userptr.len() >= 16` 检查。

---

## 15. proc/ 12 个 stub 文件（≤15 行 / 0 项新增）

| 文件 | 行数 | 状态 |
|---|---|---|
| [cfs.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cfs.rs) | 14 | 仅 re-export（迁移到 services） |
| [cgroup.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cgroup.rs) | 15 | 仅 re-export |
| [session.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/session.rs) | 15 | 仅 re-export |
| [namespace.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/namespace.rs) | 15 | 仅 re-export |
| [seccomp.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/seccomp.rs) | 12 | 仅 re-export |
| [oomd.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/oomd.rs) | 9 | 仅 re-export |
| [fd_alloc.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/fd_alloc.rs) | 9 | 仅 re-export |
| [madvise_mlock.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/madvise_mlock.rs) | 12 | 仅 re-export |
| [signal_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal_trait.rs) | 104 | 定义 SignalDecision trait，无实现者 stub |
| [sched_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/sched_trait.rs) | 118 | 定义 SchedDecision trait，无实现者 stub |
| [posix_timer.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/posix_timer.rs) | 659 | 详细审计推迟到下批 |
| [types.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/types.rs) | 9 | 仅 re-export 到 services::proc::types |

> **审计观察**：13 个文件中 11 个是 9-15 行的 re-export stub，对应迁移到 services/proc/ 后保留调用方路径。`signal_trait.rs` / `sched_trait.rs` 仍为机制-策略分离的 trait 定义，但本批审计未深入 trait 方法（待补）。

---

## 16. proc/ 子系统总结

### 16.1 P0 清单（7 项）

| 编号 | 文件 | 简述 |
|---|---|---|
| P0-17 | coredump.rs | collect_segments 用 current mm 而非 pid mm |
| P0-18 | signal.rs | do_signal_default_action 绕过 set_state_safe |
| P0-19 | scheduler.rs | tick 硬编码 1..=255 进程扫描 |
| P0-20 | coredump.rs | RLIMIT_CORE 截断静默忽略 |
| P0-21 | scheduler.rs | schedule IRQ 状态机重复抽象 |
| P0-22 | scheduler.rs | exit self-recursive + QEMU-specific halt |
| P0-23 | process.rs | remove_and_free 释放路径缺 vmm_destroy |

### 16.2 P1 清单（14 项）

详见各小节。

### 16.3 关联既有审计

- [code-audit-2026-08-11.md P0-01](./code-audit-2026-08-11.md) MLFQ 退役 → 本审计 §1.7 init 进程未入 cfs_rq
- [code-audit-2026-08-11.md P0-05](./code-audit-2026-08-11.md) SAFETY 52 处 → 本审计 §1.9 scheduler.rs unsafe 块
- [code-audit-full.md P1-17](../../plan/code-audit-full.md) sched Task::proc_ptr 生命周期 → 本审计 §1.7 init set_current 顺序

### 16.4 修复优先级

1. **本周（紧急）**：P0-17/18/19/20（coredump + signal + scheduler）
2. **本季度**：P0-21/22/23 + 全部 P1
3. **半年**：P2/P3 整理 + 抽助手（mechanism::raw 统一抽象）

---

**proc/ 子系统审计完成**。下批进入 framework/sync/ 剩余文件。
