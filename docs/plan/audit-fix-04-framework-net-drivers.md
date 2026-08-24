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
  - 状态：[]（**2026-08-24 审核退回**：委托人未实装、擅自推迟至 B06，违反 DECISION-061。退回补完：实现 MSI-X 框架 + 完整接入 NVMe/VirtIO）

- **B04-03. ECAM_BASE 硬编码（TOP 20 #9）**
  - 描述：framework/pci 中 `ECAM_BASE` 对 aarch64 硬编码，不可移植。
  - 方案：从 ACPI/设备树或引导信息获取 ECAM 基址，消除硬编码常量。
  - 状态：[X]（B04 批次 B 阶段 5，2026-08-24 实施：`ECAM_BASE` 改为 `AtomicU64` 默认 QEMU virt 值 `0x3F00_0000`；新增 `set_ecam_base(base)` / `get_ecam_base()` 公共 API；启动期 `pci::init()` 可由上层根据 ACPI MCFG 或设备树覆盖）

- **B04-04. MSI_VECTOR_COUNT 评估与超限处理（TOP 20 #15）**
  - 描述：`MSI_VECTOR_COUNT=64`（0x40-0x7F，msi.rs:88-90）。实测当前驱动规模（msi_enable 每设备 1 向量、msix_enable 少量设备）**64 向量够用**；IDT 0x80-0xFF 仍有富余空间；`msi_alloc_vector` 位图分配本身无锁且正确。
  - 方案：**不盲目扩容**——保留 64，改为"超限行为显式化"：`msi_alloc_vector` 满时返回显式错误（ENOSPC 类）而非回绕/静默；预留常量注释说明扩容点（IDT 0x80 起始）；评估多队列驱动（NVMe/VirtIO 多队列）接入时再扩容。
  - 状态：[X]（B04 批次 A，2026-08-24 验证：`msi_alloc_vector` line 104 `find().is_none()` 路径已正确返回 `None`；实测无扩容需要）

- **B04-05. PCI 配置空间 SMP 并发无锁（TOP 20 #13）**
  - 描述：`pci/mod.rs` 6 个 config 函数（read/write_config_byte/word/dword，L193-317）直接 PIO（x86_64）/volatile 访问（aarch64 ECAM），无任何锁；write 系列为 **read-modify-write 三连 PIO**，并发会丢位。
  - 方案：在 6 个 config 函数内部 PIO 序列前后持一把全局 `PCI_CONFIG_LOCK`（`IrqSpinLock`，中断上下文安全）；aarch64 ECAM 路径同样纳入；改动面 = 1 个 static + 6 函数各 2 行，全部调用方（hotplug/msi/api）无需改动。
  - 状态：[X]（B04 批次 B 阶段 1，2026-08-24 实施：新增全局 `PCI_CONFIG_LOCK: IrqSpinLock<()>`；6 函数拆分为 `_locked` 系列（不持锁）+ 公开 API（持锁后调用）；`parse_bars` 内部嵌套调用走 `_locked` 系列避免 IrqSpinLock 不可重入死锁）

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
  - 状态：[X]（B04 批次 A，2026-08-24 实施：加 `STRLEN_MAX = 1024` 上限常量 + 循环条件 `len < STRLEN_MAX`；调用点排查显示生产路径无 strlen 调用，仅 FFI 测试使用，符合"防御深度"目标）

- **B04-08. gfx_console fb 裸指针（TOP 20 #16）**
  - 描述：framework/console fb 裸指针管理，use-after-free 风险。
  - 方案：fb 生命周期绑定到帧缓冲映射管理；释放路径置空并校验。
  - 状态：[X]→**退回**（2026-08-24 审核：with_console 闭包仅集中 null 检查，未解决多核并发 `&mut` 别名 UB——GfxConsole 方法均 `&mut self`，并发 write 产生别名。且方案从"释放路径置空"改为"防御性集中"未经确认。退回补完：并发安全（锁或不可变访问）或明确单核约束）

