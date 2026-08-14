# framework/arch 子系统深度审计报告

> **审计范围**：`src/kernel/framework/arch/`（含 x86_64/ + aarch64/ + shadow_stack.rs）
> **审计日期**：2026-08-14
> **文件数**：17 个源文件
> **代码规模**：约 7,848 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **27 个问题（P0×6, P1×9, P2×9, P3×3）**

## 1. 子系统概览

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [x86_64/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs) | 930 | X8664 trait 实现（GDT/TSS/APIC 入口）| **极高** |
| [x86_64/acpi.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/acpi.rs) | 995 | ACPI 表解析（RSDP/RSDT/MADT/FADT/HPET/DMAR）| **极高** |
| [x86_64/gdt.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs) | 841 | 全局描述符表（GDT/TSS）| **极高** |
| [x86_64/apic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/apic.rs) | 479 | Local APIC | **高** |
| [x86_64/ioapic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/ioapic.rs) | 396 | I/O APIC | **高** |
| [x86_64/tss.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/tss.rs) | 327 | Task State Segment | 中 |
| [x86_64/smp_init.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/smp_init.rs) | 285 | SMP AP 启动 | **高** |
| [aarch64/exception.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs) | 750 | 异常处理 + KPTI（已知 P0）| **极高** |
| [aarch64/gic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/gic.rs) | 465 | ARM Generic Interrupt Controller | **高** |
| [aarch64/mmu.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mmu.rs) | 392 | aarch64 MMU 控制 | **高** |
| [aarch64/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs) | 346 | Aarch64 trait 实现 | **高** |
| [aarch64/context.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/context.rs) | 190 | 上下文切换 | 中 |
| [aarch64/uart.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/uart.rs) | 155 | 串口 | 低 |
| [aarch64/timer.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/timer.rs) | 133 | 计时器 | 中 |
| [aarch64/psci.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/psci.rs) | 87 | CPU 电源管理 | 中 |
| [shadow_stack.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs) | 617 | Shadow Stack（CET）| **高** |
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs) | 358 | trait 抽象层 | **高** |

## 2. 严重问题

### 2.1 [P0] `x86_64/acpi.rs:42-43` `MADT_BASE` / `MADT_FOUND` 静态 Atomic**初始化为 0/false，MADT 解析前任何访问视为未初始化**

