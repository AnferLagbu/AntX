# framework/driver + services/driver 子系统深度审计报告

> **审计范围**：`src/kernel/framework/driver/` + `src/kernel/services/driver/`
> **审计日期**：2026-08-14
> **文件数**：framework 48 + services 10 (含子目录)
> **代码规模**：约 30K LoC（含 USB/NVMe/E1000 完整驱动）
> **总体结论**：✅ 0 unsafe services / ⚠️ framework unsafe 需 SAFETY 注释 / ⚠️ 39 个问题（P0×6, P1×11, P2×16, P3×6）

## 1. 子系统概览

### 1.1 目录结构（framework/driver/）

| 目录/文件 | 字节数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [framework.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/framework.rs) | 10,737 | Driver Trait + IO 端口封装 | **高** |
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/mod.rs) | 12,637 | 子模块导出 + init_all | 中 |
| [block.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/block.rs) | 7,673 | BlockDevice 抽象 | **高** |
| [hotplug.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/hotplug.rs) | 7,769 | 设备热插拔 | 中 |
| [kexec.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/kexec.rs) | 15,959 | kexec 加载 | **高** |
| [power.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/power.rs) | 5,521 | CPU 电源管理 | **高** |
| [uefi.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/uefi.rs) | 18,280 | UEFI 运行时服务 | **高** |
| [bus/pci.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/bus/pci.rs) | - | PCI 总线扫描 | **高** |
| [char/serial.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/char/serial.rs) | - | 串口 | 中 |
| [char/vga.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/char/vga.rs) | - | VGA | 中 |
| [char/pl011.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/char/pl011.rs) | - | PL011 (aarch64) | 中 |
| [input/keyboard.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/input/keyboard.rs) | - | 键盘 | 中 |
| [net/e1000.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/net/e1000.rs) | - | E1000 网卡 | **高** |
| [storage/nvme.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/storage/nvme.rs) | - | NVMe SSD | **高** |
| [storage/ahci.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/storage/ahci.rs) | - | AHCI/SATA | **高** |
| [storage/ata.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/storage/ata.rs) | - | ATA/IDE | **高** |
| [usb/xhci.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/usb/xhci.rs) | - | xHCI USB | **高** |
| [virtio/blk.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/virtio/blk.rs) | - | VirtIO Block | 中 |
| [virtio/net.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/virtio/net.rs) | - | VirtIO Net | 中 |
| [virtio/queue.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/virtio/queue.rs) | - | VirtIO 队列 | 中 |

### 1.2 services/driver/（薄包装层）

| 文件 | 字节数 | 职责 |
|---|---:|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/mod.rs) | 3,675 | IrqDecision 注册 + init |
| [acpi.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/acpi.rs) | 6,060 | ACPI 查询 + safe 包装 |
| [power.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/power.rs) | 19,316 | 电源管理策略 |
| [uefi.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/uefi.rs) | 965 | UEFI 包装 |
| [kexec.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/kexec.rs) | 886 | kexec 包装 |
| [firmware.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/firmware.rs) | 1,069 | firmware 抽象 |

### 1.3 架构概览

```text
┌─────────────────────────────────────────────────────────────┐
│ services/driver/                 100% safe Rust            │
│  └─ 薄包装 (IrqDecision + ACPI 状态查询)                   │
├─────────────────────────────────────────────────────────────┤
│ framework/driver/                TCB (允许 unsafe)         │
│  ├─ 字符设备: serial/vga/pl011                              │
│  ├─ 存储: nvme/ahci/ata                                     │
│  ├─ 网络: e1000                                             │
│  ├─ 总线: pci                                               │
│  ├─ USB: xhci                                               │
│  ├─ VirtIO: blk/net/queue/transport                         │
│  └─ 基础设施: framework.rs (IO/MMIO) + block.rs + hotplug   │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. P0 — 严重问题（6 个）

### 2.1 [P0] `outb/inb` 公共 unsafe 函数无 SAFETY 注释审计
- **位置**：[framework.rs:38-100](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/framework.rs#L38-L100)
- **代码**：
  ```rust
  pub unsafe fn outb(port: u16, value: u8) {
      crate::arch!(outb(port, value));
  }
  ```
- **问题**：
  - 4 个 unsafe 函数（outb/inb/outw/inw）有 `# Safety` 文档注释，但**没有 `// SAFETY:` 行内注释**。
  - `audit_safety_coverage.py` 检测的是 SAFETY 注释，不是 Safety 文档，**当前覆盖率审计可能不通过**。
  - 实际调用方（如 PCI 扫描、E1000 初始化）应每次提供 SAFETY 注释。
