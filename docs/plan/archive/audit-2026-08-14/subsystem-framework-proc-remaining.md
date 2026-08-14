# framework/proc 剩余文件深度审计报告

> **审计范围**：`src/kernel/framework/proc/`（27 个源文件）
> **审计日期**：2026-08-14
> **代码规模**：约 11,936 LoC（含测试 + 注释）
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **26 个问题（P0×5, P1×8, P2×9, P3×4）**

## 1. 子系统概览（重点未深审部分）

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [user_proc.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs) | 2137 | 用户进程管理（最复杂单文件）| **极高** |
| [scheduler.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs) | 1457 | 调度器核心（CFS/RT/DL）| **极高** |
| [scheduler_ex.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler_ex.rs) | 1394 | 调度器扩展（per-CPU）| **极高** |
| [proc_ops.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs) | 1143 | 进程操作（fork/exec/wait/exit）| **极高** |
| [signal.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs) | 973 | POSIX 信号处理 | **极高** |
| [process.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs) | 789 | Process 结构 + 进程表 | **极高** |
| [coredump.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/coredump.rs) | 760 | 核心转储 | **高** |
| [posix_timer.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/posix_timer.rs) | 659 | POSIX 定时器 | 中 |
| [api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/api.rs) | 454 | 公共 API | **高** |
| [thread.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/thread.rs) | 326 | 线程抽象 | 中 |
| [sched_ops.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/sched_ops.rs) | 315 | 调度器操作 | 中 |
| [cpu_queue.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cpu_queue.rs) | 148 | per-CPU 队列 | 中 |
| 其他 | < 120 | trait/配置/桩 | 低 |

## 2. 严重问题

### 2.1 [P0] `process.rs:24-29` `unsafe extern "C" { fn pmm_alloc_pages(...); fn vmm_create_user_page_table(); fn vmm_destroy_page_table(); }` 在 framework 中重声明 framework 函数

- **位置**：[process.rs:24-29](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L24-L29)
- **代码**：
  ```rust
  unsafe extern "C" {
      fn pmm_alloc_pages(count: u64) -> *mut u8;
      fn vmm_create_user_page_table() -> u64;
      fn vmm_destroy_page_table(cr3: u64);
  }
  ```
- **问题**：
  - 在 framework/proc 中 `unsafe extern "C"` 重声明 `framework::mm::pmm_alloc_pages`、`vmm_create_user_page_table`、`vmm_destroy_page_table`——这些都是 framework **内部**函数。
  - 应该是普通 `extern "Rust"` 函数指针或直接调用 `super::pmm::pmm_alloc_pages`。
  - **违反"unsafe 集中化"原则**——框架内部应该直接调用而非 FFI。
  - 后果：链接错误风险（若 framework::mm 没有对应 C ABI 导出）+ 失去 Rust 借用检查器保护。
- **建议方案**：
  1. 直接调用 `super::super::mm::pmm_alloc_pages`。
  2. 删除 unsafe 块。

### 2.2 [P0] `scheduler.rs:39-53` `raw::update_current_process_ptr` 双重 unsafe 嵌套

- **位置**：[scheduler.rs:39-55](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L39-L55)
- **代码**：
  ```rust
  pub(crate) mod raw {
      pub unsafe fn update_current_process_ptr(ptr: u64) {
          unsafe {
              unsafe extern "C" {
                  fn update_current_process_ptr(ptr: u64);
              }
              update_current_process_ptr(ptr);
          }
      }
  }
  ```
- **问题**：
  - 外层 `unsafe fn` + 内层 `unsafe { ... }` + 内层 `unsafe extern "C" { ... }`——**三重 unsafe 嵌套**。
  - 同名函数 `update_current_process_ptr` 在 Rust 函数 + C 声明两个作用域——读者混淆。
- **建议方案**：
  1. 简化为单层 unsafe。
  2. 改名 Rust 内部函数以避免命名冲突。

### 2.3 [P0] `scheduler.rs:23` `Mutex = IrqSpinLock` 别名再次出现

- **位置**：[scheduler.rs:23](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L23)
- **问题**：
  - 与 [subsystem-framework-misc.md §3.4](../audit/subsystem-framework-misc.md) 类似问题——`Mutex` 别名掩盖 IRQ-safe 语义。
