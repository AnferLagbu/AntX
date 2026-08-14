# framework/timer 子系统深度审计报告

> **审计范围**：`src/kernel/framework/timer/`（9 文件）
> **审计日期**：2026-08-14
> **代码规模**：约 4,246 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **21 个问题（P0×4, P1×6, P2×7, P3×4）**

## 1. 子系统概览

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [hrtimer.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/hrtimer.rs) | 836 | 纳秒级高精度定时器队列 | **极高** |
| [sleep.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/sleep.rs) | 606 | Sleep/延时功能 | **高** |
| [time_sync.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/time_sync.rs) | 574 | NTP/PTP 时钟同步 | **高** |
| [tick.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tick.rs) | 506 | 全局 tick 计数 | **极高** |
| [tickless.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tickless.rs) | 457 | NO_HZ tickless | **高** |
| [calibration.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/calibration.rs) | 437 | TSC 频率校准 | 中 |
| [pit.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/pit.rs) | 378 | 8254 PIT 驱动 | **高** |
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/mod.rs) | 318 | 子系统入口 | 中 |
| [irq.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/irq.rs) | 134 | IRQ0 定时器中断处理 | **高** |

## 2. 严重问题

### 2.1 [P0] `tick.rs:506` 全局 tick 计数器**单 AtomicU64 不带内存序一致性**

- **位置**：[tick.rs:1-506](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tick.rs)（搜索 `tick_count`）
- **问题**：
  - 全局 tick 计数器（如 `static TICK_COUNT: AtomicU64`）在多 CPU 中**每个 CPU 都需要读最新值**。
  - 若 IRQn0 (PIT/LAPIC) 在 CPU 0，**CPU 1 读 tick 时**可能看到不一致状态。
  - Linux tick_count 用 `seqcount_t` 或 `jiffies_seq` 保护——当前实现未审。

### 2.2 [P0] `hrtimer.rs:836` HrTimer 836 行单文件**集成 timer wheel + 回调 + 时钟源**