- **风险**：F4 违规。
- **修复**：
  1. 文档注释中的 `# Safety` 保持。
  2. 内部 unsafe 调用（`crate::arch!(outb(port, value))`）需要行内 SAFETY 注释。
  3. 或在 trait 边界处集中加 SAFETY 注释。

### 2.2 [P0] E1000 `TxRing::alloc` kmalloc_align 失败未 panic 但后续 deref
- **位置**：[e1000.rs:84-107](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/net/e1000.rs#L84-L107)
- **代码**：
  ```rust
  let ptr = unsafe { kmalloc_align(size as u64, 16) };
  if ptr.is_null() {
      return None;
  }
  let desc_ptr = ptr as *mut E1000TxDesc;
  for i in 0..count {
      unsafe {
          (*desc_ptr.add(i)).addr = 0;
          ...
      }
  }
  ```
- **问题**：
  - `kmalloc_align` 是 C-ABI 函数，返回 `*mut c_void`，不是 `*mut u8`。
  - 跨 FFI 边界：`size as u64` 截断（u64 -> u32），但当前 `count * size_of::<E1000TxDesc>()` 上限未校验。
  - **count == 0 时** `size = 0` → `kmalloc_align(0, 16)` 返回实现定义（Linux 走 kmalloc(0) 返回 ZERO_SIZE_PTR，**非 NULL 但无效**）。
- **风险**：
  - ZERO_SIZE_PTR deref 触发内核崩溃。
  - `count == 0` 时后续 `(*desc_ptr.add(0))` 可能越界访问任意内存。
- **修复**：
  1. 加 `if count == 0 { return None; }` 短路。
  2. ZERO_SIZE_PTR 检查（`((ptr as u64) & 0xF) != 0` 是常见启发式）。
  3. 用 `NonNull::new(ptr)?.cast::<E1000TxDesc>()` 替代 raw pointer。

### 2.3 [P0] NVMe 控制器 MMIO 寄存器 packed struct 误对齐
- **位置**：[nvme.rs:75-97](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/storage/nvme.rs#L75-L97)
- **代码**：
  ```rust
  #[derive(Debug, Clone, Copy)]
  #[repr(C, packed)]
  pub struct NvmeControllerRegisters {
      pub cap: u64,    // offset 0, 8-byte aligned OK
      pub vs: u32,     // offset 8, OK
      pub intms: u32,  // offset 12, OK
      pub intmc: u32,  // offset 16
      pub cc: u32,     // offset 20
      pub rsvd1: u32,  // offset 24
      pub csts: u32,   // offset 28
      ...
  }
  ```
- **问题**：
  - `#[repr(C, packed)]` 在 `u64` 字段上对**未对齐访问 UB**（x86 容忍但 aarch64 触发异常）。
  - NVMe 规范要求 CC、CSTS 等寄存器在 4 字节边界，**BAR0 是页对齐**（4K），内部寄存器偏移由硬件保证正确。
  - **packed 强制不对齐访问** → 性能损失 + aarch64 上系统崩溃。
- **风险**：
  - aarch64 启动时 deref `cc` 字段触发 data abort。
  - 即使 x86 容忍，编译器可优化掉 volatile 读取（因 packed 字段访问无内存序保证）。
- **修复**：
  1. 改用 `#[repr(C)]` + 显式 padding 字段。
  2. 或每个字段单独 MMIO 读取（`read_volatile`），不依赖 struct 整体映射。
  3. 加 `assert!(offset_of!(NvmeControllerRegisters, cc) == 0x14)` 编译期校验。

### 2.4 [P0] `e1000.rs` 同时维护 framework + services 两份实现，编译期漂移风险
- **位置**：[e1000.rs:35-43](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/net/e1000.rs#L35-L43)
- **代码**：
  ```rust
  pub use crate::kernel::services::driver::net::e1000::{
      E1000_ICR_LSC, ...
  };
  use crate::kernel::services::driver::net::e1000::E1000Driver;
  ```
- **问题**：
  - framework 端从 services 端 re-export，但 framework 端有 `TxRing` / `RxRing` 等**unsafe 包装**实现，services 端是**safe 业务逻辑**。
  - 实际**双向依赖**：framework TxRing 调用 services E1000Driver，services E1000Driver 调用 framework virt_to_phys / mm。
  - 编译期 OK（靠 cfg gate），但语义上是循环依赖。
- **风险**：
  - 重构时易破坏分层。
  - services 不能独立测试（依赖 framework TxRing）。
- **修复**：
  1. TxRing 抽到独立子模块 `framework/driver/net/dma_ring.rs`，services 不依赖。
  2. 明确分层：framework 提供 ring 抽象，services 提供驱动业务逻辑。

### 2.5 [P0] `PCI` 总线扫描可能在 SMP 启动期与其他 CPU 资源竞争
- **位置**：[bus/pci.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/bus/pci.rs)
- **问题**：
  - PCI 扫描在 `init_all` 阶段调用，但其他 CPU 此时可能已启动。
  - PCI 配置空间访问（CONFIG_ADDRESS/CONFIG_DATA 端口）**全局单例** → 多 CPU 并发扫描同一设备 → 状态错乱。
- **风险**：
  - 设备被识别两次。
  - BAR 资源分配冲突（两 CPU 给同一设备分配 MMIO）。
- **修复**：
  1. PCI 扫描加全局 `IrqSpinLock`。
  2. 或 `init_all` 阶段显式确保单线程（AP 还未启动）。

### 2.6 [P0] `services/driver/acpi.rs:has_fadt` 硬编码 true
- **位置**：[acpi.rs:36-40](file:///home/anfer/Code/QueenX/src/kernel/services/driver/acpi.rs#L36-L40)
- **代码**：
  ```rust
  pub fn has_fadt() -> bool {
      // 真实实现: 框架层维护 has_fadt 标志, services 调用
      // 简化: FADT 解析由 parse_fadt 私有 fn 触发, 通过 get_acpi_features 查询
      true
  }
  ```
- **问题**：
  - **硬编码 true**，没有真正查询 framework 状态。
  - 上层调用方（电源管理）基于此判断是否走 ACPI 关机 → 即使未解析 FADT 也会尝试 → 空指针 deref。
- **风险**：
  - 内核 panic。
  - 误导性 API（应返回 Option<bool>）。
- **修复**：
  1. 增加 framework 端 `has_fadt()` 函数，services 委托。
  2. 返回 `Option<bool>`，None 表示无 ACPI（无 RSDP）。

---

## 3. P1 — 重要问题（11 个）

### 3.1 [P1] NVMe `queue.rs` 描述符环未实现 MSI-X 中断亲和性
- **位置**：[virtio/queue.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/virtio/queue.rs)
- **问题**：多队列 VirtIO-Net 性能未优化，所有中断走同一 CPU。

### 3.2 [P1] `E1000_RXD_ERR_*` 错误位未聚合报告给上层
- **位置**：[e1000.rs:37-40](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/net/e1000.rs#L37-L40)
- **问题**：接收描述符错误（CRC/Frame/SEQ）逐个丢弃无统计，应用层无感知。

### 3.3 [P1] AHCI 驱动 HBA 内存分配 4KB 对齐但未校验
- **位置**：[storage/ahci.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/storage/ahci.rs)
- **问题**：AHCI 规范要求 HBA 列表在 4KB 对齐 + 128 字节 cache line 对齐。

### 3.4 [P1] USB xHCI 驱动 `ring.rs` 环形缓冲区无溢出检测
- **位置**：[usb/ring.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/usb/ring.rs)
- **问题**：TRB 环满时无 TRB 错误处理 → 数据丢失。
- **修复**：实现 Event Ring TRB 错误处理（TRB_ERROR 等）。

### 3.5 [P1] `block.rs` 全局 BlockDevice 表固定大小 16 槽
- **位置**：[block.rs:1-200](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/block.rs)
- **问题**：超过 16 个块设备（LVM / RAID）时无法注册。

### 3.6 [P1] `kexec.rs` 15KB 无 CRASH 模式校验
- **位置**：[kexec.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/kexec.rs)
- **问题**：kexec_load 接受任意内核 image，未做签名验证。
- **风险**：恶意 root 加载恶意内核。
- **修复**：集成 secure boot 签名验证（至少 kexec_file_load 路径）。

### 3.7 [P1] `power.rs` 5.5KB C-state 进入未做 TSC 校准
- **位置**：[power.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/power.rs)
- **问题**：C3/C6 进入唤醒后 TSC 可能漂移，但未禁用 TSC 计时。

### 3.8 [P1] `uefi.rs` 18KB UEFI Runtime 服务调用无版本协商
- **位置**：[uefi.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/uefi.rs)
- **问题**：UEFI 2.0 vs 2.7 某些调用语义不同，未按 EFI_BOOT_SERVICES 版本分支。

### 3.9 [P1] `hotplug.rs` 7.7KB 设备热插拔事件队列无锁保护
- **位置**：[hotplug.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/hotplug.rs)
- **问题**：多 CPU 并发热插拔事件 → 事件丢失或重复。
- **修复**：加 `IrqSpinLock` 或 per-CPU 队列。

### 3.10 [P1] `e1000.rs` 中断处理函数未屏蔽中断源
- **位置**：[e1000.rs:interrupt handler](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/net/e1000.rs)
- **问题**：E1000 中断处理先 ack 再 disable 特定位 → 期间可能丢失中断。

### 3.11 [P1] `services/driver/power.rs` 19KB 单文件过大，建议拆分
- **位置**：[power.rs](file:///home/anfer/Code/QueenX/src/kernel/services/driver/power.rs)
- **问题**：违反简单优先。

---

## 4. P2 — 中等问题（16 个）

### 4.1 [P2] `mod.rs` re-export 列表超过 30 项，应分组或拆模块
### 4.2 [P2] E1000 驱动同时维护 `E1000Device` 和 `E1000Driver` 两个类型
### 4.3 [P2] NVMe `nvme_block.rs` 包装层未实现 NCQ 队列深度
### 4.4 [P2] AHCI 驱动端口扫描顺序硬编码（port 0 优先）
### 4.5 [P2] ATA 驱动 PIO 模式超时 30 秒可能误判慢速设备
### 4.6 [P2] `services/driver/acpi.rs:ioapic_list` 固定 `[Option; 8]` 数组
### 4.7 [P2] USB 设备描述符解析未处理 USB 3.0 descriptor type 0x22 (HID)
### 4.8 [P2] `framework/driver/char/serial.rs` 16550 UART FIFO 16 字节但 buffer 配置硬编码
### 4.9 [P2] `input/keyboard.rs` PS/2 控制器轮询模式无中断支持
### 4.10 [P2] `bus/pci.rs` MSI-X capability 解析仅支持 PBA 单页
### 4.11 [P2] `display/framebuffer.rs` 32bpp 格式硬编码，不支持 16bpp/24bpp
### 4.12 [P2] `virtio/queue.rs` 通知寄存器 MMIO 访问未用 IoMem 包装
### 4.13 [P2] `block.rs` 块设备请求同步 I/O 与异步 I/O 混用
### 4.14 [P2] `framework.rs` inb/outb 未提供 32-bit I/O (inl/outl) 包装
### 4.15 [P2] `kexec.rs` 重启后内存保留列表未持久化
### 4.16 [P2] `hotplug.rs` 设备命名冲突未处理（如两个同名 PCI 设备）

---

## 5. P3 — 次要问题（6 个）

### 5.1 [P3] `acpi.rs` 部分 `has_*` 函数硬编码 true/false，应走 framework
### 5.2 [P3] `framework.rs` `DriverError` 枚举缺少 `OutOfMemory` 变体
### 5.3 [P3] `mod.rs:init_all` 函数调用顺序硬编码，无重试机制
### 5.4 [P3] `services/driver/mod.rs:init` 注册失败 let _ = 丢弃
### 5.5 [P3] `uefi.rs` 部分 UEFI 调用未处理 EFI_NOT_FOUND 错误码
### 5.6 [P3] `firmware.rs` 1KB 仅 placeholder

---

## 6. SAFETY 注释覆盖率（重点审计）

### 6.1 framework/driver/ 中的 unsafe 函数统计

| 文件 | unsafe 函数数 | SAFETY 注释 | 覆盖率 |
|---|---:|---:|---:|
| framework.rs | 4 (outb/inb/outw/inw) | ❌ 仅 `# Safety` 文档 | 0% |
| e1000.rs | ~12 (TxRing/RxRing/E1000Device) | ⚠️ 部分 | ~50% |
| nvme.rs | ~30 (MMIO/DMA/completion) | ⚠️ 部分 | ~40% |
| ahci.rs | ~20 | ⚠️ 部分 | ~50% |
| xhci.rs | ~40 | ⚠️ 部分 | ~30% |
| pci.rs | ~15 | ⚠️ 部分 | ~40% |
| serial.rs | ~5 | ⚠️ 部分 | ~60% |
| 其他 | < 5/文件 | 较高 | ~80% |

**总体 SAFETY 覆盖率**：约 50%（远低于 audit_safety_coverage.py 阈值 99.6%）

### 6.2 关键缺失示例

```rust
// e1000.rs:130 错误示例
pub fn prepare(&mut self, buf_phys: u64, buf_len: u16) {
    // SAFETY: tail 在 0..count 范围内; ptr 由 kmalloc_align 分配且大小足够。
    let desc = unsafe { &mut *self.ptr.add(self.tail) };  // ← tail 实际可能 ≥ count
    ...
}
```

**`tail` 字段未在 `prepare` 前自增 → 可能越界**。

### 6.3 修复策略

1. **逐文件 SAFE 化**：用安全抽象替代 unsafe（如 `NonNull::add` 替代 `ptr.add`）。
2. **最小化 unsafe 块**：每个 unsafe 块 < 5 行。
3. **强制 SAFETY 行内注释**：CI 钩子 + audit_safety_coverage.py 100% 阈值。

---

## 7. 与硬规则 / 不变式对照

| 硬规则/不变式 | 状态 | 备注 |
|---|---|---|
| F1 services 0 unsafe | ✅ | services/driver/ 全部 deny(unsafe_code) |
| F2 services 不直接访问 framework 内部 | ✅ 通过 `framework::arch::acpi` 等公共 API | |
| F3 模块间无循环依赖 | ⚠️ e1000 framework ↔ services 隐循环 | 见 2.4 |
| F4 framework unsafe 配 SAFETY | ❌ 覆盖率约 50% | 见 §6 |
| I1 内核态 CPU 状态保护 | ✅ | |
| I2 内核内存保护 | ✅ | |
| I3 用户态 CPU 状态通过 framework | ✅ | |
| I4 用户内存通过 framework | ✅ | |
| I5 MMIO/PIO 通过 framework | ✅ | |
| I6 DMA 不可写内核 | ⚠️ 需 NVMe 驱动审计 | |

---

## 8. 测试覆盖

| 文件 | 单元测试 | 集成测试 |
|---|---:|---:|
| e1000.rs | ❌ | ❌ |
| nvme.rs | ❌ | ❌ |
| ahci.rs | ❌ | ❌ |
| xhci.rs | ❌ | ❌ |
| pci.rs | ❌ | ❌ |
| framework.rs | ❌ | ❌ |
| block.rs | ❌ | ❌ |

**总评**：所有 framework/driver 驱动均无单元测试，**应加入 host-tests 模拟 MMIO 行为**。

---

## 9. 修复优先级

| 优先级 | 问题 | 工作量 | 风险 |
|---|---|---:|---|
| P0-1 | 2.1 SAFETY 覆盖率 0% | 8h | F4 违规 |
| P0-2 | 2.3 NVMe packed struct 误对齐 | 4h | aarch64 崩溃 |
| P0-3 | 2.2 ZERO_SIZE_PTR 风险 | 2h | 内核 panic |
| P0-4 | 2.6 has_fadt 硬编码 true | 1h | 内核 panic |
| P0-5 | 2.5 PCI 并发扫描 | 4h | 设备重复 |
| P0-6 | 2.4 e1000 循环依赖 | 8h | 重构风险 |
| P1 | 11 项 | 40h | |
| P2/P3 | 22 项 | 20h | 维护性 |

**总计**：约 90h
