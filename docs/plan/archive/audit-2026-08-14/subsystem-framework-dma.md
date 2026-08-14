# framework/dma + dma_buf 子系统深度审计报告

> **审计范围**：`src/kernel/framework/dma/`（3 文件） + `src/kernel/framework/dma_buf.rs`（1 文件）
> **审计日期**：2026-08-14
> **文件数**：4 个源文件
> **代码规模**：约 1.3K LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **29 个问题（P0×6, P1×9, P2×10, P3×4）**

## 1. 子系统概览

### 1.1 目录结构

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [dma/mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs) | 195 | 子系统入口、类型定义、MMIO 虚拟地址分配器 | 中 |
| [dma/api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/api.rs) | 103 | DmaEngine trait 抽象 | 低 |
| [dma/engine.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs) | 686 | DmaEngine 核心实现：coherent alloc / ioremap / scatter-gather / cache flush | **极高** |
| [dma_buf.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/dma_buf.rs) | 300 | DmaStream / DmaCoherent 安全 RAII 包装 + 状态机 | **极高** |

### 1.2 子系统职责

DMA 子系统 = 内核与外设之间数据搬运的核心 TCB。覆盖：
- 一致性 DMA 缓冲区分配（`alloc_coherent`/`free_coherent`）
- ioremap MMIO 映射
- 流式 DMA 映射（`map_single`/`unmap_single`）
- 散射聚集表（SG）
- 缓存一致性维护（`cache_flush`/`cache_invalidate`）

