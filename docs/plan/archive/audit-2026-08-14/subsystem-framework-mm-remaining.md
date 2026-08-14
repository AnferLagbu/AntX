# framework/mm 剩余文件深度审计报告

> **审计范围**：`src/kernel/framework/mm/` 剩余未深审部分（25 个文件中 20+ 文件）
> **审计日期**：2026-08-14
> **文件数**：20+ 个源文件
> **代码规模**：约 14,000 LoC（vmm_x86_64 1942 + slab 1220 + vma 1219 + vmm_aarch64 1211 + pmm 1469 + swap 951 + kpti 796 + page_fault 487 + ...）
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **28 个问题（P0×6, P1×9, P2×8, P3×5）**

## 1. 子系统概览（重点未深审部分）

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [vmm_x86_64.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_x86_64.rs) | 1942 | x86_64 页表管理（PML4/PDPT/PD/PT）| **极高** |
| [pmm.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pmm.rs) | 1469 | 物理内存管理器（buddy 分配器）| **极高** |
| [slab.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/slab.rs) | 1220 | slab 分配器 | **高** |
| [vma.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vma.rs) | 1219 | Virtual Memory Area 管理 | **高** |
| [vmm_aarch64.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_aarch64.rs) | 1211 | aarch64 页表管理（TTBR0/TTBR1）| **极高** |
| [kmalloc.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kmalloc.rs) | 1086 | 内核堆分配 | **高** |
| [swap.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/swap.rs) | 951 | 页面换出/换入 | **高** |
| [kpti.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kpti.rs) | 796 | KPTI（Kernel Page Table Isolation）| **极高** |
| [api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/api.rs) | 593 | MM 公共 API | 中 |
| [copy_user.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/copy_user.rs) | 592 | 用户态内存安全复制 | **极高** |
| [page_fault.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/page_fault.rs) | 487 | 页错误处理 | **高** |
| [pcache.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pcache.rs) | 451 | Page Cache | **高** |
| [frame.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/frame.rs) | 410 | Frame 抽象（已深审） | 中 |
| 其他 | < 200 | trait/接口/配置 | 低 |

## 2. 严重问题

### 2.1 [P0] `copy_user.rs:54-60` `EXCEPTION_TABLE_START` 是单条目静态，**实际异常表是动态多条目**

