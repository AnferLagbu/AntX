# static mut 迁移计划

> **目标**: 将 framework 层 38 个 `static mut` 全局可变静态变量迁移到安全替代方案
>
> **前置条件**: 无
>
> **创建时间**: 2026-07-21
>
> **最终状态**: ✅ 29/38 已迁移, 9 个已文档化例外 (2026-07-22)
> - 已迁移 29 个: Task1(5) + Task2(10) + Task3(12) + Task4(2, 已有 CPU_INFO/KB_READ_SLOT)
> - 已文档化例外 9 个: aarch64 页表(5) + x86_64 per-CPU(2) + smoltcp 自引用(2)
> - 例外原因: 启动最早期写入 (sync primitives 不存在) 或 smoltcp 自引用约束

---

## 一、问题描述

`static mut` 在 Rust 未来版本中将被废弃。当前 framework 层有 38 个 `static mut` 变量，需要迁移到安全的替代方案：

| 替代方案 | 适用场景 | 说明 |
|----------|----------|------|
| `OnceLock<T>` | 启动期单次初始化，之后只读 | 线程安全，无 unsafe |
| `AtomicU8/U32/U64` | 简单标量，并发读写 | 无锁，高性能 |
| `IrqSpinLock<T>` | 并发读写，需中断安全 | 中断上下文安全 |
| `RwLock<T>` | 读多写少 | 允许并发读 |
| `UnsafeCell<T>` | 需要内部可变性的复杂类型 | 保留 unsafe，但集中管理 |

---

## 二、迁移分类

### 2.1 已完成 (2 个)

| 变量 | 原类型 | 新类型 | 文件 |
|------|--------|--------|------|
| `CPU_INFO` | `static mut Option<CpuInfo>` + `AtomicBool` | `OnceLock<CpuInfo>` | `cpu/mod.rs` |
| `KB_READ_SLOT` | `static mut u8` | `AtomicU8` | `driver/input/keyboard.rs` |

### 2.2 低复杂度 — 启动后只读 (5 个)

| 变量 | 建议方案 | 文件 | 难点 |
|------|----------|------|------|
| `VGA_DRIVER` | `OnceLock<VgaDriver>` + 内部可变性 | `driver/char/vga.rs` | putchar 需 `&mut self` |
| `GLOBAL_FRAMEBUFFER` | `OnceLock<Framebuffer>` + 内部可变性 | `driver/display/mod.rs` | init 需 `&mut self` |
| `SERIAL_PORTS` | `OnceLock<[SerialPort; 4]>` | `driver/char/serial.rs` | 多端口 |
| `GLOBAL_DMA` | `OnceLock<DmaEngine>` | `dma/engine.rs` | 内部状态 |
| `GRANT_RECORDS` | `OnceLock<[GrantRecord; N]>` | `credo/grant.rs` | 需 InteriorMutability |

### 2.3 中复杂度 — 并发读写 (10 个)

| 变量 | 建议方案 | 文件 | 难点 |
|------|----------|------|------|
| `ISR_TABLE` | `IrqSpinLock<[Option<Handler>; 256]>` | `irqline.rs` | 中断上下文访问 |
| `LOG_SINKS` | `IrqSpinLock<[SinkPtr; N]>` | `klog/mod.rs` | 高频写入 |
| `SLAB_CACHES` | `OnceLock<[KmemCache; 8]>` | `mm/kmalloc_slab.rs` | 启动后只读 |
| `GENERAL_CACHES` | `OnceLock<[KmemCache; N]>` | `mm/slab.rs` | 启动后只读 |
| `SLAB_INITIALIZED` | `AtomicBool` | `mm/slab.rs` | 简单标志 |
| `CURRENT_MM` | `AtomicPtr<MmStruct>` | `mm/vma.rs` | per-CPU 潜力 |
| `KALLOC_BUF` | `OnceLock<AlignedKallocBuf>` | `driver/net/e1000.rs` | e1000 专用 |
| `KALLOC_OFF` | `AtomicUsize` | `driver/net/e1000.rs` | 偏移量 |
| `NET_SNAPSHOT` | `OnceLock<NetSnapshot>` | `net/save.rs` | 恢复域 |
| `DHCP_HANDLE` | `OnceLock<SocketHandle>` | `net/init.rs` | 单次初始化 |