**调用方契约**（[api.rs:6-11](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/api.rs#L6-L11)）：
- `driver::storage::nvme` —— NVMe 命令队列的 DMA 缓冲区
- `driver::storage::ahci` —— AHCI PRDT 表的 DMA 映射
- `driver::net::e1000` —— E1000 收发描述符的 DMA 映射
- `driver::virtio::blk/net` —— VirtIO 队列的 DMA 映射
- `fs::hvfs` —— HvFS 页缓存直接 I/O

## 2. 严重问题

### 2.1 [P0] `engine.rs:570` `static GLOBAL_DMA: Mutex<DmaEngine>` 双重锁嵌套风险

- **位置**：[engine.rs:570](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L570) `GLOBAL_DMA`
- **代码**：
  ```rust
  static GLOBAL_DMA: Mutex<DmaEngine> = Mutex::new(DmaEngine::new());

  pub fn get_dma() -> IrqSpinLockGuard<'static, DmaEngine> {
      GLOBAL_DMA.lock()
  }

  pub(crate) fn dma() -> IrqSpinLockGuard<'static, DmaEngine> {
      GLOBAL_DMA.lock()
  }
  ```
- **问题**：
  - `DmaEngine` 内部已使用 `Mutex<Vec<DmaMapping>>` 和 `Mutex<Vec<(VirtAddr, PhysAddr, usize)>>>`（[engine.rs:19-22](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L19-L22)）。
  - **外层 GLOBAL_DMA 锁 + 内层 mappings/mmio_regions 锁 = 嵌套锁**。
  - 现有调用：
    - `shutdown()` 同时持 mappings + mmio_regions（[engine.rs:57-68](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L57-L68)）— 嵌套持两锁。
    - `submit_transfer → ioremap → dma().ioremap()`（[engine.rs:684-686](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L684-L686)）— 外层 GLOBAL_DMA + 内层 mmio_regions 嵌套。
  - **但 `ioremap` 内部只在 map 失败时 `mmio_regions.lock().push(...)`（[engine.rs:218](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L218)）**，因此外层 GLOBAL_DMA 持锁期间再次获取 mmio_regions，**IrqSpinLock 同线程持锁可能死锁**（取决于具体实现）。
- **建议方案**：
  1. 删除外层 GLOBAL_DMA 锁，所有访问走 `get_dma_engine() -> &DmaEngine` (OnceLock) 或 `static GLOBAL_DMA: OnceLock<DmaEngine>`。
  2. 内部 Vec 仍需 lock，因为是 mut 操作。

### 2.2 [P0] `engine.rs:418-493` `cache_flush` 架构分支下 `let need_flush = false` 硬编码（cache 一致性失效）

- **位置**：[engine.rs:418-456](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L418-L456)
- **代码**：
  ```rust
  #[cfg(target_arch = "x86_64")]
  {
      let need_flush = false; // TODO(TRACK-1F2A45): 由 DmaStream 的 coherent 属性决定
      if need_flush {
          // CLFLUSHOPT/CLFLUSH 实际实现
      } else {
          core::sync::atomic::fence(Ordering::SeqCst);
      }
  }
  ```
- **问题**：
  - `need_flush` **硬编码为 `false`**——x86_64 上**始终不做 cache flush**。
  - 注释承认"应由 DmaStream 的 coherent 属性决定"，但 `coherent: bool` 字段已存在（[engine.rs:48](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L48) `DmaMapping.is_coherent`）却**未被使用**。
  - 后果：非一致性设备（IOMMU、某些 NVMe 卡）走 x86_64 路径时**实际数据不一致**，但代码假装一致。
- **建议方案**：
  1. 将 `need_flush = !mapping.is_coherent` 替换硬编码。
  2. 调用方必须传入 `mapping` 参数。

### 2.3 [P0] `engine.rs:634-657` `submit_transfer` 同步 ioremap 后**未 iounmap**，泄漏 MMIO 区

- **位置**：[engine.rs:634-657](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L634-L657)
- **代码**：
  ```rust
  pub fn submit_transfer(src: PhysAddr, dst: PhysAddr, size: usize, _dir: DmaDirection) -> Option<usize> {
      let _slot = (0..MAX_DMA_TRANSFERS).find(...);
      unsafe {
          let src_virt = ioremap(src.0, size)?;
          let dst_virt = ioremap(dst.0, size)?;
          core::ptr::copy_nonoverlapping(src_virt as *const u8, dst_virt as *mut u8, size);
          DmaEngine::barrier_device();
      }
      Some(0)
  }
  ```
- **问题**：
  - `ioremap()` 调用 `dma().ioremap(PhysAddr(phys), size)`（[engine.rs:684-686](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L684-L686)）将物理地址映射到 MMIO_VIRT_BASE（0xFFFF900000000000+）。
  - 复制完成后**没有调用 `iounmap`**，**MMIO 区永久泄漏**。
  - `MMIO_NEXT` 单调递增（[mod.rs:185](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L185) `fetch_add(aligned_pages * PAGE_SIZE)`），**累积泄漏导致 MMIO 虚拟地址空间耗尽**。
  - 同时 `mmio_regions` Vec 持续增长，**最终 OOM**。
- **建议方案**：
  1. 复制完成后立即 `iounmap(src_virt, size); iounmap(dst_virt, size);`。
  2. 配套：RAII 包装 `MmapGuard` 自动释放。

### 2.4 [P0] `engine.rs:188-192` `alloc_mmio_virt` 用 `AtomicU64::fetch_add` 单调递增，无 free 回收

- **位置**：[mod.rs:185-192](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L185-L192)
- **代码**：
  ```rust
  static MMIO_NEXT: AtomicU64 = AtomicU64::new(MMIO_VIRT_BASE);

  fn alloc_mmio_virt(size: usize) -> VirtAddr {
      let pages = (size as u64).div_ceil(PAGE_SIZE);
      let aligned_pages = pages.max(1);
      let addr = MMIO_NEXT.fetch_add(aligned_pages * PAGE_SIZE, Ordering::Relaxed);
      VirtAddr(addr)
  }
  ```
- **问题**：
  - **单调递增，永远不回收**。
  - 与 §2.3 的泄漏叠加，**长期运行后 MMIO_VIRT_BASE 空间耗尽**（理论上限 0xFFFF900000000000 到 0xFFFFFFFFFFFFFFFF = 64TB，但 64TB 不可能耗尽）。
  - 但 `mmio_regions` Vec 持续增长可能 OOM。
  - 同时**没有去重保护**：同一 `(virt, phys, size)` 可被 ioremap 多次，**多个虚拟地址映射到同一物理地址**。
- **建议方案**：
  1. `mmio_regions` 改为 `BTreeMap<VirtAddr, RegionInfo>`，分配时检查区间是否已被占用。
  2. 提供 `free_mmio_virt(virt, size)` 回收。
  3. 或改用范围分配器（参考 `linked_list_allocator`）。

### 2.5 [P0] `engine.rs:106-110` `iomem` 直接读取 MMIO_VIRT_BASE 区，**用户空间可访问该区**

- **位置**：[mod.rs:20](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L20)
- **代码**：
  ```rust
  pub const MMIO_VIRT_BASE: u64 = 0xFFFF900000000000;
  ```
- **问题**：
  - MMIO 映射虚拟地址范围 `0xFFFF900000000000+` 是内核 direct-map 区之外，但 `KERNEL_BASE` 通常在 `0xFFFF800000000000+` 附近。
  - 当前未审计 `framework/mm/vmm.rs` 的用户态页表构建是否将 `0xFFFF900000000000` 范围排除。
  - 如果 KPTI/SMEP/SMAP 配置不当，**用户进程可能映射该 MMIO 区** → 直接读写外设寄存器 → 内核 rootkit。
- **建议方案**：
  1. 验证 `vmm::user_page_table()` 不映射 `0xFFFF900000000000+`。
  2. 添加 `unsafe impl Send/Sync for DmaMapping` 时显式标注非用户访问。

### 2.6 [P0] `engine.rs:151-163` `free_coherent` 按 cpu_addr 查找 → 可能释放错的物理页

- **位置**：[engine.rs:151-173](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L151-L173)
- **代码**：
  ```rust
  let mut mappings = self.mappings.lock();
  let phys_addr = mappings
      .iter()
      .find(|m| m.cpu_addr == cpu_addr && m.is_coherent)
      .map(|m| m.dma_addr);

  let pages = (size as u64).div_ceil(PAGE_SIZE);

  if let Some(phys) = phys_addr {
      pmm_free_pages_phys(phys, pages as usize);
  }

  mappings.retain(|m| m.cpu_addr != cpu_addr);
  ```
- **问题**：
  - 假设 cpu_addr 唯一标识 coherent 映射——但 `alloc_coherent` 不检查 cpu_addr 是否已被占用。
  - **两次 alloc_coherent 同一 size 可能返回不同物理页但同一虚拟页**（如果 direct-map 区有 alias）。
  - `retain(|m| m.cpu_addr != cpu_addr)` **删除所有同 cpu_addr 的映射**（包括非 coherent 的），但**只释放第一个匹配的 coherent 物理页**。
  - 后果：物理页泄漏（mappings 删除但 pmm 未释放）+ 双重释放风险。
- **建议方案**：
  1. `alloc_coherent` 必须确保新分配的虚拟地址不与现有映射冲突。
  2. `free_coherent` 改用 (cpu_addr, dma_addr) 双键定位。
  3. 释放时验证 dma_addr 与 cpu_addr 通过页表走查一致。

## 3. P1 问题

### 3.1 [P1] `engine.rs:418-456` `cache_flush` 文档承诺"按 cache line 刷写"但 need_flush=false 时根本未刷

- **位置**：[engine.rs:418-456](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L418-L456)
- **问题**：
  - 见 §2.2。当前实现**完全跳过刷 cache**。
  - 与函数 docstring 不符。

### 3.2 [P1] `engine.rs:631-633` `static DMA_TRANSFERS: [AtomicU8; 32]` 全 0/1 标记，无槽位释放

- **位置**：[engine.rs:629-657](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L629-L657)
- **代码**：
  ```rust
  const MAX_DMA_TRANSFERS: usize = 32;
  static DMA_TRANSFERS: [AtomicU8; MAX_DMA_TRANSFERS] = [const { AtomicU8::new(0) }; MAX_DMA_TRANSFERS];

  pub fn submit_transfer(...) -> Option<usize> {
      let _slot = (0..MAX_DMA_TRANSFERS).find(|i| {
          DMA_TRANSFERS[*i].compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed).is_ok()
      })?;
      ...
      Some(0)
  }
  ```
- **问题**：
  - 槽位标记为 1 后**永不复位为 0**。
  - 第 32 次调用后所有槽位被占用，**后续 submit 全部返回 None**。
  - `Some(0)` 返回的 id 永远是 0（实际未返回 slot 索引）。
- **建议方案**：
  1. `submit_transfer` 完成后 `DMA_TRANSFERS[*slot].store(0, ...)`。
  2. 或将 DMA_TRANSFERS 改为 `Vec<AtomicU8>` 动态扩容。
  3. 配套：返回 `Some(slot)` 而非 `Some(0)`。

### 3.3 [P1] `engine.rs:663-681` `submit_transfer_async` 立即调用 callback，违反 "async" 语义

- **位置**：[engine.rs:663-681](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L663-L681)
- **代码**：
  ```rust
  pub fn submit_transfer_async(src, dst, size, dir, callback) -> Option<usize> {
      let id = submit_transfer(src, dst, size, dir)?;
      let transfer = DmaTransfer {
          ...
          completed: AtomicBool::new(true),
          callback: Some(callback),
      };
      callback(&transfer);
      Some(id)
  }
  ```
- **问题**：
  - 函数名 "async" 但**同步执行 + 立即调用 callback**——与同步版 `submit_transfer` 无区别。
  - 没有真正的异步 DMA 引擎支持。
- **建议方案**：
  1. 函数名改 `submit_transfer_with_callback`。
  2. 或实装真实异步：使用 task queue + 中断唤醒。

### 3.4 [P1] `engine.rs:14` `use ... Mutex` 别名（IrqSpinLock 重命名为 Mutex）误导读者

- **位置**：[engine.rs:14](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L14)
- **代码**：
  ```rust
  use crate::kernel::framework::sync::IrqSpinLock as Mutex;
  ```
- **问题**：
  - `Mutex` 别名掩盖了 IRQ-safe 自旋锁语义。
  - 读者误以为 std::sync::Mutex，可能写出睡眠持锁或中断上下文死锁代码。
- **建议方案**：
  1. 删除别名，直接 `use ... IrqSpinLock`。
  2. 或保留别名但加注释明确 IRQ-safe 语义。

### 3.5 [P1] `engine.rs:684-686` `unsafe fn ioremap(phys: u64, size: usize)` 参数类型不一致

- **位置**：[engine.rs:683-686](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L683-L686)
- **代码**：
  ```rust
  unsafe fn ioremap(phys: u64, size: usize) -> Option<u64> {
      dma().ioremap(PhysAddr(phys), size).map(|v| v.0)
  }
  ```
- **问题**：
  - 与公开 `DmaEngine::ioremap(phys_addr: PhysAddr, size: usize)` 签名类型不一致（PhysAddr vs u64）。
  - 内部 unsafe 函数应当仅在 `submit_transfer` 中使用，但每次都重新包 PhysAddr。
- **建议方案**：
  1. 改为 `unsafe fn ioremap(phys: PhysAddr, size: usize)`。
  2. 调用方 `ioremap(src.0, size)` 改为 `ioremap(src, size)`。

### 3.6 [P1] `engine.rs:571-590` `get_dma/get_dma_mut/dma` 三个相同函数并存

- **位置**：[engine.rs:571-590](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L571-L590)
- **代码**：
  ```rust
  pub fn get_dma() -> IrqSpinLockGuard<'static, DmaEngine> { GLOBAL_DMA.lock() }
  pub fn get_dma_mut() -> IrqSpinLockGuard<'static, DmaEngine> { GLOBAL_DMA.lock() }
  pub(crate) fn dma() -> IrqSpinLockGuard<'static, DmaEngine> { GLOBAL_DMA.lock() }
  ```
- **问题**：
  - 三函数实现完全相同，仅可见性不同。
  - `get_dma_mut` 注释"与 get_dma 相同语义"，但**误导读者以为是 &mut**。
- **建议方案**：
  1. 保留 `get_dma()` 与 `dma()`（pub(crate)），删除 `get_dma_mut()`。
  2. 或三个函数都加 deprecation 警告，仅保留 `dma()`。

### 3.7 [P1] `mod.rs:175-182` `virt_to_phys` 调用 `get_vmm().get_physical()` 但未保护 NULL 指针页表走查

- **位置**：[mod.rs:174-182](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L174-L182)
- **代码**：
  ```rust
  fn virt_to_phys(virt: *const u8) -> u64 {
      if virt.is_null() { return 0; }
      get_vmm().get_physical(VirtAddr(virt as u64)).map_or(0, |p| p.0)
  }
  ```
- **问题**：
  - `is_null()` 检查 `0x0` 但 `0xFFFF_8000_0000_0000` 等内核虚拟地址低位非 0。
  - 低地址用户空间 NULL 已被 `is_null` 覆盖，但**无效的栈/堆地址**会触发页表走查返回 0 掩盖真实错误。
- **建议方案**：
  1. 添加更严格的地址范围校验。
  2. 错误时返回 `Result<u64, Error>` 而非 0（区分"未映射"与"零地址"）。

### 3.8 [P1] `dma_buf.rs:298-300` `unsafe impl Sync for DmaStream` SAFETY 注释过简

- **位置**：[dma_buf.rs:296-300](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/dma_buf.rs#L296-L300)
- **代码**：
  ```rust
  // SAFETY: DmaStream 含裸指针, 但所有可变访问通过 &mut self 排他访问, 避免数据竞争.
  unsafe impl Send for DmaStream {}
  unsafe impl Sync for DmaStream {}
  ```
- **问题**：
  - `unsafe impl Sync` 声明并发安全，但 `DmaStream::sync_state: SyncState` 可通过 `&self.sync_state()` 读取（[dma_buf.rs:190](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/dma_buf.rs#L190)）。
  - 多个 `&DmaStream` 可同时读取 sync_state——本身没问题。
  - 但 `sync_state` 修改通过 `&mut self`（[dma_buf.rs:200](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/dma_buf.rs#L200) `sync_for_device(&mut self)`），如果两线程同时持有 `&DmaStream`，sync_state 数据竞争。
  - 实际：用户持有 `DmaStream` 通常独占（外层 Arc 不共享），但**没有静态强制**。
- **建议方案**：
  1. 删除 `unsafe impl Sync`，仅保留 `Send`（通常 DMA 缓冲区跨线程转移而非共享）。
  2. 或要求外层用 `Mutex<DmaStream>` 保护并发访问。

### 3.9 [P1] `dma_buf.rs:213-227` aarch64 cache 维护 `dc cvau` 范围未对齐到 cache line 终点

- **位置**：[dma_buf.rs:208-228](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/dma_buf.rs#L208-L228)
- **代码**：
  ```rust
  let start = self.cpu_addr.as_ptr() as u64;
  let end = start + self.size as u64;
  let mut addr = start & !(CACHE_LINE_SIZE - 1);
  while addr < end {
      core::arch::asm!("dc cvau, {}", in(reg) addr);
      addr += CACHE_LINE_SIZE;
  }
  ```
- **问题**：
  - `start & !(CACHE_LINE_SIZE - 1)` 对齐起点，但 `end` **未向上对齐**，可能最后一段 cache line 未被刷新。
  - 例：`start=0x1001, cache_line=64, size=64 → end=0x1041 → 起点 0x1000, 刷 0x1000-0x1040 → 但 0x1040 开始的 cache line 仅部分覆盖**，**未刷新完整 cache line**。
  - aarch64 dc cvau 维护整行，部分覆盖地址的整行都会被维护（这是 ARM 规范），但**超出 end 的 cache line 也会被刷**——多刷无害。
  - 但**地址校验不严**，若 `cpu_addr.as_ptr()` 在用户空间，dc cvau 可能触发 data abort。
- **建议方案**：
  1. 文档化 "flush 整行，可能多刷" 的语义。
  2. 验证 cpu_addr 始终在内核 direct-map 区。

## 4. P2 问题

### 4.1 [P2] `engine.rs:51-78` `shutdown()` 已 drain mappings 但未释放非 coherent 映射的物理页

- **位置**：[engine.rs:51-78](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L51-L78)
- **代码**：
  ```rust
  for m in mappings.drain(..) {
      if m.is_coherent {
          let pages = (m.size as u64).div_ceil(PAGE_SIZE);
          pmm_free_pages_phys(m.dma_addr, pages as usize);
      }
  }
  ```
- **问题**：
  - `is_coherent=false` 的映射**不释放**（因为流式映射的物理页来自调用方，不是 DMA 引擎分配的）。
  - 但如果流式映射错误地标记为 coherent=false 但实际是 DMA 引擎分配的——会泄漏。
- **建议方案**：
  1. 文档明确"非 coherent 映射的物理页由调用方管理"。
  2. 加 `assert!(m.is_coherent || m.dma_addr.0 != 0)` 验证。

### 4.2 [P2] `engine.rs:189-220` `ioremap` rollback 仅 unmap 已 map 的页，未删除 mmio_regions 条目

- **位置**：[engine.rs:204-216](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L204-L216)
- **代码**：
  ```rust
  if get_vmm().map_page(page_virt, page_phys, flags).is_err() {
      for j in 0..i {
          let unmap_virt = VirtAddr(virt.0 + j * PAGE_SIZE);
          get_vmm().unmap_page(unmap_virt);
      }
      return None;  // ← mmio_regions.push 未执行
  }
  ```
- **问题**：
  - 当前 rollback 正确（push 在循环外）。
  - 但若 `mmio_regions.push(virt, phys, size)` 触发 alloc 失败（如 OOM）——**部分页面已映射但未记录** → 永久泄漏。
- **建议方案**：
  1. push 失败时回滚所有 map_page。
  2. 用 RAII `MmapGuard` 自动回滚。

### 4.3 [P2] `engine.rs:396-402` `get_stats()` 与 `reset_stats()` 无锁并发风险

- **位置**：[engine.rs:396-402](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L396-L402)
- **代码**：
  ```rust
  pub fn get_stats(&self) -> DmaPoolStats {
      self.stats.snapshot()
  }

  pub fn reset_stats(&self) {
      self.stats.reset()
  }
  ```
- **问题**：
  - `snapshot()` 用 `Relaxed` load——可接受（统计不要求强一致）。
  - `reset()` 用 `Relaxed` store，**正在并发的 get_stats 可能看到混合新旧值**（部分字段已 reset，部分未 reset）。
- **建议方案**：
  1. `reset()` 加全局自旋锁保护。
  2. 或文档化"reset 是 best-effort"。

### 4.4 [P2] `engine.rs:84` `is_mapped: bool` 字段无 setter

- **位置**：[mod.rs:48](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L48)
- **问题**：
  - `is_mapped` 字段初始化为 `true`，但**没有 unset 路径**（`unmap_single` 用 retain 删除条目，不修改 `is_mapped`）。
  - 字段语义模糊——是映射当前是否有效？被 retain 删除后该值无意义。

### 4.5 [P2] `mod.rs:91-101` `DmaTransfer.private_data: *mut u8` 裸指针无所有权

- **位置**：[mod.rs:99-101](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L99-L101)
- **问题**：
  - `*mut u8` 裸指针 + `Option<DmaCallback>` 回调可访问该指针。
  - 没有生命周期标注，**回调时该指针可能已 free** → use-after-free。

### 4.6 [P2] `engine.rs:486-491` `cache_flush` aarch64 路径 `cache_line_size` 未除以 8（DC CVAC 操作粒度）

- **位置**：[engine.rs:458-486](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L458-L486)
- **问题**：
  - aarch64 `dc cvac` 按 cache line 操作，cache_line_size 通常 64 字节。
  - 当前代码 `step_by(cache_line_size)` 正确。
  - 但**未验证 cache_line_size 是 2 的幂**——若不是 64 而是 128，`!(cache_line_size - 1)` 不对齐。
- **建议方案**：
  1. `assert!(CACHE_LINE_SIZE.is_power_of_two())`。

### 4.7 [P2] `mod.rs:185` `MMIO_NEXT` 用 `Relaxed` ordering 可能重排

- **位置**：[mod.rs:185-192](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L185-L185)
- **代码**：
  ```rust
  static MMIO_NEXT: AtomicU64 = AtomicU64::new(MMIO_VIRT_BASE);
  let addr = MMIO_NEXT.fetch_add(aligned_pages * PAGE_SIZE, Ordering::Relaxed);
  ```
- **问题**：
  - `Relaxed` 无序——多核并发 `alloc_mmio_virt` 可能拿到相同 addr。
  - 后果：两个设备**映射到同一虚拟地址** → 互相干扰。
- **建议方案**：
  1. 改 `Ordering::AcqRel` 或加自旋锁。

### 4.8 [P2] `engine.rs:670` `submit_transfer_async` callback 接收 `&DmaTransfer` 但 transfer 是栈变量

- **位置**：[engine.rs:670-680](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L670-L680)
- **代码**：
  ```rust
  let transfer = DmaTransfer { ... };
  callback(&transfer);
  Some(id)
  ```
- **问题**：
  - `transfer` 是栈变量，函数返回后**生命周期结束**。
  - 如果 callback 保存该引用（虽然文档说是同步触发），**use-after-free**。
- **建议方案**：
  1. callback 改 `fn(*const DmaTransfer)` 接收裸指针，文档化生命周期。
  2. 或 transfer 改为 `Box::leak`。

### 4.9 [P2] `engine.rs:55` `shutdown` 没有 `drop(DmaEngine)` 集成

- **位置**：[engine.rs:51-78](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L51-L78)
- **问题**：
  - `Vec<DmaMapping>` 与 `Vec<(VirtAddr, PhysAddr, usize)>` 自动实现 Drop（释放堆内存）。
  - 但 DmaMapping 内的物理页**必须在 shutdown 时显式释放**，否则内核 panic 后物理页泄漏。
- **建议方案**：
  1. 提供 `Drop for DmaEngine`（虽然全局单例不 Drop，但测试时需要）。

### 4.10 [P2] `api.rs:32-33` 重复声明 `DMA_MAX_MAPPINGS / DMA_MAX_SCATTER_ENTRIES`

- **位置**：[api.rs:32-33](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/api.rs#L32-L33)、[mod.rs:18-19](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L18-L19)
- **问题**：
  - `mod.rs` 定义 `DMA_MAX_MAPPINGS` + `DMA_MAX_SCATTER_ENTRIES`，`api.rs` 又重复声明。
  - 修改时容易遗漏一边。
- **建议方案**：
  1. 单一来源，`api.rs` 改为 `pub use mod::*`。

## 5. P3 问题

### 5.1 [P3] `engine.rs:398-402` `get_stats` 拷贝整个 DmaPoolStats 9 字段，每次系统调用成本

- **位置**：[engine.rs:396-402](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/engine.rs#L396-L402)
- **问题**：
  - 9 个 `AtomicU64::load(Relaxed)` + 9 个 u64 拷贝。
  - 高频调用场景下成为瓶颈。

### 5.2 [P3] `mod.rs:186-192` `alloc_mmio_virt` 不检查 MMIO 虚拟地址溢出

- **位置**：[mod.rs:185-192](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L185-L192)
- **问题**：
  - `MMIO_NEXT.fetch_add(...)` 不检查 `addr + aligned_pages * PAGE_SIZE` 是否溢出 `u64`。
  - 理论上限 0xFFFFFFFFFFFFFFFF / PAGE_SIZE = 2^52 次分配，**实际不可能耗尽**。

### 5.3 [P3] `mod.rs:67` `DmaScatterList.entries` 数组大小硬编码 `DMA_MAX_SCATTER_ENTRIES`

- **位置**：[mod.rs:62-69](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L62-L69)
- **问题**：
  - `entries: [DmaScatterEntry; 64]` 固定大小，可能被零拷贝场景浪费内存。
  - 无 `Vec` 动态版本。

### 5.4 [P3] `api.rs:76-94` `DmaEngine` trait 的 `stats()` 返回 `DmaPoolStats` (api 版本) 而非 `engine.rs` 的 `DmaPoolStats` (full version)

- **位置**：[api.rs:76-94](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/api.rs#L76-L94)、[mod.rs:104-116](file:///home/anfer/Code/QueenX/src/kernel/framework/dma/mod.rs#L104-L116)
- **问题**：
  - 两个 DmaPoolStats 类型字段不同，trait 抽象后**信息丢失**。
  - 建议 trait 返回完整 DmaPoolStats（私有化部分字段）。

## 6. 跨子系统关联

### 6.1 dma ↔ driver 集成

- `driver::storage::nvme::submit_io` 调用 `engine::submit_transfer` → 触发 §2.3 泄漏。
- `driver::net::e1000` 调用 `engine::map_single` → `&mut DmaMapping` 指针交给 C 端，C 端生命周期不受 Rust 借用检查。

### 6.2 dma ↔ mm 集成

- `engine.rs:254` `get_vmm().get_physical(buffer)?` —— `?` 在 `Option<...>` 上使用，但 `get_physical` 返回 `Option<PhysAddr>`——可能因未映射返回 None。
- `submit_transfer` 调用 `unsafe { let src_virt = ioremap(src.0, size)?; }`—— `?` 在 unsafe 块内使用，**unsafe 操作符**与 `?` 混合，可能误导编译器对 unsafe 范围的判断。

### 6.3 dma_buf.rs ↔ dma/engine.rs

- `DmaStream` 是高级 RAII 包装，理论上替代 `engine::map_single`。
- 但两者**共存**且无迁移路径——`DmaStream` 未被 `driver/` 子系统采用。
- `DmaStream::from_frame(Frame, ...)` 要求 Frame 是 `framework/mm/frame::Frame`，但 `engine.rs` 接受 `VirtAddr`——接口不一致。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 6 | 3-4 天 |
| **P1** | 9 | 4-5 天 |
| **P2** | 10 | 2-3 天 |
| **P3** | 4 | 0.5 天 |
| **合计** | **29** | **10-13 天** |

### P0 修复路径（建议执行顺序）

1. **§2.3 submit_transfer 加 iounmap**（0.5 天，立即止血）
2. **§2.2 cache_flush need_flush 改 mapping.is_coherent**（0.5 天）
3. **§2.1 删除 GLOBAL_DMA 外层锁**（1 天，避免嵌套死锁）
4. **§2.6 free_coherent 双键定位 + alloc 去重检查**（1 天）
5. **§2.4 MMIO 范围分配器回收**（1 天）
6. **§2.5 KPTI 用户空间 MMIO 区排除验证**（0.5 天，安全审计）