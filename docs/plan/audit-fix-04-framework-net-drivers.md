# 审计修复分册 04：framework 网络、驱动与外设

> 修复 framework/net、framework/pci、framework/dma、framework/console、framework/driver 与 framework/lib 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 7 章 TOP 20 + 附录 C（subsystem-framework-net/pci/dma/driver/arch-net/remaining-modules 报告）。

## 工程计划 A: PCI 与 MSI 修复

### 背景

- **PCI 子系统 4 项 P0**
  - 描述：MSI-X 实装未完成、ECAM_BASE 硬编码、配置空间 SMP 无锁、MSI_VECTOR_COUNT 严重不足。
  - 方案：按依赖顺序——先修并发锁与 ECAM 可移植性，再实装 MSI-X，最后扩容向量。
  - 状态：[]

### 待办

- **MSI-X 实装（TOP 20 #8）**
  - 描述：framework/pci MSI-X 实装未完成，NVMe/VirtIO 无法工作。
  - 方案：集成 NVMe/VirtIO 驱动（决策点 D3）；实现 MSI-X table/PBA 配置与 irq 路由。
  - 状态：[]

- **ECAM_BASE 硬编码（TOP 20 #9）**
  - 描述：framework/pci 中 `ECAM_BASE` 对 aarch64 硬编码，不可移植。
  - 方案：从 ACPI/设备树或引导信息获取 ECAM 基址，消除硬编码常量。
  - 状态：[]

- **PCI 配置空间 SMP 并发无锁（TOP 20 #13）**
  - 描述：PCI 配置空间访问无锁，多核并发配置冲突。
  - 方案：配置访问统一加锁（IrqSpinLock），中断上下文安全。
  - 状态：[]

- **MSI_VECTOR_COUNT=64 扩容（TOP 20 #15）**
  - 描述：`MSI_VECTOR_COUNT=64` 严重不足（多设备/多队列场景）。
  - 方案：评估实际可用向量数（含 IOAPIC 范围），扩容并加超限行为说明。
  - 状态：[]

## 工程计划 B: 网络、控制台与 lib 修复

### 背景

- **网络/控制台/lib P0 引用**
  - 描述：framework/net 单文件过大 + 句柄重用；console fb 裸指针 use-after-free；strlen 无上界循环。
  - 方案：按 lib 基础件 → console → net 顺序修复。
  - 状态：[]

### 待办

- **strlen 无上界循环（TOP 20 #1）**
  - 描述：[lib/string.rs:48-64](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/string.rs#L48-L64) `while *ptr != 0` 无上界，恶意指针 → 任意内存读 / #PF。
  - 方案：添加 `if len > MAX_CSTR_LEN { break }` 上限（决策点 D1）；内核内部尽量改走 `strlen_safe`。
  - 状态：[]

- **gfx_console fb 裸指针（TOP 20 #16）**
  - 描述：framework/console fb 裸指针管理，use-after-free 风险。
  - 方案：fb 生命周期绑定到帧缓冲映射管理；释放路径置空并校验。
  - 状态：[]

- **framework/net 单文件过大与句柄重用**
  - 描述：framework/net 存在单文件 >1000 行（违反简单优先）与句柄重用（u32::MAX 句柄冲突）。
  - 方案：大文件拆分（决策点 D6）；句柄分配改自增+冲突检测。
  - 状态：[]

- **framework/dma I6 不变式风险**
  - 描述：framework/dma 6 项 P0（I6 DMA 禁写内核内存 + MMIO 泄漏），详见 `subsystem-framework-dma.md`。
  - 方案：按 archive 报告逐项登记实施；DMA 缓冲与内核内存隔离校验补 host-tests。
  - 状态：[]

- **framework/driver 外设驱动缺陷**
  - 描述：`subsystem-driver.md` 报告 PIO 无 SAFETY、TX ring UB、NVMe packed 等 6 项 P0。
  - 方案：按 archive 报告逐项登记实施；packed struct 在 aarch64 的 UB 项优先。
  - 状态：[]

### 验证门槛

- **PCI/驱动回归**
  - 描述：修复后跑 QEMU 启动（-nic 前需先解 ISSUE-RT-001 或加 -nic none）+ host-tests driver 相关。
  - 方案：`make test-host` + QEMU 双架构。
  - 状态：[]

- **lib/console 回归**
  - 描述：strlen 改造后跑 string 相关 host-tests。
  - 方案：`make test-host`。
  - 状态：[]
