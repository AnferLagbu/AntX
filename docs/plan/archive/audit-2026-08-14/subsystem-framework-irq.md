# framework/irq 子系统深度审计报告

> **审计范围**：`src/kernel/framework/irq/mod.rs`（1 文件 234 LoC）
> **审计日期**：2026-08-14
> **代码规模**：234 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **13 个问题（P0×3, P1×4, P2×4, P3×2）**

## 1. 子系统概览

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [irq/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs) | 234 | Softirq 机制 + Tasklet 框架 | **极高** |

## 2. 严重问题

### 2.1 [P0] `mod.rs:106-139` `do_softirq` 中途开中断 `interrupt_enable()` 后**未保护 handlers 数组**

- **位置**：[mod.rs:106-139](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L106-L139)
- **代码**：
  ```rust
  pub fn do_softirq() {
      ...
      loop {
          let pending = SOFTIRQ.pending.swap(0, Ordering::AcqRel);
          ...
          crate::arch!(interrupt_enable());   // ← 开中断
          for i in 0..MAX_SOFTIRQS {
              let bit = 1u64 << i;
              if pending & bit != 0 {
                  if let Some(handler) = handlers[i] {   // ← handlers 在开中断期读
                      handler();
                  }
              }
          }
          crate::arch!(interrupt_disable());
      }
      ...
  }
  ```
- **问题**：
  - 开中断后**当前 CPU 可被硬中断抢占**。
  - 硬中断 handler 可能 `raise_softirq(nr)` → 修改 `pending`，但**当前 handlers 读已是局部变量**——不影响。
  - **真正风险**：`open_softirq`（[mod.rs:88-92](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L88-L92)）在开中断期被调用 → 修改 `handlers` 数组 → **`UnsafeCell` 解引用 UB**。
  - 注释（[mod.rs:23-25](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L23-L25)）说"启动期单线程"，但**未强制**。
- **建议方案**：
  1. `handlers` 用 `RwLock<>` 保护。
  2. 或加 `opened: AtomicBool` 标志，open_softirq 后锁定。

### 2.2 [P0] `mod.rs:222-233` `tasklet_softirq_handler` 遍历过程中 `drop(table); func(); return;` —— **锁释放后再调 func**

- **位置**：[mod.rs:222-233](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L222-L233)
- **代码**：
  ```rust
  fn tasklet_softirq_handler() {
      let table = TASKLETS.lock();
      for entry in table.iter() {
          if entry.scheduled.load(Ordering::Acquire) {
              entry.scheduled.store(false, Ordering::Release);
              if let Some(func) = entry.func {
                  drop(table);   // ← 显式释放锁
                  func();        // ← 在锁外执行
                  return;        // ← 只执行一个
              }
          }
      }
  }
  ```
- **问题**：
  - 锁释放 → 另一个 CPU 可注册新 tasklet → `func` 函数指针仍有效（`Some` 已保留）。
  - 但**同时** `schedule_tasklet(id)`（[mod.rs:212-219](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L212-L219)）可能在 func 执行期间重新标记 `scheduled=true`。
  - 当前 tasklet 不重复执行（`return`），但下一个 softirq 轮次会再执行。
  - **真正的 race**：func 内部调用 `register_tasklet()`（持有锁的版本）→ 死锁。
- **建议方案**：
  1. 文档化"tasklet handler 内禁止调用 register_tasklet"。
  2. 或用 RCU 替代 IrqSpinLock。

### 2.3 [P0] `mod.rs:106-139` `do_softirq` 中 `running.compare_exchange` 仅阻止**当前 CPU 重入**，**多 CPU 同时调用 do_softirq → 多个 CPU 各自跑 softirq**

- **位置**：[mod.rs:107-113](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L107-L113)
- **代码**：
  ```rust
  pub fn do_softirq() {
      if SOFTIRQ.running.compare_exchange(false, true, ...).is_err() {
          return;
      }
      ...
  }
  ```
- **问题**：
  - `running` 是**全局** AtomicBool（不是 per-CPU）。
  - CPU A 在 do_softirq 中 → CPU B 也调用 do_softirq → **CPU B 直接返回**。
  - 但**Linux 设计是 per-CPU running**（每个 CPU 独立进入 softirq）。
  - 当前实现**多核下只有一个 CPU 处理 softirq**，另一个 CPU 的 pending 永远不被处理（直到第一个退出）。
- **建议方案**：
  1. `running: [AtomicBool; MAX_CPUS]` per-CPU。
  2. 或用 `CpuLocal<bool>`。

## 3. P1 问题

### 3.1 [P1] `mod.rs:88-92` `open_softirq` 在 `running=true` 期间可被调用

- **位置**：[mod.rs:88-92](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L88-L92)
- **问题**：
  - 同 §2.1，handlers 数组在 do_softirq 中读取时 open_softirq 可能写。

### 3.2 [P1] `mod.rs:69` `SoftirqHandler = fn()` 无参数

- **位置**：[mod.rs:69](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L69)
- **问题**：
  - handler 无法接收上下文（CPU ID、pending mask）。
  - 每个 handler 必须从全局读上下文——重入风险。

