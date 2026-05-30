# AntX 审计修复工程 — 委派任务清单

> **来源**: [审计报告](../changelog/AUDIT_REPORT_2026-05-30.md) + [Clippy 附录](../changelog/AUDIT_REPORT_2026-05-30.md#附录-a-clippy-死代码检测报告)
> **生成日期**: 2026-05-30
> **用法**: 每个任务独立可委派，包含完整上下文、修复步骤和验证方法。

---

## 任务总览

| 批次 | 优先级 | 任务数 | 预估总工时 | 可并行 |
|------|--------|--------|-----------|--------|
| Batch A | P0 立即修复 | 6 | ~12h | ✅ |
| Batch B | P1 第二优先 | 8 | ~20h | ✅ |
| Batch C | P2 第三优先 | 5 | ~16h | ✅ |
| Batch D | 文档修复 | 4 | ~8h | ✅ |
| Batch E | Clippy 清理 | 4 | ~8h | ✅ |

---

## Batch A: P0 立即修复（优先委派）

---

### Task A1: 修复 HvFS checksum 假 SHA256

- **审计参考**: C40
- **严重程度**: 🔴 P0 Critical
- **委派标签**: `filesystem` `crypto` `integrity`

**文件**:
- `src/kernel/fs/hvfs/checksum.rs` — 当前使用非标准/简化哈希
- `src/kernel/credo/sha256.rs` — 标准 SHA256 实现（复用目标）

**当前问题**:
HvFS checksum 模块声称使用 SHA256 进行数据完整性校验，但实际使用简化的或非标准的哈希函数（疑似 Fletcher 变体）。这不能提供密码学级别的完整性保护，导致静默数据损坏无法检测。

**修复步骤**:
1. 阅读 `src/kernel/fs/hvfs/checksum.rs`，确认当前哈希函数的具体实现
2. 阅读 `src/kernel/credo/sha256.rs`，确认 SHA256 API 签名
3. 修改 `checksum.rs` 中所有 `compute_checksum()` / `verify_checksum()` 调用，替换为 Credo 的 SHA256
4. 更新 `host-tests/src/checksum.rs` 中的测试用例，使用 SHA256 预期值验证
5. 如果校验和格式有变（如长度从 8 字节变为 32 字节），同步修改 SPA/Vdev 层中依赖校验和长度的代码

**验证**:
```bash
cd host-tests && cargo test checksum
cd host-tests && cargo test hvfs
```
确认所有测试通过，且 SHA256 校验和与预期值一致。

**预估工时**: 2h

---

### Task A2: 修复 NVMe PRP2 始终为零

- **审计参考**: C56
- **严重程度**: 🔴 P0 Critical
- **委派标签**: `driver` `storage` `nvme` `dma`

**文件**:
- `src/kernel/driver/storage/nvme.rs` — PRP 列表构造逻辑

**当前问题**:
NVMe 命令的 PRP2 （Physical Region Page 2）条目在处理多页 DMA 传输时未被正确设置，始终为 0。当 I/O 跨越 4KB 页面边界时，第二页数据被 DMA 写入物理地址 0，覆盖 IVT/中断向量表等关键低地址内存。

NVMe 规范要求：
- 传输 ≤ 1 页: PRP1 = 数据物理地址，PRP2 = 0
- 传输 = 2 页: PRP1 = 第1页物理地址，PRP2 = 第2页物理地址
- 传输 > 2 页: PRP1 = 第1页物理地址，PRP2 = PRP 列表页物理地址

**修复步骤**:
1. 找到 `nvme.rs` 中构造 SQ entry 的函数（约 L140-L160 附近）
2. 检查 PRP 填充逻辑，确认 PRP2 的分支：
   - 1 页传输: PRP2 = 0 ✅
   - 2 页传输: PRP2 = data_phys + 4096
   - >2 页: PRP2 = PRP list 物理地址，PRP list 中填充第 2-N 页的物理地址
3. 实现 `prp_list_alloc()` 辅助函数，分配连续物理页作为 PRP 列表
4. 添加边界检查：确保 PRP 列表不超过 1 页（512 entries × 8 bytes）
5. 确保 PRP 列表物理地址在命令完成后被正确释放

**验证**:
- 创建 > 4KB 的文件写入测试，验证读写数据一致性
- 如果 QEMU 支持，使用 `-trace nvme_*` 追踪 NVMe DMA 地址

**预估工时**: 3h

---

### Task A3: 修复 lwIP virt_to_phys 假恒等映射

- **审计参考**: C63
- **严重程度**: 🔴 P0 Critical
- **委派标签**: `network` `lwip` `dma` `memory`

**文件**:
- `src/kernel/net/sys_arch.rs` — `virt_to_phys()` 函数
- `src/kernel/net/arch/net_glue.c` — C 侧桥接（如果有类似函数）

**当前问题**:
lwIP 的 `sys_arch` 适配层中 `virt_to_phys()` 返回恒等值（`addr as u32`），假设虚拟地址等于物理地址。AntX 使用高半内核（x86_64: 虚拟地址 = 物理地址 + 0xFFFF800000000000），恒等映射完全错误。当 lwIP 调用此函数获取 DMA 缓冲区物理地址时，返回的"物理地址"实际是虚拟地址高 32 位截断值，导致网卡 DMA 写入随机物理地址 → 内核内存损坏。

**修复步骤**:
1. 找到 `sys_arch.rs` 中 `virt_to_phys()` 的实现
2. 实现真正的虚拟地址到物理地址转换：
   - 对于内核直接映射区（`0xFFFF800000000000` ~ `0xFFFF80FFFFFFFFFF`），`phys = virt - KERNEL_BASE`
   - 对于其他地址，需要遍历页表查找（罕见路径）
3. 同时实现 `phys_to_virt()` 的反向转换
4. 确保返回值使用 `usize` 或 `u64`（而非 `u32`），以支持 >4GB 物理地址
5. 检查 lwipopts.h 中是否还有其他假恒等的宏定义需要修正

**验证**:
- 启用网络功能（DHCP 获取 IP），验证不触发 page fault
- 使用 QEMU `-d guest_errors` 追踪非法内存访问

**预估工时**: 2h

---

### Task A4: 修复 Barrier tick() 递归死锁

- **审计参考**: C51
- **严重程度**: 🔴 P0 Critical
- **委派标签**: `barrier` `deadlock` `recovery`

**文件**:
- `src/kernel/barrier/recovery.rs` — `tick()` 函数（如果在此文件）
- `src/kernel/proc/scheduler.rs` — 调度器 `tick()` 中调用 barrier 的位置

**当前问题**:
调度器 `tick()` 函数在心跳检测中获取 `RECOVERY_LOCK`，然后调用 `bsr_try_recover()`（或其他恢复函数），而恢复函数内部又尝试获取同一个 `RECOVERY_LOCK`（通过 `recovery_domain_register()` 或 `cascade_recover()` 等函数）。这是经典的递归死锁模式——任何心跳丢失都会 100% 触发系统完全死锁。

**修复步骤**:
1. 使用 `grep -rn "RECOVERY.*LOCK\|recovery.*lock" src/kernel/` 找到所有涉及 `RECOVERY_LOCK` 的位置
2. 识别 `tick()` 中获取锁和调用恢复函数的位置
3. 修复方案（二选一）：
   - **推荐**: `tick()` 中设置 `NEED_RECOVERY` 原子标志 → 立即释放锁 → 由独立的内核线程/软中断执行恢复
   - **备选**: 使用可重入锁（`spin::RwLock` 的 write lock 改为 read lock 用于检查，write lock 用于恢复），但这增加复杂度
4. 确保恢复逻辑不在中断上下文中执行耗时操作（如内存分配）

**验证**:
- 注入心跳丢失（通过 fault_injection feature），验证系统不死锁且能恢复
- 检查 `make test` 中的 barrier 相关测试通过

**预估工时**: 2h

---

### Task A5: 修复内存分配器 alloc/dealloc 双路径不一致

- **审计参考**: C11, C12
- **严重程度**: 🔴 P0 Critical
- **委派标签**: `memory` `allocator` `slab`

**文件**:
- `src/rust/src/memory_allocator.rs` — 全局分配器

**当前问题**:
查看 [memory_allocator.rs](file:///home/anfer/Code/AntX/src/rust/src/memory_allocator.rs):

```rust
// alloc: 当 kmalloc 失败时回退到 pmm_alloc_page()
if size <= PAGE_THRESHOLD {           // 2048
    let ptr = kmalloc(size as u64);    // 尝试 slab 分配
    if !ptr.is_null() { return ptr; }  // 成功
}
// 回退: 用 pmm_alloc_page 分配整页
let phys = pmm_alloc_page() as u64;
(phys + KERNEL_BASE) as *mut u8

// dealloc: 无法区分来源
if size <= PAGE_THRESHOLD {
    kfree(ptr);  // ← BUG: 如果是回退路径分配的，这里应该调用 pmm_free_page
}
```

当 `kmalloc()` 返回 NULL（slab 耗尽）时，对于 ≤2048 字节的分配请求，代码回退到 `pmm_alloc_page()` 分配整页。但 `dealloc` 仍按 `size <= PAGE_THRESHOLD` 判断，对回退路径分配的页面错误调用 `kfree()` → slab 元数据损坏。

此外，`KERNEL_BASE` 常量（`0xFFFF800000000000`）与链接脚本中的 VMA 基址（`0xFFFF800001000000`）不一致。

**修复步骤**:
1. 在 `KernelAllocator` 结构体中添加一个内部状态来追踪分配来源：
   ```rust
   // 方案: 使用分配块的前 8 字节存储标记
   // [标记: u64] [实际数据...]
   // 标记 = 0x01 → 来自 kmalloc
   // 标记 = 0x02 → 来自 pmm_alloc_page
   ```
2. 修改 `alloc()`: 分配时在返回指针前写入来源标记，返回 `ptr + 8`
3. 修改 `dealloc()`: 读取 `ptr - 8` 的标记，根据标记选择 `kfree` 或 `pmm_free_page`
4. 或者更简单的方案：将 `PAGE_THRESHOLD` 设为 slab 最大对象大小的精确值，确保 `kmalloc` 总能满足 ≤ `PAGE_THRESHOLD` 的请求，不需要回退
5. **同时修复 C12**: 统一 `KERNEL_BASE` 值。从链接脚本导出 `__kernel_start` 符号，或硬编码改为与链接脚本一致

**验证**:
- 运行 `make test-host` 中的 buddy 和 hvfs 测试
- 压力测试：循环分配/释放不同大小的内存块

**预估工时**: 2h

---

### Task A6: 修复链接脚本 .rela.dyn 缺失 PHDR

- **审计参考**: C70, C71, C72, C73
- **严重程度**: 🔴 P0 Critical
- **委派标签**: `build` `linker` `bootstrap`

**文件**:
- `src/kernel/link/x86_64.ld` — x86_64 链接脚本
- `src/kernel/link/aarch64.ld` — aarch64 链接脚本
- `src/link.ld` — 冗余链接脚本（应删除）

**当前问题**:

1. `.rela.dyn`、`.dynsym`、`.dynstr` 段没有 PHDR 声明，运行时不被 PT_LOAD 加载
2. AArch64 链接脚本完全缺失这三个段
3. `src/link.ld` 和 `src/kernel/link/x86_64.ld` 内容不一致且冗余
4. 缺少 `__bss_end` 符号

**修复步骤**:

**x86_64.ld**:
1. 找到 `.rela.dyn` 段定义（约 L86），在其前面添加 `:kernel` PHDR 声明
2. 同样处理 `.dynsym` 和 `.dynstr` 段
3. 在 BSS 段末尾添加 `__bss_end = .;`（约 L83）
4. 验证所有段都有正确的 PHDR（`:boot` 或 `:kernel`）

**aarch64.ld**:
1. 如果 AArch64 内核使用 `-fPIC`：复制 x86_64 的重定位段定义
2. 如果 AArch64 不使用 PIC：从 Makefile 中移除 `-fPIC` 编译选项

**清理**:
1. 删除 `src/link.ld`
2. 更新 Makefile 中所有对 `src/link.ld` 的引用，改为 `src/kernel/link/x86_64.ld`

**验证**:
```bash
make clean && make ARCH=x86_64
make clean && make ARCH=aarch64
# 检查编译出的 kernel.bin 能用 objdump 正确解析段
x86_64-linux-gnu-objdump -h build/kernel.bin | grep -E "rela\.dyn|dynsym|dynstr"
```

**预估工时**: 1.5h

---

## Batch B: P1 第二优先

---

### Task B1: 修复 SpinLockGuard / MutexGuard Drop 内存屏障

- **审计参考**: C45, C46
- **严重程度**: 🟠 P1 High
- **委派标签**: `sync` `lock` `memory-ordering`

**文件**:
- `src/kernel/sync/types.rs` — Guard Drop 实现（L298-L345）

**当前问题**:

`SpinLockGuard::drop()` (L298-306):
```rust
fn drop(&mut self) {
    self._lock.locked.store(0, Ordering::Release);
    core::sync::atomic::fence(Ordering::SeqCst); // ← fence 在 store 之后
}
```
这里 store-release 后再 fence 是正确的，但 fence 应该在 store 之前，确保临界区内的写入在解锁前全局可见。正确顺序应为：fence → store-release。

`MutexGuard::drop()` (L331-345):
```rust
fn drop(&mut self) {
    let depth = self._mutex.depth.fetch_sub(1, Ordering::AcqRel);
    if depth <= 1 {
        self._mutex.locked.store(0, Ordering::Release);
        self._mutex.owner.store(-1, Ordering::Release);
        self._mutex.acquire_time.store(0, Ordering::Release);
        self._mutex.inner_spinlock.locked.store(0, Ordering::Release); // ← 绕过 inner_spinlock
    }
}
```
更严重的问题是：`raw_lock()` 和 `raw_unlock()` 都通过 `inner_spinlock.raw_lock()` 保护内部字段，但 `MutexGuard::drop()` 直接操作 `locked`/`owner` 等字段，完全绕过了 `inner_spinlock` 的保护协议。

**修复步骤**:
1. **SpinLockGuard::drop()**: 将 `fence(SeqCst)` 移到 `store(Release)` 之前
2. **MutexGuard::drop()**: 改为调用 `self._mutex.inner_spinlock.raw_lock()` → 修改字段 → `self._mutex.inner_spinlock.raw_unlock()`，与 `raw_unlock()` 保持一致
3. 或者：将 `MutexGuard::drop()` 改为直接调用 `Mutex::raw_unlock()`（如果生命周期允许）

**验证**:
- 压力测试：SMP 多核并发锁竞争（如果支持的话）
- 代码审查确认所有 Drop 路径与对应的 lock/unlock 路径一致

**预估工时**: 1.5h

---

### Task B2: 修复 COW 引用计数竞态

- **审计参考**: C5
- **严重程度**: 🟠 P1 High
- **委派标签**: `memory` `cow` `concurrency`

**文件**:
- `src/kernel/mm/cow.rs` — `cow_inc_ref()` / `cow_dec_ref()`

**当前问题**:
COW 引用计数的 `cow_inc_ref()` 和 `cow_dec_ref()` 虽然使用了 `spin::Mutex<Option<BTreeMap>>` 保护整体 map，但 refcount 的读取-修改-写入（`*refs.entry(key).or_insert(0) += 1`）在 Rust 中是非原子的。在 SMP 多核环境下，两个 CPU 可能同时执行 COW 操作导致 refcount 下溢。不过实际上，由于 `spin::Mutex` 保护了 map 访问，单个页面的并发 inc/dec 是互斥的。所以真正的风险可能是**调用方在锁外部读取 refcount 做判断**。

**修复步骤**:
1. 检查所有 `cow_dec_ref()` 的调用方，确认返回值的判断在锁内完成
2. 如果 refcount 需要在锁外读取，改用 `AtomicU32` 存储而非普通 `u32`
3. 在 `cow_dec_ref()` 返回 `true`（refcount 归零）后、释放页面前，确保没有 TOCTOU 窗口
4. 添加测试：并发 fork/exit 压力测试

**验证**: 运行 `make test-host` 确保 buddy 和内存测试通过。

**预估工时**: 1h

---

### Task B3: 修复 PMM 伙伴分配器并发安全

- **审计参考**: C14
- **严重程度**: 🟠 P1 High
- **委派标签**: `memory` `pmm` `concurrency`

**文件**:
- `src/kernel/mm/pmm.rs` — 物理内存管理器

**当前问题**:
`pmm_alloc_page()` 和 `pmm_free_page()` 操作全局 buddy bitmap，但没有锁保护。SMP 环境下并发分配/释放会导致 bitmap 损坏，可能引发双分配（两个 CPU 分配到同一物理页）。

**修复步骤**:
1. 在 `pmm.rs` 中引入 `spin::Mutex` 或自定义自旋锁保护 buddy bitmap
2. 确保锁的粒度合理：避免在持锁期间做耗时操作（如清零页面）
3. 检查 `pmm_alloc_pages()` 和 `pmm_free_pages()` 同样加锁
4. 如果性能敏感，考虑使用 per-CPU 页面缓存（先分配，再批量归还）

**验证**: SMP 多核内存分配压力测试。

**预估工时**: 2h

---

### Task B4: 修复 ELF 加载器畸形文件防护

- **审计参考**: C17
- **严重程度**: 🟠 P1 High
- **委派标签**: `proc` `elf` `security`

**文件**:
- `src/kernel/proc/elf.rs` — ELF 加载器

**当前问题**:
ELF 解析器信任文件中的 `e_phoff`、`e_phentsize`、`p_offset`、`p_filesz` 字段。恶意构造的 ELF 可导致：
- `p_offset + p_filesz` 整数溢出 → 读取超出文件范围的内存
- 无限大的 `e_phnum` → 循环耗尽内核栈

**修复步骤**:
1. 对所有偏移/大小做溢出检查：使用 `checked_add()` 或 `saturating_add()`
2. 验证 `e_phoff + e_phnum * e_phentsize ≤ file_size`
3. 对每个 PHDR：验证 `p_offset + p_filesz ≤ file_size` 且 `p_offset + p_memsz ≤ file_size`
4. 限制 `e_phnum` 最大值（如 128）
5. 验证 `p_vaddr + p_memsz` 不超过用户地址空间最大值
6. 拒绝 `p_filesz > p_memsz` 的段

**验证**: 使用已知的畸形 ELF 文件测试（如 AFL 生成的样本）。

**预估工时**: 2h

---

### Task B5: 修复 syscall.md 完全过时 — 从源码重新生成

- **审计参考**: C81
- **严重程度**: 🟠 P1 High
- **委派标签**: `documentation` `syscall`

**文件**:
- `docs/api/syscall.md` — 过时的系统调用文档
- `src/kernel/syscall/types.rs` — 实际的 syscall 编号定义
- `src/user/lib/src/sys.rs` — 用户态 syscall 常量

**当前问题**:
`syscall.md` 描述的系统调用编号完全与实际实现不同。例如文档写 `SYS_FS_OPEN = 20`，代码中是 `SYS_open = 2`（POSIX）。用户态库已迁移到 POSIX 编号，但文档从未更新。

**修复步骤**:
1. 读取 `src/kernel/syscall/types.rs` 和 `src/user/lib/src/sys.rs`，提取所有 syscall 编号和名称
2. 以表格形式重建 `syscall.md`:
   - Syscall 编号 / 名称 / 功能描述 / 参数 / 返回值 / 实现状态
3. 标注哪些 syscall 已实现、哪些是 stub、哪些未实现
4. 将 Credo 私有 syscall（400+）单独列为一个子表

**验证**: 交叉检查用户态 `sys.rs` 中的 `SYS_*` 常量与文档一致。

**预估工时**: 2h

---

### Task B6: 重写 kernel-architecture.md 反映 Rust 代码结构

- **审计参考**: C82
- **严重程度**: 🟠 P1 High
- **委派标签**: `documentation` `architecture`

**文件**:
- `docs/architecture/kernel-architecture.md`

**当前问题**:
文档描述的目录结构是 C 语言项目（`main.c`、`process.c`、`pmm.c`），实际代码已全部转换为 Rust。新开发者按文档找代码完全失败。

**修复步骤**:
1. 以实际 `src/kernel/` 目录结构为基础重写模块组织章节
2. 更新所有文件名引用：`.c` → `.rs`
3. 更新架构图，反映实际的 Rust 模块层次
4. 添加 Rust crate 结构说明（`src/rust/` 是内核库，`src/kernel/` 是内嵌模块）
5. 添加 feature flag 说明

**预估工时**: 1.5h

---

### Task B7: 修复 WASM 沙箱边界检查遗漏

- **审计参考**: C67, C68
- **严重程度**: 🟠 P1 High
- **委派标签**: `wasm` `security` `sandbox`

**文件**:
- `src/kernel/wasm/interpreter.rs` — WASM 解释器

**当前问题**:
1. (C67) `i32.div_s(i32::MIN, -1)` 未 trap — 违反 WASM 规范
2. (C68) 内存边界检查 `addr + offset + size > mem_size` — 整数溢出回绕可通过检查

**修复步骤**:
1. 在 `i32.div_s` 和 `i64.div_s` 实现中，检查 `lhs == MIN && rhs == -1`，触发 trap
2. 内存边界检查改为：
   ```rust
   let end = addr.checked_add(offset as u32)
       .and_then(|v| v.checked_add(size))
       .ok_or(Trap::OutOfBounds)?;
   if end as usize > mem.len() { return Err(Trap::OutOfBounds); }
   ```
3. 对所有 `load`/`store` 操作码统一应用此检查

**验证**: 使用 WASM spec test suite 的 `binary.wast` 和 `memory.wast` 测试。

**预估工时**: 1.5h

---

### Task B8: 修复 CREDO 密码学基础问题

- **审计参考**: C28, C34, C36
- **严重程度**: 🟠 P1 High
- **委派标签**: `security` `credo` `crypto`

**文件**:
- `src/kernel/credo/storage.rs` — 密码哈希（C28: 无 salt）
- `src/kernel/credo/identity.rs` — Token 比对（C34: 时序攻击）
- `src/kernel/credo/types.rs` — 随机数生成（C36: 可预测）

**当前问题**:
1. 密码存储为纯 `SHA256(password)`，无 salt → 彩虹表攻击
2. Token 比对用 `==` 逐字节比较 → 时序侧信道
3. Token/Nonce 生成缺少 CSPRNG

**修复步骤**:
1. **加盐**: 修改 `storage.rs`，为每个用户生成 16 字节随机 salt，存储 `HMAC-SHA256(salt, password) || salt`
2. **固定时间比较**: 修改 `identity.rs` 的 token 比对，使用 constant-time comparison:
   ```rust
   fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
       if a.len() != b.len() { return false; }
       let mut diff = 0u8;
       for (x, y) in a.iter().zip(b.iter()) {
           diff |= x ^ y;
       }
       diff == 0
   }
   ```
3. **随机源**: 连接 RDRAND 指令（x86）或硬件随机源作为 CSPRNG 种子，构建基于 ChaCha20 的确定性随机生成器

**验证**: 单元测试 + 审查。

**预估工时**: 3h

---

## Batch C: P2 第三优先

---

### Task C1: 修复 Seqlock 相关问题

- **审计参考**: C47, C48
- **严重程度**: 🟡 P2 Medium
- **委派标签**: `sync` `seqlock`

**文件**: `src/kernel/sync/seqlock.rs`

**问题**:
- 写者无互斥（C47）：假设单写者但无机制保证
- 读者可能读到脏数据（C48）：两次 seq 读取之间缺少 compiler fence

**修复步骤**:
1. 在 `write_lock()` 中使用自旋锁或 CAS 保证单写者
2. 在读者读前和读后各添加 `compiler_fence(Ordering::Acquire)`

**预估工时**: 1h

---

### Task C2: 修复 RCU 多核支持

- **审计参考**: C49, C50
- **严重程度**: 🟡 P2 Medium
- **委派标签**: `sync` `rcu`

**文件**: `src/kernel/sync/rcu.rs`

**问题**: Grace period 检测依赖单核假设；`call_rcu()` 回调永不执行。

**修复步骤**:
1. 实现 per-CPU quiescent state 跟踪
2. 使用 IPI 通知其他 CPU 报告 quiescent state
3. 实现 `rcu_do_batch()` 在 grace period 结束后执行延迟回调

**预估工时**: 3h

---

### Task C3: 修复 VMM 页表操作缺陷

- **审计参考**: C3, C9, C13
- **严重程度**: 🟡 P2 Medium
- **委派标签**: `memory` `vmm` `tlb`

**文件**:
- `src/kernel/mm/vmm.rs` — 页表管理

**问题**:
- 修改页表后未 invlpg（C3）
- `phys_to_virt` 的 KERNEL_BASE 与链接脚本不一致（C9）
- 四级页表遍历不检查 P 位（C13）

**修复步骤**:
1. 每次页表条目修改后调用 `invlpg(addr)` — 注意 x86_64 和 aarch64 的实现差异
2. 统一 `KERNEL_BASE` 值（链接脚本、memory_allocator.rs、vmm.rs 三处一致）
3. 在 `vmm_walk()` 中逐级检查 Present 位

**预估工时**: 2h

---

### Task C4: 修复驱动超时和错误恢复

- **审计参考**: C58, C60, C61
- **严重程度**: 🟡 P2 Medium
- **委派标签**: `driver` `timeout` `error-recovery`

**文件**:
- `src/kernel/driver/storage/ata.rs` — ATA 无超时（C58）
- `src/kernel/driver/usb/xhci.rs` — xHCI 无错误恢复（C60）
- `src/kernel/driver/virtio/queue.rs` — 描述符耗尽无处理（C61）

**修复步骤**:
1. **ATA**: `while (status & BSY)` 添加超时计数器，超时后返回 `-ETIMEDOUT`
2. **xHCI**: 在传输错误后实现 ring dequeue pointer 复位流程
3. **VirtIO**: 描述符耗尽时返回错误或阻塞等待

**预估工时**: 3h

---

### Task C5: 修复 IPC 权限和并发安全

- **审计参考**: C77, C78, C79, C80
- **严重程度**: 🟡 P2 Medium
- **委派标签**: `ipc` `security`

**文件**:
- `src/kernel/ipc/shm.rs` — 物理地址暴露（C77）
- `src/kernel/ipc/mod.rs` — 全局表无锁（C78）
- `src/kernel/ipc/signal.rs` — 无权限检查（C79）
- `src/kernel/ipc/pipe.rs` — 缓冲区溢出（C80）

**修复步骤**:
1. 使用不透明的 `shm_id` 替代包含物理地址的结构
2. 为全局 IPC 表添加锁保护
3. 信号发送前检查发送者权限（UID 匹配或 root）
4. 管道写满时返回 `-EAGAIN` 或阻塞写入者

**预估工时**: 3h

---

## Batch D: 文档修复

---

### Task D1: 修复 README.md 失效链接

- **委派标签**: `documentation`

**文件**: `README.md`

**问题**: 引用 `docs/development/hivefs.md`、`docs/development/pwid-model.md`、`docs/development/klog-system.md` 等不存在的文件。

**修复**: 检查所有链接，修复或移除失效引用。

**预估工时**: 0.5h

---

### Task D2: 更新 boot-process.md

- **委派标签**: `documentation`

**文件**: `docs/architecture/boot-process.md`

**问题**: 描述的函数名 `antx_init()`、`kernel_main()` 已不存在，实际为 `kernel_init()`。

**修复**: 更新启动流程，对照 `src/rust/src/lib.rs` 中的 `kernel_init()` 和 `src/kernel/boot/` 目录。

**预估工时**: 1h

---

### Task D3: 更新 overview.md

- **委派标签**: `documentation`

**文件**: `docs/architecture/overview.md`

**问题**: 引用不存在的 `subsystems/filesystem/hvfs.md`；ARM 状态描述不准确。

**修复**: 移除失效引用，更新架构描述。

**预估工时**: 0.5h

---

### Task D4: 更新 test-framework.md

- **委派标签**: `documentation`

**文件**: `docs/testing/test-framework.md`

**问题**: 引用不存在的 `.github/` CI 配置和 `make coverage` 目标。

**修复**: 移除不存在的 CI 引用，更新测试命令。

**预估工时**: 0.5h

---

## Batch E: Clippy 清理

---

### Task E1: 限定 dead_code 抑制范围

- **审计参考**: Clippy A.1
- **严重程度**: 🟡 P1 High（隐藏了 2 个编译错误）
- **委派标签**: `clippy` `code-quality`

**文件**: `src/rust/src/lib.rs` L25

**当前问题**:
```rust
#![allow(dead_code)]  // 全局抑制，隐藏了 837 个警告
```

**修复步骤**:
1. 移除 `#![allow(dead_code)]` 全局行
2. 将 ~160 个硬件寄存器常量移到单独的 `registers/` 模块文件中：
   - `src/kernel/driver/registers/apic.rs`
   - `src/kernel/driver/registers/xhci.rs`
   - `src/kernel/driver/registers/e1000.rs`
   - 等等
3. 每个寄存器文件顶部加 `#![allow(dead_code)]` 限定作用域
4. 确保编译通过，无新增的假阳性死代码警告

**验证**: `cargo clippy --target x86_64-unknown-none -Z build-std=...` 死代码警告显著减少。

**预估工时**: 2h

---

### Task E2: 清理死 Feature Flags

- **审计参考**: Clippy A.4, Cargo 审计 #2
- **委派标签**: `build` `cargo`

**文件**: `src/rust/Cargo.toml`

**当前问题**: 19 个 feature flag 在 Rust 代码中零引用。

**修复步骤**:
1. 从 `Cargo.toml` 删除 19 个死 feature：
   - lwIP 系列（11 个）: `ipv6`, `dhcp`, `http_client`, `mdns`, `mqtt`, `sntp`, `smtp`, `tftp`, `snmp`, `netbios`, `lwiperf`
   - 子系统预留（8 个）: `net`, `alloc`, `async`, `lock_stats`, `debug_mutex`, `atomic_stats`, `log`, `json_export`
2. 如果 lwIP 系列在 C/Makefile 侧仍需要，在 `Cargo.toml` 添加注释说明
3. 如果子系统预留有实现计划，移到 `ROADMAP.md` 中跟踪，而非留空 feature

**验证**: `cargo check --all-features` 无变化。

**预估工时**: 0.5h

---

### Task E3: 修复 smp feature 编译错误

- **审计参考**: Clippy A.1.3
- **严重程度**: 🔴 P1 High（编译失败）
- **委派标签**: `bugfix` `smp`

**文件**: `src/kernel/mm/vmm.rs` L919-L920

**当前问题**:
```rust
if smp::is_smp_enabled() && smp::get_cpu_count() > 1 {  // ← 函数名错误
    smp::send_tlb_invalidate_ipi(addr);                   // ← 参数类型不匹配 (u64 vs u8)
}
```

**修复步骤**:
1. `smp::is_smp_enabled()` → `smp::is_enabled()`（函数重命名）
2. `smp::send_tlb_invalidate_ipi(addr)` → `smp::send_tlb_invalidate_ipi(addr.try_into().unwrap_or(0xFF))`

**验证**: `cargo check --target x86_64-unknown-none --features smp -Z build-std=...` 通过。

**预估工时**: 0.25h

---

### Task E4: 清理用户态死代码

- **审计参考**: Clippy A.2
- **委派标签**: `clippy` `userspace`

**文件**:
| 文件 | 死代码项 |
|------|---------|
| `src/user/lib/src/fs.rs:L4` | 未使用导入 `O_TRUNC` |
| `src/user/axsh/src/commands/system.rs` | 死函数 `sync()` |
| `src/user/axsh/src/commands/pipeline.rs` | 死变体 `StdinFile` |
| `src/user/axsh/src/commands/general.rs` | 死导入 `print_dec`, `print_hex` |
| `src/user/fbterm/src/main.rs` | 死方法 `clear_line()` |

**修复步骤**: 逐项删除或标记 `#[allow(dead_code)]` + TODO。

**预估工时**: 0.5h

---

---

## Batch F: 架构级优化（待排期）

---

### Task F1: 系统调用机制从 `int 0x80` 迁移到 `syscall`/`sysret`

- **来源**: 修复审阅反馈 (2026-06-09)
- **严重程度**: 🟡 P2 Medium（功能正确，性能优化）
- **委派标签**: `syscall` `x86_64` `performance`

**文件**:
- `src/kernel/syscall/types.rs` — `SYSCALL_INT` 常量定义
- `src/user/lib/src/sys.rs` — 用户态 `int 0x80` 内联汇编
- `src/kernel/idt/handlers.rs` — 中断门 handler
- `src/kernel/boot/` — 启动阶段 MSR 初始化

**当前问题**:

AntX 在 x86_64 长模式下使用 `int 0x80` 软中断作为系统调用机制。`int 0x80` 是 i386 时代的遗留路径，在 64-bit 长模式下存在以下问题：

1. **性能**: `int 0x80` 走完整中断门流程（IDT 查表 → 权限检查 → 栈切换 → 压栈 → `iret` 返回），每次约 150-250 cycles。`syscall`/`sysret` 走硬件加速路径（MSR 直接加载目标 RIP/CS），仅 60-80 cycles，快 **2-4 倍**
2. **业界惯例**: 无现代 x86_64 操作系统将 `int 0x80` 作为原生 64-bit 系统调用路径。Linux/macOS/Windows/FreeBSD 全部使用 `syscall`
3. **寄存器语义**: `int 0x80` 遵循 32-bit 时代的寄存器约定（`eax`/`ebx`/`ecx`/`edx`），在 64-bit 代码中语义混乱

**修复步骤**:

1. **内核侧 MSR 初始化**（在启动早期执行，约 `kernel_init()` 阶段）:
   ```rust
   // IA32_STAR: 高 32 位 = 内核 CS, 低 48:32 位 = 兼容模式用户 CS
   wrmsr(IA32_STAR, (KERNEL_CS as u64) << 32 | ((USER_CS32 - 16) as u64) << 48);
   // IA32_LSTAR: syscall 入口点 RIP
   wrmsr(IA32_LSTAR, syscall_entry as u64);
   // IA32_FMASK: 进入内核时自动清零的标志位（至少关 IF = bit 9）
   wrmsr(IA32_FMASK, 1 << 9);
   // IA32_EFER.SCE (bit 0): 启用 syscall 指令
   wrmsr(IA32_EFER, rdmsr(IA32_EFER) | 1);
   ```

2. **内核侧 syscall handler**（新建或改造现有 `int 0x80` handler）:
   ```nasm
   syscall_entry:
       swapgs              ; 切换 GS base 到内核
       mov [gs:0xNN], rsp  ; 保存用户栈指针
       mov rsp, [gs:0xMM]  ; 加载内核栈
       push rcx            ; 保存用户 RIP (syscall 存入 RCX)
       push r11            ; 保存用户 RFLAGS (syscall 存入 R11)
       ; ... 调用 Rust syscall dispatcher ...
       pop r11
       pop rcx
       mov rsp, [gs:0xNN]  ; 恢复用户栈
       swapgs
       sysretq
   ```
   注意：`syscall` **不自动切换栈**，需 per-CPU 内核栈（可用 GS 段或 TSS IST 实现）

3. **用户态侧**（`src/user/lib/src/sys.rs`）:
   ```rust
   // 将 int 0x80 替换为 syscall
   // 注意 syscall 破坏 RCX 和 R11（用于保存 RIP/RFLAGS）
   asm!("syscall",
       inout("rax") num => ret,  // rax = syscall number → return value
       in("rdi") a1, in("rsi") a2, in("rdx") a3,
       in("r10") a4, in("r8") a5, in("r9") a6,
       out("rcx") _, out("r11") _,
       options(nostack)
   );
   ```

4. **AArch64 兼容**: AArch64 已使用 `svc #0`（符合 ARM 惯例），无需改动

5. **向后兼容**: 可先保留 `int 0x80` handler 作为兼容路径，新增 `syscall` 作为主路径，待验证稳定后移除旧路径

**验证**:
```bash
# 编译验证
make clean && make ARCH=x86_64
# QEMU 启动验证：init 进程进入 Ring 3 后 axsh 正常交互
make run
# 性能对比：用 RDTSC 测量 syscall 往返延迟
```

**预估工时**: 4h

**与业界对比**:

| 操作系统 | 32-bit 机制 | 64-bit 机制 |
|----------|------------|------------|
| Linux | `int 0x80` → `sysenter` | `syscall` |
| macOS / XNU | — | `syscall` |
| Windows NT | `int 0x2E` → `sysenter` | `syscall` |
| FreeBSD | `int 0x80` | `syscall` |
| **AntX 当前** | — | `int 0x80` ⚠️ |
| **AntX 目标** | — | `syscall` ✅ |

---

## 附录: 委派建议

### 按技能领域分组

| 领域 | 任务 | 适合人员 |
|------|------|---------|
| 内存管理 | A5, B2, B3, C3 | 熟悉内核内存管理 |
| 文件系统 | A1, A2 | 熟悉存储/文件系统 |
| 网络 | A3 | 熟悉网络栈/DMA |
| 同步并发 | A4, B1, C1, C2 | 熟悉内存模型/锁 |
| 安全 | B7, B8, C5 | 熟悉密码学/沙箱 |
| 文档 | B5, B6, D1-D4 | 熟悉项目全局 |
| 构建/CI | A6, E2, E3 | 熟悉构建系统 |
| x86_64 底层 | **F1** | 熟悉 x86 MSR/GDT/TSS/syscall |
| 快速胜利 | E1, E3, E4 | 新人入门 |