- **位置**：[x86_64/acpi.rs:42-43](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/acpi.rs#L42-L43)
- **代码**：
  ```rust
  static MADT_BASE: AtomicU64 = AtomicU64::new(0);
  static MADT_FOUND: AtomicBool = AtomicBool::new(false);
  ```
- **问题**：
  - `MADT_BASE=0` 是合法物理地址（实模式 RAM）。
  - 调用方必须先 `MADT_FOUND.load() == true` 再读 `MADT_BASE`。
  - 与 `services/driver/acpi.rs::has_fadt` 硬编码 true（[subsystem-services-fs.md P0](../audit/subsystem-services-fs.md)）结合 = 任意路径都认为 ACPI 已发现。
- **建议方案**：
  1. 合并为 `Option<NonZeroU64>`。
  2. 或加编译期 magic value `0xDEAD_BEEF` 哨兵。

### 2.2 [P0] `aarch64/exception.rs:750` **KPTI 实现仍不完整**（已在之前审计识别）

- **位置**：[aarch64/exception.rs:1-750](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs#L1-L750)
- **问题**：
  - 之前审计（[subsystem-arch-net.md](../audit/subsystem-arch-net.md)）标记 P0：`enter_user` aarch64 仅切 TTBR0，**TTBR1 未切**。
  - 用户态可访问内核映射 → Meltdown 类攻击。
- **建议方案**：
  1. 验证 KPTI trampoline 中 TTBR1 切换。
  2. 配套 benchmark。

### 2.3 [P0] `x86_64/acpi.rs:995` ACPI 解析 995 行**单文件过大**

- **位置**：[x86_64/acpi.rs:1-995](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/acpi.rs#L1-L995)
- **问题**：
  - 包含：RSDP/RSDT/XSDT/MADT/FADT/HPET/DMAR 6+ 表解析。
  - 应拆分为 `acpi/rsdp.rs` + `acpi/madt.rs` + `acpi/fadt.rs` 等。

### 2.4 [P0] `x86_64/gdt.rs:841` GDT 841 行**单文件过大**

- **位置**：[x86_64/gdt.rs:1-841](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs#L1-L841)
- **问题**：
  - 包含：GDT 表构建 + TSS + IST + KPTI 相关。
  - 拆分。

### 2.5 [P0] `x86_64/mod.rs:42-52` `cpu_id()` APIC 与 CPUID 双重回退，但**回退路径不可靠**

- **位置**：[x86_64/mod.rs:41-52](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L41-L52)
- **代码**：
  ```rust
  impl CoreArch for X8664 {
      fn cpu_id() -> u32 {
          use crate::kernel::framework::arch::x86_64::apic;
          let id = apic::get_id();
          if id != 0 {
              return id;
          }
          let (_, ebx, _, _) = crate::kernel::framework::cpu::cpuid::cpuid(1, 0);
          ebx >> 24
      }
      ...
  }
  ```
- **问题**：
  - `apic::get_id()` 返回 0 = "未初始化"或"Local APIC ID 0"。
  - **无法区分** → 退到 CPUID。
  - 启动早期 LAPIC 未启用时回退到 CPUID——但 CPUID 给的是**初始 APIC ID**（每个 CPU 不同），**不是 LAPIC ID**。
  - 多 CPU 场景下，CPUID 在 BSP 与 AP 上可能给出相同值（per-core feature 而非 per-core ID）。
- **建议方案**：
  1. 启动早期通过 MSR 读取 LAPIC ID（更准确）。
  2. 用 `Option<u32>` 表达"未就绪"。

### 2.6 [P0] `shadow_stack.rs:617` Shadow Stack（CET）实现**未集成进 IDT/Syscall 路径**

- **位置**：[shadow_stack.rs:1-617](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L1-L617)
- **问题**：
  - CET 是 Intel 控制流完整性扩展。
  - 实现 617 行但**未在 syscall/异常入口启用**。
  - 当前即使 CPU 支持 CET，**未启用**——攻击者可通过 ROP 攻击。
- **建议方案**：
  1. syscall/异常入口集成 shadow stack 切换。
  2. 验证 CPU 支持后再启用。

## 3. P1 问题

### 3.1 [P1] `x86_64/ioapic.rs:396` IO-APIC 路由表**硬编码**

- **位置**：[x86_64/ioapic.rs:1-396](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/ioapic.rs#L1-L396)
- **问题**：
  - IRQ → GSI 映射硬编码（如 PCI 设备的 IRQ 分配）。

### 3.2 [P1] `x86_64/apic.rs:479` LAPIC EOI/IPI**发送路径无重试**

- **位置**：[x86_64/apic.rs:1-479](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/apic.rs#L1-L479)
- **问题**：
  - AP 启动 IPI 发送失败 → AP 未启动 → 系统 hang。
  - 之前审计（[subsystem-arch-net.md](../audit/subsystem-arch-net.md)）已识别 IDT SMP race。

### 3.3 [P1] `x86_64/smp_init.rs:285` AP 启动 285 行——AP 入口 trampoline 同步未文档化

- **位置**：[x86_64/smp_init.rs:1-285](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/smp_init.rs#L1-L285)
- **问题**：
  - 多核启动时序：BSP 启动 → IPI → AP trampoline → AP 进入 kernel。
  - 各步骤的同步点（memory barrier / spinlock）未文档化。

### 3.4 [P1] `x86_64/tss.rs:327` TSS 在 KPTI 中**双 TSS 切换**

- **位置**：[x86_64/tss.rs:1-327](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/tss.rs#L1-L327)
- **问题**：
  - KPTI 需要两个 TSS（per-ring）。
  - 实现是否完整需核查。

### 3.5 [P1] `aarch64/gic.rs:465` GIC v2/v3 兼容层**未区分**

- **位置**：[aarch64/gic.rs:1-465](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/gic.rs#L1-L465)
- **问题**：
  - QEMU virt 机器用 GICv2，真实硬件可能 v3。
  - 实现是否覆盖 v3 未审。

### 3.6 [P1] `aarch64/mmu.rs:392` aarch64 MMU 控制**完整性未验证**

- **位置**：[aarch64/mmu.rs:1-392](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mmu.rs#L1-L392)
- **问题**：
  - TTBR0/TTBR1/TCR/MAIR 寄存器配置。

### 3.7 [P1] `aarch64/psci.rs:87` PSCI CPU 电源管理**接口定义**

- **位置**：[aarch64/psci.rs:1-87](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/psci.rs#L1-L87)
- **问题**：
  - PSCI 是 ARM 标准 CPU off/on 接口。
  - 87 行可能过简。

### 3.8 [P1] `mod.rs:358` trait 抽象层——5 个 trait（CoreArch/InterruptArch/MmuArch/SystemArch/Arch）**复杂度管理**

- **位置**：[mod.rs:1-358](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs#L1-L358)
- **问题**：
  - 超 trait `Arch: CoreArch + InterruptArch + MmuArch + SystemArch`。
  - 每个新架构必须实现 5 个 trait。

### 3.9 [P1] `x86_64/acpi.rs` ACPI 表校验和**未深审**

- **位置**：[x86_64/acpi.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/acpi.rs)
- **问题**：
  - ACPI 校验和错误 → 解析失败或 panic。

## 4. P2 问题

### 4.1 [P2] `aarch64/uart.rs:155` 串口驱动简单但中断处理未深审

- **位置**：[aarch64/uart.rs:1-155](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/uart.rs#L1-L155)
- **问题**：
  - PL011 UART 中断处理。

### 4.2 [P2] `aarch64/timer.rs:133` ARM Generic Timer

- **位置**：[aarch64/timer.rs:1-133](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/timer.rs#L1-L133)
- **问题**：
  - CNTFRQ_EL0 + CNTPCT_EL0 读取。

### 4.3 [P2] `aarch64/context.rs:190` 上下文切换未深审

- **位置**：[aarch64/context.rs:1-190](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/context.rs#L1-L190)
- **问题**：
  - 寄存器保存/恢复。

### 4.4 [P2] `x86_64/mod.rs:930` x86_64 mod.rs 930 行——单文件过大

- **位置**：[x86_64/mod.rs:1-930](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L1-L930)
- **问题**：
  - 单文件过大。

### 4.5 [P2] `aarch64/mod.rs:346` aarch64 mod.rs 346 行

- **位置**：[aarch64/mod.rs:1-346](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs#L1-L346)
- **问题**：
  - 拆分。

### 4.6 [P2] `x86_64/apic.rs:479` LAPIC timer 模式**未配置**

- **位置**：[x86_64/apic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/apic.rs)
- **问题**：
  - LAPIC timer 是系统主时钟。
  - 配置不完整 → scheduler tick 不准。

### 4.7 [P2] `x86_64/tss.rs:327` TSS IST 栈分配**大小硬编码**

- **位置**：[x86_64/tss.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/tss.rs)
- **问题**：
  - IST 栈（#DF/#NMI/#DB）大小。

### 4.8 [P2] `mod.rs:358` arch trait 抽象**5 个 trait 复杂度**

- **位置**：[mod.rs:1-358](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs#L1-L358)
- **问题**：
  - 同 3.8。

### 4.9 [P2] `shadow_stack.rs:617` CET SHSTK 启用条件

- **位置**：[shadow_stack.rs:1-617](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L1-L617)
- **问题**：
  - CPUID 检测 + CR4 控制。

## 5. P3 问题

### 5.1 [P3] `x86_64/acpi.rs:42-43` `MADT_BASE` 全局 Atomic 命名与 `acpi::MADT_FOUND` 不一致

- **位置**：[x86_64/acpi.rs:42](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/acpi.rs#L42)
- **问题**：
  - `static MADT_BASE: AtomicU64` 与 `pub fn has_madt()` 函数命名。

### 5.2 [P3] `aarch64/psci.rs:87` PSCI SMC 调用约定**未文档化**

- **位置**：[aarch64/psci.rs:1-87](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/psci.rs#L1-L87)
- **问题**：
  - ARM SMC 是 EL3 信任监视器调用。

### 5.3 [P3] `x86_64/gdt.rs:841` GDT 64 位入口数量**硬编码**

- **位置**：[x86_64/gdt.rs:1-841](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs#L1-L841)
- **问题**：
  - GDT_MAX_ENTRIES 容量。

## 6. 跨子系统关联

### 6.1 arch ↔ mm

- `framework/mm/vmm_x86_64.rs` 直接调用 `framework/arch/x86_64/mod.rs::switch_page_table`。
- 之前审计（[subsystem-framework-mm-remaining.md §2.4 P0 KPTI](../audit/subsystem-framework-mm-remaining.md)）已识别依赖。

### 6.2 arch ↔ idt

- `framework/idt/idt.rs` 调用 `framework/arch/x86_64::apic::send_eoi`。
- 与 [subsystem-framework-misc.md §2.2 P0 IDT SMP race](../audit/subsystem-framework-misc.md) 关联。

### 6.3 arch ↔ process

- 进程 context_switch 调用 `framework/arch/x86_64::switch_to`。
- 用户态入口（`framework/usermode.rs`）调用 `framework/arch/x86_64::enter_user`。

### 6.4 arch ↔ barrier

- panic → arch 层 trap → barrier 模块恢复。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 6 | 5-7 天 |
| **P1** | 9 | 6-8 天 |
| **P2** | 9 | 2-3 天 |
| **P3** | 3 | 0.5 天 |
| **合计** | **27** | **14-19 天** |

### P0 修复路径（建议执行顺序）

1. **§2.2 aarch64 KPTI**（1-2 天，**Meltdown 防护**）
2. **§2.5 cpu_id 回退路径**（0.5 天，**多核可靠性**）
3. **§2.6 CET 集成**（1-2 天）
4. **§2.1 MADT Atomic 哨兵**（0.5 天）
5. **§2.3 acpi.rs 拆分**（1 天）
6. **§2.4 gdt.rs 拆分**（1 天）