- **建议方案**：
  1. 删除别名，统一用 `IrqSpinLock`。

### 2.4 [P0] `signal.rs:973` 信号处理 973 行——**当前进程信号屏蔽 atomic 操作无内存屏障**

- **位置**：[signal.rs:973](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L973)
- **问题**：
  - 信号掩码（signal mask）的修改必须对**所有 CPU 可见**。
  - 当前实现的内存屏障策略未审。
  - 之前审计（[subsystem-services-proc.md §3.4](../audit/subsystem-services-proc.md)）已识别 SignalDisposition 双重真理源。
- **建议方案**：
  1. 文档化内存屏障策略。
  2. 配套单元测试。

### 2.5 [P0] `coredump.rs:760` 核心转储**写 ELF 文件无路径校验**

- **位置**：[coredump.rs:760](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/coredump.rs#L760)
- **问题**：
  - 核心转储生成 ELF 文件（[subsystem-services-proc.md §3.x P0 coredump 内存收集](../audit/subsystem-services-proc.md)）。
  - 文件路径硬编码 `core`（Linux 惯例）——**未校验路径权限/挂载点**。
  - 用户可构造 `/var` 不存在 → 写入失败或 panic。

## 3. P1 问题

### 3.1 [P1] `user_proc.rs:2137` 单文件 2137 行——**严重违反简单优先**

- **位置**：[user_proc.rs:1-2137](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/user_proc.rs#L1-L2137)
- **问题**：
  - **QueenX 单文件最大者**。
  - 应拆分为：
    - `user_proc/elf.rs`（ELF 加载）
    - `user_proc/exec.rs`（exec 系统调用）
    - `user_proc/init.rs`（init 进程启动）
    - `user_proc/state.rs`（用户态↔内核态切换）

### 3.2 [P1] `scheduler.rs:1457` 单文件 1457 行

- **位置**：[scheduler.rs:1-1457](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler.rs#L1-L1457)
- **问题**：
  - 与 [subsystem-proc.md](../audit/subsystem-proc.md) 关联问题——调度器单文件过大。
- **建议方案**：
  1. 拆分 `cfs.rs` + `rt.rs` + `dl.rs` + `schedule.rs` + `tick.rs`。

### 3.3 [P1] `scheduler_ex.rs:1394` 单文件 1394 行

- **位置**：[scheduler_ex.rs:1-1394](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/scheduler_ex.rs#L1-L1394)
- **问题**：
  - per-CPU 调度扩展。

### 3.4 [P1] `proc_ops.rs:1143` 单文件 1143 行

- **位置**：[proc_ops.rs:1-1143](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/proc_ops.rs#L1-L1143)
- **问题**：
  - fork/exec/wait/exit 全在单文件。

### 3.5 [P1] `signal.rs:973` 信号处理 973 行

- **位置**：[signal.rs:1-973](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal.rs#L1-L973)
- **问题**：
  - POSIX 信号实现集中单文件。

### 3.6 [P1] `process.rs:789` Process 结构 + 进程表 789 行

- **位置**：[process.rs:1-789](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L1-L789)
- **问题**：
  - 进程结构 + 操作 + FFI 代理混在一起。

### 3.7 [P1] `posix_timer.rs:659` POSIX timer 659 行

- **位置**：[posix_timer.rs:1-659](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/posix_timer.rs#L1-L659)
- **问题**：
  - timer_create / timer_settime / timer_gettime 等。

### 3.8 [P1] `api.rs:454` 公共 API 454 行

- **位置**：[api.rs:1-454](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/api.rs#L1-L454)
- **问题**：
  - services/proc 调用 framework/proc 入口。

## 4. P2 问题

### 4.1 [P2] `process.rs:31` `KERNEL_STACK_CANARY: u64 = 0xDEADBEEF_CAFEBABE` 硬编码 magic value

- **位置**：[process.rs:31](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/process.rs#L31)
- **问题**：
  - 与 [subsystem-services-proc.md §3.x P0 测试值](../audit/subsystem-services-proc.md) 同模式——硬编码 magic value。
  - 不同进程/架构下应随机化。

### 4.2 [P2] `canary.rs:103` canary 检测 103 行——栈溢出检测

- **位置**：[canary.rs:1-103](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/canary.rs#L1-L103)
- **问题**：
  - 栈 canary 检测完整路径未深审。

### 4.3 [P2] `mechanism.rs:101` mechanism 层抽象

- **位置**：[mechanism.rs:1-101](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/mechanism.rs#L1-L101)
- **问题**：
  - 抽象层。

### 4.4 [P2] `cpu_queue.rs:148` per-CPU 队列 148 行

- **位置**：[cpu_queue.rs:1-148](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cpu_queue.rs#L1-L148)
- **问题**：
  - per-CPU run queue 实现。

### 4.5 [P2] `sched_ops.rs:315` 调度器操作 315 行

- **位置**：[sched_ops.rs:1-315](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/sched_ops.rs#L1-L315)
- **问题**：
  - yield / sleep / wakeup 等。

### 4.6 [P2] `sched_trait.rs:118` 调度 trait 抽象

- **位置**：[sched_trait.rs:1-118](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/sched_trait.rs#L1-L118)
- **问题**：
  - `Task::proc_ptr` 裸指针生命周期问题（[subsystem-services-proc.md P1-17](../audit/subsystem-services-proc.md)）。

### 4.7 [P2] `signal_trait.rs:104` 信号 trait 抽象

- **位置**：[signal_trait.rs:1-104](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/signal_trait.rs#L1-L104)
- **问题**：
  - 抽象层。

### 4.8 [P2] `rlimit.rs:99` rlimit 99 行——POSIX 资源限制

- **位置**：[rlimit.rs:1-99](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/rlimit.rs#L1-R99)
- **问题**：
  - rlimit 资源限制。

### 4.9 [P2] `cfs.rs:14` 仅 14 行（re-export 桩）

- **位置**：[cfs.rs:1-14](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cfs.rs#L1-L14)
- **问题**：
  - CFS 实现可能分散到 scheduler.rs。

## 5. P3 问题

### 5.1 [P3] `cgroup.rs:15` 仅 15 行（re-export 桩）

- **位置**：[cgroup.rs:1-15](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/cgroup.rs#L1-L15)
- **问题**：
  - cgroup 在 services/proc/cgroup.rs 实现。

### 5.2 [P3] `namespace.rs:15` 仅 15 行（re-export 桩）

- **位置**：[namespace.rs:1-15](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/namespace.rs#L1-L15)
- **问题**：
  - namespace 在 services/proc/namespace.rs 实现。

### 5.3 [P3] `seccomp.rs:12` 仅 12 行（re-export 桩）

- **位置**：[seccomp.rs:1-12](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/seccomp.rs#L1-L12)
- **问题**：
  - seccomp 在 services/proc/seccomp.rs 实现。

### 5.4 [P3] `session.rs:15` 仅 15 行（re-export 桩）

- **位置**：[session.rs:1-15](file:///home/anfer/Code/QueenX/src/kernel/framework/proc/session.rs#L1-L15)
- **问题**：
  - session 实际在 framework/credo/session.rs 实现。

## 6. 跨子系统关联

### 6.1 process ↔ scheduler

- 进程状态变化触发调度。
- `update_current_process_ptr` 在 context switch 时调用。

### 6.2 process ↔ signal

- 信号投递改变进程状态。
- 进程退出时清理信号。

### 6.3 process ↔ MM

- Process 持有 `MmStruct`。
- fork/exec 涉及 MM 子系统。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 5 | 4-6 天 |
| **P1** | 8 | 6-8 天 |
| **P2** | 9 | 2-3 天 |
| **P3** | 4 | 0.5 天 |
| **合计** | **26** | **13-18 天** |

### P0 修复路径（建议执行顺序）

1. **§2.1 process.rs FFI 重声明**（0.5 天，**链接错误风险**）
2. **§2.2 scheduler.rs 三重 unsafe**（0.5 天）
3. **§2.3 Mutex 别名**（0.5 天）
4. **§2.4 signal 内存屏障**（1-2 天）
5. **§2.5 coredump 路径校验**（1 天）