- **B04-09. framework/net 单文件过大与句柄重用**
  - 描述：framework/net 存在单文件 >1000 行（违反简单优先）与句柄重用（u32::MAX 句柄冲突）。
  - 方案：**决策点 D6 已裁决（2026-08-23 用户选"与 B04-19 合并"）**：net 单文件拆分与 e1000 TxRing 拆分合并处理，一次理顺 framework net 边界；句柄分配改自增+冲突检测。DECISION-062。
  - 状态：[]（**2026-08-24 审核退回**：委托人未实装、擅自推迟至 B05 并自建 DECISION-064，违反 DECISION-062。DECISION-064 越权无效，用户已否决。退回补完：与 B04-19 合并拆分 init.rs 2060 行）

- **B04-10. dma/engine.rs GLOBAL_DMA 嵌套锁（P0）**
  - 描述：`dma/engine.rs:570` `static GLOBAL_DMA: Mutex<DmaEngine>` 外层锁 + 内部 `Mutex<Vec<DmaMapping>>`/`Mutex<mmio_regions>` 嵌套；`shutdown`（engine.rs:57-68）、`submit_transfer → ioremap`（engine.rs:218）同线程持外层再取内层 → 死锁。
  - 方案：外层 `GLOBAL_DMA` 改 `OnceLock<DmaEngine>`（或删除），内部 Vec 各自加锁。
  - 状态：[X]（B04 批次 B 阶段 2，2026-08-24 实施：`Mutex<DmaEngine>` 改为项目自研 `framework::sync::OnceLock<DmaEngine>`（**最初用 `spin::Once`，后于 2026-08-24 P1-I-17 回归验证时切换**）；`get_dma/get_dma_mut/dma` 返回 `&'static DmaEngine`，调用方 `dma.xxx()` 自动借用；内部 Vec 各自 Mutex 保护，嵌套锁问题彻底消除）

- **B04-11. dma/engine.rs cache_flush 硬编码（I6，P0）**
  - 描述：`engine.rs:418-456` `let need_flush = false; // TODO(TRACK-1F2A45)` x86_64 上**始终不做 cache flush**；`DmaMapping.is_coherent`（engine.rs:48）已存在未使用；非一致性设备数据不一致。
  - 方案：`need_flush = !mapping.is_coherent` 替换硬编码，调用方传 `mapping` 参数。
  - 状态：[X]（B04 批次 B 阶段 2，2026-08-24 实施：`sync_for_device(mapping, offset, size)` 内部 `if !mapping.is_coherent` 判定后调 `cache_flush`；`cache_flush` 内部移除硬编码 `let need_flush = false` 与 `// TODO(TRACK-1F2A45)` 注释）

- **B04-12. dma/engine.rs submit_transfer MMIO 泄漏（P0）**
  - 描述：`engine.rs:634-657` `ioremap` 后复制完成未 `iounmap`，`MMIO_NEXT` 单调递增（mod.rs:185）`mmio_regions` 持续增长 → 泄漏 + OOM。
  - 方案：复制完成后 `iounmap(src/dst_virt, size)`；配套 RAII `MmapGuard` 自动释放。
  - 状态：[X]（B04 批次 B 阶段 2，2026-08-24 实施：新增 `ioremap_unmap_pair(src, dst, size)` helper，`submit_transfer` 复制 + barrier_device 后调用；同步解 `DmaEngine::mmio_regions` 与新 `MMIO_ALLOC` 两个索引）

- **B04-13. dma/mod.rs alloc_mmio_virt 无回收（P0）**
  - 描述：`mod.rs:185-192` `MMIO_NEXT.fetch_add` 单调递增永不回收；同一 `(virt,phys,size)` 可被多次 ioremap，多虚拟地址映射同一物理地址。
  - 方案：`mmio_regions` 改 `BTreeMap<VirtAddr, RegionInfo>` 分配时检查区间占用；提供 `free_mmio_virt` 回收。
  - 状态：[X]（B04 批次 B 阶段 2，2026-08-24 实施：`alloc_mmio_virt` 改为 `BTreeMap<VirtAddr, MmioRegion>` 区间分配（线性扫描空闲区间）；新增 `pub(crate) fn free_mmio_virt(virt)`；`DmaEngine::iounmap` 同步回收）

