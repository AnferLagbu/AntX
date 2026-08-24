# 审计修复分册 04：framework 网络、驱动与外设

> 修复 framework/net、framework/pci、framework/dma、framework/console、framework/driver 与 framework/lib 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 7 章 TOP 20 + 附录 C（subsystem-framework-net/pci/dma/driver/arch-net/remaining-modules 报告）。

## 工程计划 A: PCI 与 MSI 修复

### 背景

- **B04-01. PCI 子系统 4 项 P0**
  - 描述：MSI-X 实装未完成、ECAM_BASE 硬编码、配置空间 SMP 无锁、MSI_VECTOR_COUNT 严重不足。
  - 方案：按依赖顺序——先修并发锁与 ECAM 可移植性，再实装 MSI-X，最后扩容向量。
  - 状态：[]

### 待办

- **B04-02. MSI-X 实装（TOP 20 #8）**
  - 描述：framework/pci MSI-X 实装未完成，NVMe/VirtIO 无法工作。
  - 方案：**决策点 D3 已裁决（2026-08-23 用户选"一次性完整接入"）**：实现 MSI-X table/PBA 配置与 irq 路由 + 向量分配，并完整接入 NVMe/VirtIO 驱动；QEMU 验证中断路径（`-nic none` 仅禁用网卡，不影响 VirtIO 块/NVMe 验证）。DECISION-061。
  - 状态：[]

- **B04-03. ECAM_BASE 硬编码（TOP 20 #9）**
  - 描述：framework/pci 中 `ECAM_BASE` 对 aarch64 硬编码，不可移植。
  - 方案：从 ACPI/设备树或引导信息获取 ECAM 基址，消除硬编码常量。
  - 状态：[]

- **B04-04. MSI_VECTOR_COUNT 评估与超限处理（TOP 20 #15）**
  - 描述：`MSI_VECTOR_COUNT=64`（0x40-0x7F，msi.rs:88-90）。实测当前驱动规模（msi_enable 每设备 1 向量、msix_enable 少量设备）**64 向量够用**；IDT 0x80-0xFF 仍有富余空间；`msi_alloc_vector` 位图分配本身无锁且正确。
  - 方案：**不盲目扩容**——保留 64，改为"超限行为显式化"：`msi_alloc_vector` 满时返回显式错误（ENOSPC 类）而非回绕/静默；预留常量注释说明扩容点（IDT 0x80 起始）；评估多队列驱动（NVMe/VirtIO 多队列）接入时再扩容。
  - 状态：[]

- **B04-05. PCI 配置空间 SMP 并发无锁（TOP 20 #13）**
  - 描述：`pci/mod.rs` 6 个 config 函数（read/write_config_byte/word/dword，L193-317）直接 PIO（x86_64）/volatile 访问（aarch64 ECAM），无任何锁；write 系列为 **read-modify-write 三连 PIO**，并发会丢位。
  - 方案：在 6 个 config 函数内部 PIO 序列前后持一把全局 `PCI_CONFIG_LOCK`（`IrqSpinLock`，中断上下文安全）；aarch64 ECAM 路径同样纳入；改动面 = 1 个 static + 6 函数各 2 行，全部调用方（hotplug/msi/api）无需改动。
  - 状态：[]

## 工程计划 B: 网络、控制台与 lib 修复

### 背景

- **B04-06. 网络/控制台/lib P0 引用**
  - 描述：framework/net 单文件过大 + 句柄重用；console fb 裸指针 use-after-free；strlen 无上界循环。
  - 方案：按 lib 基础件 → console → net 顺序修复。
  - 状态：[]

### 待办

