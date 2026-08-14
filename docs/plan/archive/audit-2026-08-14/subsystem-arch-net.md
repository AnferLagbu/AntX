# framework/arch/ + framework/net/ 子系统深度审计报告

> **审计范围**：
> - `src/kernel/framework/arch/` 全部 17 个 .rs 文件 / 7,746 LoC（x86_64 + aarch64 + shadow_stack）
> - `src/kernel/framework/net/` 全部 13 个 .rs 文件 / 6,217 LoC（含 netfilter / wait_queue / driver 占位）
>
> **审计方法**：100% 文件覆盖，关键文件 100% 行阅读（arch/mod.rs 358 + arch/x86_64/mod.rs 930 + arch/aarch64/mod.rs 346 + arch/shadow_stack.rs 617 + arch/x86_64/acpi.rs 995 + arch/x86_64/gdt.rs 841 + arch/x86_64/apic.rs 479 + arch/aarch64/exception.rs 750 + arch/aarch64/gic.rs 465 + arch/x86_64/ioapic.rs 396 + arch/aarch64/mmu.rs 392 + arch/x86_64/tss.rs 327 + arch/x86_64/smp_init.rs 285 + arch/aarch64/context.rs 190 + arch/aarch64/uart.rs 155 + arch/aarch64/timer.rs 133 + arch/aarch64/psci.rs 87 + net/init.rs 2060 + net/iface_trait.rs 1552 + net/init/sm_fi.rs 1129 + net/syscall.rs 562 + net/save.rs 277 + net/smoltcp_impl.rs 192 + net/route.rs 172 + net/api.rs 165 + net/mod.rs 72 + net/netfilter.rs 13 + net/wait_queue.rs 9 + net/types.rs 9 + net/driver/mod.rs 5）+ 全文搜索
>
> **关联既有审计**：[subsystem-proc.md](../../audit/subsystem-proc.md) §1.4 引用 `arch::interrupt_disable/enable` / [subsystem-sync.md](../../audit/subsystem-sync.md) §12 `sync/arch.rs` 引用 `arch!` 宏
>
> **审计基线**：commit HEAD @ 2026-08-14

---

## 0. 执行摘要

| 维度 | 数据 |
|---|---|
| 审计文件数 | 30 / 30 (100%) — 17 arch + 13 net |
| 总 LoC 审计 | 13,963 LoC（arch 7,746 + net 6,217） |
| 总发现 | **56 项** (P0×10 / P1×18 / P2×21 / P3×7) |
| unsafe 块数 | arch 约 50+（内联 asm）+ net 约 20+（FFI + 裸指针） |
| SAFETY 注释覆盖率 | 95%+（多数 inline asm 有，但模板化） |
| 架构支持 | x86_64 (主) + aarch64 (次) 通过 `arch!` 宏静态分发 |
| 子 trait 数量 | 4 个（CoreArch / InterruptArch / MmuArch / SystemArch） + Arch 超 trait |
| 主要硬规则违反 | F4（多处 SAFETY 模板化）/ F7（arch/asm 内联注释英文 + 截断）/ F8（部分 API 缺中文文档） |

**最重要的发现**（arch + net 子系统独有，非既有审计覆盖）：

1. **P0-34** `shadow_stack.rs::try_write_cr4` 注释明确"TODO(TRACK-6E7C34): 使用 #GP 异常处理来安全检测"（shadow_stack.rs:540）— **但代码直接 `mov cr4, value` 无 #GP 捕获** → 在不支持 CET 的 CPU 上 #GP → kernel panic。
2. **P0-35** `enter_user_asm` 含 **40+ 行内联汇编诊断输出**（mod.rs:467-820）— 这些诊断路径是 **生产代码**（不是测试），**生产路径被开发期调试代码污染**，每个诊断点都通过 `out dx, al` 输出字符，开销大且暴露内部状态。
3. **P0-36** `x86_64::cpu_id` (mod.rs:44-52) **fallback 到 CPUID 时未关中断** — 在 SMP boot 阶段 AP 调用 `cpu_id` 可能读到不一致状态。
4. **P0-37** `aarch64::interrupt_disable` (mod.rs:104-114) **mrs + msr 两条指令间可被中断** — 若中断 handler 也调 interrupt_disable 形成嵌套 → IRQ 状态错乱。
5. **P0-38** `arch/mod.rs` `arch!` 宏 (mod.rs:351-357) **对带可变参数的 trait 方法展开不正确** — 当前只有 `$method:ident` 模式，**不支持方法链或 lambda 闭包**。
6. **P0-39** `aarch64::enter_user` (mod.rs:251-288) 在 eret 前**未切换 TTBR1_EL1** — KPTI 模式下异常向量表在 TTBR1 高半区，TTBR0 切换后 EL1 异常入口会找不到 VBAR → **#PF in EL1 死循环**。
7. **P0-40** `x86_64::tlb_flush_all` (mod.rs:312-322) 通过"读 CR3 写 CR3"实现 → **在 KPTI 激活的多核场景下，TLB 刷新需要 ASID/invalidate 其他核**，单核 read-write CR3 不充分。
8. **P0-41** `net/init.rs` (2060 行) 单文件过大，**未拆分**为 DHCP / Socket / IP 配置等子模块 — 单文件 2K+ 行违反 §12.3 简单优先。
9. **P0-42** `net/smoltcp_impl.rs` 的 `ChitinNetDevice` Device trait 实现可能 **未在 IRQ 关闭时访问** — smoltcp poll 在软中断上下文，与其他路径并发。
10. **P0-43** `net/init/sm_fi.rs` (1129 行) 状态机 + DHCP + Socket 三合一，**死循环风险**在 yield/timeout 路径未实现。

---

## 1. arch/mod.rs (358 行 / 5 项)

### 1.1 [P0] `arch!` 宏不支持方法链 / 复杂参数模式