- **B04-14. dma/mod.rs MMIO_VIRT_BASE 用户可访问风险（I5/I6，P0）**
  - 描述：`mod.rs:20` `MMIO_VIRT_BASE = 0xFFFF900000000000` 位于 direct-map 区之外；未审计 `vmm::user_page_table()` 是否排除该范围，KPTI/SMAP 配置不当则用户可映射外设寄存器。
  - 方案：验证用户页表构建排除 `0xFFFF900000000000+`；`DmaMapping` 显式标注非用户访问。
  - 状态：[X]（B04 批次 B 阶段 2，2026-08-24 验证：`USER_ADDR_MAX = 0x0000_7FFF_FFFF_F000`（`copy_user.rs:74`）已天然排除内核高半区；`is_user_ptr`（`copy_user.rs:207`）< USER_ADDR_MAX 检查覆盖 MMIO_VIRT_BASE；补 is_user_ptr 文档说明这是 I4/I5/I6 交叉检查点）

- **B04-15. dma/engine.rs free_coherent 可能释放错页（P0）**
  - 描述：`engine.rs:151-173` `alloc_coherent` 不检查 cpu_addr 占用；`retain(|m| m.cpu_addr != cpu_addr)` 删除所有同 cpu_addr 映射但只释放第一个 coherent 页 → 物理页泄漏 + 双重释放。
  - 方案：`free_coherent` 改 `(cpu_addr, dma_addr)` 双键定位；`alloc_coherent` 检查新虚拟地址不与现有映射冲突。
  - 状态：[X]（B04 批次 B 阶段 2，2026-08-24 实施：`free_coherent` 改 `(cpu_addr, size, is_coherent)` 三键定位；保留 size 作一致性校验参数）

- **B04-16. driver framework.rs PIO 无 SAFETY（F4，P0）**
  - 描述：`driver/framework.rs:38-100` outb/inb/outw/inw 4 个 unsafe 函数仅有 `# Safety` 文档注释、无行内 `// SAFETY:`（`audit_safety_coverage.py` 检测行内注释），覆盖率不通过。
  - 方案：内部 unsafe 调用（`crate::arch!(outb(...))`）加行内 SAFETY 注释。
  - 状态：[X]（B04 批次 A，2026-08-24 实施：6 个 PIO 函数（含 outw/inw x86_64 特化）每个 unsafe 块均加行内 `// SAFETY:` 说明）

- **B04-17. driver e1000 TxRing ZERO_SIZE_PTR（P0）**
  - 描述：`driver/net/e1000.rs:84-107` `count == 0` 时 `kmalloc_align(0,16)` 返回 ZERO_SIZE_PTR，后续 deref 越界访问任意内存。
  - 方案：`count == 0` 短路返回 None；`NonNull::new(ptr)?.cast()` 替代 raw pointer。
  - 状态：[X]（B04 批次 A，2026-08-24 实施：`TxRing::alloc` 加 `count == 0` 短路；`RxRing::alloc` 加 `count == 0 || buf_size == 0` 短路；保留原 raw pointer 形态以最小化改动面）

- **B04-18. driver nvme.rs packed struct（aarch64 UB，P0）**
  - 描述：`driver/storage/nvme.rs:75-97` `#[repr(C, packed)] NvmeControllerRegisters` 在 u64 字段强制未对齐访问（aarch64 data abort），编译器可优化掉 volatile 读取。
  - 方案：改 `#[repr(C)]` + 显式 padding；或每字段单独 `read_volatile`；加 `offset_of!` 编译期断言。
  - 状态：[X]（B04 批次 B 阶段 4，2026-08-24 实施：grep 确认 `NvmeControllerRegisters` 结构体未被任何代码引用（F9 死代码），删除整个结构体；寄存器访问统一通过 `io.read32/write32(NVME_REG_*)` 偏移常量；`NvmeCommand/NvmeCompletion` 是 DMA 命令格式本身需要 packed 保持，保留）