### 3.3 [P1] `mod.rs:118-119` `pending.swap(0)` 清空所有 pending——**新 raise 在 swap 后到 dispatch 前丢失**

- **位置**：[mod.rs:117-136](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L117-L136)
- **代码**：
  ```rust
  loop {
      let pending = SOFTIRQ.pending.swap(0, Ordering::AcqRel);
      if pending == 0 { break; }
      crate::arch!(interrupt_enable());
      // ← 新 raise_softirq(nr) 在此处发生
      for i in 0..MAX_SOFTIRQS { ... }
      crate::arch!(interrupt_disable());
      // ← 新 pending 留在下一轮 swap
  }
  ```
- **问题**：
  - **新 pending 实际被保留到下一轮**——OK。
  - 但**handler 内部 raise 同一 nr** → 当前轮次 handler 已执行 → 下一轮再执行 → 重复触发。
  - Linux 设计 softirq 内 raise_softirq 同 nr 合并（不会递归执行）。

### 3.4 [P1] `mod.rs:30` `MAX_SOFTIRQS = 9` 与 `SoftirqVec::Count = 8` 不一致

- **位置**：[mod.rs:30](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L30)、[mod.rs:43](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L43)
- **问题**：
  - `MAX_SOFTIRQS = 9` 但只有 8 个有效 softirq（0-7）+ Count=8。
  - 数组索引访问 `handlers[8]` 永不匹配但数组长度是 9——索引 8 永远 None。

## 4. P2 问题

### 4.1 [P2] `mod.rs:152-157` `softirq_init` 仅注册 Tasklet —— 其他 softirq 未注册

- **位置**：[mod.rs:151-157](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L151-L157)
- **代码**：
  ```rust
  pub extern "C" fn softirq_init() {
      open_softirq(SoftirqVec::Tasklet, tasklet_softirq_handler);
  }
  ```
- **问题**：
  - Timer/NetRx/NetTx/Block 等未注册——但模块注释（[mod.rs:14-19](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L14-L19)）说各子系统在初始化时注册。
  - **注册时机未文档化**。

### 4.2 [P2] `mod.rs:194` `MAX_TASKLETS = 32` 硬编码

- **位置**：[mod.rs:189-194](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L189-L194)
- **问题**：
  - 32 个 tasklet 容量过小？实际取决于用例。

### 4.3 [P2] `mod.rs:200-208` `register_tasklet` O(N) 线性扫描

- **位置**：[mod.rs:196-209](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L196-L209)
- **问题**：
  - 注册 tasklet 时遍历 32 项——启动期一次性，可接受。
  - 但若频繁注册/注销则效率低。

### 4.4 [P2] `mod.rs:230` `return` 后**未调度下一个 pending tasklet**

- **位置**：[mod.rs:222-233](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L222-L233)
- **问题**：
  - 单次 softirq 轮次只执行一个 tasklet，下一个 tasklet 需等下次 raise。
  - Linux 设计单轮可执行多个 tasklet 直到没有 scheduled。

## 5. P3 问题

### 5.1 [P3] `mod.rs:69-71` `pub type SoftirqHandler = fn()` 与 `TaskletFn` 同签名——**可互换**

- **位置**：[mod.rs:69](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L69)、[mod.rs:172](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L172)
- **问题**：
  - `pub type SoftirqHandler = fn()` 与 `pub type TaskletFn = fn()` 等价。
  - 应区分（带不同 trait bound）。

### 5.2 [P3] `mod.rs:120-122` `pending == 0 break` 但**当前循环变量已 swap 清空——可能丢失新 raise**

- **位置**：[mod.rs:117-136](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L117-L136)
- **问题**：
  - break 时 `running` 已被设为 false。
  - 但若有 raise 在 break 与 running=false 之间，**pending 非 0 但无 CPU 处理**——直到下次 do_softirq 调用。

## 6. 跨子系统关联

### 6.1 irq ↔ arch

- `crate::arch!(interrupt_enable())` 在 do_softirq 中调用。
- 与 [subsystem-framework-arch.md §2.x P0 IDT SMP race](../audit/subsystem-framework-arch.md) 关联。

### 6.2 irq ↔ timer

- Timer softirq 通常注册到 `do_softirq` 中处理定时器账本。

### 6.3 irq ↔ net

- NetRx/NetTx softirq 处理网络中断下半部。

### 6.4 irq ↔ mm (Kswapd)

- `SoftirqVec::Kswapd`（[mod.rs:42-43](file:///home/anfer/Code/QueenX/src/kernel/framework/irq/mod.rs#L42-L43)）处理 swap 回收。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 3 | 3-4 天 |
| **P1** | 4 | 2-3 天 |
| **P2** | 4 | 1-2 天 |
| **P3** | 2 | 0.5 天 |
| **合计** | **13** | **7-10 天** |

### P0 修复路径（建议执行顺序）

1. **§2.3 do_softirq 全局 running → per-CPU**（1-2 天，**多核可靠性**）
2. **§2.1 handlers 数组并发读写**（1 天）
3. **§2.2 tasklet handler 内 register_tasklet 死锁**（0.5 天）