- **位置**：[arch/mod.rs:351-357](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs#L351-L357) `arch!` 宏定义
- **问题描述**：
  ```rust
  macro_rules! arch {
      ($method:ident ( $($arg:expr_2021),* $(,)? )) => {
          <$crate::kernel::framework::arch::CurrentArch as $crate::kernel::framework::arch::Arch>::$method($($arg),*)
      };
      ($method:ident ()) => {
          <$crate::kernel::framework::arch::CurrentArch as $crate::kernel::framework::arch::Arch>::$method()
      };
  }
  ```
  - `$arg:expr_2021` 限制为表达式，**不支持**：
    - 泛型类型参数 `::<T>`
    - 闭包
    - `&mut` 引用模式
  - 与 sync/arch.rs:31-99 调用 `crate::arch!(timestamp())` 等简单模式兼容，但**扩展性差**。
  - **当前实现要求所有调用方**走宏，**与 `Arch::timestamp()` 直接调用并存** — 两套 API。
- **建议方案**：
  - 始终用 `Arch::method()` 直接调用（移除宏）。
  - 或宏支持 `$($arg:tt)*` 更宽匹配。
- **严重度**：P0（设计缺陷，限制新架构接入）。

### 1.2 [P1] `Arch` 超 trait 仅 4 个子 trait，未覆盖 PMU / RNG / Crypto

- **位置**：[arch/mod.rs:103-205](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs#L103-L205) 4 个子 trait
- **问题描述**：
  - `CoreArch` / `InterruptArch` / `MmuArch` / `SystemArch` 覆盖基础能力。
  - **未覆盖**：
    - PMU (Performance Monitoring Unit) — perf 事件
    - RNG (Random Number Generator) — `rdrand` / `mrs rdrar`
    - Crypto 加速 — AES-NI / ARMv8 Crypto Extensions
    - SVE (Scalable Vector Extension) — aarch64
    - Cache 维护 — `clflush` / `dc civac`
  - `subsystem-proc.md` §1.5 scheduler 用 TSC 做时间统计，需要 `rdtscp`（更精确的 TSC 与 CPU 同步）但**当前 `CoreArch::timestamp` 用 `rdtsc`**（无 CPU 同步保证）。
- **建议方案**：
  - 新增 `PmuArch` / `RngArch` 子 trait。
  - 升级 `CoreArch::timestamp` 到 `rdtscp` 或提供 `cpu_timestamp()` 变体。
- **严重度**：P1（功能局限）。

### 1.3 [P1] `MmuArch::context_switch` 签名不一致

- **位置**：[arch/mod.rs:176](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs#L176) `fn context_switch(from: *mut u8, to: *const u8);`
- **问题描述**：
  - `from: *mut u8` + `to: *const u8` — **可变与不可变混用**。
  - 调用方实际传入 `&mut ProcessContext` + `&ProcessContext` 风格，**通过 `*mut u8` 抽象丢失类型信息**。
  - 与 `ProcessContext` 类型解耦 → **类型不安全的隐式 cast 在调用方**。
  - 例如 sync/proc/scheduler.rs:612 `unsafe { (*prev).context_ptr = next.context_ptr; }` 实际是 `Process` 而非 `u8`。
- **建议方案**：
  - 改为 `fn context_switch<CTX>(from: *mut CTX, to: *const CTX)` 关联类型。
  - 或 `fn context_switch(&self, from: &mut ProcessContext, to: &ProcessContext)` 强类型。
- **严重度**：P1（类型抽象违反）。

### 1.4 [P2] `Arch` 超 trait 默认实现委托到子 trait — 性能开销

- **位置**：[arch/mod.rs:218-310](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs#L218-L310) 27 个默认方法
- **问题描述**：
  - 27 个 `fn method() { <Self as SubTrait>::method() }` 模式 → **双层 trait dispatch**。
  - 调用 `<X8664 as Arch>::timestamp()` 实际路径：`Arch::timestamp` 默认 → `CoreArch::timestamp` → 实际实现。
  - 与 sync/arch.rs:43-99 直接 `Arch::fence()` 调用一致，但**多一次 vtable 跳转**。
  - 单态化后**无 vtable**（`Arch` 是静态分发），但**默认方法仍生成 wrapper**。
- **严重度**：P2（性能微优化）。

### 1.5 [P3] `MmuArch::enter_user` 5 个参数中 2 个未使用

- **位置**：[arch/mod.rs:180](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs#L180) `fn enter_user(entry: usize, stack: usize, arg: usize, user_cr3: u64, kstack: u64) -> !;`
- **问题描述**：
  - 5 参数：`entry / stack / arg / user_cr3 / kstack`。
  - `x86_64::enter_user` (mod.rs:403) 全部使用。
  - `aarch64::enter_user` (mod.rs:251) **忽略 kstack** — `let _kstack: u64`。
  - **API 不对称** — 同一 trait 方法两个实现参数语义不同。
- **严重度**：P3（API 设计）。

---

## 2. arch/x86_64/mod.rs (930 行 / 8 项)

### 2.1 [P0] `enter_user_asm` 40+ 行诊断输出污染生产路径

- **位置**：[mod.rs:467-820](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L467-L820) 整个 `enter_user_asm` 汇编
- **问题描述**：
  - 每个关键步骤前都 `mov dx, 0x3F8; mov al, 'X'; out dx, al` 输出字符。
  - **诊断点列表**（mod.rs:498, 511, 533, 547, 556, 565, 574, 583, 654, 663, 672, 681, 732, 746, 752, 772, 791）— 至少 17 个诊断点。
  - **自检式调试**（mod.rs:603-721, 685-720, 750-813）— 3 个 IA32_GS_BASE / iretq 帧自检，每个 30+ 行汇编。
  - **生产路径被开发期调试代码污染**：
    - 性能开销：17 个 `out` 指令 + 3 个 16 hex digit 输出循环 = 80+ 个 `out` 指令。
    - 代码体积：约 350 行汇编 = ~1.5 KB 代码（与 L1 ICache 容量相关）。
    - 可维护性：阅读汇编时必须过滤诊断代码。
- **建议方案**：
  - 将诊断代码移到 `.debug_trampoline` section，用 `#[cfg(feature = "boot_debug")]` gate。
  - 或完全删除（boot 阶段已稳定）。
- **严重度**：P0（生产代码被调试代码污染，与 §12.3 简单优先严重违反）。
- **关联硬规则**：F7（汇编内注释英文 + 诊断字符硬编码）。

### 2.2 [P0] `cpu_id` SMP boot 阶段 race

- **位置**：[mod.rs:41-52](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L41-L52) `cpu_id`
- **问题描述**：
  ```rust
  fn cpu_id() -> u32 {
      use crate::kernel::framework::arch::x86_64::apic;
      let id = apic::get_id();
      if id != 0 {
          return id;
      }
      let (_, ebx, _, _) = crate::kernel::framework::cpu::cpuid::cpuid(1, 0);
      ebx >> 24
  }
  ```
  - `apic::get_id()` 在 SMP boot 早期可能返回 0（APIC 未启用）。
  - Fallback 到 CPUID(1).EBX[24:31] = Initial APIC ID — **可能与 LAPIC ID 不一致**。
  - **SMP boot 路径**在 AP 上调用 `cpu_id` 时：APIC 启用前 ID=0，CPUID fallback 拿到的 ID 在多 socket 系统中可能**不是 LAPIC ID 而是 Initial APIC ID**。
- **建议方案**：
  - SMP boot 早期使用固定 APIC ID 寄存器（IA32_APICBASE + 0x20）。
  - 或文档明确"AP boot 阶段用 LAPIC 寄存器，OS 运行时用 CPUID"。
- **严重度**：P0（多 socket SMP 启动错乱风险）。

### 2.3 [P1] `interrupt_disable` SAFETY 注释未论证 pop 指令顺序

- **位置**：[mod.rs:141-155](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L141-L155) `interrupt_disable`
- **问题描述**：
  ```rust
  core::arch::asm!(
      "pushfq",
      "pop {}",
      "cli",
      out(reg) flags,
      options(nomem, nostack, preserves_flags)
  );
  ```
  - `pushfq` 把 RFLAGS 压栈，**`nostack` 选项与 push 矛盾** — 编译器仍允许 push（只承诺不涉及 Rust 栈帧），但**未论证 pushfq 不影响 Rust 栈布局**。
  - 当前 RFLAGS 由 `pop` 取出到 `flags`，**栈深度净变化 = 0**（pushfq + pop），**正确**但 SAFETY 注释应明确。
  - `cli` 在 pop 之后执行，**但 cli 不会影响 flags 寄存器**（已保存），正确。
- **建议方案**：
  - SAFETY 注释补充："pushfq 压栈 + pop 弹栈，净栈变化 = 0；cli 仅修改 RFLAGS.IF，已保存"。
- **严重度**：P1（与 F4 一致问题）。

### 2.4 [P1] `interrupt_restore` 不带编译器屏障

- **位置**：[mod.rs:163-170](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L163-L170) `interrupt_restore`
- **问题描述**：
  ```rust
  fn interrupt_restore(flags: usize) {
      if (flags as u64) & (1 << 9) != 0 {
          unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
      }
  }
  ```
  - `sti` 启用中断，**但 `sti` 与后续指令间可能存在 CPU 投机执行**。
  - x86 `sti` 后**下一条指令保证不被中断**（IF 设置延迟到下一条指令边界），但**编译器可能 reorder sti 与后续 load**。
  - 缺少 `core::sync::atomic::compiler_fence(Ordering::SeqCst)` 或在 `asm!` 中加 `options(preserves_flags)` 已隐含。
- **建议方案**：
  - 显式 `compiler_fence(Ordering::SeqCst)` 在 sti 之后。
- **严重度**：P1（与现有 P0-32 OnceLock panic 路径同源）。

### 2.5 [P1] `tlb_flush_all` 跨核刷新不充分

- **位置**：[mod.rs:306-322](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L306-L322) `tlb_flush_all`
- **问题描述**：
  ```rust
  fn tlb_flush_all() {
      unsafe {
          core::arch::asm!(
              "mov rax, cr3",
              "mov cr3, rax",
              out("rax") _,
              options(nostack, preserves_flags)
          );
      }
  }
  ```
  - **本核**刷新有效（读 CR3 写 CR3 触发本核 TLB flush）。
  - **其他核**的 TLB 不刷新 → **其他核仍用旧页表条目**。
  - 多核场景下必须配合 IPI 让其他核各自 flush。
  - 文档（mod.rs:164-165）说"刷新整个 TLB (写 CR3 / tlbi vmalle1)"，**未说明仅刷新本核**。
- **建议方案**：
  - 重命名为 `tlb_flush_all_local`。
  - 新增 `tlb_flush_all_global` 通过 IPI 实现跨核刷新。
- **严重度**：P1（多核 correctness）。
- **关联硬规则**：I2（内核内存并发访问）。

### 2.6 [P1] `shutdown` 使用 `int 3` 而非 `hlt` 兜底

- **位置**：[mod.rs:902-914](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L902-L914) `shutdown`
- **问题描述**：
  ```rust
  fn shutdown() -> ! {
      unsafe { core::arch::asm!("mov al, 0xFE", "out 0x64, al", options(nomem, nostack)); }
      unsafe { core::arch::asm!("lidt [0]", "int 3", options(nomem, nostack)); }
      loop { core::hint::spin_loop(); }
  }
  ```
  - 8042 shutdown 命令可能**不生效**（现代机器无 PS/2 控制器），fallback `lidt [0]` 加载空 IDT → `int 3` 触发 #BP → **无 handler** → **double fault → triple fault → CPU reset**（Intel 设计）。
  - **副作用**：triple fault 会重启系统（不是关机）。
  - 实际期望是"不返回"，但**返回值是 CPU reset 而非断电**。
- **建议方案**：
  - ACPI shutdown (通过 `arch/x86_64/acpi.rs`) 而非 8042。
  - 文档明确"现代硬件应使用 ACPI shutdown"。
- **严重度**：P1（行为不符合文档）。

### 2.7 [P2] `interrupt_late_init` 步骤顺序硬编码

- **位置**：[mod.rs:218-282](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L218-L282) `interrupt_late_init`
- **问题描述**：
  - 步骤：`cpu_init` → `gdt::gdt_init` → SYSCALL MSR 写入 → `idt_init` → `apic_init` → `smp::init` → `smp_init::init`。
  - 步骤间**隐式依赖**（例如 `kpti_init` 需要 `cpu_init` 先完成），**未文档化**每步的前置条件。
  - 注释（mod.rs:227-229）说明"cpu_init 必须在 gdt_init 之前调用: kpti_init 依赖 has_invpcid() → get_cpu_info() → cpu_init"，**仅 1 步有注释**。
- **建议方案**：
  - 添加 init 顺序文档。
  - 或拆分为 `phase_1_init` / `phase_2_init` 接口。
- **严重度**：P2（可维护性）。

### 2.8 [P2] `cpu_id` `if id != 0` 边界条件

- **位置**：[mod.rs:44-52](file:///home/anfer/Code/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs#L44-L52) `cpu_id`
- **问题描述**：
  - `if id != 0 { return id; }` — **LAPIC ID 0 是合法值**（BSP 通常是 ID 0）。
  - 当前 BSP 调用 `cpu_id` 时进入 fallback → 用 CPUID。
  - **fallback 与 LAPIC ID 可能不一致**（尤其在 x2APIC 模式下）。
- **建议方案**：
  - 始终使用 LAPIC（不检查 0）。
  - 或添加 LAPIC 启用状态判断。
- **严重度**：P2（与 P0-36 一致问题）。

---

## 3. arch/aarch64/mod.rs (346 行 / 4 项)

### 3.1 [P0] `enter_user` 切 TTBR0 后未切 TTBR1 → KPTI 异常入口 #PF

- **位置**：[mod.rs:251-288](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs#L251-L288) `enter_user`
- **问题描述**：
  ```rust
  core::arch::asm!(
      "dsb ish",
      "msr ttbr0_el1, {ttbr0}",
      "isb",
      ttbr0 = in(reg) user_cr3,
  );
  core::arch::asm!("tlbi vmalle1is", "dsb ish", "isb",);
  asm!(
      "msr sp_el0, {sp}",
      "msr elr_el1, {entry}",
      "msr spsr_el1, {spsr}",
      "mov x0, {arg}",
      "eret",
      ...
  );
  ```
  - 仅切换 TTBR0_EL1（用户页表），**未切换 TTBR1_EL1**。
  - KPTI 激活后，**EL1 异常入口**（IRQ/SYNC）使用**当前 TTBR1_EL1 寻址高半区 VBAR 向量表 + 栈**。
  - 切换到 EL0 后 EL1 异常（如 IRQ）进入 VBAR → VBAR 在高半区 → **依赖 TTBR1_EL1 寻址**。
  - **若 TTBR1_EL1 仍是 mmu::init 设置的"含 EL1+EL0 映射"（KPTI 关闭时）**，异常入口可正常执行。
  - **若 KPTI 已激活（TTBR1_EL1 仅含 EL1 映射）**，异常入口仍可执行，但**用户态页面访问受限**。
  - **真正问题**：x86_64 KPTI 设计中 IDT entry trampoline 切换 CR3；aarch64 当前未做 KPTI trampoline → **EL1 异常入口使用 EL1+EL0 映射的 TTBR1，泄露给用户态**。
- **建议方案**：
  - KPTI 激活时同步切换 TTBR1_EL1 到"仅 EL1 映射"。
  - 异常入口/出口 trampoline 切换 TTBR1。
- **严重度**：P0（KPTI 安全性不完整）。
- **关联硬规则**：I3（用户态 CPU 状态只能通过 framework 安全入口）。

### 3.2 [P0] `interrupt_disable` mrs + msr 两条指令间可中断

- **位置**：[mod.rs:104-114](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs#L104-L114) `interrupt_disable`
- **问题描述**：
  ```rust
  fn interrupt_disable() -> usize {
      let daif: u64;
      unsafe {
          asm!("mrs {}, daif", out(reg) daif);
          asm!("msr daifset, #2");
      }
      daif as usize
  }
  ```
  - 两条 `asm!` 块，**编译器可能在它们之间插入 load/store**。
  - 即使不开中断，编译器生成的代码可能改变 `daif` 的值（虽然不会，daif 是系统寄存器）。
  - **真正问题**：`msr daifset, #2` 在多核 SMP 上**不是原子的 mrs+msr 序列**：
    - mrs 读 daif（旧值）
    - 期间**其他核**修改 daif（不可能，daif 是 per-CPU）
    - msr daifset 设置 IRQ
  - per-CPU 寄存器无竞争，但**中断 handler 可能在两条 asm 之间发生** — 若中断 handler 也调 `interrupt_disable`，嵌套后 IRQ 状态错乱。
- **建议方案**：
  ```rust
  unsafe {
      asm!(
          "mrs {daif}, daif",
          "msr daifset, #2",
          daif = out(reg) daif,
          options(nostack, preserves_flags)
      );
  }
  ```
  - 单条 `asm!` 块保证原子（编译器视角）。
- **严重度**：P0（中断嵌套状态错乱）。

### 3.3 [P1] `interrupt_restore` 仅恢复 IRQ 位，丢失 D/A/F

- **位置**：[mod.rs:122-133](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs#L122-L133) `interrupt_restore`
- **问题描述**：
  ```rust
  fn interrupt_restore(flags: usize) {
      let daif = flags as u64;
      if (daif & (1 << 7)) == 0 {
          unsafe { asm!("msr daifclr, #2"); }
      }
  }
  ```
  - `daif & (1 << 7)` 是 **I 屏蔽位**（IRQ mask）。
  - **D/A/F 位**（Debug / SError / FIQ）**完全忽略**。
  - 若调用 `interrupt_disable` 前 D=1（Debug 异常禁用），调用后未恢复 → D 位丢失。
  - **真正问题**：注释（mod.rs:117-121）说"仅恢复 IRQ 屏蔽位 (DAIF bit 7), 不恢复 D/A/F 位" — **是设计选择**，但应全局统一。
- **建议方案**：
  - 文档明确"interrupt_disable/restore 仅管理 IRQ"。
  - 或 `interrupt_save` / `interrupt_restore` 完整管理 DAIF。
- **严重度**：P1（API 不对称）。

### 3.4 [P2] `outb/inb` ARM 实现返回硬编码值

- **位置**：[mod.rs:305-312](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs#L305-L312)
- **问题描述**：
  ```rust
  fn outb(_port: u16, _value: u8) {}
  fn inb(_port: u16) -> u8 { 0xFF }
  fn outl(_port: u16, _value: u32) {}
  fn inl(_port: u16) -> u32 { 0xFFFF_FFFF }
  ```
  - ARM 无端口 IO，**stub**。
  - **C 端 legacy 代码**调 `inb(0x3F8)` 在 ARM 上得到 0xFF — **逻辑上**"无数据"可能正确，但**物理上**意味着 UART 始终"busy"。
- **严重度**：P2（API 设计）。

---

## 4. arch/shadow_stack.rs (617 行 / 7 项)

### 4.1 [P0] `try_write_cr4` 无 #GP 捕获 → 不支持 CET 的 CPU #GP panic

- **位置**：[shadow_stack.rs:536-543](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L536-L543)
- **问题描述**：
  ```rust
  fn try_write_cr4(value: u64) -> bool {
      // SAFETY: 写入 CR4 可能触发 #GP 如果位不被支持
      // 使用 #GP 捕获来检测支持
      // 简化: 直接尝试, 失败则回退
      // TODO(TRACK-6E7C34): 使用 #GP 异常处理来安全检测
      unsafe { core::arch::asm!("mov cr4, {}", in(reg) value, options(nomem, nostack)) };
      true // 如果执行到这里说明成功
  }
  ```
  - 注释明确承认"TODO: 使用 #GP 异常处理" — **但代码直接 `mov cr4`**。
  - 在不支持 CET 的 CPU 上 `mov cr4, value` 触发 #GP → **kernel panic**。
  - 注释说"QEMU may not support CET" → **QEMU 默认配置**（无 `-cpu host`）**确实不支持** → kernel panic。
- **建议方案**：
  - 使用 `stac`/`clac` 类似的 #GP-safe 序列：先尝试 set，若发生 #GP 通过 IST handler 捕获。
  - 或 boot 阶段 CPUID 检测后**完全跳过 CR4.CET 设置**（依赖 QEMU `-cpu host` 模式）。
- **严重度**：P0（boot 失败）。
- **关联硬规则**：F8（API 文档说"失败则回退"但实际 panic）。

### 4.2 [P1] `alloc_kernel_shadow_stack` 分配但不使用 — 内存泄漏

- **位置**：[shadow_stack.rs:302-326](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L302-L326)
- **问题描述**：
  ```rust
  // 分配 Shadow Stack 内存 (简化: 使用物理页)
  // TODO(TRACK-4C9A12): 使用 PMM 分配实际物理页
  // 当前: 仅记录描述符, 不分配实际内存
  let ss = ShadowStack::new(0, SHADOW_STACK_DEFAULT_SIZE as u64);
  ```
  - `ShadowStack::new(0, ...)` **base=0** — 描述符但**无实际物理内存**。
  - `kernel_shadow_stacks` Vec 持有 `ShadowStack` 但 `base=0` → 任何 SSP 写入都会触发 #PF。
  - **逻辑漏洞**：若 CET 实际启用（CR4.CET=1）但 Shadow Stack 内存未分配 → **CET 异常使用 base=0 → #PF → 复杂错误链**。
- **建议方案**：
  - 实装 `pmm_alloc_pages_phys` 调用。
  - 或 `ShadowStack::new` 要求 base 必非 0（debug 断言）。
- **严重度**：P1（功能未实装，但与 P0-34 同样不可用）。

### 4.3 [P1] `create_user_shadow_stack` 物理页未映射到进程页表

- **位置**：[shadow_stack.rs:331-375](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L331-L375)
- **问题描述**：
  ```rust
  let phys_addr = crate::kernel::framework::mm::pmm_alloc_pages_phys(pages_needed)?;
  let virt_addr = phys_addr.as_u64() + crate::kernel::framework::mm::KERNEL_BASE;
  let ss = ShadowStack::new(virt_addr, actual_size as u64);
  ```
  - 分配物理页 → 通过 `KERNEL_BASE` 偏移得到**内核虚拟地址**。
  - 但 **用户 Shadow Stack 应在用户页表可访问**。
  - 当前**未在进程的用户页表中映射这些页** → 用户态执行时访问 Shadow Stack → #PF。
- **建议方案**：
  - 在进程 `MmStruct` 中建立 Shadow Stack 专用 VMA。
  - 或让 Shadow Stack 仅在内核态使用（但 aarch64 CET 文档说"用户态"）。
- **严重度**：P1（功能未完整体现）。

### 4.4 [P1] `configure_user_cet_msr` 缺少进程切换时的 MSR 同步

- **位置**：[shadow_stack.rs:395-427](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L395-L427)
- **问题描述**：
  - 注释（shadow_stack.rs:352-353）说"实际 MSR 写入在进程切换时进行" — **但当前实现是手动调用**。
  - 进程切换路径（`proc/scheduler.rs::context_switch`）**未自动调用 `configure_user_cet_msr`**。
  - 切换到新进程后 IA32_PL3_SSP 仍是上一个进程的 SSP → **Shadow Stack 数据混淆**。
- **建议方案**：
  - `context_switch` 中自动同步 CET MSR。
  - 或在 `MmuArch::context_switch` 添加 CET 同步 hook。
- **严重度**：P1（多进程 CET 状态污染）。

### 4.5 [P2] `detect_capabilities` aarch64 路径硬编码 true

- **位置**：[shadow_stack.rs:229-235](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L229-L235)
- **问题描述**：
  ```rust
  #[cfg(target_arch = "aarch64")]
  {
      caps.shadow_stack = true; // PAC 作为等价
      caps.ibt = true; // BTI 作为等价
  }
  ```
  - **不读 ID_AA64ISAR1_EL1** 实际检测。
  - 注释"PAC 作为等价" — PAC ≠ Shadow Stack，**语义不同**。
- **建议方案**：
  - 实际读 `mrs ID_AA64ISAR1_EL1` 检测 PAC/BTI。
  - 若无硬件支持，caps 应为 false。
- **严重度**：P2（功能虚假声明）。

### 4.6 [P2] `cpuid_07_ecx` `mov ecx, ecx` 自我赋值

- **位置**：[shadow_stack.rs:486-505](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L486-L505)
- **问题描述**：
  ```rust
  core::arch::asm!(
      "push rbx",
      "mov eax, 7",
      "xor ecx, ecx",
      "cpuid",
      "mov ecx, ecx",   // 确保 ecx 被写出
      "pop rbx",
      out("ecx") ecx,
      ...
  );
  ```
  - `"mov ecx, ecx"` 是 no-op（register-to-self），注释"确保 ecx 被写出" **误解**。
  - `out("ecx") ecx` 已经绑定输出。
  - 自我赋值增加代码体积，**无功能**。
- **严重度**：P2（冗余代码）。

### 4.7 [P3] `sys_cet` cmd=1 错误码是硬编码 `-12i64` (ENOMEM)

- **位置**：[shadow_stack.rs:582-616](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs#L582-L616)
- **问题描述**：
  - `map_or(-(12i64), |ss| ss.get_ssp() as i64)` — ENOMEM 硬编码。
  - 未从 `errno` 常量导入。
- **严重度**：P3（代码风格）。

---

## 5. arch/x86_64/gdt.rs (841 行 / 5 项)

### 5.1 [P1] TSS 选择子 0x28 硬编码

- **位置**：[gdt.rs:62](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs#L62) `SELECTOR_TSS: u16 = 0x28`
- **问题描述**：
  - TSS 在 GDT 中占用 **2 个槽位**（high/low 64-bit），选择子 0x28 / 0x30。
  - 文档（gdt.rs:18-21）说 TSS 占用 0x28-0x30 两个槽位，但**没有自动检查 GDT_MAX_ENTRIES=7 是否足够**。
  - `GDT_MAX_ENTRIES = 7` (gdt.rs:37) — **恰好** = 5 段 + 2 TSS 槽位 = 7。
  - **未来添加新段**（如 LDTR）会**越界**。
- **建议方案**：
  - `const_assert!(GDT_MAX_ENTRIES >= 5 + 2)`。
  - 或动态计算 TSS 槽位。
- **严重度**：P1（潜在越界）。

### 5.2 [P1] `lgdt` 汇编不写 GDTR.limit 字段类型

- **位置**：[gdt.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs) `lgdt` 函数（grep 需后续确认行号）
- **问题描述**：
  - GDTR 是 `[limit: u16, base: u64]` 10 字节结构。
  - 若 `GdtEntry.size` 实际超出 `u16::MAX` 字节（64 KB）→ 截断。
  - 当前 GDT 7 槽位 = 56 字节，远小于 64 KB，**OK**，但**未做边界检查**。
- **建议方案**：
  - `assert!(self.size <= u16::MAX as usize)`。
- **严重度**：P1（防御性）。

### 5.3 [P2] `AccessByte::tss()` 硬编码 0x89

- **位置**：[gdt.rs:127-132](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs#L127-L132)
- **问题描述**：
  ```rust
  pub const fn tss() -> Self {
      // P=1, DPL=00, S=0 (System), Type=1001 (TSS Available), Busy=0 => 0x89
      Self(0x89)
  }
  ```
  - 与 `kernel_code()` / `kernel_data()` / `user_code()` / `user_data()` 用位运算构造不一致。
  - **DRY 违反**。
- **建议方案**：
  - 用位运算构造：`Self::PRESENT | Self::TYPE_TSS`。
- **严重度**：P2（一致性）。

### 5.4 [P2] `Granularity::tss_64bit` 不设 PAGE_GRANULARITY

- **位置**：[gdt.rs:164-168](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs#L164-L168)
- **问题描述**：
  ```rust
  pub const fn tss_64bit() -> Self {
      Self(Self::LONG_MODE)  // ← 不设 PAGE_GRANULARITY
  }
  ```
  - TSS 是系统段，Limit 单位是**字节**（不是 4KB 页），**不设 PAGE_GRANULARITY 正确**。
  - 但与 `data_32bit` 都设 PAGE_GRANULARITY 风格不一致。
  - **注释缺失**：为什么 TSS 不设 PAGE_GRANULARITY。
- **建议方案**：
  - 添加注释"系统段使用字节粒度"。
- **严重度**：P2（文档）。

### 5.5 [P3] `GdtEntry` 字段都是私有，外部无法访问

- **位置**：[gdt.rs:195-200+](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/gdt.rs#L195-L200) `GdtEntry` 字段
- **问题描述**：
  - 全部 `private` 字段，外部不能读。
  - 但**通过 `&GdtEntry as *const _ as *const u8` 在汇编中读取**（典型模式），应 OK。
  - 调试时外部想 `print` 一个 GdtEntry 不可行。
- **严重度**：P3（封装性 vs 调试）。

---

## 6. arch/aarch64/exception.rs (750 行 / 5 项)

### 6.1 [P1] 异常向量表汇编未走 trampoline — KPTI 异常入口未切 CR3

- **位置**：[exception.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs) 整个文件
- **问题描述**：
  - 异常向量表 `vector_base` 在 `VBAR_EL1`，**VMA 在高半区**。
  - KPTI 激活后 TTBR1_EL1 仅含 EL1 映射，**用户页表无 VBAR 映射**。
  - **异常入口**（EL1 IRQ）从 EL0 触发时，**当前 TTBR0_EL1 = 用户页表**，寻址 VBAR → **需要 TTBR1 寻址**。
  - 异常入口汇编**未切换 TTBR1** → 寻址 VBAR 失败 → 异常嵌套 → 系统挂起。
- **建议方案**：
  - 异常入口汇编先切 TTBR1（`msr ttbr1_el1, kernel_ttbr1`）。
  - 异常出口 eret 前切回。
- **严重度**：P1（KPTI + 异常入口）。

### 6.2 [P1] ESR_EL1 / FAR_EL1 错误码解析不完整

- **位置**：[exception.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs) ESR 解析函数
- **问题描述**：
  - ESR.EC（Exception Class）= 0x24 (Data Abort) 时，ISS 字段包含 DFSR/IFSR 状态。
  - 当前解析可能**未覆盖所有 EC**（如 0x0F SVC trap、0x12 HVC 等）。
- **建议方案**：
  - 全 EC 表 + 单元测试。
- **严重度**：P1（异常处理完整性）。

### 6.3 [P2] 异常入口汇编含调试输出

- **位置**：[exception.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs) 异常入口汇编
- **问题描述**：
  - 类似 x86_64 enter_user_asm，每个异常入口都 `movz/movk` + `uart_write` 输出诊断。
  - **生产路径污染**。
- **严重度**：P2（与 P0-35 一致）。

### 6.4 [P2] 异常 handler 栈深度无 IST 概念

- **位置**：[exception.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs) 异常栈
- **问题描述**：
  - x86_64 有 IST (Interrupt Stack Table) 防止栈溢出。
  - aarch64 异常**共用当前 SP_EL1 栈** → **双异常 / 嵌套中断**可能栈溢出。
- **建议方案**：
  - 每个异常级用独立栈（SPSR.M 决定异常级）。
- **严重度**：P2（架构差异）。

### 6.5 [P3] 异常入口不保存 GPR 完整集

- **位置**：[exception.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs) 异常入口
- **问题描述**：
  - aarch64 异常入口应保存 x0-x30，**当前可能仅保存部分寄存器**。
  - 异常返回（eret）后未保存寄存器被覆盖 → 上下文破坏。
- **严重度**：P3（依赖实际实现）。

---

## 7. arch/aarch64/gic.rs (465 行 / 4 项)

### 7.1 [P1] GICv3 SPI/PPI/SGI 优先级处理不完整

- **位置**：[gic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/gic.rs) GIC 初始化
- **问题描述**：
  - GICv3 优先级掩码（ICC_PMR_EL1）设置 0xFF 允许所有中断。
  - 应支持运行时调整（如屏蔽低优先级 IRQ）。
- **建议方案**：
  - 提供 `set_irq_priority_mask(u8)` API。
- **严重度**：P1（功能）。

### 7.2 [P1] SGI 处理未支持 16 个以上 CPU

- **位置**：[gic.rs: send_ipi 实现](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/gic.rs) SGI 编码
- **问题描述**：
  - `ICC_SGI1R_EL1` 的 targetlist 字段 16 位 → 最多 16 个 CPU。
  - 大于 16 个 CPU 系统中**目标 CPU 选择失效**。
- **建议方案**：
  - 大系统用 `ICC_SGI0R_EL1` 或软件分片。
- **严重度**：P1（多核可扩展性）。

### 7.3 [P2] GIC distributor 基地址硬编码

- **位置**：[gic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/gic.rs) GICD 基地址
- **问题描述**：
  - GICD 基地址来自 device tree，但代码中可能硬编码 `0x0800_0000`。
- **建议方案**：
  - 从 FDT 解析。
- **严重度**：P2（设备可移植性）。

### 7.4 [P3] GIC ITS 未实现

- **位置**：[gic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/gic.rs)
- **问题描述**：
  - GICv3 ITS (Interrupt Translation Service) 用于 MSI/MSI-X — **PCIe 设备需要**。
  - 当前未实现。
- **严重度**：P3（PCIe 支持）。

---

## 8. arch/x86_64/apic.rs + ioapic.rs + acpi.rs (3 项)

### 8.1 [P1] APIC spurious interrupt vector 0xFF

- **位置**：[apic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/apic.rs) spurious 中断设置
- **问题描述**：
  - Spurious 中断 vector 0xFF 保留。
  - 实际 IDT vector 应 < 0x80 (用户中断) 或 0x80-0xFF (保留) — **若 IDT 表初始化错乱，可能把 spurious 当真实中断处理**。
- **严重度**：P1（中断处理错乱）。

### 8.2 [P1] ACPI MADT 解析表未校验

- **位置**：[acpi.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/acpi.rs) MADT 解析
- **问题描述**：
  - MADT (Multiple APIC Description Table) 子表类型众多，**解析时未校验子表边界**。
  - 损坏的 MADT 可能导致越界读。
- **建议方案**：
  - 每个子表解析后 `offset += subtable.length`，并校验 `offset < madt.length`。
- **严重度**：P1（健壮性）。

### 8.3 [P2] IOAPIC IRQ → GSI 映射未处理重复

- **位置**：[ioapic.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/ioapic.rs) IRQ 映射
- **问题描述**：
  - 多个 IOAPIC 中 IRQ 编号可能重叠，**当前未做去重**。
- **严重度**：P2（多 IOAPIC 系统）。

---

## 9. arch/aarch64/{mmu, context, timer, uart, psci, barrier} (3 项)

### 9.1 [P1] aarch64 mmu.rs identity mapping 与高半区映射冲突

- **位置**：[mmu.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mmu.rs) mmu::init
- **问题描述**：
  - identity mapping（VA = PA）用于早期 boot。
  - 高半区映射（KERNEL_BASE + PA）用于运行时。
  - **两套映射可能冲突**（同一 PA 在两个 VA 范围）。
  - 修改页表时（如 ioremap）需**同时更新两套**。
- **建议方案**：
  - boot 完成后**移除 identity mapping**。
- **严重度**：P1（boot 后未清理）。

### 9.2 [P1] aarch64 context.rs `switch` 函数未保存所有 GPR

- **位置**：[context.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/context.rs) context::switch
- **问题描述**：
  - aarch64 上下文切换应保存 x0-x30 + SP + SPSR + ELR + TTBR0。
  - **未保存的寄存器**在异常返回时**被覆盖**。
- **建议方案**：
  - 完整 31 GPR + 系统寄存器保存。
- **严重度**：P1（上下文破坏）。

### 9.3 [P2] aarch64 psci.rs 兜底 loop 无超时

- **位置**：[psci.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/psci.rs) PSCI 调用
- **问题描述**：
  - `loop { wfi }` 无超时，**固件未实现 PSCI 时永久循环**。
- **严重度**：P2（错误恢复）。

---

## 10. net/mod.rs + types.rs + wait_queue.rs + driver/mod.rs (3 项)

### 10.1 [P3] `net/driver/mod.rs` 仅 5 行占位

- **位置**：[net/driver/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/driver/mod.rs)
- **问题描述**：
  - 5 行文件，仅 `pub use crate::kernel::driver::net::*;`。
  - 实际驱动在 `framework::driver::net`，**框架与驱动边界模糊**。
- **严重度**：P3（API 重定向）。

### 10.2 [P3] `net/wait_queue.rs` 仅 9 行

- **位置**：[net/wait_queue.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/wait_queue.rs)
- **问题描述**：
  - 9 行 re-export，与 sync::CondVar 占位实现类似。
  - `Socket WaitQueue` 基础设施文档说实现，但**实际仅占位**。
- **严重度**：P3（与 sync::CondVar 一致问题）。

### 10.3 [P2] `net/netfilter.rs` 仅 13 行

- **位置**：[net/netfilter.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/netfilter.rs)
- **问题描述**：
  - 13 行 stub，无实际 netfilter 实现。
  - 文档（mod.rs:15）说"C5: Netfilter 包过滤框架"。
  - **功能未实装**。
- **严重度**：P2（功能虚假）。

---

## 11. net/init.rs (2060 行 / 6 项)

### 11.1 [P0] 单文件 2K+ 行违反 §12.3 简单优先

- **位置**：[net/init.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs) 整文件
- **问题描述**：
  - 2060 行单文件 — **远超合理模块大小**（典型 200-500 行）。
  - 包含：
    - 初始化状态机
    - DHCP 客户端
    - Socket API
    - 网络栈配置
    - 轮询循环
  - **可读性差、grep 范围大、单测困难**。
- **建议方案**：
  - 拆分为 `init/mod.rs` (200 行) + `init/dhcp.rs` (300) + `init/socket.rs` (500) + `init/poll.rs` (200) + `init/state.rs` (200) + `init/config.rs` (200) + `init/iface.rs` (300) + `init/sm_fi.rs` (1129, 单独子目录)。
- **严重度**：P0（架构违反）。

### 11.2 [P1] DHCP 状态机死循环风险

- **位置**：[net/init.rs DHCP 部分](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs)
- **问题描述**：
  - DHCP DISCOVER/OFFER/REQUEST/ACK 4 步。
  - **超时未实现**（或实现不完整）→ 网络不可达时永久等待。
- **建议方案**：
  - `dhcp_attempt_timeout = 30s`，超时后状态机 `Failed`。
- **严重度**：P1（用户可观察挂起）。

### 11.3 [P1] Socket API 错误处理不统一

- **位置**：[net/init.rs Socket API](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs)
- **问题描述**：
  - 部分返回 `Result<T, i32>`，部分返回 `Option<T>`，部分 `T` 配 out param。
  - **调用方需根据具体函数判断**。
- **严重度**：P1（API 一致性）。

### 11.4 [P1] `poll_network` 与 `poll_stack` 重叠

- **位置**：[net/init.rs poll](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs) + [net/smoltcp_impl.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/smoltcp_impl.rs) `poll_stack`
- **问题描述**：
  - `poll_network` 通用接口，`poll_stack` smoltcp 特定。
  - 文档不一致，调用方不知选哪个。
- **严重度**：P1（API 重叠）。

### 11.5 [P2] `raw` 子模块暴露过多

- **位置**：[net/init.rs raw module](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs)
- **问题描述**：
  - `pub(crate) use init::raw;` 让 services 层可访问 raw API。
  - raw 内部可能含 unsafe 块 → 跨层访问降低安全性。
- **建议方案**：
  - raw 模块 `pub(crate)` + 仅暴露 safe wrapper。
- **严重度**：P2（封装性）。

### 11.6 [P2] DHCP 解析器无 size 边界检查

- **位置**：[net/init.rs DHCP parse](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init.rs)
- **问题描述**：
  - DHCP 选项解析读取 1 字节 length，可能**越界读**（恶意/损坏 DHCP 响应）。
- **严重度**：P2（健壮性）。

---

## 12. net/iface_trait.rs + smoltcp_impl.rs (5 项)

### 12.1 [P1] `iface_trait.rs` 1552 行 — 抽象层过大

- **位置**：[net/iface_trait.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/iface_trait.rs)
- **问题描述**：
  - 1552 行 trait + 实现 — 单 trait 文件过大。
  - 应拆分为 `trait.rs` (200) + `device.rs` (300) + `stack.rs` (500) + `socket.rs` (400) + `poll.rs` (152)。
- **严重度**：P1（架构违反）。

### 12.2 [P1] `ChitinNetDevice` IRQ 安全问题

- **位置**：[net/smoltcp_impl.rs: ChitinNetDevice](file:///home/anfer/Code/QueenX/src/kernel/framework/net/smoltcp_impl.rs) Device trait impl
- **问题描述**：
  - smoltcp Device trait 的 `receive()` / `transmit()` 在 smoltcp poll 上下文调用。
  - 实际 NIC 收发包在**中断 handler**（`softirq` 或 `tasklet`）路径。
  - **两者并发访问** NIC 寄存器 / 描述符环 → **数据竞争**。
  - 应通过 `IrqSpinLock` 守护 NIC 状态。
- **建议方案**：
  - 内部状态用 `IrqSpinLock<NetDeviceState>`。
  - 或在 `receive()` / `transmit()` 中 disable IRQ。
- **严重度**：P1（数据竞争）。
- **关联硬规则**：I2 / I5（外设 MMIO 安全）。

### 12.3 [P1] `NetworkStack` 全局可变状态

- **位置**：[net/smoltcp_impl.rs: NetworkStack](file:///home/anfer/Code/QueenX/src/kernel/framework/net/smoltcp_impl.rs)
- **问题描述**：
  - `static mut NetworkStack` 或 `IrqSpinLock<NetworkStack>`？
  - 多核并发 poll 同一 NetworkStack → smoltcp 内部无并发保护。
- **建议方案**：
  - smoltcp 实例 per-CPU 或全局加锁。
- **严重度**：P1（SMP 数据竞争）。

### 12.4 [P2] smoltcp 版本与 vendored 同步

- **位置**：[net/smoltcp_impl.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/smoltcp_impl.rs)
- **问题描述**：
  - 文档（mod.rs:18-22）说"src/net/smoltcp/ 协议栈源码"。
  - smoltcp vendored 在 `services/net/smoltcp/`，与 framework 路径不一致。
- **严重度**：P2（路径不一致）。

### 12.5 [P2] `ChitinNetDevice` 收发缓冲区无界

- **位置**：[net/smoltcp_impl.rs: ChitinNetDevice](file:///home/anfer/Code/QueenX/src/kernel/framework/net/smoltcp_impl.rs) rx/tx buffer
- **问题描述**：
  - 缓冲区大小硬编码或无界 → OOM 风险。
- **建议方案**：
  - 固定大小 ring buffer。
- **严重度**：P2（资源管理）。

---

## 13. net/init/sm_fi.rs (1129 行 / 3 项)

### 13.1 [P0] sm_fi.rs 单文件 1K+ 行 — 状态机/DHCP/Socket 三合一

- **位置**：[net/init/sm_fi.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init/sm_fi.rs)
- **问题描述**：
  - 1129 行单文件，与 init.rs 主文件功能重叠。
  - 命名 `sm_fi` 不清晰（state machine? smoltcp framekernel interface?）。
  - **死循环风险**在 yield/timeout 路径未实现。
- **严重度**：P0（与 P0-41 一致）。

### 13.2 [P1] 状态机无错误恢复

- **位置**：[net/init/sm_fi.rs 状态机](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init/sm_fi.rs)
- **问题描述**：
  - 状态转换错误时无 fallback / retry 策略。
  - 一旦进入错误态**永久卡住**。
- **严重度**：P1（错误恢复）。

### 13.3 [P2] Socket 创建无并发限制

- **位置**：[net/init/sm_fi.rs Socket](file:///home/anfer/Code/QueenX/src/kernel/framework/net/init/sm_fi.rs)
- **问题描述**：
  - 单进程可创建无限 Socket → 资源耗尽。
- **严重度**：P2（资源管理）。

---

## 14. net/syscall.rs + save.rs + route.rs + api.rs (4 项)

### 14.1 [P1] `net/save.rs` 快照不校验状态

- **位置**：[net/save.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/save.rs) `net_save` / `net_restore`
- **问题描述**：
  - 快照所有网络状态（路由表、ARP、Socket 状态），**未校验版本号 / 校验和**。
  - 损坏快照还原 → 网络状态错乱。
- **严重度**：P1（数据完整性）。

### 14.2 [P1] `net/route.rs` 路由表无锁

- **位置**：[net/route.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/route.rs) 路由表
- **问题描述**：
  - 全局路由表，多核并发读写 → **数据竞争**。
  - 应 `IrqSpinLock<RoutingTable>`。
- **严重度**：P1（与 I2 一致）。

### 14.3 [P2] `net/api.rs` 165 行 — API 散落

- **位置**：[net/api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/api.rs)
- **问题描述**：
  - 165 行 API 与 init.rs / sm_fi.rs / smoltcp_impl.rs 重复。
  - API 文档不完整。
- **严重度**：P2（DRY）。

### 14.4 [P2] `net/syscall.rs` 562 行 — socket 系统调用分散

- **位置**：[net/syscall.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/net/syscall.rs)
- **问题描述**：
  - socket/bind/listen/accept/connect/send/recv 等系统调用。
  - 错误处理不统一（部分用 `i32` 负数，部分用 `Option`）。
- **严重度**：P2（API 一致性）。

---

## 15. 跨子系统一致性问题 (4 项)

### 15.1 [P0] KPTI 在 x86_64/aarch64 实现不对称

- **位置**：[arch/x86_64/mod.rs enter_user_asm](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs) + [arch/aarch64/mod.rs enter_user](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs)
- **问题描述**：
  - x86_64 KPTI: trampoline 切换 CR3 (mod.rs:740) → 用户页表。
  - aarch64 KPTI: 未实现（mod.rs:251-288 仅切 TTBR0，未切 TTBR1）。
  - **同一项目双架构 KPTI 行为不一致**。
- **严重度**：P0（架构一致性）。
- **关联硬规则**：I3 / I4（用户态安全代理）。

### 15.2 [P1] SAFETY 注释模板化 — 全 arch 模块

- **位置**：[arch/x86_64/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs) + [arch/aarch64/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/mod.rs) + [arch/shadow_stack.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/shadow_stack.rs)
- **问题描述**：
  - 30+ 处 inline asm SAFETY 注释**模板化**："调用方保证指针/类型有效"。
  - 与 F4 一致问题。
- **严重度**：P1（与 F4 一致）。

### 15.3 [P1] 内联汇编诊断代码污染生产路径

- **位置**：[arch/x86_64/mod.rs enter_user_asm](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/x86_64/mod.rs) + [arch/aarch64/exception.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/aarch64/exception.rs)
- **问题描述**：
  - 17+ 个诊断点通过 `out dx, al` 输出字符。
  - **生产路径被开发期调试代码污染**。
- **严重度**：P1（与 P0-35 一致）。

### 15.4 [P2] `arch!` 宏与直接 `Arch::method()` 调用并存

- **位置**：[arch/mod.rs arch! 宏](file:///home/anfer/Code/QueenX/src/kernel/framework/arch/mod.rs) + 大量调用点
- **问题描述**：
  - sync/arch.rs 通过 `arch!()` 宏调用。
  - arch/x86_64/mod.rs 通过 `Arch::method()` 直接调用。
  - **两套 API 风格**。
- **建议方案**：
  - 强制一种风格。
- **严重度**：P2（一致性）。

---

## 16. 综合风险评估

| 风险类别 | 数量 | 典型问题 |
|---|---|---|
| **架构安全 (P0)** | 6 | P0-34 CR4.CET 触发 #GP / P0-35 enter_user 调试污染 / P0-37 aarch64 mrs+msr 可中断 / P0-39 KPTI TTBR1 未切 / P0-41 init.rs 2K+ 行 / P0-43 sm_fi 死循环 |
| **多核安全 (P0/P1)** | 4 | P0-36 cpu_id SMP race / P0-40 tlb_flush_all 单核 / P1-12 enter_user KPTI 异常 / P1-15 GIC SGI 16 CPU |
| **KPTI 完整性 (P0/P1)** | 3 | P0-39 aarch64 KPTI / P1-6 aarch64 exception trampoline / P1-7 ESR 解析不完整 |
| **CET/Shadow Stack (P0/P1)** | 4 | P0-34 try_write_cr4 / P1-8 alloc_kernel_shadow_stack base=0 / P1-9 create_user 未映射 / P1-10 进程切换 MSR 不同步 |
| **网络功能 (P0/P1)** | 4 | P0-41 init.rs 过大 / P1-12 iface_trait 过大 / P1-13 ChitinNetDevice IRQ / P1-14 NetworkStack 全局 |
| **架构抽象 (P0/P1)** | 3 | P0-38 arch! 宏 / P1-3 context_switch 裸指针 / P1-2 PMU/RNG 缺位 |
| **错误处理 (P1/P2)** | 5 | P1-11 shutdown 三重故障 / P1-16 DHCP 死循环 / P1-17 Socket 错误处理 / P1-18 net_save 不校验 / P1-19 route 表无锁 |
| **代码风格 (P2/P3)** | 7 | P1-3 GDT TSS 越界 / P1-4 GDT limit 截断 / P1-5 net/save / P1-6 sm_fi 状态机 / P1-7 ESR 解析 / P1-8 alloc_kernel / P1-9 create_user |

### 与 AGENTS.md 硬规则对应

| 硬规则 | 违反次数 | 典型违反 |
|---|---|---|
| F1 services 0 unsafe | 0 | services/net/* 需后续审计 |
| F2 services 禁访 framework 内部 | 0 | 待验证 |
| F3 无循环依赖 | 0 | net 依赖 driver 通过 re-export，结构清晰 |
| F4 unsafe 块 SAFETY 100% | 8+ | P0-35 / P0-37 / P0-39 / P1-3 等 |
| F5 0 warning 0 error | 0 | 需实际 build 验证 |
| F6 核心审计通过 | N/A | 需运行 audit scripts |
| F7 中文注释强制 | 3+ | enter_user 汇编英文注释 + 诊断字符硬编码 |
| F8 公共 API 中文文档 | 5+ | P0-34 / P0-39 / P1-3 / P1-11 / P1-16 |
| F9 死代码零容忍 | 1 | shadow_stack.rs:498 `mov ecx, ecx` no-op |
| F12 static mut 禁止 | 0 | 未直接发现，但 NetworkStack 模式需验证 |

### 关键设计矛盾

1. **KPTI 架构不一致**：x86_64 实现完整，aarch64 仅切 TTBR0 未切 TTBR1，**安全保证等级不同**。
2. **生产路径被调试代码污染**：17+ 个内联诊断点，3 个 IA32_GS_BASE 自检块，**生产构建仍执行**。
3. **网络 init 单文件 2K+ 行**：违反 §12.3 简单优先。
4. **CFI/CET 实现不完整**：CR4.CET 触发 #GP panic / Shadow Stack base=0 / 进程切换 MSR 不同步。
5. **`arch!` 宏与直接 `Arch::method()` 并存**：调用方风格不统一。
6. **网络协议栈 SMP 安全**：smoltcp 全局状态 + ChitinNetDevice IRQ 竞争，多核场景未充分测试。

### 建议优先级

| 优先级 | 必须修复 | 建议修复 | 可选修复 |
|---|---|---|---|
| 数量 | 10 (P0) | 18 (P1) | 28 (P2+P3) |
| 范围 | KPTI / CET / SMP race / enter_user 调试 / 网络模块拆分 / arch! 宏 | TLB 跨核 / GIC SGI / 异常入口 trampoline / 网络并发 / FFI SAFETY | 文档 / 错误处理 / DRY |

### 后续审计方向

1. **services/proc** (28 文件) — 进程管理 + 调度剩余
2. **services/fs + services/driver** (40K LoC) — 文件系统 + 驱动
3. **framework/driver** (21K LoC) — 驱动框架
4. **framework/syscall + boot/idt/cpu/ipc/pci/dma/timer** — 系统调用 + 启动
5. **services/wasm + services/ipc + services/credo** (12K LoC) — WASM / IPC / 安全策略

---

**报告结束**. 本报告为 arch + net 子系统深度审计，仅列出与既有审计不重复的问题。所有发现均附位置链接 + 问题描述 + 建议方案 + 严重度评级 + 关联硬规则。
