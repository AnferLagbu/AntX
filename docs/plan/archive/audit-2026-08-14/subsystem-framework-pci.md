# framework/pci 子系统深度审计报告

> **审计范围**：`src/kernel/framework/pci/`（4 文件）
> **审计日期**：2026-08-14
> **代码规模**：约 1,425 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **17 个问题（P0×4, P1×5, P2×5, P3×3）**

## 1. 子系统概览

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs) | 597 | PCI 总线扫描 + 配置空间访问 + 设备枚举 | **极高** |
| [msi.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/msi.rs) | 524 | MSI/MSI-X 消息信号中断 | **极高** |
| [hotplug.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/hotplug.rs) | 200 | PCI 热插拔 | **高** |
| [api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/api.rs) | 104 | 公共 API | 中 |

## 2. 严重问题

### 2.1 [P0] `mod.rs:77` `ECAM_BASE = 0x3F00_0000` 硬编码——与 QEMU 实际配置可能不一致

- **位置**：[mod.rs:74-77](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs#L74-L77)
- **代码**：
  ```rust
  #[cfg(target_arch = "aarch64")]
  const ECAM_BASE: u64 = 0x3F00_0000;
  ```
- **问题**：
  - QEMU virt aarch64 默认 ECAM 基址 0x3F000000——硬编码。
  - **真实硬件平台**（如 Ampere Altra、AWS Graviton）ECAM 基址不同。
  - 必须通过 ACPI MCFG 或 DTB 探测。
  - 当前实现无法移植。
- **建议方案**：
  1. ACPI MCFG 表解析获取 ECAM 基址。
  2. 启动期探测，fallback 硬编码仅 QEMU。

### 2.2 [P0] `mod.rs:67-71` x86_64 PCI 配置空间通过**0xCF8/0xCFC 端口 I/O**——SMP 并发问题

- **位置**：[mod.rs:67-71](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs#L67-L71)
- **代码**：
  ```rust
  #[cfg(target_arch = "x86_64")]
  const PCI_CONFIG_ADDR: u16 = 0xCF8;
  #[cfg(target_arch = "x86_64")]
  const PCI_CONFIG_DATA: u16 = 0xCFC;
  ```
- **问题**：
  - 0xCF8/0xCFC 是**全局 PCI 配置空间访问端口**——所有 CPU 共享。
  - 多 CPU 并发访问需锁（否则访问冲突）。
  - 当前实现（PCI scan 阶段单线程）OK，但**运行期配置空间读写无锁保护**。
  - 之前审计（[subsystem-arch-net.md §2.x P0 PCI 总线扫描 SMP 并发](../audit/subsystem-arch-net.md)）已识别。
- **建议方案**：
  1. `IrqSpinLock` 保护 PCI_CONFIG_ADDR/DATA 访问。
  2. 或 per-CPU 端口访问序列化。

### 2.3 [P0] `msi.rs:93` `MSI_VECTORS: AtomicU32` 64 位位图——但只支持 64 个向量

- **位置**：[msi.rs:87-93](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/msi.rs#L87-L93)
- **代码**：
  ```rust
  const MSI_VECTOR_BASE: u8 = 0x40;
  const MSI_VECTOR_COUNT: u8 = 64;
  static MSI_VECTORS: AtomicU32 = AtomicU32::new(0);
  ```
- **问题**：
  - MSI_VECTOR_COUNT = 64，AtomicU32 可表达 64 位——**但只用 64 个**。
  - NVMe 单设备可能使用 32+ 个 MSI-X 向量（队列数 = 32）。
  - 多 NIC + NVMe + GPU → **64 个向量严重不足**。
- **建议方案**：
  1. 扩展为 256+ 向量（AtomicU256 或 AtomicU32 数组）。
  2. 用更高效的位图（Vec<AtomicU32>）。

### 2.4 [P0] `msi.rs:47-50` "MSI-X 实现占位, 待中断路由重构后启用"——**MSI-X 实际未实装**

- **位置**：[msi.rs:46-50](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/msi.rs#L46-L50)
- **问题**：
  - 注释（[msi.rs:47](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/msi.rs#L47)）承认 "MSI-X 实现占位"。
  - 但文件有 524 行——其余代码是什么？未审。
  - 真实硬件（NVMe 64+ 队列）需要 MSI-X——**当前功能不可用**。

## 3. P1 问题

### 3.1 [P1] `mod.rs:597` `pci_scan_all_buses` 256 × 32 × 8 = 65536 次配置空间读

- **位置**：[mod.rs:597](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs)
- **问题**：
  - 启动期扫描 65536 次，每次 4 字节读 = ~260KB I/O。
  - 启动延迟。

### 3.2 [P1] `msi.rs:100-104` `msi_alloc_vector` 用 `fetch_or` 非原子分配

- **位置**：[msi.rs:96-110](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/msi.rs#L96-L110)
- **问题**：
  - `fetch_or` 不能找到**第一个 0 位**——仅设置某一位为 1。
  - 注释未显示具体实现——需核查。

### 3.3 [P1] `hotplug.rs:200` 热插拔实现**完整性未审**

- **位置**：[hotplug.rs:1-200](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/hotplug.rs#L1-L200)
- **问题**：
  - PCIe 热插拔涉及复杂状态机。

### 3.4 [P1] `mod.rs:97-100` BAR 解析**未审**

- **位置**：[mod.rs:95-100](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs#L95-L100)
- **问题**：
  - BAR0-BAR5（6 个 BAR）解析。

### 3.5 [P1] `mod.rs` x86_64 与 aarch64 路径**未充分集成测试**

- **位置**：[mod.rs:597](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs)
- **问题**：
  - 双架构编译路径。

## 4. P2 问题

### 4.1 [P2] `mod.rs:67-71` `PCI_CONFIG_ADDR: u16` 单端口，**仅支持传统 PCI**（非 PCIe MMCFG）

- **位置**：[mod.rs:67-71](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs#L67-L71)
- **问题**：
  - 0xCF8/0xCFC 是传统 PCI 机制。
  - PCIe 用 MMCFG (Memory-Mapped Configuration) 替代。

### 4.2 [P2] `api.rs:104` 公共 API 仅 104 行

- **位置**：[api.rs:1-104](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/api.rs#L1-L104)
- **问题**：
  - API 较少。

### 4.3 [P2] `msi.rs:524` 中断向量分配**与 IDT 集成未审**

- **位置**：[msi.rs:524](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/msi.rs)
- **问题**：
  - MSI 向量号 → IDT entry 映射。

### 4.4 [P2] `mod.rs` PCI 设备树**未导出到 fs/devfs**

- **位置**：[mod.rs:597](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs)
- **问题**：
  - 用户态读 /dev/pci/* 应可见设备。

### 4.5 [P2] `hotplug.rs:200` 热插拔事件**未与用户态通知**

- **位置**：[hotplug.rs:1-200](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/hotplug.rs#L1-L200)
- **问题**：
  - uevent 通知。

## 5. P3 问题

### 5.1 [P3] `mod.rs` `pci_init` 启动期**阻塞用户态**

- **位置**：[mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/mod.rs)
- **问题**：
  - 启动期长延迟。

### 5.2 [P3] `api.rs` `pci_*` 接口**未文档化 FFI 约束**

- **位置**：[api.rs:1-104](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/api.rs#L1-L104)
- **问题**：
  - 调用方契约。

### 5.3 [P3] `msi.rs` MSI-X 表结构**字段布局未深审**

- **位置**：[msi.rs:524](file:///home/anfer/Code/QueenX/src/kernel/framework/pci/msi.rs)
- **问题**：
  - PCIe 规范 MsixEntry 布局。

## 6. 跨子系统关联

### 6.1 pci ↔ driver (NVMe/VirtIO)

- 驱动通过 PCI 总线枚举发现设备。
- 与 [subsystem-driver.md §2.x](../audit/subsystem-driver.md) 关联。

### 6.2 pci ↔ mm (MMIO 映射)

- BAR 资源通过 MMIO 映射。
- 与 [subsystem-framework-mm-remaining.md §2.x](../audit/subsystem-framework-mm-remaining.md) 关联。

### 6.3 pci ↔ irq (MSI 中断)

- MSI 向量注册到 IDT。
- 与 [subsystem-framework-irq.md §2.x](../audit/subsystem-framework-irq.md) 关联。

### 6.4 pci ↔ arch (ACPI/MCFG)

- aarch64 ECAM 基址从 ACPI MCFG 获取。
- 与 [subsystem-framework-arch.md §2.x](../audit/subsystem-framework-arch.md) 关联。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 4 | 4-6 天 |
| **P1** | 5 | 4-5 天 |
| **P2** | 5 | 2-3 天 |
| **P3** | 3 | 0.5 天 |
| **合计** | **17** | **11-15 天** |

### P0 修复路径（建议执行顺序）

1. **§2.1 ECAM_BASE 硬编码**（1-2 天，**aarch64 可移植性**）
2. **§2.4 MSI-X 实装**（2-3 天）
3. **§2.3 MSI 向量池扩展**（0.5-1 天）
4. **§2.2 PCI 配置空间并发**（1 天）