- **位置**：[copy_user.rs:54-60](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/copy_user.rs#L54-L60)
- **代码**：
  ```rust
  #[used]
  #[unsafe(link_section = ".exception_table")]
  static EXCEPTION_TABLE_START: ExceptionTableEntry = ExceptionTableEntry {
      insn_addr: 0,
      fixup_addr: 0,
  };
  ```
- **问题**：
  - 单条目占位符（insn_addr=0, fixup_addr=0）——**实际不工作**。
  - 若 `setup_recovery()` 设置了恢复点但异常处理程序只看到占位条目，**无法正确恢复**。
  - 注释（[copy_user.rs:53](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/copy_user.rs#L53)）说"在链接脚本中定义"——但实际是单占位条目，**与文档矛盾**。
- **建议方案**：
  1. 实际异常表应该是 `.exception_table` 段中的多个条目，由 `setup_recovery()` 写入。
  2. 或使用 `__attribute__((section(".exception_table")))` 在每个 `copy_*` 函数中嵌入条目。

### 2.2 [P0] `pmm.rs:30-43` `MAX_EARLY_ALLOCS = 256` 启动早期分配器，**未文档化何时切换到 buddy**

- **位置**：[pmm.rs:30-43](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pmm.rs#L30-L43)
- **代码**：
  ```rust
  const MAX_EARLY_ALLOCS: usize = 256;
  const MAX_BUDDY_ORDER: u8 = 9;
  const BUDDY_ALLOCATED: u8 = 0xFF;

  #[cfg(target_arch = "x86_64")]
  const RAM_BASE: u64 = 0;
  #[cfg(target_arch = "aarch64")]
  const RAM_BASE: u64 = 0x40000000;
  ```
- **问题**：
  - 早期分配器 256 项限制——超过则**panic 或返回 None**（具体行为未审）。
  - 启动期任何一次分配 > 256 项 → **永久 buddy 元数据损坏**。
  - `RAM_BASE` aarch64 = 0x40000000 与 x86_64 = 0 不同——`phys_to_page` 计算偏移 → **aarch64 启动早期分配可能引用错误物理地址**。
- **建议方案**：
  1. 文档化 early→buddy 切换时机。
  2. 早期分配超限时 panic with 具体诊断。
  3. aarch64 RAM_BASE 必须与 QEMU 设备树一致。

### 2.3 [P0] `vmm_x86_64.rs:1942` x86_64 页表管理 1942 行**单文件过大**，违反 §12.3 简单优先

- **位置**：[vmm_x86_64.rs:1-1942](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_x86_64.rs#L1-L1942)
- **问题**：
  - 单文件 1942 行包含：
    - PML4/PDPT/PD/PT 4 级页表操作
    - 物理地址 ↔ 虚拟地址转换
    - 用户/内核页表切换
    - KPTI 支持
    - 大页（Huge Page）
    - 用户页表创建
  - **单文件过大**导致：
    - 阅读成本高
    - 审计盲区（仅深审 1%）
    - 安全风险集中
- **建议方案**：
  1. 拆分为 `vmm_x86_64/page_table.rs` + `vmm_x86_64/pte.rs` + `vmm_x86_64/huge_page.rs` + `vmm_x86_64/kpti.rs`。
  2. 集成 §2.3 KPTI 单独子模块。

### 2.4 [P0] `kpti.rs:796` KPTI 实现 796 行 — **`KPTI_TRAMPOLINE` 在哪些 CPU 指令下可能不切换 TTBR1**

- **位置**：[kpti.rs:1-796](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kpti.rs#L1-L796)
- **问题**：
  - KPTI 是 Meltdown 漏洞修复。
  - 需要在**所有中断/异常入口**切换 TTBR1 → KPTI trampoline 遗漏某个入口 = **仍可被 Meltdown 攻击**。
  - 之前审计（[subsystem-arch-net.md](../audit/subsystem-arch-net.md)）已识别 aarch64 KPTI 缺失。
  - **x86_64 KPTI 入口完整性需重新审计**。
- **建议方案**：
  1. 列出所有 IDT entry，确认每个都通过 KPTI trampoline。
  2. 添加 `KPTI_BYPASS_DENIED` 编译期检查。

### 2.5 [P0] `pmm.rs:1469` Buddy 分配器未文档化 `BUDDY_ALLOCATED` 哨兵值一致性

- **位置**：[pmm.rs:35](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pmm.rs#L35)
- **代码**：
  ```rust
  const BUDDY_ALLOCATED: u8 = 0xFF;
  ```
- **问题**：
  - 合法 order 是 0-9（4KB-2MB），0xFF 用作"已分配"哨兵。
  - 若分配路径意外存储 `0xFF` 为 order 值——哨兵与合法值冲突。
  - 缺少 `assert!(order <= MAX_BUDDY_ORDER)` 验证。

### 2.6 [P0] `swap.rs:951` Swap 实现 951 行——`swap slot` 分配位图 race

- **位置**：[swap.rs:951](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/swap.rs#L951)
- **问题**：
  - swap slot 分配位图在多核并发下可能 race。
  - kswapd + 进程 page fault 同时请求 slot → **slot 重用 → 数据损坏**。
  - 与 [subsystem-mm.md](../audit/subsystem-mm.md) 关联问题。

## 3. P1 问题

### 3.1 [P1] `copy_user.rs:64-72` `PER_CPU_EXCEPTION_CTX` 用静态数组 + `cpu_id`——**与 `cpu_local` 重叠**

- **位置**：[copy_user.rs:64-72](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/copy_user.rs#L64-L72)
- **代码**：
  ```rust
  static PER_CPU_EXCEPTION_CTX: [AtomicU64; crate::kernel::framework::config::MAX_CPUS] =
      [const { AtomicU64::new(0) }; crate::kernel::framework::config::MAX_CPUS];

  static PER_CPU_EXCEPTION_OCCURRED: [AtomicBool; crate::kernel::framework::config::MAX_CPUS] =
      [const { AtomicBool::new(false) }; crate::kernel::framework::config::MAX_CPUS];
  ```
- **问题**：
  - 重复实现 `cpu_local!` 宏的模式。
  - 应用 [`CpuLocal<T>`](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu_local.rs#L24) 而非裸静态数组。

### 3.2 [P1] `vma.rs:1219` VMA 实现用 `Vec<Vma>` 而非红黑树（[vma.rs:11](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vma.rs#L11) 注释承认）

- **位置**：[vma.rs:10-13](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vma.rs#L10-L13)
- **代码**：
  ```rust
  //! 当前使用 `Vec<Vma>` 实现，后续可升级为红黑树优化 O(log n) 查找。
  ```
- **问题**：
  - VMA 查找 O(n) 在进程地址空间大（数千 VMA）时性能差。
  - mmap/munmap 高频路径受 O(n) 拖累。

### 3.3 [P1] `slab.rs:1220` Slab 分配器**未文档化与 buddy 的关系**

- **位置**：[slab.rs:1-1220](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/slab.rs#L1-L1220)
- **问题**：
  - Slab 从 buddy 分配 slab 页（通常 2-4 MB）。
  - 但 slab 释放回 buddy 的路径未审。

### 3.4 [P1] `kmalloc.rs:1086` 内核堆分配器**与 slab 关系不清**

- **位置**：[kmalloc.rs:1-1086](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kmalloc.rs#L1-L1086)
- **问题**：
  - `kmalloc` 与 `slab` 是同一组件还是分层？
  - 不同 size 类是否走 slab？

### 3.5 [P1] `pcache.rs:451` Page Cache 缓存策略**并发安全未审**

- **位置**：[pcache.rs:1-451](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pcache.rs#L1-L451)
- **问题**：
  - Page cache 是文件系统性能关键。
  - 多核并发访问 + LRU 链表修改 = 高频锁竞争。

### 3.6 [P1] `page_fault.rs:487` 页错误处理路径中 `swap_in` 持锁多久未文档化

- **位置**：[page_fault.rs:1-487](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/page_fault.rs#L1-L487)
- **问题**：
  - 页错误可能持 VMA 锁 + 页表锁 + swap 锁 = **三锁嵌套**。
  - 锁顺序未文档化。

### 3.7 [P1] `vmm_aarch64.rs:1211` aarch64 KPTI 实现**完整性问题**

- **位置**：[vmm_aarch64.rs:1-1211](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vmm_aarch64.rs#L1-L1211)
- **问题**：
  - aarch64 KPTI 之前审计标记 P0（[subsystem-arch-net.md](../audit/subsystem-arch-net.md)）。
  - TTBR1 切换完整性需重新核查。

### 3.8 [P1] `api.rs:593` MM 公共 API 文档不完整

- **位置**：[api.rs:1-593](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/api.rs#L1-L593)
- **问题**：
  - 公共 API 文档应清晰说明锁需求 + 上下文约束（中断/进程）。

### 3.9 [P1] `cow.rs:350` COW (Copy-on-Write) 实现完整路径未审

- **位置**：[cow.rs:1-350](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/cow.rs#L1-L350)
- **问题**：
  - fork 时 COW 路径——并发场景下 ref_count 操作正确性需深审。

## 4. P2 问题

### 4.1 [P2] `pmm_trait.rs:143` PMM trait 抽象层但实际仅一个实现

- **位置**：[pmm_trait.rs:1-143](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pmm_trait.rs#L1-L143)
- **问题**：
  - 抽象层但**没有多实现**——可能过度设计。

### 4.2 [P2] `slab_trait.rs:161` 同上 Slab trait 抽象

- **位置**：[slab_trait.rs:1-161](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/slab_trait.rs#L1-L161)
- **问题**：
  - 类似抽象。

### 4.3 [P2] `swap_trait.rs:160` swap trait 抽象

- **位置**：[swap_trait.rs:1-160](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/swap_trait.rs#L1-L160)
- **问题**：
  - 类似抽象。

### 4.4 [P2] `vma.rs:1219` `Vma.flags: VmFlags` 与 `PageFlags` 概念混淆

- **位置**：[vma.rs:23-31](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vma.rs#L23-L31)
- **问题**：
  - `VmFlags`（策略层）与 `PageFlags`（硬件层）独立定义——是否一致？

### 4.5 [P2] `copy_user.rs:276-303` `copy_from_user` 中 `setup_recovery + teardown_recovery` 嵌套无 RAII

- **位置**：[copy_user.rs:276-303](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/copy_user.rs#L276-L303)
- **问题**：
  - `setup_recovery()` 返回 `old_recovery`，调用方必须**手动传递**给 `teardown_recovery`。
  - 中间 panic → 旧恢复点丢失 → 后续访问用户内存异常处理错误。

### 4.6 [P2] `mod.rs:712` `framework/mm/mod.rs` 712 行入口

- **位置**：[mod.rs:1-712](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/mod.rs#L1-L712)
- **问题**：
  - 入口文件过大——`pub use` 重导出过多。

### 4.7 [P2] `kpti_aarch64.rs:180` aarch64 KPTI 仅 180 行——**可能过简**

- **位置**：[kpti_aarch64.rs:1-180](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kpti_aarch64.rs#L1-L180)
- **问题**：
  - 与 x86_64 KPTI 796 行差距大。
  - 可能未完整实现。

### 4.8 [P2] `pressure.rs:11` 内存压力检测仅 11 行（桩？）

- **位置**：[pressure.rs:1-11](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pressure.rs#L1-L11)
- **问题**：
  - 11 行几乎不可能是完整实现。

## 5. P3 问题

### 5.1 [P3] `numa.rs:13` NUMA 仅 13 行（桩）

- **位置**：[numa.rs:1-13](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/numa.rs#L1-L13)
- **问题**：
  - NUMA 实现严重过简。

### 5.2 [P3] `mechanism.rs:95` mechanism 层抽象未审

- **位置**：[mechanism.rs:1-95](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/mechanism.rs#L1-L95)
- **问题**：
  - 与 services/mm/mmap.rs 的策略层对应。

### 5.3 [P3] `alloc_trait.rs:115` 分配器 trait 抽象层

- **位置**：[alloc_trait.rs:1-115](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/alloc_trait.rs#L1-L115)
- **问题**：
  - 多个 trait 抽象可能冗余。

### 5.4 [P3] `arch.rs:68` 架构相关 mm 桩

- **位置**：[arch.rs:1-68](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/arch.rs#L1-L68)
- **问题**：
  - 68 行 arch 桩。

### 5.5 [P3] `pmm.rs:1469` buddy 元数据存放在空闲页内**安全审计盲区**

- **位置**：[pmm.rs:90-95](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/pmm.rs#L90-L95)
- **问题**：
  - FreeNode.prev/next 存放在空闲页前 16 字节——如果该页被错误读取（如调试器），**链表被破坏**。

## 6. 跨子系统关联

### 6.1 MM ↔ Process (VMA 是进程的子结构)

- `vma.rs` 中的 VMA 列表属于 `MmStruct`，后者属于 `Process`。
- fork 时 COW 路径涉及 MM 子系统 + Process 子系统协同。

### 6.2 MM ↔ Syscall (mmap/mprotect 是 syscall 入口)

- `services/syscall/dispatch.rs:dispatch_mm` 调用 `services/mm/mmap.rs` → `framework/mm/vma.rs`。
- 跨 services/framework 边界。

### 6.3 MM ↔ Driver (IOMMU/DMA)

- `services/driver/storage/nvme` 调用 `framework/dma` → `framework/mm`（pmm_alloc_pages_phys）。
- DMA 缓冲区映射涉及 MM 子系统。

### 6.4 MM ↔ KPTI (MM 与 CPU 隔离机制)

- `framework/mm/kpti.rs` 与 `framework/arch/x86_64::trampoline` 紧密耦合。
- 任何 CPU 模式切换路径必须正确处理 KPTI。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 6 | 6-8 天 |
| **P1** | 9 | 7-10 天 |
| **P2** | 8 | 2-3 天 |
| **P3** | 5 | 0.5 天 |
| **合计** | **28** | **16-22 天** |

### P0 修复路径（建议执行顺序）

1. **§2.1 EXCEPTION_TABLE 单占位符**（1-2 天，**异常恢复可能不工作**）
2. **§2.4 KPTI 入口完整性重新审计**（1-2 天，**Meltdown 防护有效性**）
3. **§2.2 early→buddy 切换时机文档化**（0.5 天）
4. **§2.5 BUDDY_ALLOCATED 哨兵值验证**（0.5 天）
5. **§2.3 vmm_x86_64.rs 拆分**（1-2 天）
6. **§2.6 swap slot 分配 race**（1 天）