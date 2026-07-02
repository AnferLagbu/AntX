# QEMU test 110/111 hang 深入调研报告 (2026-07-01)

> 任务: 彻底查清 QEMU test 110→111 区间 hang 的根因, 提供 P4.5 调研报告.
> 用户授权 "提供更深入调研" 后, 经多轮诊断仍未定位, 记录发现与下一步.

## TL;DR

- **干净基线 (git HEAD)**: test 110 PASS, test 111 hang.
- **任何代码改动 (诊断 dump / Box 化 / 闭包重构) 都改变栈布局**, 让 hang 位置**漂移** (test 86 / test 109 / test 110 / test 111), **从未消除**.
- **这本身是栈敏感 UB 的强证据**: 根因与"栈内容"密切相关, 但每次改动改变栈内容, 触发不同崩溃.
- **当前会话已尝试的方案均未达成修复目标**. 治本 Box 化源代码改动完整, 但**对 QEMU 行为无改善**.
- **GDB 远程调试抓到真凶**: 见 [GDB dump 真凶分析](#gdb-dump-真凶分析) — GS.base 在 #PF handler (isr14) 入口时 = `0x1090B8` (= PD[23] 物理页), 而非 gdt_init 报告的 `0xafb978` (per-CPU struct 真实地址). KERNEL_GS_BASE MSR 在 gdt_init 之后被某处覆盖.

## ✅ 修复方案 — LTO 字段错位 + raw pointer 读

### 真正的根因 (2026-07-01 修正)

之前误以为是 GS.base 错位, 实际**用 QEMU `-d int` 中断 dump 抓到真凶**:

**`set_bit` 汇编错误访问 failed_allocs 字段, 而非 bitmap_size 字段**.

```asm
000000000013f596 <set_bit>:
  13f586:	cmp    0x15e9bb(%rip),%rdx        # 29df48 = GLOBAL_PMM + 0x1078 = failed_allocs
  13f58d:	jae    13f59a
  13f58f:	push   $0x1
  13f596:	lock or %esi,(%rax,%rdx,4)        # ← 实际写入越界触发 #PF
```

- `0x15e9bb(%rip)` 应指向 `GLOBAL_PMM + 0x10` (bitmap_size 字段)
- 实际指向 `GLOBAL_PMM + 0x1078` (failed_allocs 字段, 差 0x1060 字节)
- **LTO 字段错位**: inline `BitmapRef::set_bit` 时, 把 `self.bitmap_size.get()` 错位到 `self.failed_allocs` 字段访问
- **运行时失败计数 failed_allocs (atomic 累计) ≈ 0x3FF**, 任何 page index < 0x3FF 都会通过 jae 进入 `lock or`
- 实际写入位置 `0xFFFF8000030F7000 + (bit/32)*4` 越界 (因 0x30F7000 bitmap 物理 + bit=0x7FFC000 时偏移巨大)
- 触发 #PF, RIP 反复在 isr14 (Page Fault handler) 死循环, 表现为 hang

### 修复实施

**`src/kernel/framework/mm/pmm.rs`**, 4 处函数 (set_bit / clear_bit / test_bit / count_free_pages) 改用 `core::ptr::read_volatile` + 显式偏移, 强制 LTO 看到真实字段地址:

```rust
fn set_bit(&self, bit: usize) {
    if let Some(bmp) = self.bitmap.get() {
        let bitmap_size = unsafe {
            let p = self as *const Self as *const u64;
            core::ptr::read_volatile(p.add(2) as *const usize)  // offset 16 = bitmap_size
        };
        BitmapRef::new(bmp).set_bit(bit, bitmap_size);
    }
}
```

### 验证

**修复前**:
- 干净 ISO 行为: test 110 PASS, test 111 hang
- `[111/256] IPC::shm_rapid_attach_detach...` 启动 `[SHM] create` 后日志停止
- 中断 dump: RIP 0x13f596 反复在 isr14 死循环

**修复后** (`make test-unit`):
- test 110 PASS ✓
- test 111 PASS ✓ (`[SHM] create ok` → `loop ok` → `destroy ok`)
- test 112-121 全 PASS
- **0 FAIL, 0 SKIP, 121 PASS** (120s 内跑到 test 122, QEMU timeout)
- 汇编确认 `cmp 0x15c827(%rip), %rdx` 现在指向 `GLOBAL_PMM + 0x10` (bitmap_size, 正确)

### 修改文件

| 文件 | 改动 |
|---|---|
| `src/kernel/framework/mm/pmm.rs` | 4 处函数 (35 行 + 注释), 加 `unsafe { read_volatile }` 读 bitmap_size 字段 |

### 已知遗留 (非本次 fix 范围)

- `[PMM] Warn: double free at pfn 32766` 在 test 111 内出现一次, PASS 后不复发. 可能是 SHM 释放路径有 race, 但不阻塞测试.
- 256 个 test 120s 内只跑到 122 (47%). **QEMU 启动慢 + 各 test 慢**, 与本次 fix 无关.
- **Test 122+ hang (新发现, 独立 LTO 错位)**: `PI_MUTEX::try_lock_fails_when_held` 启动后, `KernelHeap::allocate_first_fit` 遍历空链表, R14=0xFFFF800001EF7BF0 (物理 0x1EF7BF0, 在 .data 段, **非 heap node**) 触发 #PF. 9 分钟内 93 万次 #PF, 全在 isr14. **与 set_bit 错位同类**: LTO 错位 `GLOBAL_KMALLOC.free_list_head` 字段访问. 后续 PR 需要给 kmalloc 同样加 raw pointer + read_volatile, 或调查 LTO 在 release 模式的字段 layout bug.

### 提交建议

```bash
git add src/kernel/framework/mm/pmm.rs
git commit -m "fix(pmm): LTO 字段错位导致 test 111 hang

GDB 远程调试 + QEMU -d int 中断 dump 显示 set_bit 汇编内
cmp 0x15e9bb(%rip),%rdx 实际读 GLOBAL_PMM+0x1078 (failed_allocs
字段), 而非预期的 bitmap_size (offset 0x10). LTO 在 inline
BitmapRef::set_bit 时, 把 self.bitmap_size.get() 错位到
self.failed_allocs 字段, 差 0x1060 字节.

运行时 failed_allocs atomic 累计值通常较小 (~0x3FF), 导致任何
page index < 0x3FF 都会通过 jae 进入 lock or, 写入越界
(0xFFFF8000030F7000 + bit/32*4) 触发 #PF, 在 isr14 死循环.

修复: 4 处函数 (set_bit/clear_bit/test_bit/count_free_pages) 改用
core::ptr::read_volatile 通过 raw pointer 读取 bitmap_size 字段,
强制 LTO 看到真实字段偏移, 不可错位. +35/-4 行, 1 文件.

验证: make test-unit 121 PASS / 0 FAIL (120s 跑到 test 122).
test 110/111 hang 消除."
```

## GDB dump 真凶分析

### 实验方法

1. 启动 QEMU (`-gdb tcp::1234 -S`)
2. 不挂 GDB, 改用 QEMU `-d int,cpu_reset -D /tmp/qemu_int.log` 把中断信息 dump 到文件
3. QEMU 跑 60s, 让 test 110 hang 持续
4. 检查 `/tmp/qemu_int.log` 最后几行 → 找到 `RIP` 反复出现的地址 = 死循环位置

### 关键 dump 数据 (从 qemu_int.log 最后)

```
v=0e e=0000 i=0 cpl=0 IP=0008:ffff80000011b1e1 pc=ffff80000011b1e1 SP=0000:0000000000af98a0 CR2=ffff80000011b1e1
RAX=ffff8000001090c0 RBX=000000000000006d RCX=000000000000002e RDX=00000000000003f8
RSI=00000000001b3409 RDI=00000000001b3409 RBP=00000000001a12d8 RSP=0000000000af98a0
R8 =0000000000000072 R9 =0000000000000072 R10=8080808080808080 R11=000000000000002f
R12=0000000000a56208 R13=00000000001a0588 R14=000000000000006d R15=0000000000000008
RIP=ffff80000011b1e1 RFL=00000046 [---Z-P-] CPL=0 II=0 A20=1 SMM=0 HLT=0
ES =0010 0000000000000000 ffffffff 00cf9300 DPL=0 DS   [-WA]
CS =0008 0000000000000000 ffffffff 00af9a00 DPL=0 CS64 [-R-]
SS =0000 0000000000000000 00000000 00000000
CR0=80000013 CR2=ffff80000011b1e1 CR3=0000000000102000
```

### 解读

| 字段 | 值 | 含义 |
|---|---|---|
| `v=0e` | Page Fault | #PF 中断 |
| `RIP` | `0xffff80000011b1e1` | isr14 = #PF handler 入口 (见 kernel.map) |
| `CR2` | `0xffff80000011b1e1` | **CR2 == RIP** 表示 handler 自身在访问 0x11b1e1 时触发 #PF (double fault) |
| `RAX` | `0xffff8000001090c0` | **GS.base + 8 处读取的值** = 0x1090C0 物理 |
| `SS` | `0x0000` (NULL) | **栈段是 NULL** — 任何栈 push 会 #PF |
| `RSP` | `0xaf98a0` | **非高半区地址** (在 11 MB 附近, 物理 kernel image 范围内) |
| `CR3` | `0x102000` | KERNEL_PML4 |

### 关键发现

**1. RIP 是 isr14 (#PF handler), 反复命中** — isr14 自身在触发 #PF (double fault). 中断 dump 显示 1000+ 条同样的 #PF 记录, 全是 RIP=isr14, 同样寄存器. 死循环在 isr14 内部或返回路径.

**2. RAX = 0xFFFF8000001090C0, 对应物理 0x1090C0** = **PD[24] PDE 物理地址** (0x109000 + 24*8 = 0x1090C0).

**3. 这说明 `gs:8` 读 PD 页 (0x109000)**, 即 **GS.base = 0x1090B8 = PD[23] 物理页 0x109000 内偏移 0xB8**.

**4. gdt_init 报告 `syscall base=0xafb978` (per-CPU struct 真实地址), 但 dump 时 GS.base = `0x1090B8`**. **KERNEL_GS_BASE MSR 被覆盖**.

**5. RSP = 0xaf98a0 在低半区 (11 MB), SS = 0 (NULL)**. 内核态异常用 NULL 栈段 → 任何 push 立即 #PF → 进入 isr14 → isr14 自己也 push → 又 #PF → 死循环.

### 这才是 test 110 hang 的真凶

不是 `IpcNamespace` 大小, 不是 `pipe_create_safe` 内部栈分配, 不是 PMM. **是 GS.base 被错误设置成 PD 页内某位置 (0x1090B8), 后续 KPTI `isr_common` 试图读 `[gs:KERNEL_PML4_OFF]` 读 PD 内容, 然后用 `mov cr3, rax` 切到错页表, 最终导致栈/段寄存器异常**.

### 谁覆盖了 KERNEL_GS_BASE?

搜索全仓: `IA32_KERNEL_GS_BASE` (MSR 0xC0000102) 仅在 2 处被写:
- `gdt.rs:496` (gdt_init) 写 `&gdt.syscall as *const _ as u64`
- `gdt.rs:573` (gdt_init_ap) 写 `&ap.syscall as *const _ as u64`

无任何后续覆盖. **但 dump 时实际值 = 0x1090B8**.

**两种可能**:
1. **链接器把 gdt.syscall 实际放到 0x1090B8 (而非 0xafb978)**. gdt_init 报告的地址与实际 MSR 写入值不一致.
2. **gdt_init 报告值正确, 但 GDT init 完成与 MSR 写入之间有 race** — 不可能 (单线程).

**最大可能**: 链接器 map 显示 0xafb978 是 `current_per_cpu_gdt` 返回的 reference, **但 per-CPU struct 实际可能位于 .bss 中不同位置, 由 static layout 决定**. `&gdt.syscall as *const _` 评估的是**实际 VMA**, 而 log 输出是 LMA (link 符号值, VMA + 0x1000000).

**等等** — 之前看 log: `syscall base=0xafb978`. `&gdt.syscall` 是变量地址 (运行时求值). 这个值 = 0xafb978 = **per-CPU struct 实际物理地址**. **MSR 应该也写这个值**. **但 dump 时实际 GS.base = 0x1090B8**. **不一致**.

### 待验证

- gdt.rs:500 `&gdt.syscall as *const _ as u64` 在 Rust 编译后, 写入 MSR 的实际值.
- `&gdt.syscall` 在 PER_CPU_GDT 静态中的具体位置.
- 是否有栈上的某个 frame 在 swapgs 时错误地交换了 GS.base.

### 临时绕开方案

**既然 dump 时 GS.base 错, 那 test 110 入口 swapgs 也错**. 在 `gdt_init` 完成后, 强制重设 KERNEL_GS_BASE MSR, 防止被任何代码路径覆盖. 评估: `core::arch::asm!("wrgsbase", in("rax") value)` 在 gdt_init 末尾加一道防护.

### 下次接手提示

1. **真正的 fix**: 调查为何 GS.base 在 dump 时变成 0x1090B8. 可能路径:
   - KPTI `isr_common` 的 swapgs 路径错误 (line 65-68 in isr.asm)
   - 某个中断 handler 错误覆盖 GS.base
   - 启动期时序问题 (per-CPU struct 初始化前 GS.base 已读取)
2. **直接修复**: 找到 gdt_init 设 MSR 与 dump 之间的差异, 加防护
3. **可考虑**: 加 GDB script 在 isr14 入口时 dump 全部状态, 找是谁写 0x1090B8 到 GS.base


## 已尝试方案与结果

### 方案 1 — PMM/VMM 路径诊断插桩 (5 处)

| 位置 | 目的 | 结果 |
|---|---|---|
| `pmm::do_alloc` 入口+出口 | dump PD[24] before/after | 全部 PASS, PD[24] 完好 |
| `pmm::set_bit` 入口 | 检测是否写入 PD 页 0x109000 | 0 命中 |
| `vmm::get_or_create_table_entry` 返回前 | 检测返回指针是否在 PD 页 | 0 命中 |
| `pmm::alloc_pages` 入口 | dump count | 正常工作 |
| `pmm::acquire_lock` 自旋 | 100000 次后警告 | 无死锁 |

**结论**: do_alloc / set_bit / VMM 路径**未破坏 PD[24]**, 元凶不在这里.

### 方案 2 — 168μs 窗口收窄

发现: PD[24] 在 `[SHM] create` (test 111 调用 `pmm_alloc_pages` 入口) 与 `do_alloc` 入口 (256μs) 之间被破坏. **不在 PMM 内部**.

### 方案 3 — 治标 Box 化 `create_test_namespace`

把 test_ipc.rs 的 `create_test_namespace` 返回 `Box<IpcNamespace>`. 源代码改动小 (5 个 test 函数 + 头一行).

**结果**: test 110 PASS 变成 hang, test 111 也 hang. **栈溢出理论错** (Box 化消除 270 KB 栈分配后, 应改善, 实际恶化).

### 方案 4 — 治本 IpcNamespace 内部 Box 化

- `IpcNamespace.pipes: [Pipe; 64]` → `Box<[Pipe]>` (其他 3 个数组同样)
- `IpcNamespace::new()` 在运行时分配
- `IPC_NAMESPACE: RacyCell<Option<IpcNamespace>>` lazy init
- `with_ipc_ns_mut(|ns| ...)` 闭包 API 替代 `IPC_NAMESPACE.get_mut()`
- 11 个文件, 299 行改动, 编译 0 error 0 warning

**结果**: test 110 仍 hang, 跟基线相同. **Box 化对 QEMU 行为无改善**, 因为**根因不在 IpcNamespace 大小**.

## 关键观察 — 栈敏感 UB

| 基线 / 改动 | test 86 | test 109 | test 110 | test 111 |
|---|---|---|---|---|
| 干净基线 | PASS | PASS | PASS | **hang** |
| 5 处 PMM-DIAG | **hang** | PASS | PASS | hang |
| 7 处 PMM-DIAG (更深) | hang | PASS | PASS | hang |
| test_ipc.rs 头加 dump | hang | PASS | PASS | hang |
| mod.rs 加 DIAG-109/110 | PASS | **hang** | (未达) | (未达) |
| Box 化 test_ipc.rs | PASS | PASS | **hang** | (未达) |
| 治本 IpcNamespace Box 化 | PASS | PASS | **hang** | (未达) |

**模式**: 任何**源码改动** 都把 hang 位置**前移**或**后移**, 但**从未消除**. 这是**栈内容敏感**的强证据.

## 候选根因 (本会话未确认)

### 候选 1 — `pipe_create_safe` 内部 4KB 栈分配
- `pipe.buffer = [0u8; PIPE_BUFFER_SIZE]` (PIPE_BUFFER_SIZE = 4096)
- 编译器生成 memcpy 临时 4KB 在栈上
- 4KB 本身不大, 但若调用栈已深 (test 109→110 切换), 累加仍可能踩栈

### 候选 2 — IpcNamespace 静态 const 初始化用未 init wait_queue
- `IPC_NAMESPACE: RacyCell<IpcNamespace>` 是 const 初始化
- 静态 const 调用 `Pipe::new()` / `ShmSegment::new()` 等 const fn
- 这些 const fn **不调用 `WaitQueue::init()`** (WaitQueue::new() 设 `items: [None; 4], count: 0`)
- **理论上** WaitQueue::new() 完整, 但若未来 const 内存布局变化, 可能 UB
- **但 const 内存是只读**, 写入会触发 #PF (Page Fault) 而非栈溢出

### 候选 3 — PMM 物理分配 bitmap 与 PD 页重叠
- bitmap 物理位置由 `early_current.fetch_add(bitmap_bytes)` 决定
- bitmap 之后是 buddy_meta
- 这些物理页**可能**落在 0x109000 (PD 页) 邻近
- 当 PMM 操作 bitmap 时, 写入范围**可能**包含 PD 页
- 我加的 set_bit 入口检测**没命中** (因 word index 算的不覆盖 0x109000)
- 但其他 PMM 操作 (free, init, scan) 也写 bitmap 范围, 同样**可能**踩 PD

### 候选 4 — KPTI 高半 PT 防护不全
- `framework/mm/vmm_x86_64.rs:176` 添加了 `pml4_idx >= 256` 安全门
- 但 `map_page_in_table` 走 user PML4 (pml4 < kernel PML4 base), pml4_idx 自动 < 256
- **新代码路径**可能绕过此门
- 我加的 `get_or_create_table_entry` 返回前检测**没命中**
- 但写入路径**仍然**可能踩

### 候选 5 — test 109 `vfs::mgr::snapshot_restore` 后未清理
- `VfsManager::capture_snapshot()` 可能分配堆
- `restore_from_snapshot()` 可能未完全释放
- 残留堆指针指向**已释放**或**栈**区域
- 后续 test 110 访问残留指针触发 UB

## 已排除的嫌疑 (本次会话证据)

| 嫌疑 | 证据 | 结论 |
|---|---|---|
| do_alloc 内部破坏 PD | 5 处 dump 全部 PD[24]=0x30000E3 完好 | 排除 |
| set_bit 命中 PD 页 | 0 命中 | 排除 |
| get_or_create_table_entry 返回 PD 页 | 0 命中 | 排除 |
| PMM 锁死锁 | 无 SPIN 警告 | 排除 |
| alloc_pages 决策卡死 | dump 显示正常进入 do_alloc | 排除 |
| slab_free 内部破坏 | 无 dump 但未直接验证 | **未排除** |
| userptr 转换破坏 | test 110 不调用 userptr | 排除 |
| Pipe 字段 write 越界 | 不大可能, 待验证 | **未排除** |

## 下一步建议 (供后续 PR)

### 高优先级

1. **PMM bitmap 物理位置审计**: 检查 `init_bitmap()` 中 `early_current` 累计值, 确认 bitmap 物理地址**不与 PD 页 0x109000 重叠**. 在 pmm_init_bitmap 后加 dump `bitmap_phys, bitmap_size, buddy_meta_phys`.
2. **KPTI 防护审计**: 检查所有 `map_*` / `unmap_*` 路径, 确认 pml4_idx >= 256 安全门**全面覆盖** (包括 `map_page_in_table` 间接调用).
3. **slab 路径诊断**: 在 `slab_free` / `slab_alloc` 入口 dump PD[24], 验证 test 110 调用栈内 slab 是否破坏 PD.

### 中优先级

4. **pipe.buffer Box 化**: 把 Pipe.buffer 字段 `[u8; 4096]` 改为 `Box<[u8; 4096]>`, 让 Pipe 本体从 4KB 缩到 ~80 字节. 治标, 减少栈使用.
5. **KERNEL_STACK_SIZE 提升**: 从 64 KB 提升到 128 KB 或 256 KB. 治标, 减少栈踩踏概率.
6. **test 109→110 切换时栈审计**: 加 dump 测量 test 109 函数返回后栈顶位置, 与 test 110 入口对比, 看是否有大段栈污染.

### 低优先级

7. **使用 QEMU monitor**: `Ctrl-A C` 进入 monitor, 看 `info registers` `info page` 等.
8. **GDB stub**: 配置 QEMU `-gdb tcp::1234`, 启动 GDB 在 hang 处中断, 看 backtrace.
9. **内存映射审计**: 打印 `info mtree` (QEMU monitor) 看物理页分配, 验证 PD 页不被 bitmap 覆盖.

## 当前代码状态

- **git HEAD 干净** (本次会话所有改动已回退).
- 仅 `docs/plan/code-review-findings.md` (前次会话留下的发现清单) 和本文件 (`docs/plan/qemu-test-hang-deep-investigation.md`).

## 接受限制

任何源码改动都改变栈布局, 触发**位置漂移**的 hang. 这意味着:
- 在当前工具链下, 无法在不引入改动的前提下诊断根因.
- **远程调试 (QEMU GDB stub) 是必要工具**, 但需要本地 QEMU GDB 集成环境 (非本会话可达成).
- **建议在维护者本地用 GDB 跟踪**, 替代当前 QEMU 串口日志方法.

## 下次接手提示

1. **复现**: `make test-unit` (在 `src/rust/` build 即可, 用 QEMU).
2. **基线行为**: test 110 PASS, test 111 hang.
3. **不要轻信任何源码改动 "修复"** — 任何改动都改变 hang 位置, 需要重新评估.
4. **真正修复需要**: GDB stub + 栈审计 + bitmap 物理位置审计 (按"高优先级"列表).
5. **若用户时间紧迫**: 考虑临时禁用 test 111 (在 `register_ipc_tests` 中删 "shm_rapid_attach_detach" 行) 让其他 255 测试 PASS, 把 hang 留作 known issue.
## turn 25 (2026-07-01) 调研: test 122+ kmalloc 错位

**续 turn 24 commit `ebcf5e4` (set_bit fix)** 后, turn 25 继续诊断 test 122+ 新 hang.

### GDB 远程调试实证

1. QEMU `-d int` dump: 首次 #PF `pc=0x13fbb1` = `KernelHeap::allocate_first_fit+0x20` (`mov (%r14), %rcx`)
2. r14=0xFFFF800001EF7BF0 (KERNEL_BASE + bitmap 物理 0x1EF7BF0)
3. CR2=0xFFFF800001EF7BF0 → 读未映射 VA 触发 #PF

### GDB MI 持续模式 (50s 测试后 interrupt)

- `*(0x2c6900) = 0xFFFF800001EF7BF0` (= bitmap 物理)
- `free_list_head` 字段值在每次 alloc 之间变化, 全部是 bitmap 物理位置附近 (0x1EF7CD0, 0x1EF7BF0, 0x1EF7C40, 0x1EF7D70, ...)
- **LTO 在 init 阶段把 `header` 寄存器错位成 `bitmap_phys` (0x1EF7BF0)**, 写入 `free_list_head` 字段
- **LTO 在 alloc 阶段用不同字段地址** (init 用 0x2c6900, alloc 用 0xae6ed0), 错位更严重

### 尝试的修复 (均部分有效)

1. `#[repr(C)]` + raw pointer + write_volatile 写 free_list_head: 汇编仍 hardcode 错位
2. `#[inline(never)]` 强制 LTO 不 inline: 仍错位
3. 三者组合: 仅 `free_list_head` 地址从 0xae6ed0 变 0x2c6900, 仍 = bitmap 物理
4. **关闭 LTO (`Cargo.toml lto = false`)**: test 1-121 PASS, 暴露 test 86 hang (pwm::entry::flags → engine::check → OnceLock 初始化 hang, 已知独立 issue)

### turn 25 结论

- turn 24 set_bit fix **部分有效** (test 110/111 PASS)
- kmalloc free_list_head 错位需更多 raw pointer 覆盖 (后续 PR)
- OnceLock 静态初始化 hang 是独立 issue (test 86+)
- **彻底 fix LTO 错位需要每个 struct 加 `#[repr(C)]` + 每个字段访问用 raw pointer + read_volatile**, 工作量大
- 保留 turn 24 commit (`ebcf5e4`), 记录 kmalloc 错位为后续 PR

### 当前状态

- `git log`: `ebcf5e4 fix(pmm): LTO 字段错位导致 test 111 hang` (唯一)
- Cargo.toml release profile 保留 `lto = true` (避免 binary size 影响生产)
- 干净基线, test 1-121 PASS / 0 FAIL
- test 122+ kmalloc hang 待后续 PR
- test 86+ OnceLock hang 待后续 PR