### 2.4 高复杂度 — 复杂类型 (21 个)

| 子系统 | 变量数 | 建议方案 |
|--------|--------|----------|
| net 层 | 12 | 重构为 `OnceLock<NetState>` 包装所有网络状态 |
| arch 层 | 7 | 启动期初始化，保留 `UnsafeCell` 但集中管理 |
| mm 层 | 3 | `OnceLock` 或 `RwLock` |

---

## 三、实施任务

### Task 1: 低复杂度迁移 (5 个) **状态: [X]** (commit 2335ae14)

**Files**: `vga.rs`, `display/mod.rs`, `serial.rs`, `dma/engine.rs`, `credo/grant.rs`

**Approach**: 使用 `IrqSpinLock` 替代 `static mut`, 统一锁保护模式

### Task 2: 中复杂度迁移 (10 个) **状态: [X]** (2026-07-22)

**Files**: `irqline.rs`, `klog/mod.rs`, `mm/kmalloc_slab.rs`, `mm/slab.rs`, `mm/vma.rs`, `mm/kmalloc.rs`, `driver/net/e1000.rs`, `driver/virtio/blk.rs`

**Approach**: `IrqSpinLock` 替代大部分, `AtomicBool`/`AtomicPtr`/`AtomicUsize` 替代简单标量

### Task 3: 高复杂度迁移 — net 层 (12 个) **状态: [X]** (2026-07-22)

**Files**: `net/init.rs`, `net/save.rs`

**Approach**: 统一为 `IrqSpinLock<NetState>` 结构体, 替代 NET_LOCK + 12 static mut
**保留**: SOCKET_STORAGE/SOCKET_SET (smoltcp 自引用约束, 无法移入结构体)

### Task 4: 高复杂度迁移 — arch/mm 层 (7 个) **状态: [X]** (2026-07-22)

**Files**: `arch/aarch64/mmu.rs`, `arch/x86_64/gdt.rs`, `arch/x86_64/smp_init.rs`

**Approarch**: 保留 static mut + 添加详细 SAFETY 不变量文档 (启动最早期写入, sync primitives 不存在)
**例外原因**: 页表在 MMU 启用前写入, per-CPU 在 SIPI 序列中写入, 均无法使用 IrqSpinLock

### Task 5: 全量验证 **状态: [X]** (2026-07-22)

**Steps**:
1. ✅ 双架构编译 0 warning 0 error
2. ⚠️ 9 个 static mut 残留 (已文档化例外, 见 Task 4)
3. ✅ host-tests 通过
4. ✅ 审计全部通过

---

## 四、风险评估

| 风险 | 等级 | 缓解措施 |
|------|------|----------|
| InteriorMutability 引入 UB | 高 | 每个迁移项需 SAFETY 注释 + 单元测试 |
| 中断上下文死锁 | 中 | IrqSpinLock 仅在非中断路径使用 |
| 性能回归 | 低 | OnceLock/Atomic 无锁，性能不降 |
| QEMU 测试覆盖不足 | 低 | host-tests 覆盖核心路径 |

---

## 五、预估工作量

| Task | 工作量 |
|------|--------|
| Task 1: 低复杂度 (5 个) | ~2 天 |
| Task 2: 中复杂度 (10 个) | ~3 天 |
| Task 3: net 层 (12 个) | ~4 天 |
| Task 4: arch/mm 层 (10 个) | ~3 天 |
| Task 5: 全量验证 | ~1 天 |
| **总计** | **~13 天** |