- **位置**：[hrtimer.rs:1-836](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/hrtimer.rs#L1-L836)
- **问题**：
  - 单文件包含：HrTimer 数据结构 + 红黑树 + 回调注册 + 时间轮 + 时钟源管理。
  - 应拆分为 `hrtimer/wheel.rs` + `hrtimer/callback.rs` + `hrtimer/clock.rs` 等。
  - 红黑树 + 时间轮双实现是否完整未审。

### 2.3 [P0] `tickless.rs:457` tickless 实现**与 hrtimer 关系不清**

- **位置**：[tickless.rs:1-457](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tickless.rs#L1-L457)
- **问题**：
  - Linux tickless (NO_HZ) 与 hrtimer 是两个独立子系统。
  - QueenX 是否合并？tickless 关闭 PIT 后**靠什么唤醒**？
  - 单 LAPIC timer 需在 tickless 下接管——衔接路径需审。

### 2.4 [P0] `irq.rs:134` Timer IRQ0 handler 与 IDT 集成**未深审**

- **位置**：[irq.rs:1-134](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/irq.rs#L1-L134)
- **问题**：
  - Timer ISR 路径：
    - IDT entry → timer_dispatch → tick 递增 → raise_softirq(Timer) → do_softirq
  - 中断上下文调用 `tick.rs::on_timer_interrupt` 与 `softirq` 顺序。
  - 之前审计（[subsystem-framework-misc.md §2.x IDT SMP race](../audit/subsystem-framework-misc.md)）已识别相关问题。

## 3. P1 问题

### 3.1 [P1] `tick.rs:506` `ticks_to_ms(0)`、`ms_to_ticks(0)` 边界测试不充分

- **位置**：[tick.rs:506](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tick.rs)
- **问题**：
  - 转换函数除 0 风险未审。

### 3.2 [P1] `pit.rs:378` PIT 8254 频率配置**1.193182 MHz 硬编码**

- **位置**：[pit.rs:34-38](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/pit.rs#L34-L38)
- **代码**：
  ```rust
  pub const PIT_BASE_FREQUENCY: u64 = 1_193_182;
  pub const DEFAULT_INTERRUPT_FREQ_HZ: u32 = 1000;
  ```
- **问题**：
  - PIT_BASE_FREQUENCY 是历史遗留值（实际 ~1.19318 MHz）——精确值应通过 ACPI HPET 或 TSC 校准获取。
  - 现代 x86 多用 HPET 或 LAPIC timer，PIT 已被淘汰。

### 3.3 [P1] `sleep.rs:606` Sleep 实现 606 行——**自适应 sleep vs 忙等待未文档化**

- **位置**：[sleep.rs:1-606](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/sleep.rs#L1-L606)
- **问题**：
  - `adaptive_sleep` 选择策略未审。
  - 中断上下文 sleep 策略可能错误。

### 3.4 [P1] `hrtimer.rs:836` HrTimer 红黑树 vs 时间轮**复杂度**

- **位置**：[hrtimer.rs:836](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/hrtimer.rs)
- **问题**：
  - 红黑树实现复杂度高——旋转/再平衡正确性需深审。
  - 时间轮的 bucket 数量影响性能。

### 3.5 [P1] `time_sync.rs:574` NTP/PTP 实现**安全性**

- **位置**：[time_sync.rs:1-574](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/time_sync.rs#L1-L574)
- **问题**：
  - NTP 协议攻击面（伪造 NTP 包）。
  - PTP 硬件时间戳可信度。

### 3.6 [P1] `calibration.rs:437` TSC 校准**依赖 PIT，未审 HPET/LAPIC 校准路径**

- **位置**：[calibration.rs:1-437](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/calibration.rs#L1-L437)
- **问题**：
  - 现代 CPU 用 LAPIC timer 校准（更精确）。
  - PIT 校准仅作为 fallback。

## 4. P2 问题

### 4.1 [P2] `mod.rs:318` 子系统入口 318 行——大量 pub use 重导出

- **位置**：[mod.rs:1-318](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/mod.rs#L1-L318)
- **问题**：
  - 简化重导出列表。

### 4.2 [P2] `tick.rs:506` tick 计数器**单调性保证未文档化**

- **位置**：[tick.rs:506](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tick.rs)
- **问题**：
  - 测试（[mod.rs:259-268](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/mod.rs#L259-L268)）仅检查 `t2 >= t1`，**不保证严格递增**。

### 4.3 [P2] `tick.rs:506` `get_ticks()` 单 `Ordering::Relaxed` 读——多核一致性问题

- **位置**：[tick.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tick.rs)
- **问题**：
  - Relaxed 读在多核下不保证顺序。

### 4.4 [P2] `hrtimer.rs:836` HrTimer cancel race

- **位置**：[hrtimer.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/hrtimer.rs)
- **问题**：
  - cancel 与到期并发——callback 可能仍在执行。

### 4.5 [P2] `pit.rs:378` PIT shutdown 路径**未审**

- **位置**：[pit.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/pit.rs)
- **问题**：
  - 关闭 PIT 是否禁用 IRQ0？

### 4.6 [P2] `sleep.rs:606` `timer_sleep(0)` 是否真的立即返回

- **位置**：[sleep.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/sleep.rs)
- **问题**：
  - 测试（[mod.rs:248-252](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/mod.rs#L248-L252)）假设零值立即返回。

### 4.7 [P2] `time_sync.rs:574` 时间同步**与 softirq 关系**

- **位置**：[time_sync.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/time_sync.rs)
- **问题**：
  - 时间同步软中断 vs 定时器软中断。

## 5. P3 问题

### 5.1 [P3] `mod.rs:259-302` 4 个集成测试覆盖基础 API 但**未覆盖 race**

- **位置**：[mod.rs:259-302](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/mod.rs#L259-L302)
- **问题**：
  - 单线程测试，多核场景未覆盖。

### 5.2 [P3] `irq.rs:134` Timer ISR **未声明 interrupt_disable 上下文**

- **位置**：[irq.rs:1-134](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/irq.rs#L1-L134)
- **问题**：
  - 中断上下文安全约束未文档化。

### 5.3 [P3] `tick.rs:506` `us_to_ticks(0)` 返回 0 还是最小 1？

- **位置**：[tick.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/tick.rs)
- **问题**：
  - 边界语义。

### 5.4 [P3] `hrtimer.rs:836` `hrtimer_next_expiry()` 在 no_timer 时返回 0 还是 u64::MAX？

- **位置**：[hrtimer.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/timer/hrtimer.rs)
- **问题**：
  - 文档说明。

## 6. 跨子系统关联

### 6.1 timer ↔ arch

- PIT 通过 `crate::arch!(outb/inb)`。
- 与 [subsystem-framework-arch.md §2.x](../audit/subsystem-framework-arch.md) 关联。

### 6.2 timer ↔ irq (softirq)

- Timer ISR → raise_softirq(Timer) → do_softirq → process timers。
- 与 [subsystem-framework-irq.md §2.x](../audit/subsystem-framework-irq.md) 关联。

### 6.3 timer ↔ process (scheduler)

- scheduler tick 依赖 timer subsystem。
- 与 [subsystem-framework-proc-remaining.md](../audit/subsystem-framework-proc-remaining.md) 关联。

### 6.4 timer ↔ barrier

- kswapd softirq 依赖 timer tick。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 4 | 4-6 天 |
| **P1** | 6 | 5-7 天 |
| **P2** | 7 | 2-3 天 |
| **P3** | 4 | 0.5-1 天 |
| **合计** | **21** | **12-17 天** |

### P0 修复路径（建议执行顺序）

1. **§2.2 hrtimer.rs 单文件拆分**（1-2 天）
2. **§2.1 tick 计数器内存序**（1 天）
3. **§2.3 tickless 与 hrtimer 衔接**（1 天）
4. **§2.4 Timer IRQ handler 路径**（1-2 天）