- **B04-07. strlen 无上界循环（TOP 20 #1）**
  - 描述：[lib/string.rs:48-64](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/string.rs#L48-L64) `while *ptr != 0` 无上界，恶意指针 → 任意内存读 / #PF。
  - 方案：**决策点 D1 已裁决（2026-08-23 用户选"按推荐方案"）**：strlen 加 `if len > MAX_CSTR_LEN { break }` 上限；内核内部调用点改走 `strlen_safe`。DECISION-060。
  - 状态：[]

- **B04-08. gfx_console fb 裸指针（TOP 20 #16）**
  - 描述：framework/console fb 裸指针管理，use-after-free 风险。
  - 方案：fb 生命周期绑定到帧缓冲映射管理；释放路径置空并校验。
  - 状态：[]

- **B04-09. framework/net 单文件过大与句柄重用**
  - 描述：framework/net 存在单文件 >1000 行（违反简单优先）与句柄重用（u32::MAX 句柄冲突）。
  - 方案：**决策点 D6 已裁决（2026-08-23 用户选"与 B04-19 合并"）**：net 单文件拆分与 e1000 TxRing 拆分合并处理，一次理顺 framework net 边界；句柄分配改自增+冲突检测。DECISION-062。
  - 状态：[]

- **B04-10. dma/engine.rs GLOBAL_DMA 嵌套锁（P0）**
  - 描述：`dma/engine.rs:570` `static GLOBAL_DMA: Mutex<DmaEngine>` 外层锁 + 内部 `Mutex<Vec<DmaMapping>>`/`Mutex<mmio_regions>` 嵌套；`shutdown`（engine.rs:57-68）、`submit_transfer → ioremap`（engine.rs:218）同线程持外层再取内层 → 死锁。
  - 方案：外层 `GLOBAL_DMA` 改 `OnceLock<DmaEngine>`（或删除），内部 Vec 各自加锁。
  - 状态：[]

- **B04-11. dma/engine.rs cache_flush 硬编码（I6，P0）**
  - 描述：`engine.rs:418-456` `let need_flush = false; // TODO(TRACK-1F2A45)` x86_64 上**始终不做 cache flush**；`DmaMapping.is_coherent`（engine.rs:48）已存在未使用；非一致性设备数据不一致。
  - 方案：`need_flush = !mapping.is_coherent` 替换硬编码，调用方传 `mapping` 参数。
  - 状态：[]

- **B04-12. dma/engine.rs submit_transfer MMIO 泄漏（P0）**
  - 描述：`engine.rs:634-657` `ioremap` 后复制完成未 `iounmap`，`MMIO_NEXT` 单调递增（mod.rs:185）`mmio_regions` 持续增长 → 泄漏 + OOM。
  - 方案：复制完成后 `iounmap(src/dst_virt, size)`；配套 RAII `MmapGuard` 自动释放。
  - 状态：[]

- **B04-13. dma/mod.rs alloc_mmio_virt 无回收（P0）**
  - 描述：`mod.rs:185-192` `MMIO_NEXT.fetch_add` 单调递增永不回收；同一 `(virt,phys,size)` 可被多次 ioremap，多虚拟地址映射同一物理地址。
  - 方案：`mmio_regions` 改 `BTreeMap<VirtAddr, RegionInfo>` 分配时检查区间占用；提供 `free_mmio_virt` 回收。
  - 状态：[]

- **B04-14. dma/mod.rs MMIO_VIRT_BASE 用户可访问风险（I5/I6，P0）**
  - 描述：`mod.rs:20` `MMIO_VIRT_BASE = 0xFFFF900000000000` 位于 direct-map 区之外；未审计 `vmm::user_page_table()` 是否排除该范围，KPTI/SMAP 配置不当则用户可映射外设寄存器。
  - 方案：验证用户页表构建排除 `0xFFFF900000000000+`；`DmaMapping` 显式标注非用户访问。
  - 状态：[]

- **B04-15. dma/engine.rs free_coherent 可能释放错页（P0）**
  - 描述：`engine.rs:151-173` `alloc_coherent` 不检查 cpu_addr 占用；`retain(|m| m.cpu_addr != cpu_addr)` 删除所有同 cpu_addr 映射但只释放第一个 coherent 页 → 物理页泄漏 + 双重释放。
  - 方案：`free_coherent` 改 `(cpu_addr, dma_addr)` 双键定位；`alloc_coherent` 检查新虚拟地址不与现有映射冲突。
  - 状态：[]

- **B04-16. driver framework.rs PIO 无 SAFETY（F4，P0）**
  - 描述：`driver/framework.rs:38-100` outb/inb/outw/inw 4 个 unsafe 函数仅有 `# Safety` 文档注释、无行内 `// SAFETY:`（`audit_safety_coverage.py` 检测行内注释），覆盖率不通过。
  - 方案：内部 unsafe 调用（`crate::arch!(outb(...))`）加行内 SAFETY 注释。
  - 状态：[]

- **B04-17. driver e1000 TxRing ZERO_SIZE_PTR（P0）**
  - 描述：`driver/net/e1000.rs:84-107` `count == 0` 时 `kmalloc_align(0,16)` 返回 ZERO_SIZE_PTR，后续 deref 越界访问任意内存。
  - 方案：`count == 0` 短路返回 None；`NonNull::new(ptr)?.cast()` 替代 raw pointer。
  - 状态：[]

- **B04-18. driver nvme.rs packed struct（aarch64 UB，P0）**
  - 描述：`driver/storage/nvme.rs:75-97` `#[repr(C, packed)] NvmeControllerRegisters` 在 u64 字段强制未对齐访问（aarch64 data abort），编译器可优化掉 volatile 读取。
  - 方案：改 `#[repr(C)]` + 显式 padding；或每字段单独 `read_volatile`；加 `offset_of!` 编译期断言。
  - 状态：[]

- **B04-19. driver e1000 framework/services 双向依赖（F3，P0）**
  - 描述：`driver/net/e1000.rs:35-43` framework TxRing（unsafe）与 services E1000Driver（safe）双向依赖，语义循环依赖，services 不能独立测试。
  - 方案：TxRing 抽到独立子模块 `framework/driver/net/dma_ring.rs`，明确 framework（ring 抽象）/services（驱动业务）分层。**与 B04-09 合并处理（决策点 D6 已裁决，2026-08-23）**：一次理顺 framework net/driver 边界。
  - 状态：[]

- **B04-20. driver bus/pci.rs SMP 并发扫描（P0）**
  - 描述：PCI 扫描在 `init_all` 阶段但其他 CPU 可能已启动；CONFIG_ADDRESS/DATA 端口全局单例，多 CPU 并发扫描同一设备 → 状态错乱/BAR 冲突。
  - 方案：PCI 扫描加全局 `IrqSpinLock`，或 `init_all` 阶段显式确保单线程（AP 未启动）。
  - 状态：[]

- **B04-21. services/driver/acpi.rs has_fadt 硬编码（P0）**
  - 描述：`services/driver/acpi.rs:36-40` `has_fadt` 硬编码 `true` 未查询 framework；电源管理基于此走 ACPI 关机 → 未解析 FADT 也尝试 → 空指针 deref。
  - 方案：framework 端增加 `has_fadt()` 函数，services 委托；返回 `Option<bool>`（None = 无 ACPI）。
  - 状态：[]

### 验证门槛

- **B04-22. PCI/驱动回归**
  - 描述：修复后跑 QEMU 启动（-nic 前需先解 ISSUE-RT-001 或加 -nic none）+ host-tests driver 相关。
  - 方案：`make test-host` + QEMU 双架构。
  - 状态：[]

- **B04-23. lib/console 回归**
  - 描述：strlen 改造后跑 string 相关 host-tests。
  - 方案：`make test-host`。
  - 状态：[]

### 决策记录

- **DECISION-060**
  - 描述：B04-07 strlen 无上界循环采用审计推荐方案（决策点 D1）。
  - 方案：strlen 加 `MAX_CSTR_LEN` 上限 `break`；内核内部调用点改走 `strlen_safe`。用户 2026-08-23 选"按推荐方案（上限+改造）"，放弃"仅加上限"（避免后续追改调用点）。
  - 状态：[X]

- **DECISION-061**
  - 描述：B04-02 MSI-X 实装范围采用"一次性完整接入"（决策点 D3）。
  - 方案：实现 MSI-X table/PBA 配置与 irq 路由 + 向量分配，并完整接入 NVMe/VirtIO 驱动；QEMU 验证中断路径（`-nic none` 仅禁用网卡，不影响 VirtIO 块/NVMe 验证）。用户 2026-08-23 选"一次性完整接入"，放弃"框架优先分步"与"仅框架驱动另册"（MSI-X 无驱动验证属空转）。
  - 状态：[X]

- **DECISION-062**
  - 描述：B04-09 framework/net 单文件拆分采用"与 B04-19 合并"（决策点 D6）。
  - 方案：net 单文件拆分与 e1000 TxRing 拆分（`framework/driver/net/dma_ring.rs`）合并处理，一次理顺 framework net 边界；句柄分配改自增+冲突检测。用户 2026-08-23 选"与 B04-19 合并"。
  - 状态：[X]