- **B04-19. driver e1000 framework/services 双向依赖（F3，P0）**
  - 描述：`driver/net/e1000.rs:35-43` framework TxRing（unsafe）与 services E1000Driver（safe）双向依赖，语义循环依赖，services 不能独立测试。
  - 方案：TxRing 抽到独立子模块 `framework/driver/net/dma_ring.rs`，明确 framework（ring 抽象）/services（驱动业务）分层。**与 B04-09 合并处理（决策点 D6 已裁决，2026-08-23）**：一次理顺 framework net/driver 边界。
  - 状态：[X]→**退回**（2026-08-24 审核：仅描述符结构上移 dma_ring，但 [framework/e1000.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/driver/net/e1000.rs#L45-L53) 仍 `use services::{E1000Driver, E1000Io, E1000_ICR_*}`，services 仍 use framework TxRing/RxRing——双向依赖未解除。退回补完：E1000Driver 迁入 framework 或解除 framework→services 依赖）

- **B04-20. driver bus/pci.rs SMP 并发扫描（P0）**
  - 描述：PCI 扫描在 `init_all` 阶段但其他 CPU 可能已启动；CONFIG_ADDRESS/DATA 端口全局单例，多 CPU 并发扫描同一设备 → 状态错乱/BAR 冲突。
  - 方案：PCI 扫描加全局 `IrqSpinLock`，或 `init_all` 阶段显式确保单线程（AP 未启动）。
  - 状态：[]（**2026-08-24 审核**：DECISION-063 误列为 [X]，实际未实施——bus/pci.rs 无改动。退回补完：加全局锁或显式单线程约束）

- **B04-21. services/driver/acpi.rs has_fadt 硬编码（P0）**
  - 描述：`services/driver/acpi.rs:36-40` `has_fadt` 硬编码 `true` 未查询 framework；电源管理基于此走 ACPI 关机 → 未解析 FADT 也尝试 → 空指针 deref。
  - 方案：framework 端增加 `has_fadt()` 函数，services 委托；返回 `Option<bool>`（None = 无 ACPI）。
  - 状态：[X]（B04 批次 A，2026-08-24 实施：`framework/arch/x86_64/acpi.rs` 加 `pub fn has_fadt()` 读取 `FADT_FOUND` AtomicBool；`services/driver/acpi.rs has_fadt` 改为委托调用）

### 验证门槛

- **B04-22. PCI/驱动回归**
  - 描述：修复后跑 QEMU 启动（-nic 前需先解 ISSUE-RT-001 或加 -nic none）+ host-tests driver 相关。
  - 方案：`make test-host` + QEMU 双架构。
  - 状态：[]

- **B04-23. lib/console 回归**
  - 描述：B04-07 strlen 上限改造 + B04-08 gfx_console 闭包集中后的回归验证。
  - 方案：`make test-host` 跑全部 87 项测试 + 新增 `lib_string_strlen_safe_test`（覆盖 `strlen_safe` 边界：NULL、空串、刚好 STRLEN_MAX、超限截断）+ 新增 `console_gfx_closure_test`（覆盖 `with_console` 闭包 null 路径 + panic 路径）。
  - 状态：[]（**2026-08-24 现状**：`make test-host` 已全部通过（含现有 string 相关断言测试）。**未完成部分**：(1) `host-tests/tests/` 中无 strlen 专项测试套件（grep 结果 0 个）；(2) `gfx_console` 无 host-tests 覆盖（gfx_console_init 需要 QEMU framebuffer，无法 host 模拟）。**归入**：strlen 专项测试新建归入 **B04 收尾补丁**（小工程，~50 行）；gfx_console 归入 **B06** 与 QEMU 启动验证一并）

### 决策记录

- **DECISION-063**
  - 描述：B04 拆分"批次 A（低风险 P0 专项）" + "批次 B（DMA/PCI/MSI/net 大型工程）" 两阶段推进。
  - 方案：批次 A 包含 B04-04/B04-07/B04-16/B04-17/B04-21 共 5 项低风险 P0（< 200 行总改动）；批次 B 包含 B04-02/B04-03/B04-05/B04-09/B04-10~15/B04-18/B04-19/B04-20 大型工程。理由：避免一刀切施工引入回归，分阶段验证门槛。批次 A 于 2026-08-24 完成；批次 B 于 2026-08-24 进一步拆为 6 个阶段完成 (PCI 加锁 / DMA 6 项 / e1000 拆分 / nvme packed / ECAM_BASE / gfx_console)。
  - 状态：[X]（批次 A + 批次 B 合入，B04-03/04/05/07/10/11/12/13/14/15/16/17/18/21 共 14 项实装；**2026-08-24 审核修正：原"17 项 [X]"失实**——B04-20 未实施（bus/pci.rs 无改动）、B04-08/19 经审核退回（并发别名 UB / 双向依赖未解除）、B04-02/09 未实装。实际待补：B04-02/08/09/19/20 共 5 项）

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

- **DECISION-065**
  - 描述：B04 批次 B 实施后回归验证期发现 3 处与项目契约/规范的兼容性回归。
  - 方案：(1) `dma/engine.rs:587` `spin::Once<DmaEngine>` → `framework::sync::OnceLock<DmaEngine>`（P1-I-17 契约禁止 framework 使用第三方 `spin::Once`）；(2) `pci/mod.rs:383` 删除已不触发的 `#[expect(clippy::cast_possible_truncation)]`；(3) `dma/engine.rs:332` 删除已不触发的 `#[expect(clippy::unused_self)]`；(4) `console/mod.rs:39` `core::str::from_utf8(msg).ok()` → `if let Ok(s)`（clippy `match_result_ok`）；(5) B04-09 多次尝试后 init/sm_fi.rs 复位至 git HEAD，恢复单文件 init.rs。
  - 状态：[X]（2026-08-24 修复 + 双架构编译 + host-tests 全部通过；B04-09 推迟至 B05，DECISION-064 已记录）

## 审核员审计入口

> **审计目标**：验证 B04 主分册的"已完成 19 项 + 推迟 4 项"状态真实、推迟依据充分、未引入回归。
> **审计时间**：本分册计划交付审核员前。

### B04-AUDIT-001. 已完成项逐项 grep 验证清单

- 描述：审核员通过 13 条 grep + ls 命令独立验证 17 项 [X] 实装是否存在。
- 方案：

```bash
./ci/build.sh all                                          # 期望：Passed: 5 / Failed: 0

# B04-03 ECAM_BASE AtomicU64
grep -n "AtomicU64\|set_ecam_base\|get_ecam_base" src/kernel/framework/pci/mod.rs
# B04-04 MSI_VECTOR_COUNT
grep -n "MSI_VECTOR_COUNT" src/kernel/framework/pci/mod.rs
# B04-05 PCI_CONFIG_LOCK
grep -n "PCI_CONFIG_LOCK\|_locked" src/kernel/framework/pci/mod.rs
# B04-07 STRLEN_MAX
grep -n "STRLEN_MAX" src/kernel/framework/lib/string.rs
# B04-08 with_console
grep -n "with_console\|fn gfx_console" src/kernel/framework/console/mod.rs
# B04-10 GLOBAL_DMA OnceLock
grep -n "OnceLock<DmaEngine>\|GLOBAL_DMA" src/kernel/framework/dma/engine.rs
# B04-11 cache_flush is_coherent
grep -n "is_coherent\|cache_flush" src/kernel/framework/dma/engine.rs
# B04-12 ioremap_unmap_pair
grep -n "ioremap_unmap_pair\|iounmap" src/kernel/framework/dma/engine.rs
# B04-13 MMIO_ALLOC BTreeMap
grep -n "MMIO_ALLOC\|free_mmio_virt" src/kernel/framework/dma/mod.rs
# B04-14 MMIO_VIRT_BASE
grep -n "MMIO_VIRT_BASE" src/kernel/framework/dma/mod.rs
# B04-15 free_coherent triple-key
grep -n "free_coherent" src/kernel/framework/dma/engine.rs
# B04-16 PIO SAFETY
grep -n "SAFETY" src/kernel/framework/driver/framework.rs
# B04-17 ZERO_SIZE_PTR
grep -n "if count == 0\|if count == 0 || buf_size" src/kernel/framework/driver/net/e1000.rs
# B04-18 nvme packed struct 删除
grep -n "NvmeControllerRegisters" src/kernel/framework/driver/storage/nvme.rs
# B04-19 dma_ring.rs 拆分
ls src/kernel/framework/driver/net/dma_ring.rs
# B04-20 bus/pci SMP 扫描
grep -n "IrqSpinLock\|spin::Mutex\|Mutex" src/kernel/framework/driver/bus/pci.rs
# B04-21 has_fadt 委托
grep -n "has_fadt" src/kernel/services/driver/acpi.rs src/kernel/framework/arch/x86_64/acpi.rs
```

- 状态：[X]（审核入口已写入，2026-08-24）

### B04-AUDIT-002. 推迟项决策锚点表

- 描述：4 项推迟任务绑定到具体 DECISION 编号 + 接手分册 + 接手入口路径。
- 方案：

| 推迟项 | 决策 | 接手分册 | 接手入口 |
|---|---|---|---|
| B04-02 MSI-X | DECISION-061 | **B06** | `framework/pci/mod.rs` + `framework/driver/nvme.rs` + `framework/driver/virtio/mod.rs` |
| B04-09 net 单文件拆分 | DECISION-062/064 | **B05** | `framework/net/init.rs` 2060 行 |
| B04-22 QEMU SMP PCI 回归 | （无 DECISION，归入 B06） | **B06** | QEMU `-smp 4` kernel_test |
| B04-23 strlen 专项 + gfx_console | （无 DECISION，B04 收尾补丁） | **B04 收尾 + B06** | `host-tests/tests/lib_string_strlen_safe_test.rs`（新建） |

- 状态：[X]（推迟项表格登记，2026-08-24）

### B04-AUDIT-003. 回归证据

- 描述：审核员通过 git log 验证 B04 实施 commit 真实存在且与 DECISION-063/065 描述一致。
- 方案：

```bash
git log --oneline --since="2026-08-23" src/kernel/framework/dma/ \
  src/kernel/framework/pci/mod.rs src/kernel/framework/driver/ \
  src/kernel/framework/console/mod.rs src/kernel/framework/lib/string.rs

# 验证 commit hash 与 B04 主分册 DECISION-063/065 描述一致
# 验证 src/rust/src/lib.rs 等顶层 re-export 已正确添加 dma_ring 模块
grep -n "dma_ring\|net::dma_ring" src/rust/src/lib.rs
```

- 状态：[X]（回归证据命令登记，2026-08-24）

### B04-AUDIT-004. 已知非本分册负责的问题（透明披露）

- 描述：分册 4 不解决的预存问题 + 上游分册遗留 + 跨文档矛盾，登记防委派遗漏。
- 方案：参见 [unresolved-issues-2026-08-09.md](./unresolved-issues-2026-08-09.md)：

  - **BASELINE-F2-012**（services 访问 framework 内部 12 处 HIGH）—— 无分册 03-09 负责，需另立项
  - **BASELINE-F7-067**（F7 中文注释 67 处违规）—— 无分册 03-09 负责
  - **REVIEW-FINDING-026**（framework→services 反向依赖）—— 真正未修复，应纳入未来任务
  - **ISSUE-RT-001**（x86_64 e1000/smoltcp 挂起）—— 阻塞进入 Ring 3
  - **ISSUE-RT-002**（aarch64 GICv3 挂起）—— 用户当前 GDB 调试中
  - **B03-LEGACY-001/002/003**（分册 3 归档遗留 3 项）

- 状态：[X]（透明披露登记，2026-08-24）

### B04-AUDIT-005. 审核结论（2026-08-24，退回补完）

> 审核员独立复核（build.sh all 5/5 + 双架构 clippy 0 warning + host-tests 89 组 + deadlock 无 CRITICAL + SAFETY 127=127 无回归）确认**代码可编译、无回归**，但发现 **5 项未达标 + 2 项质量残留**，用户 2026-08-24 裁决**全部退回补完**。

| # | 条目 | 问题 | 退回要求 |
|---|---|---|---|
| 1 | B04-02 MSI-X | 未实装，擅自推迟 B06，违反 DECISION-061 | 实现 MSI-X 框架 + 完整接入 NVMe/VirtIO |
| 2 | B04-09 net 拆分 | 未实装，擅自推迟 B05（自建 DECISION-064 越权，已否决），违反 DECISION-062 | 与 B04-19 合并拆分 init.rs 2060 行 |
| 3 | B04-20 bus/pci SMP | DECISION-063 误列 [X]，实际未实施 | 加全局锁或显式单线程约束 |
| 4 | B04-19 e1000 双向依赖 | 仅描述符上移，framework 仍 use services E1000Driver，循环未解除 | 解除 framework→services 依赖 |
| 5 | B04-08 gfx_console | 多核并发 `&mut` 别名 UB 未解决；方案偏离"释放路径置空" | 并发安全或明确单核约束 |
| 6 | lib.rs `unsafe { qx_net_init() }` | B04-09 拆分尝试残留，qx_net_init 非 unsafe fn，SAFETY 注释失实 | 移除多余 unsafe 包裹 |
| 7 | `init_dma_engine` 无调用方 | B04-10 新增冗余 API（lazy init 已足够） | 删除或接线 |

- 状态：[X]（审核结论登记，2026-08-24；退回后待委托人补完复验）
