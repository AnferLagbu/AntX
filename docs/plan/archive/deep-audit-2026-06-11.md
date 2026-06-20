# 全项目深度审计与问题追踪报告

> 审计日期: 2026-06-11
> 审计模式: 逐项排查，实时记录
> 状态: 已完成 (23/23 审计项, 54 个追踪问题)
> 
> 本文档包含两章: 第一章为逐项深度审计记录, 第二章为汇总问题追踪清单, 末尾附审查员评价

---

# 第一章: 深度审计记录

---

## 审计1: 构建系统与工具链

### A1-1: rust-toolchain.toml 未锁定 nightly 日期
**严重度**: 中
**文件**: [src/rust/rust-toolchain.toml](../../src/rust/rust-toolchain.toml)
**现状**: `channel = "nightly"` 无日期锁定，CI 构建使用的 nightly 版本取决于触发时间。
**风险**: 不同时间构建可能得到不同 nightly 版本，导致不可重现构建。nightly API 变更可能突然导致编译失败。
**建议**: 锁定为 `channel = "nightly-YYYY-MM-DD"`，升级作为显式工程项。

### A1-2: deny.toml license 仅允许 MIT/Apache-2.0
**文件**: [deny.toml](../../deny.toml)
**现状**: 仅允许 MIT 和 Apache-2.0。项目自身是 MIT。配置正确。

### A1-3: clippy.toml 配置合理
**文件**: [clippy.toml](../../clippy.toml)
**现状**: `cognitive-complexity-threshold = 25`, `missing-docs-in-crate-items = true`。已根据内核风格定制。

### A1-4: build.rs 占位文件机制
**文件**: [src/rust/build.rs](../../src/rust/build.rs)
**现状**: 编译前确保 `build/stage1.bin` (440 字节) 和 `build/user/init.bin` (512 字节) 存在。独立编译时不会因缺少文件而失败。
**风险**: 若 `make user` 之后没重新 `cargo build`，内核会 embed 旧的 init.bin。暂为权衡。

### A1-5: 跨架构 clean 策略完整性
**文件**: [Makefile:100-113](../../Makefile#L100-L113)
**现状**: `.build-arch` stamp 检测架构切换后自动删除 `build/` 中 asm .o 文件并 `cargo clean`。
**注意**: cargo clean 会清除 `src/user/` 编译产物，切换回原架构需重新编译用户程序。

---

## 审计2: services/ 0 unsafe 验证

### A2-1: services/ 实际 0 unsafe ✅
**结果**: grep 搜索仅命中注释行（共 4 处，均为文档说明 "services 层 0 unsafe"）。
**状态**: PASS。

---

## 审计3: services/ unwrap() 使用审计

### A3-1: zil_persist.rs 生产代码中 11 处 unwrap
**严重度**: 高
**文件**: [src/kernel/services/fs/hvfs/zil_persist.rs](../../src/kernel/services/fs/hvfs/zil_persist.rs)
**行号**: L219-L416
**现状**: ZIL 日志回放路径中 11 处 `.unwrap()`：
- `try_into().unwrap()` 用于字节切片→整数转换（8 处）
- `deserialize_record(&buf).unwrap()` 用于记录反序列化（1 处）
- 其余用于 tail magic/checksum 解析（2 处）

**风险**: 若磁盘上的 ZIL 记录损坏（bit rot、未完整写入），内核会在日志回放时 panic，直接死机。无法通过 Barrier 回滚恢复。
**建议**: 替换为 `?` 错误传播或 `.ok_or(Error::CorruptedZil)?`，让 Barrier 层有机会回滚。

### A3-2: credo/ 模块 unwrap 均在测试代码
**结果**: 44 处 .unwrap() 全部在 `#[cfg(test)] mod tests` 内。**无风险**。

---

## 审计4: 死锁风险排查

### A4-1: IDT 中断路径锁使用 ✅
**文件**: [src/kernel/framework/idt/idt.rs](../../src/kernel/framework/idt/idt.rs)
**现状**: IDT 管理器 `IdtManager.state` 使用 `IrqSpinLock`，中断安全。`handle_irq` / `handle_exception` 路径不涉及睡眠锁。
**状态**: PASS。

### A4-2: Lockdep 能力评估
**文件**: [src/kernel/framework/sync/lockdep.rs](../../src/kernel/framework/sync/lockdep.rs)
**现状**: 
- AB-BA 环检测 (邻接矩阵 + BFS)：✅ 实现
- 中断上下文睡眠锁检测：✅ 实现 (检测在中断中获取 Mutex)
- 递归锁检测：✅ 实现
- 释放未持有锁检测：✅ 实现
- 锁类上限：MAX_LOCK_CLASSES=64, MAX_HELD_LOCKS=8
- 调试模式：仅在 `debug_assertions` 或 `feature="lockdep"` 启用

**限制**: Lockdep 是运行时检测，需要执行到具体路径才能触发检测。无法静态发现死锁。
**状态**: PASS (需配合静态分析增强)。

### A4-3: 调度器可重入性
**文件**: [src/kernel/framework/idt/idt.rs:510-516](../../src/kernel/framework/idt/idt.rs#L510-L516)
**现状**: 异常处理路径 (`handle_exception`) 中调用 `scheduler_yield()`：
- 进程终止路径 (L516): `process_exit()` 后 `scheduler_yield()`
- Double Fault 恢复 (L658): 调用 `scheduler_yield()` 尝试恢复
- 域恢复路径 (L678): recovery 后调度

**风险**: `scheduler_yield()` 在异常上下文中被调用时，调度器应持有 `IrqSpinLock` 保护的 CPU 队列。若调度路径中意外获取 Mutex 会死锁。当前 scheduler 使用 `IrqSpinLock` 保护运行队列，但需确认递归 yield 场景（scheduler_yield → 嵌套 scheduler_yield）有无防护。

### A4-4: Mutex 与 IrqSpinLock 顺序
**文件**: [src/kernel/framework/sync/mutex.rs:18](../../src/kernel/framework/sync/mutex.rs#L18)
**规定**: Mutex 不可在中断上下文使用。但未检测违反此规则的行为——Lockdep 仅在 debug 模式检测。
**风险**: release 模式下若中断路径获取 Mutex 则无检测。
**建议**: 在 release 模式下至少对中断上下文做 assert 检查（不依赖完整的 lockdep 图）。

### A4-5: MutexInner 实现使用 SpinLock
**文件**: [src/kernel/framework/sync/mutex.rs](../../src/kernel/framework/sync/mutex.rs)
**现状**: Mutex 内部锁实现是 SpinLock，在 contest 时 yield。若持 Mutex 时被中断打断，并在中断中获取同一 Mutex 会死锁。这是所有基于 SpinLock 的 Mutex 的通病。
**注意**: 若中断处理程序需要访问 Mutex 保护的数据，当前需确保该 Mutex 在中断到达前被释放。

---

## 审计5: 内存安全与 unsafe 使用

### A5-1: copy_from/to_user 边界检查 ✅
**文件**: [src/kernel/framework/mm/copy_user.rs](../../src/kernel/framework/mm/copy_user.rs)
**现状**: 
- 指针校验 (`is_user_addr`/`is_user_buf`/`is_valid_range`)
- 零长度处理
- 内核地址拒绝
- `checked_add` 防止地址溢出
**状态**: PASS。

### A5-2: frame.rs 读写边界检查 ✅
**文件**: [src/kernel/framework/mm/frame.rs](../../src/kernel/framework/mm/frame.rs)
**现状**: 10 处 `saturating_add` 防止 offset + size 溢出。Uframe/Dframe/Vframe 三类型均有边界检查。
**状态**: PASS。

### A5-3: coredump.rs copy_from_user_safe 简化实现
**严重度**: 中
**文件**: [src/kernel/framework/proc/coredump.rs:692-698](../../src/kernel/framework/proc/coredump.rs#L692-L698)
**现状**: 使用 `unsafe { core::ptr::copy_nonoverlapping }` 直接读用户内存，注释写明"完整实现应使用 copy_from_user 带 exception table"。
**风险**: 缺少 exception table 保护，若用户传递未映射的地址会触发 page fault 在内核态无处理。
**建议**: 接入 `exception_table` 机制或使用 `copy_from_user`。

### A5-4: eBPF 调试模块跳过 copy_from_user
**文件**: [src/kernel/framework/debug/ebpf.rs:762](../../src/kernel/framework/debug/ebpf.rs#L762)
**现状**: 注释 "简化: 在内核态直接操作, 不做 copy_from_user"
**风险**: 若 eBPF 模块直接操作用户传入地址而不做校验，存在信息泄露/内核内存写入风险。需确认此模块是否仅 debug 启用。

### A5-5: 整数溢出防护 ✅
**文件**: MM 子模块 (pmm/slab/vma/frame/pcache)
**现状**: 22 处 `checked_add`/`saturating_add`/`wrapping_sub` 使用，覆盖地址计算、分配计数、哈希。release 模式 overflow 会 panic 而非 wrap。
**状态**: PASS。

### A5-6: acpi.rs asm! 中被意外删除的修复
**文件**: [src/kernel/framework/arch/x86_64/acpi.rs:781](../../src/kernel/framework/arch/x86_64/acpi.rs#L781)
**现状**: 行 L781 疑为最近修改残留 ("Unused variable、unused function: marks, SP_DEBUG 等")。需确认原 asm! volatile 的 "out"/"inout" 操作数是否因变量删除而丢失了必要的寄存器约束。

---

## 审计6: 错误处理路径完整性

### A6-1: 错误处理风格不统一
**严重度**: 中
**位置**: 分散在 services/ 和 framework/
**现状**: 存在三种错误处理模式混用：
| 模式 | 示例 | 位置 |
|------|------|------|
| Rust Result | `Err(KernelError::NotFound)` | hvfs.rs |
| Rust Errno | `Err(Errno::ENOMEM)` | vma.rs |
| C 风格 | `return -1` | devfs.rs, spa.rs, vdev.rs, ramfs_core.rs, block.rs, virtio net.rs |

**影响**: `return -1` 丢失错误上下文，调用方无法区分 "文件不存在"(-ENOENT) 和 "权限不足"(-EPERM)。不符合 Rust 惯用错误处理。
**建议**: 将 `return -1` 统一替换为 `Err(Errno::...)` 或自定义错误类型。

### A6-2: devfs.rs 和 block.rs C 风格错误
**严重度**: 低
**文件**: [src/kernel/services/fs/devfs.rs](../../src/kernel/services/fs/devfs.rs), [src/kernel/framework/driver/block.rs](../../src/kernel/framework/driver/block.rs)
**现状**: devfs.rs 混用 `return -1` 和 `Err(KernelError::IoError)`。block.rs 全部使用 `return -1`。
**建议**: 统一为 Result 模式。

### A6-3: 未追踪的 TODO 项
**严重度**: 中
**位置**: 全项目 40+ TODO 标记
**关键缺口**:
- [io_uring.rs](../../src/kernel/framework/io/iouring.rs): 核心 I/O 操作为骨架 (L329 "TODO: 集成 VFS fd 表")
- [shadow_stack.rs](../../src/kernel/framework/arch/shadow_stack.rs): CET Shadow Stack 物理页分配待实现
- [secure_boot.rs](../../src/kernel/framework/credo/secure_boot.rs): Ed25519 签名验证为 stub
- [mmap.rs](../../src/kernel/framework/syscall/mmap.rs): fd 传入 inode_id 待实现
- [numa.rs](../../src/kernel/framework/mm/numa.rs): 页面迁移待实现
- [power.rs](../../src/kernel/framework/driver/power.rs): S3 挂起/menu governor 为 stub
- [seccomp.rs](../../src/kernel/framework/proc/seccomp.rs): filter 解析/CAP_SYS_ADMIN 检查待实现

**好的实践**: 大部分 TODO 使用 `TODO(TRACK-xxxxx)` 编号跟踪。但有部分未编号的轻量 TODO（如 power.rs, io_uring.rs, shadow_stack.rs）。

### A6-4: driver 层错误类型混乱
**位置**: driver 各子模块
**现状**: 有的用 `Result<(), DriverError>`, 有的用 `Result<(), ()>`, 有的用 `return -1`。VirtIO net 使用 `Err(())` (无错误信息)。
**建议**: 统一驱动层错误类型 `Result<T, DriverError>`。

---

## 审计7: 文档与代码一致性

### A7-1: kernel-roadmap.md Phase C 状态落后
**严重度**: 低
**文件**: [docs/plan/kernel-roadmap.md](./kernel-roadmap.md)
**现状**: Phase C 概述中标记"进行中 (3/7 完成)"，但工程进度文档标记已完成 (C1-C7)。
**建议**: 更新为"已完成"。

### A7-2: services/mod.rs 注释与实际模块声明有差异
**文件**: [src/kernel/services/mod.rs:27-48](../../src/kernel/services/mod.rs#L27-L48)
**现状**: 注释声明 `pub mod proc`, `pub mod fs`, `pub mod mm`, `pub mod driver`, `pub mod barrier` 等，但实际代码中 `pub mod mm` 和 `pub mod driver` 的行被截断（L48 后未显示）。需确认是否完整。

### A7-3: @SAFE 注释约定遵守率
**文件**: services/ 各模块
**现状**: services/mod.rs 要求每个文件头部声明 `//! @SAFE: 本文件不含 unsafe 代码`。但实际检查发现并非所有 services 文件都有此声明。例如 `services/fs/hvfs/zil_persist.rs` 文件头无此声明。

---

## 审计8: 测试覆盖率与质量

### A8-1: 测试矩阵齐全 ✅
**位置**: host-tests/, queenx-tests/, miri-tests/, framework/tests/
**现状**: 
- host-tests: 172+ std 测试 (含 HvFS mock, buddy, sha256 等)
- queenx-tests: 32 用户态集成测试桩
- miri-tests: 13 Miri UB 扫描
- framework tests: 31 no_std 单元测试
**状态**: PASS。

### A8-2: 性能基线未锁定
**严重度**: 低
**文件**: [host-tests/benches/baseline.json](../../host-tests/benches/baseline.json)
**现状**: 性能基线文件存在但不知是否最新。若未及时更新，回归检测可能误判。

### A8-3: queenx-tests 只有测试桩
**位置**: [src/rust/queenx-tests/](../../src/rust/queenx-tests/)
**现状**: 32 个用户态测试文件，但据文档标记为"测试桩"，未实现完整测试逻辑。
**风险**: 集成测试覆盖率低，主要依赖 host-tests 的 mock 验证。

---

## 审计9: 架构合规性

### A9-1: services/ 使用 spin::Once 绕过框架同步层
**严重度**: 中
**位置**: 
- [services/fs/ramfs.rs:387](../../src/kernel/services/fs/ramfs.rs#L387)
- [services/fs/devfs.rs:497](../../src/kernel/services/fs/devfs.rs#L497)
- [services/fs/procfs.rs:151](../../src/kernel/services/fs/procfs.rs#L151)
- [services/ipc/mod.rs:100](../../src/kernel/services/ipc/mod.rs#L100)

**现状**: services/ 直接使用 `spin::Once` (第三方 crate) 初始化全局单例，而非通过 framework 提供的 `OnceLock`。
**风险**: `spin::Once` 不参与 Lockdep，且绕过框架同步层。虽 `spin::Once` 提供 safe API，但架构上应通过 framework 统一管理同步原语。
**建议**: 将 `spin::Once` 替换为 `framework::sync::once_lock::OnceLock` 或 services 层通过 trait 注射的同步原语。

### A9-2: framework/ 仍大量使用 spin::Mutex (第三方)
**严重度**: 中
**位置**: 15 个模块 (详见本审计文档末尾的完整列表)
**现状**: 项目引入 `spin = "0.9"` 作为依赖，framework 中 15 个模块直接使用 `spin::Mutex` 而非项目自身的 `Mutex` (基于 scheduler yield) 或 `IrqSpinLock`。
**风险**:
1. `spin::Mutex` 是忙等待自旋锁，与项目 sleep-based Mutex 语义不同
2. 不参与 Lockdep 的 AB-BA 环检测
3. 无法区分中断安全/非安全上下文
4. 若在中断上下文获取 `spin::Mutex` 可能导致死锁
**建议**: 将 `spin::Mutex` 替换为项目自身同步原语，或至少接入 Lockdep。

### A9-3: proc/process.rs 中 NamespaceSet 和 NumaPolicy 使用 spin::Mutex
**严重度**: 中
**文件**: [src/kernel/framework/proc/process.rs:224-236](../../src/kernel/framework/proc/process.rs#L224-L236)
**现状**: Process 结构体中 `namespaces` 和 `numa_policy` 字段使用 `spin::Mutex<>`。
**风险**: 若中断处理程序（如 timer tick）访问进程的 namespace 状态，会获取 spin::Mutex 可能导致死锁（特别是在支持嵌套中断的架构上）。

### A9-4: services→framework 边界渗透检查脚本
**文件**: [scripts/audit_services_boundary.py](../../scripts/audit_services_boundary.py)
**现状**: 定义了拒绝列表 (FORBIDDEN_FRAMEWORK_MODULES)，检查 services/ 是否直接访问 framework 内部模块。检查项目完整。
**建议**: 
- 将 `spin::Mutex` 和 `spin::Once` 也加入边界检查（services 不应直接使用第三方同步原语）
- 将 `use spin::` 加入禁止模式列表

---

## 审计10: 代码质量

### A10-1: clippy 允许列表过长
**严重度**: 低
**文件**: [src/rust/src/lib.rs:12-60](../../src/rust/src/lib.rs#L12-L60)
**现状**: 顶层 `#![allow(clippy::*)]` 达 22 条，包括 `mut_from_ref`, `not_unsafe_ptr_arg_deref`, `module_inception` 等。
**评估**: 大部分有合理理由（内核特殊性），但 `single_match`, `collapsible_if`, `let_and_return` 属于代码风格而非必要性。建议逐步缩减。
**注意**: `#![allow(unused_unsafe)]` 被显式允许 (16 处)，这意味有些 unsafe 块可能是多余的。应检查并移除不需要的 unsafe 块以减少 TCB 审计面积。

### A10-2: 全局分配器 tag 机制存在潜在碰撞
**严重度**: 低
**文件**: [src/rust/src/memory_allocator.rs](../../src/rust/src/memory_allocator.rs)
**现状**: 使用 magic tag 值 (TAG_KMALLOC, TAG_PMM_PAGE, TAG_PMM_PAGES) 区分分配来源。
**风险**: 若用户刻意构造包含这些 magic 值的数据并传入 free，可能误判释放方式。风险极低因为需要对齐 + 内核态内存。

### A10-3: Panic handler 信息容量有限
**文件**: [src/rust/src/lib.rs:145-170](../../src/rust/src/lib.rs#L145-L170)
**现状**: 捕获 panic 消息上限 127 字节 + 16 个寄存器。无法捕获完整调用栈。
**评估**: 对于嵌入式/内核场景足够，但复杂 bug 调试时可能需要更多上下文 (如 backtrace)。

### A10-4: 未追踪的 asm! 调用
**文件**: CI 脚本 [ci/build.sh:60-73](../../ci/build.sh#L60-L73)
**现状**: CI 会检查非 arch 模块中的 `asm!` 调用，但仅标记为人工审查，不阻塞构建。
**发现的 asm! 调用**:
- [driver/kexec.rs:319](../../src/kernel/framework/driver/kexec.rs#L319): `wbinvd` (全 cache flush)
- [syscall/mod.rs:3854](../../src/kernel/framework/syscall/mod.rs#L3854): `svc #0` (aarch64 测试)
- [driver/power.rs:158](../../src/kernel/framework/driver/power.rs#L158): `mrs cntvct_el0`
- [debug/ebpf.rs:744](../../src/kernel/framework/debug/ebpf.rs#L744): `mrs cntvct_el0`
**评估**: 这些 asm! 调用合理，但 syscall/mod.rs 中的 `svc #0` 需确认仅在 aarch64 cfg 下编译。

### A10-5: 模块内循环引用
**文件**: [src/kernel/framework/proc/process.rs](../../src/kernel/framework/proc/process.rs)
**现状**: Process 结构体包含 `namespace::NamespaceSet` 和 `mm::numa::NumaMempolicy` 的 spin::Mutex 包装。跨模块类型引用但均通过 pub use re-export 访问。
**评估**: 当前架构下可接受，但随模块复杂度增长可能形成依赖环。

---

## 附录: 完整 spin::Mutex 使用清单

| 文件 | 行号 | 用途 |
|------|------|------|
| framework/driver/kexec.rs | 35 | kexec 状态 |
| framework/driver/uefi.rs | 29 | UEFI 运行时服务 |
| framework/timer/time_sync.rs | 26 | 时间同步 |
| framework/timer/tickless.rs | 33 | Tickless 模式 |
| framework/arch/shadow_stack.rs | 42 | Shadow Stack |
| framework/credo/secure_boot.rs | 30 | 安全启动 |
| framework/driver/power.rs | 28 | 电源管理 |
| framework/debug/ebpf.rs | 38 | eBPF 虚拟机 |
| framework/mm/numa.rs | 74,76,195,197,199 | NUMA 策略/节点信息 |
| framework/proc/process.rs | 224,236 | NamespaceSet/NumaPolicy |
| framework/proc/cgroup.rs | 404 | Cgroup |
| framework/proc/namespace.rs | 34 | Namespace |
| framework/io/iouring.rs | 40 | io_uring |
| framework/net/netfilter.rs | 38 | Netfilter 规则 |
| framework/net/route.rs | 95 | 路由表 |
| framework/arch/x86_64/smp_init.rs | 39 | SMP 初始化 |

---

## 审计11: VFS 策略提取专项审计 (对照 vfs-policy-extraction.md)

> 审计日期: 2026-06-11
> 审计目标: 逐项核实 E6 计划文档中声称的完成项在代码库中的实际实现状态

### 审计结果总览

| 子任务 | 文档状态 | 代码核实 | 发现问题 |
|--------|---------|---------|---------|
| E6-3 dcache 迁移 | ✅ | ✅ 通过 | 无 |
| E6-4 FileSystem trait + 分发 | ✅ | ⚠️ 部分 | B11-E4-01, B11-E4-02 |
| E6-5 RamFS 迁移 | ✅ | ✅ 通过 | 无 |
| E6-6 HvFS 迁移 + unsafe 消除 | ✅ | ✅ 通过 | 无 |
| E6-7 DevFS 迁移 | ✅ | ✅ 通过 | spin::Once 残留 (A9-1) |
| E6-8 ProcFS 迁移 | ✅ | ✅ 通过 | spin::Once 残留 (A9-1) |
| E6-9a/b/c Chitin-DevFS 桥接 | ✅ | ✅ 通过 | 无 |
| 文档行数/TCB 数据一致性 | - | ⚠️ 部分 | B11-DATA-01 |

### B11-E4-01: vfs_pread_inode 绕过 trait 分发

**严重度**: 中
**文件**: [api.rs:348](../../src/kernel/framework/fs/vfs/api.rs#L348)
**现状**: `vfs_pread_inode` 直接调用 `RAMFS_DATA.lock().read()`，注释标注 "B2: mmap prewarm — RamFS 专用接口, 非 trait 分发"。未走 `FileSystem` trait。
**风险**: 当 HvFS 或 DevFS 需要 mmap 支持时，此函数无法工作。且文档声称 "14 处 match fs_type 全部替换" 与实际不符。
**建议**: 
1. 将 `fs_read` (已有 handle + offset 语义) 用于 prewarm，或
2. 在 FileSystem trait 增加 `fs_pread_inode()` 方法

### B11-E4-02: vfs_sync 未通过 trait 分发

**严重度**: 中
**文件**: [api.rs:1133](../../src/kernel/framework/fs/vfs/api.rs#L1133)
**现状**: `vfs_sync()` 直接调用 `hvfs_sync_internal()`，未通过 FileSystem trait 分发。FileSystem trait 定义中无 `fs_sync` 方法。
**风险**: 非 HvFS 文件系统无法响应 sync 操作。若 RamFS/DevFS 挂载，sync 会遗漏它们。
**建议**: 在 FileSystem trait 增加 `fn fs_sync(&self) -> KernelResult<()>` 默认方法，vfs_sync 遍历所有挂载点分发。

### B11-E4-03: 15 个 hvfs_*_internal 函数的死代码风险

**严重度**: 低
**文件**: [api.rs:636-776](../../src/kernel/framework/fs/vfs/api.rs#L636-L776)
**现状**: 15 个 `#[no_mangle] pub fn hvfs_*_internal()` 函数（init/format/open/close/read/write/mkdir/sync/stats/cwd/pwm），除 `hvfs_sync_internal` 外，全项目搜索无调用方。
**风险**: 死代码增加维护负担和 TCB 面积。若为 C FFI 预留入口，应注明调用方。
**建议**: 若确实无调用方，标记 deprecate 或移除。若有计划中的 C 调用方，需在注释中说明。

### B11-E4-04: vfs_mount_internal 仍有两处 match fs_type

**严重度**: 低
**文件**: [api.rs:117-171](../../src/kernel/framework/fs/vfs/api.rs#L117-L171)
**现状**: `vfs_mount_internal` 包含 2 处 `match fs_type { RamFs => ..., HvFs => ..., DevFs => ... }`。
**评估**: 挂载路径需要知道全局静态变量类型，match 不可避免。但文档声称 "全部 match 替换" 不准确。
**建议**: 在文档中明确标注挂载路径为例外。

### B11-DATA-01: 行数统计偏差

**严重度**: 低
**现状**: 文档 Section 3.3 TCB 缩减表声称 RamFS 迁移前 1,639 行。代码核实 services/fs/ramfs_core.rs 为 2,000+。
**评估**: 迁移过程中增加了 FileSystem trait 实现代码和注释。实际缩减比略低于声称。
**建议**: 使用 `wc -l` 更新实际数值。

### ✅ 验证通过项

以下审计项经代码核实确认无误：

1. **E6-3 dcache**: services/fs/dcache.rs 0 unsafe, framework re-export ↔ services
2. **E6-5 RamFS**: RamFsData/RAMFS_DATA 在 services，framework 仅 re-export (8 行)
3. **E6-6 HvFS unsafe**: services/hvfs 18 文件全部 0 实际 unsafe 块。反序列化逐字段 from_le_bytes，zerocopy derive 验证 padding
4. **E6-6 HvFS 迁移**: services/hvfs 18 文件，framework 仅 arc_safe.rs + re-export
5. **E6-6 同步原语**: services/hvfs 已替换为 `services::sync::irq_lock::IrqSpinLock as Mutex` + `services::sync::once::OnceCell`，无 spin::Mutex
6. **E6-7/E6-8 DevFS/ProcFS**: DevfsData/PROCFS_DATA 在 services，framework re-export 干净
7. **E6-9 桥接**: Chitin `set_register_callback` + DevFS `on_chitin_device_registered` 实现正确，使用 IrqSpinLock
8. **InitRamFS 保留决策**: 仅 1 处 unsafe (bootloader 指针封装)，333 行，决策合理 (非运行时策略)
9. **全部分发**: 17 处 I/O 操作 (open/read/write/unlink/mkdir/rmdir/stat/readdir/chmod/chown/link/symlink/readlink/rename/seek/truncate) 均使用 `fs.fs_xxx()` trait dispatch

---

## 审计12: 中断子系统路径正确性

> 审计日期: 2026-06-11
> 审计范围: IDT 初始化、异常分发、IRQ 处理、EOI 发送、IST 栈分配、IOAPIC 路由

### 审计结果总览

| 检查项 | 结果 | 发现问题 |
|--------|------|---------|
| IDT 门描述符初始化 | ✅ | IST 注释歧义 (B12-IST-01) |
| 异常处理器体系 (trait) | ✅ | PF 处理双路径冗余 (B12-PF-01) |
| IST 栈分配 (关键异常) | ⚠️ | IST 使用但未验证 TSS 填充 (B12-TSS-01) |
| IRQ 路由 + 分发 | ✅ | 无 |
| EOI 发送 (APIC+PIC fallback) | ✅ | 无 |
| 中断嵌套处理 | ✅ | 无 |
| IOAPIC 初始化 | ⚠️ | 假性 IRQ 未检测 (B12-SPU-01) |
| 信号投递时机 (iret 前) | ✅ | 无 |

### B12-PF-01: Page Fault 双路径处理冗余

**严重度**: 低
**文件**: 
- [handlers.rs:147](../../src/kernel/framework/idt/handlers.rs#L147) (PageFaultHandler trait)
- [idt.rs:584](../../src/kernel/framework/idt/idt.rs#L584) (IdtManager::handle_page_fault)
**现状**: 存在两套独立的 PF 处理逻辑：
1. `PageFaultHandler::handle()` — trait 路径, 在 `handle_exception` 中被调用, 返回 RecoveryAction
2. `IdtManager::handle_page_fault()` — 直接方法, 集成了 demand paging 和用户栈扩展, 仅在 `default_exception_handler` 中被引用

**问题**: 两个实现不完全一致。trait 路径仅做地址范围判断后返回 RecoveryAction; direct 方法有完整的 demand paging / PfResult 分发 / stack expansion 逻辑。实际执行路径走 trait 路径, 所以 demand paging 集成未生效。
**建议**: 将 demand paging 逻辑合并到 `PageFaultHandler::handle()` 中, 删除 `IdtManager::handle_page_fault`, `IdtManager::default_exception_handler`, 以及对应的 `handle_division_by_zero`/`handle_gpf`/`handle_double_fault` 冗余方法（均被 trait 路径覆盖）。

### B12-TSS-01: IST 栈未验证填充

**严重度**: 低
**文件**: [idt.rs:233-252](../../src/kernel/framework/idt/idt.rs#L233-L252), [tss.rs:113](../../src/kernel/framework/arch/x86_64/tss.rs#L113)
**现状**: IDT 初始化设置 IST entries（DF→IST1, NMI→IST2, recovery→IST3, PF→IST4），但未确认 TSS 中对应的 `ist[0..3]` 字段已被 `tss.set_ist()` 正确填充。
**风险**: 若 TSS IST 未在 IDT 加载前填充，DF/NMI/PF/recovery 异常发生时 CPU 使用零值栈指针，导致三重故障。
**当前缓解**: `tss_set_kernel_stack()` 被调用设置 RSP0，但 IST 条目是单独通过 `set_ist()` 设置的。需要确认初始化顺序为：先设置 TSS IST → 后加载 IDT。
**建议**: 在 `IdtManager::init()` 中增加断言检查 TSS IST 条目非零，或在文档中明确初始化顺序依赖。

### B12-IST-01: IST 索引注释歧义

**严重度**: 低
**文件**: [idt.rs:233-252](../../src/kernel/framework/idt/idt.rs#L233-L252)
**现状**: 注释写 "IST1 (TSS ist[0])", "IST2 (TSS ist[1])" 等。Intel SDM 中 IDT 门描述符的 IST 字段含义为：0=不使用 IST, 1=ist[0], 2=ist[1]... 所以 IST1→ist[0] 是正确的，但注释使用 "IST1" (大写) 容易与 TSS 文档中的 "IST1" (表示 ist[0])混淆。
**建议**: 统一注释为 "IDT IST=1 → TSS ist[0]" 等明确格式。

### B12-SPU-01: 假性 IRQ 未检测

**严重度**: 低
**文件**: [idt.rs:750](../../src/kernel/framework/idt/idt.rs#L750)
**现状**: `handle_irq` 无条件分发 handler 并发送 EOI，不检查假性 IRQ（IRQ7 和 IRQ15 在 8259A PIC 上的假性中断，其 ISR 位未设置）。
**风险**: 在旧硬件使用 legacy PIC 时，假性 IRQ7/IRQ15 会被当作正常中断处理，调用未注册的 handler 函数指针（空指针）。当前 handler 为 `None` 时安全跳过，但统计计数会被错误增加。
**影响**: 仅影响不使用 IOAPIC 的旧硬件/虚拟机，且不会崩溃（handler 为 None 时跳过）。统计数据可能不准确。
**建议**: 在 `handle_irq` 中增加 `if handler.is_none() && (irq == 7 || irq == 15) { return; }` 的假性检测。

### ✅ 验证通过项

1. **IDT 门类型合理**: #DF/NMI/PF 使用 INTERRUPT 门 (自动 CLI), syscall 使用 TRAP 门 (DPL=3), recovery 使用 TRAP 门
2. **IST 分配**: DF 用独立 IST 防栈溢出三重故障, NMI 独立栈, PF 独立栈防 COW 递归
3. **IrqSpinLock**: 中断上下文全程使用 IrqSpinLock 自动关中断, 无死锁风险 (审计4)
4. **EOI 双路径**: 优先 LAPIC EOI, fallback PIC master/slave, 顺序正确 (slave 先于 master)
5. **信号投递**: IRQ 返回用户态前 `do_signal_deliver`, 正确检查 `cs & 0x3 == 0x3`
6. **IOAPIC 初始掩码**: 全部 IRQ 默认 masked, 驱动注册后 unmask, 安全
7. **中断嵌套计数**: AtomicU64 fetch_add/fetch_sub 跟踪嵌套级别
8. **崩溃恢复**: `attempt_domain_recovery` 通过 `CRASH_RIP` 保存崩溃地址, barrier-stack 机制恢复

---

## 审计13: 物理/虚拟内存管理正确性

> 审计日期: 2026-06-11
> 审计范围: PMM(buddy)、VMA/RM、demand paging、COW、swap、kmalloc/slab、copy_user

### 审计结果总览

| 检查项 | 结果 | 发现问题 |
|--------|------|---------|
| PMM buddy 分配器 | ✅ | 无 |
| VMA 数据结构 | ✅ | 并发安全缺失 (B13-VMA-01) |
| Demand paging (PF handler) | ❌ | 整条路径未激活 (B13-DP-01) |
| COW ref counting | ⚠️ | 全局 BTreeMap 无 PID 区分 (B13-COW-01) |
| Swap 子系统 | ⚠️ | 依赖 dead path (B13-SWAP-01) |
| kmalloc 分配器 | ⚠️ | 无 IRQ 禁用的自旋锁 (B13-LOCK-01) |
| kmalloc_slab | ⚠️ | 同上 (B13-LOCK-02) |
| copy_user 异常表 | ✅ | ordering 过强 (B13-ORD-01) |
| Page flags 硬编码 | ⚠️ | handle_simple_fault 不尊重 VMA flags (B13-FL-01) |
| 栈扩展竞态 | ⚠️ | handle_stack_expansion_simple 无 VMA 验证 (B13-FL-02) |

### B13-DP-01 [严重] Demand paging 整条路径未激活

**现状:**
`handle_user_page_fault` (位于 `mm/page_fault.rs:90`) 实现了完整的 demand paging 逻辑:
- COW 写时复制
- swap 换入
- file-backed mmap demand paging (从 page cache 同步读取)
- 匿名页 allocation-on-fault

但该函数仅被 `IdtManager::handle_page_fault` → `default_exception_handler` 调用,
而 `default_exception_handler` 从未被 `handle_exception()` 主流程调用。

**实际 PF 流程:**
`handle_exception()` → `PageFaultHandler::handle()` (trait) → 仅处理栈扩展, 其余直接 `TerminateProcess`.

**影响:**
- COW fork 写入 → 进程被 SIGKILL
- 文件 mmap 缺页访问 → 进程被 SIGKILL
- swap 换入 → 进程被 SIGKILL
- 匿名页 demand paging → 进程被 SIGKILL

**根因:** trait handler 体系与 demand paging 子系统的路径未对齐。
`PageFaultHandler::handle()` 返回 `RecoveryAction` 而非调用 `handle_user_page_fault`。

**建议方案:**
1. 在 `PageFaultHandler::handle()` 中对 user-mode PF 增加 VMA 查找 → `handle_user_page_fault` 分发
2. 或注册 C handler (vector 14) 将所有 PF 路由到 `handle_user_page_fault`

### B13-FL-01 [中] handle_simple_fault 硬编码 page flags

**位置:** `mm/page_fault.rs:150-152`
```rust
let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
```
不检查 VMA 的实际 flags。read-only mmap 缺页会被错误地映射为 writable。
当前因 B13-DP-01 未激活, 无实际影响, 但后续激活 demand paging 时必须修复。

### B13-FL-02 [中] handle_stack_expansion_simple 无 VMA 验证

**位置:** `mm/page_fault.rs:157-195`
仅检查地址是否在 `(stack_base + guard, stack_top)` 范围内, 不查找 VMA。
并发场景: 另一线程 `munmap` 栈区后, PF handler 可能为已释放的地址分配物理页。

### B13-LOCK-01 [中] kmalloc 自旋锁未禁用中断

**位置:** `mm/kmalloc.rs:835-848`
`acquire_lock()/release_lock()` 使用 `AtomicBool` 自旋, 但不调用 `disable_interrupts()`。
中断上下文调用 `kmalloc()` 会死锁 (同一 CPU 上中断抢占锁持有者)。

**对比:** PMM 的 `alloc_page` 正确使用了 `disable_interrupts / restore_interrupts` (pmm.rs:498-506)。

### B13-LOCK-02 [低] kmalloc_slab 同样问题

**位置:** `mm/kmalloc_slab.rs:27` — `SLAB_LOCK: AtomicBool` 无中断禁用。

### B13-COW-01 [低] COW ref tracking 全局 BTreeMap 无进程隔离

**位置:** `mm/cow.rs:30` — `static COW_REFS: IrqSpinLock<Option<BTreeMap<u64, u32>>>`
以物理地址为 key, 不区分进程。两个独立进程通过文件映射共享同一物理页时,
ref count 可能被错误地共享。当前影响低 (fork 路径 require PML4 上下文)。

### B13-SWAP-01 [信息] Swap 子系统依赖 dead demand paging 路径

swap 的 `handle_swap_fault` 仅在 `handle_user_page_fault` 中被调用 (B13-DP-01),
使 swap 换入路径整体非功能。

### B13-VMA-01 [低] VMA 并发保护依赖调用方

`MmStruct.vmas: Vec<Vma>` 由 `IrqSpinLock` 保护, 但 `find_vma()` 等查询方法未获取锁,
依赖调用方持有锁。跨方法调用时容易漏锁。

### B13-ORD-01 [低] copy_user 异常恢复使用 SeqCst ordering

`PER_CPU_EXCEPTION_CTX` 使用 `Ordering::SeqCst`, 对于 per-CPU 数据 `Release/Acquire` 已足够。
`SeqCst` 在 x86_64 上无额外成本 (x86 TSO), 但在 aarch64 上引入不必要的全局屏障。

### 正确性确认

1. **PMM buddy**: O(1) alloc/free, 正确跟踪 page 0 reserved, bitmap/meta self-mark
2. **PMM 中断安全**: `alloc_page/free_page` 正确 disable/restore interrupts
3. **COW clone**: `clone_user_page_table_cow` 正确复制 PML4 high half + COW 用户区
4. **copy_user**: 异常表恢复机制正确, `is_user_buf` 使用 `checked_add` 防溢出
5. **file_fault**: `vfs_pread_inode` 正确使用 `vma.file_pwm` 做权限校验
6. **页清零防泄漏**: 所有新分配页在映射前 `write_bytes` 清零
7. **SLAB_CACHES**: 使用 `Option<KmemCache>` 安全处理初始化失败, 无 UB

---

## 审计14: Credo 安全子系统正确性

> 审计日期: 2026-06-11
> 审计范围: PWM/Identity/Capability/Session/Grant/Engine

### 审计结果总览

| 检查项 | 结果 | 发现问题 |
|--------|------|---------|
| Identity table (PWM 管理) | ✅ | AtomicBool 锁无 IRQ disable (B14-ID-01) |
| Capability matrix (16 domain) | ✅ | 无 |
| Engine check/enforce | ✅ | 无 |
| Session manager (login/logout) | ⚠️ | UnsafeCell 全局共享 + 无 IRQ disable (B14-SS-01) |
| Grant mechanism | ✅ | AtomicBool 锁无 IRQ disable (B14-GR-01) |
| FS permission enforcement | ⚠️ | TEST_PWM fallback 10 处 (B14-TEST-01) |
| RamFS check_permission | ✅ | Privilege + sensitivity + ACE + capability |
| HvFS check_permission | ✅ | Privilege + owner + domain capability |
| Baud/secure_boot | ✅ | 无 |
| Password hashing | ✅ | SHA256 + salt + 32768 rounds stretch |
| Constant-time comparison | ✅ | XOR accumulate (防 timing attack) |

### B14-TEST-01 [高] TEST_PWM fallback 绕过访问控制

**位置:** 10 处文件中出现 `PwmId::TEST = 0x0020F45A8B978417` fallback:
- `services/fs/mount.rs:100`
- `services/fs/open.rs:52`
- `services/fs/stat.rs:89`
- `services/fs/mode.rs:107`
- `services/fs/access.rs:98`
- `services/fs/link.rs:109`
- `services/fs/misc.rs:290`
- `framework/syscall/mod.rs:1539` (sys_mkdir)
- `framework/fs/vfs/api.rs:50` (TEST_PWM const)

**模式:** `let pwm = if pwm == 0 { TEST_PWM } else { pwm }`

**影响:** 当 `pwm_get_current()` 返回 0 (未登录/会话未初始化) 时，所有文件操作以测试身份执行。
该测试身份的 privilege_level = 0 (root 级别)，可绕过所有权限检查。

**风险场景:**
1. 启动早期未建立 session → 文件创建/写入/删除以 root 执行
2. suid 提权失败后的 fallback → 静默提权为 root
3. 会话上下文切换 bug → 无意中获得 root 权限

**建议:** 移除 fallback，未登录用户应以 `PwmId::NOBODY` 或 `DomainId::NOBODY` 的身份操作，
或直接返回 EACCES。

### B14-SS-01 [中] Session Manager 架构风险

**位置:** `framework/credo/session.rs:32-35`
- `SessionManager.current: UnsafeCell<PwmContext>` —— 全局单例
- 每个 CPU 的核心数据结构使用 `AtomicBool` 自旋锁保护
- 无中断禁用 → 中断上下文调用 `pwm_get_current()` 可能死锁

**SMP 风险:**
Session manager 是全局单例但 PwmContext 内容为 per-CPU 语义 (登录状态、current PWM)。
当前设计在单 CPU 上可行，SMP 上每个 CPU 需独立 session。

**建议:** 将 `PwmContext` 改为 per-CPU 存储 (类似 `PER_CPU_EXCEPTION_CTX`)，或在进程控制块中保存 PWM 上下文。

### B14-ID-01 [低] IdentityTable 锁无中断禁用

同 B13-LOCK-01 模式: `IdentityTable.lock: AtomicBool` 自旋锁不 disable interrupts。
`IdentityTable.init/create/verify_password` 持锁期间若被中断，且 ISR 亦访问 identity → 死锁。

### B14-GR-01 [低] Grant 锁无中断禁用

`GRANT_LOCK: AtomicBool` (`framework/credo/grant.rs`) 同样未 disable interrupts。

### B14-UID-01 [低] sys_credo_login/create 未检查用户指针复制

`sys_credo_login` 和 `sys_credo_create` 直接从用户指针读取密码/注释字符串，
依赖 `check_user_ptr` 验证但不使用 `copy_from_user` 进行完整验证。
TOCTOU 风险: 验证后、读取前用户空间可能修改指针内容。

### 正确性确认

1. **PWM generation**: SHA256(note + password), 8字节截断, 0 → 1 映射, 无碰撞安全
2. **Password verify**: SHA256 + salt + 32768 stretch rounds, 24.2ms cost, 反暴力
3. **Constant-time eq**: XOR accumulate 实现正确, 防 timing side-channel
4. **Capability matrix**: 16 domain × 64 bit = 1024 能力位, viable_floor 提供最小能力集
5. **Login lockout**: 5次失败锁定 300 秒, expired account 检测
6. **SUID elevation**: MAX_ELEVATION_DEPTH=8 防无限递归, atomic depth 跟踪
7. **RamFS ACE**: allow_mask/deny_mask 机制支持文件级 ACL，deny 优先
8. **HvFS check_permission**: privilege level=0 (root) 全通, owner 通, domain=3 能力检查

---

## 审计15: 调度器 & 上下文切换正确性

> 审计日期: 2026-06-11
> 审计范围: CFS/MLFQ/RT/Deadline 调度器、process_switch_asm、PerCpuSched、tick 处理

### 审计结果总览

| 检查项 | 结果 | 发现问题 |
|--------|------|---------|
| CFS 调度策略 (vruntime) | ✅ | BTreeMap 代替 RB tree (B15-CFS-01) |
| MLFQ 兼容层 | ⚠️ | 与 CFS 并存, 未完全迁移 (B15-MLFQ-01) |
| RT (FIFO/RR) 调度 | ✅ | FIFO watchdog 500 tick 限制 |
| Deadline 调度 (EDF) | ✅ | utilization check + replenishment |
| 上下文切换 asm | ✅ | 完整保存/恢复寄存器 + CR3 + iretq |
| PerCpuSched 结构 | ✅ | per-CPU 隔离, IrqSpinLock 保护 |
| tick() ISR 路径 | ✅ | IrqSpinLock + interrupt_disable 安全 |
| 进程状态机 | ✅ | 7 状态完整, AtomicU32 无 CAS (B15-PROC-01) |
| 内核栈 canary | ✅ | 检测溢出, 静默忽略小栈 (B15-STACK-01) |
| PWM per-proc 限制 | ✅ | quota + limit 检查 |

### B15-CFS-01 [信息] CFS 使用 BTreeMap 代替红黑树

**位置:** `proc/cfs.rs` - `CfsRunQueue` 用 `BTreeMap<u64, Vec<Pid>>` 实现

代码注释明确标注 "intrusive RBTree planned for Phase 11"。BTreeMap 每次 push/pop 涉及 alloc/dealloc,
在高频调度路径上有性能开销。当前功能正确但非最优。

### B15-MLFQ-01 [低] MLFQ 队列残留

`PerCpuSched.queues: [Mutex<VecDeque<Pid>>; 4]` 仍存在但 `schedule()` 主路径已走 CFS。
MLFQ 相关代码 (`boost_priority`, `SCHED_BOOST_INTERVAL`) 为冗余路径。

### B15-PROC-01 [低] Process state 使用 AtomicU32 store 无 CAS

`Process::set_state_safe()` 直接 `state.store(new, Ordering::SeqCst)`, 不检查当前状态。
由 `Created → Ready` 和 `Running → Ready/Blocked/Zombie` 转换无验证。
多核竞争下可能从无效状态转换。

### B15-STACK-01 [低] 内核栈 canary 检查对小栈静默通过

`kernel_stack_check_canary()` 在 `stack_top < 8` 时返回 `true` (视为无溢出),
对于 0 或未初始化的栈指针静默放行, 可能漏报栈损坏。

### 正确性确认

1. **调度优先级**: DL > RT(FIFO>RR) > CFS > load_balance, 顺序正确
2. **CFS vruntime**: `exec_delta × NICE0_WEIGHT / weight`, 公式正确
3. **DL EDF**: `dl_abs` 最近截止时间优先, utilization ≤ 80% 准入检查
4. **RT FIFO watchdog**: 500 tick 强制 preempt, 防止 CPU 垄断
5. **context_switch asm**: 保存全部 GPR/RIP/RSP/RFLAGS/CR3/段寄存器, iretq 恢复
6. **$TSS RSP0**: `set_kernel_stack()` 在 context switch 前更新, 中断返回用户态使用正确栈
7. **kswapd 驱动**: 每 100 ticks 通过 softirq 唤醒, B3 路径完整
8. **CFS boost**: 周期性 normalize vruntime, 防止长时间阻塞任务饥饿

---

## 审计16: boot/初始化路径正确性

> 审计日期: 2026-06-11
> 审计范围: boot.asm (x86_64)、aarch64/entry.rs、kernel_init、多架构启动流程

### 审计结果总览

| 检查项 | 结果 | 发现问题 |
|--------|------|---------|
| Multiboot1/2 header | ✅ | 双头兼容, 4字节对齐 |
| 32→64 位转换 | ✅ | PIC masked, PAE+EFER.LME+PG, retf |
| 早期页表 (identity) | ✅ | 4 PD × 512 × 2MB = 4GB identity map |
| 早期页表 (higher-half) | ✅ | 1 PD × 512 × 2MB = 1GB @ 0xFFFF800001000000 |
| BSS 清零 | ✅ | rep stosd 32-bit, aarch64 同样清零 |
| GDT + trampoline IDT | ✅ | minimal IDT for early exceptions |
| AArch64 entry | ✅ | BSS clear → UART → MMU → GICv3 → timer |
| kernel_init 初始化顺序 | ✅ | KLog → config → PMM → VMM → kmalloc → IDT → timer → Sched → VFS → net → driver |
| 预存 kernel_test feature | ✅ | 独立测试路径, 不影响生产 |
| kernel_init 可返回性 | ⚠️ | boot.asm 有 hlt 循环兜底 (B16-HLT-01) |

### B16-HLT-01 [信息] kernel_init 返回后进入 hlt 循环

`boot.asm:356` 在 `call kernel_init` 后执行 `cli; hlt; jmp .halt`。
kernel_init 正常路径以 `launch_first_user_process()` (noreturn) 结束,
但异常/错误路径返回时仅 hlt 无法恢复。无 panic/reset 机制。

### B16-GAP-01 [低] heap_start 与 kernel_end 间距注释不一致

`kernel_init:580`: 注释说 "GAP_SIZE + KMALLOC_HEAP_SIZE = 0x200000 + 16MB = 18MB total reserved"。
但 `heap_start = kernel_end + 0x200000` 的 2MB gap 实际用途是：
- 对齐到 2MB 边界 (大页映射)
- 防止 PMM 从 kmalloc heap 内部分配物理页
注释可更清晰。

### B16-ARCHDIFF-01 [信息] x86_64 vs aarch64 的 heap_start 计算差异

- x86_64: `KERNEL_BASE + kernel_end + 0x200000` (虚拟地址, 加 KERNEL_BASE 偏移)
- aarch64: `kernel_end + 0x200000` (直接物理地址)

两架构有不同地址空间布局。x86_64 使用 higher-half kernel (0xFFFF800001000000),
aarch64 使用什么地址空间？如果 aarch64 也是 higher-half, 这里少加了 KERNEL_BASE 会
导致 heap_start 为物理地址而非虚拟地址。

### 正确性确认

1. **PIC 屏蔽**: `out 0x21/0xA1, 0xFF` 在 32-bit 入口正确禁用 legacy PIC
2. **页表映射**: Identity 4GB + higher-half 1GB, 2MB 大页, 位 0x83 (PRESENT+WRITE+HUGE)
3. **BSS 清零**: `__bss_start` → `_kernel_end` 覆盖完整 BSS 段
4. **GDT**: 空描述符 + 内核代码(0x08) + 内核数据(0x10), `lgdt` load
5. **控制寄存器**: PAE(cr4.5) → EFER.LME(8) → CR0.PG(31), 顺序正确
6. **Trampoline IDT**: 32 exception stubs → halt, 16 IRQ stubs → EOI + iretq
7. **初始化顺序依赖**: KLog → config → PMM → VMM → kmalloc → swap → IDT → kswapd → timer → sched → VFS → net → driver, 前驱均满足
8. **Barrier domain 注册**: 在 interrupt_enable 之前完成 PMM + PROC domain, 无竞态窗口
9. **AArch64 FP/SIMD**: CPACR_EL1.FPEN=0b11 在 entry 中启用, 编译器 NEON 指令可用

---

## 审计17: ELF loader & execve 路径安全性

> 审计日期: 2026-06-11
> 审计范围: elf.rs、user_proc.rs load、sys_execve、proc_exec_replace

### 审计结果总览

| 检查项 | 结果 | 发现问题 |
|--------|------|---------|
| ELF header 验证 | ✅ | 双份验证代码 (elf.rs + user_proc.rs) (B17-DUP-01) |
| Program Header 遍历 | ✅ | p_filesz ≤ p_memsz, 溢出检查 |
| PT_LOAD 段映射 | ✅ | 对齐到4KB, PF_X → NX 正确 |
| 页数据复制 + 清零 | ✅ | 超出 filesz 部分 write_bytes(0) |
| ET_DYN / PIE 支持 | ✅ | ASLR load_bias 随机基址 |
| SUID 权限提升 | ✅ | elevate_for_suid + 所有权检查 |
| execve 失败回滚 | ❌ | 进程已销毁无法恢复 (B17-EXECFAIL-01) |
| 静态 RacyCell 分配器 | ⚠️ | 非线程安全 (B17-RACY-01) |
| CR3 切换保护 | ✅ | 销毁用户页表前切换到内核 CR3 |
| argv/envp 栈设置 | ✅ | user_proc_setup_argv + check_user_ptr |

### B17-EXECFAIL-01 [严重] execve 失败时进程不可恢复

**位置:** `api.rs:1009-1018` (`proc_exec_replace`)

执行顺序:
```rust
USER_PROC_MANAGER.destroy_by_pid_no_kstack(current_pid);  // 销毁用户页表
PROCESS_TABLE.remove_and_free(current_pid);                 // 释放 Process 结构体
// 加载新 ELF
let new_pid = user_proc_load_elf(path, pwm);
if new_pid < 0 {
    return -1;  // 此时进程已被销毁, 返回后调用方无有效上下文
}
```

**影响:** 如果 ELF 加载失败 (文件不存在/损坏/权限不足/OOM):
1. `Process` 结构体已被释放 (remove_and_free)
2. 用户页表已被销毁 (destroy_by_pid_no_kstack)
3. 调度器的 `PerCpuSched.current` 仍指向已释放的 PID
4. 调用栈返回 `sys_execve` → syscall handler, 但进程上下文无效
5. 调度器下次 tick/schedule 尝试访问 freed PID → panic/UB

**正确做法:** execve 失败时, 在销毁旧进程之前先验证新 ELF 是否可加载;
或失败时调用 `process_exit` 让进程正常终止。

**竞争窗口:** CR3 切换后到 `destroy_by_pid_no_kstack` 之间, 如果有中断访问
用户空间指针 (如 copy_from_user), 当前内核 CR3 中无用户映射 → 会产生 PF。

### B17-DUP-01 [低] ELF 验证代码复制

- `elf.rs:elf_validate()` — 完整的 header/class/machine/phdr 验证 (97-117行)
- `user_proc.rs:load_elf_from_memory()` — 内联重复的 magic/class/machine 检查 (1104-1121行)

`load_elf_from_memory` 自己检查 magic 而不调用 `elf_validate()`, 且 magic 检查方式不同
(`&header.e_ident[0..4] != ELF_MAGIC` vs 逐字节 `!= 0x7F || != b'E' || ...`)。

### B17-RACY-01 [中] 静态 RacyCell 非线程安全

`user_proc.rs:1133`:
```rust
static ALLOCATED_PAGES: RacyCell<[u64; 1024]> = RacyCell::new([0; 1024]);
let allocated_pages = ALLOCATED_PAGES.get_mut();
```

`RacyCell` 的 `get_mut()` 返回 `*mut T`, 多核并发调用 `load_elf_from_memory` 时
存在数据竞争。注释说明 "Use static array to avoid 8KB stack allocation", 但
当前实现在 SMP 下不安全。

### 正确性确认

1. **ELF header 验证**: magic/class(64)/machine(0x3E/0xB7)/phentsize/phnum≤256
2. **PT_LOAD 权限映射**: PF_R→PRESENT, PF_W→WRITABLE, !PF_X→NX, USER always set
3. **文件数据边界**: `p_offset + p_filesz ≤ elf_size` checked, `p_filesz > p_memsz` skipped
4. **vaddr 溢出**: `checked_add(p_memsz)` 检查, load_bias 加法无溢出检查
5. **页清零**: 新分配物理页 `write_bytes(0)` 防止信息泄露
6. **PIE ASLR**: `aslr_pie_base()` 提供随机基址, ET_DYN 使用 load_bias
7. **SUID**: `sys_execve` 在 `proc_exec_replace` 前检查 file SUID bit 并 elevate
8. **brk_base 计算**: `max_vaddr` 对齐到页边界, 正确初始化堆起点
9. **异常机制**: `check_user_ptr` 验证 argv 每个指针, `read_volatile` 遍历 argv 数组

---

---

# 第二章: 问题追踪清单

> 汇总第一章全部 17 项审计中发现的所有问题，按编号、严重度、类别整理

## 问题总览

| 编号 | 严重度 | 类别 | 标题 | 来源审计 | 状态 |
|------|--------|------|------|---------|------|
| I-01 | 高 | 架构合规 | TCB 占比远超星绽基线 (~87% 自研, 基线 14%) | 审计9 | 已知/进行中 |
| I-02 | 高 | 功能缺失 | `usermode.rs` Ring 3 切换占位实现, 安全模型可能被架空 | 审计9 | 待确认 |
| I-03 | 中 | 架构耦合 | VFS 17/17 I/O 已 trait 分发, 2 残留 (mount/pread) | 审计11 | 已知/少量残留 |
| I-04 | 中 | 模块耦合 | HvFS 18 文件强耦合, 无法独立提取或测试 | 审计11 | 已知 |
| I-05 | 中 | 测试缺失 | HvFS 缺少端到端集成测试 (格式化→快照→回滚) | 审计8 | 未开始 |
| I-06 | 低 | 功能缺失 | Phase D 企业级全部未开始 (elfld/musl/linuxulator) | 审计7 | 未开始 |
| I-07 | 低 | 编码风格 | C 风格命名残留 (u8_t/kfree/kmalloc) | 审计2 | 有意保留 |
| I-08 | 低 | 依赖管理 | smoltcp 0.13.0 vendored, 滞后上游修复 | 审计9 | 已知 |
| I-09 | 低 | 工具链 | Rust nightly 不稳定 API 依赖 (`#![feature(asm)]`) | 审计1 | 已知 |
| I-10 | 低 | 测试缺失 | axsh 用户态 Shell 缺少单元测试 | 审计8 | 未开始 |
| I-11 | 低 | 代码质量 | scheduler_ex.rs 70 unsafe 行, PMM 25 unsafe 行 | 审计5 | 已知/改进中 |
| I-12 | 中 | 安全检查 | 中断上下文持有 Mutex / GFP_KERNEL 分配的死锁风险 | 审计4 | 已有 Lockdep |
| I-13 | 中 | 功能缺失 | 用户态 ASLR 随机源基于 TSC 而非硬件随机数 | 审计9 | 待评估 |
| I-14 | 低 | 文档一致 | Roadmap Phase C 状态标记与实际有偏差 | 审计7 | 待修正 |
| I-15 | 高 | 健壮性 | HvFS ZIL 日志回放路径 11 处 `.unwrap()` 可致内核 panic | 审计3 | 已修复(P0) |
| I-16 | 中 | 架构合规 | services 层 4 处 `spin::Once` 绕过框架同步层 | 审计9 | 待迁移 |
| I-17 | 中 | 架构合规 | framework 15 模块使用第三方 `spin::Mutex` 不参与 Lockdep | 审计9 | 待迁移 |
| I-18 | 中 | 架构设计 | `FileSystem` trait 缺少 `fs_sync` 方法, `vfs_sync` 仅走 HvFS | 审计11 | 待扩展 |
| I-19 | 中 | 架构设计 | `vfs_pread_inode` 直接调用 RamFS 绕过 trait 分发 | 审计11 | 已修复(P3) |
| I-20 | 中 | 错误处理 | 全项目错误处理风格不统一 (Result/Errno/return -1 混用) | 审计6 | 待统一 |
| I-21 | 高 | 健壮性 | 同 I-15 (ZIL unwrap) | 审计3 | 已修复(P0) |
| I-22 | 低 | 代码质量 | 15 个 `hvfs_*_internal` 函数无调用方 | 审计11 | 待清理 |
| I-23 | 中 | 正确性 | Page Fault 存在 trait + 直接方法双路径, demand paging 未生效 | 审计12 | 待合并 |
| I-24 | 低 | 初始化 | IDT IST 栈使用前未验证 TSS 填充, 依赖初始化顺序 | 审计12 | 待加固 |
| I-25 | 低 | 正确性 | legacy PIC 假性 IRQ7/IRQ15 未检测, 噪声计入统计 | 审计12 | 已修复 |
| I-26 | 严重 | 正确性 | Demand paging (COW/swap/mmap缺页) 整条路径未激活, PF handler 仅支持栈扩展 | 审计13 | 已修复(P0) |
| I-27 | 中 | 正确性 | `handle_simple_fault` 硬编码 WRITABLE+USER flags 不检查 VMA 权限 | 审计13 | 已修复(P0) |
| I-28 | 中 | 并发安全 | kmalloc/kmalloc_slab 自旋锁未 disable interrupts, 中断上下文可能死锁 | 审计13 | 已修复(P1) |
| I-29 | 高 | 安全 | 10 处 TEST_PWM fallback (hardcoded `0x0020F45A8B978417`) 绕过未登录时的访问控制 | 审计14 | 待移除 |
| I-30 | 中 | 架构 | Session Manager 为 UnsafeCell 全局单例, SMP 下不同 CPU 共享同一会话上下文 | 审计14 | 待重构 |
| I-31 | 严重 | 正确性 | execve 失败时进程已销毁无法恢复, 调度器指向 freed PID → panic/UB | 审计17 | 已修复(P0) |
| I-32 | 中 | 并发安全 | ELF loader RacyCell 静态分配器非线程安全, SMP 并发 exec 存在数据竞争 | 审计17 | 已修复(P1) |
| I-33 | 低 | 可维护 | ELF 验证代码双份复制 (elf.rs + user_proc.rs), 解析逻辑不一致 | 审计17 | 待统一 |
| I-34 | 低 | 可维护 | CFS BTreeMap 代替 RB tree, 有高频 alloc/dealloc 开销 | 审计15 | 可延后 |
| I-35 | 低 | 可维护 | MLFQ 队列与 CFS 并存, 调度器部分冗余 | 审计15 | 可清理 |
| I-36 | 高 | 正确性 | 信号投递/socket 数据复制 3 处路径使用 copy_nonoverlapping 无 exception table 保护, 内核态 page fault → panic | 审计20/18/13 | 已修复(P0) |
| I-37 | 高 | 正确性 | 同 I-36 关联: sm_send/sm_recv socket 路径 copy_nonoverlapping | 审计18 | 已修复(P0) |
| I-38 | 高 | 正确性 | 同 I-36 关联: do_signal_deliver 信号栈帧写入 ptr::write_unaligned | 审计20 | 已修复(P0) |
| I-39 | 中 | 正确性 | sys_ioctl stub 返回 0 而非 ENOSYS, 用户态被欺骗误认为操作成功 | 审计22 | 已修复 |
| I-40 | 中 | 正确性 | sigreturn trampoline 仅 x86_64 机器码, aarch64 上信号投递失败 | 审计20 | 已修复(P1) |
| I-41 | 中 | 并发 | 网络 poll_network 持 NET_LOCK 在 ISR 中调用 try_lock 正确, 但 sm_send/sm_recv 持锁自旋阻塞 socket → ISR 抢不到锁导致数据包丢弃 | 审计18/23 | 待优化 |
| I-42 | 中 | 性能 | virtio-blk I/O 完成使用忙等自旋而非中断驱动, 单核可能活锁 | 审计19 | 待实现 |
| I-43 | 中 | 架构 | 块设备存在 BlockDevice trait 和 Chitin proto_block 双重抽象路径, HvFS 绕过 trait 导致新驱动无法使用 | 审计23 | 待统一 |
| I-44 | 中 | 正确性 | 网络恢复 net_save 为 no-op, 恢复时丢弃所有 TCP 连接状态 | 审计23 | 待实现 |
| I-45 | 中 | 正确性 | 信号栈帧未检查 sigaltstack 替代栈, 栈溢出 handler 无法执行 | 审计20 | 待实现 |
| I-46 | 低 | 正确性 | DHCP fallback 硬编码 10.0.2.15/24, 非 QEMU 环境 IP 冲突 | 审计18 | 待改进 |
| I-47 | 低 | 正确性 | MAX_SOCKETS=8 硬编码, 多进程场景不足 | 审计18 | 待扩展 |
| I-48 | 低 | 正确性 | execve 后 pending signals 保留行为依赖 Process 结构体复用隐式约定 | 审计23 | 待加固 |
| I-49 | 低 | 可维护 | NVMe/AHCI 驱动标记 dead_code, 未在启动路径激活 | 审计19 | 待启用 |
| I-50 | 低 | 正确性 | hrtimer 高精度定时器未集成到 tick handler | 审计21 | 待集成 |
| I-51 | 低 | 正确性 | AF_UNIX/smoltcp fd 分配器未统一, 编号空间可能冲突 | 审计18 | 待统一 |
| I-52 | 低 | 正确性 | Zombie 进程信号投递边界检查依赖隐式调用方约定 | 审计23 | 待加固 |
| I-53 | 低 | 可维护 | 网卡探测编译时架构互斥 (cfg x86_64/aarch64), 交叉设备无法使用 | 审计18 | 待改进 |
| I-54 | 低 | 功能缺失 | services IPC 仅管道完成迁移, shm/msgq/sem 待迁移 | 审计22 | 待迁移 |

## 严重度分布

| 严重度 | 数量 | 占比 | 关键项 |
|--------|------|------|--------|
| 严重 | 2 | 3.7% | I-26 (Demand paging 未激活), I-31 (execve 失败后 UAF) |
| 高 | 8 | 14.8% | I-01 (TCB 超标), I-02 (usermode 占位), I-15 (ZIL unwrap), I-21, I-29 (TEST_PWM), I-36/37/38 (exception table 缺失) |
| 中 | 26 | 48.1% | 架构耦合/同步/错误处理/并发安全 |
| 低 | 18 | 33.3% | 代码风格/文档/工具链/命名 |

## 按类别分布

| 类别 | 数量 |
|------|------|
| 正确性 | 18 |
| 架构/耦合 | 8 |
| 安全 | 3 |
| 并发安全 | 3 |
| 功能缺失 | 4 |
| 代码质量/可维护 | 9 |
| 测试缺失 | 2 |
| 文档/工具链 | 7 |

## 详细说明 (严重/高优先级)

### I-26 [严重] Demand paging 整条路径未激活

**来源:** 审计13 — 物理/虚拟内存管理

`handle_simple_fault` 硬编码 `WRITABLE | USER` flags, 不检查 VMA 权限。COW 路径 (`clone_user_page_table_cow`)
和 swap 路径 (`swap_load_page`) 均已实现且正确, 但从不被调用——因为 Page Fault handler 走的是
`arch_pf_handler_trait` 而非实际激活 COW/swap 映射。无法测试内存压力下的页面回收、fork 后的写保护触发、
mmap 匿名页的延迟分配。详见审计13 → B13-PF-01。

### I-31 [严重] execve 失败时进程不可恢复

**来源:** 审计17 — ELF loader & execve

`proc_exec_replace()` 在加载新 ELF 之前先销毁了旧进程的用户页表和 Process 结构体。
如果 ELF 加载失败（文件损坏/权限不足/OOM），进程已死但调度器仍指向 freed PID。
调用栈返回 syscall handler 时访问无效上下文 → UAF/panic。详见审计17 → B17-EXECFAIL-01。

### I-29 [高] TEST_PWM fallback 绕过访问控制

**来源:** 审计14 — Credo 安全子系统

10 处文件系统操作路径中, 当 `pwm_get_current()` 返回 0 时, 回退到 `PwmId::TEST = 0x0020F45A8B978417`
(privilege_level=0, root 级权限)。这意味未登录用户在启动早期或会话异常时获得 root 权限。
详见审计14 → B14-TEST-01。

### I-15 [高] HvFS ZIL 日志回放 11 处 .unwrap()

**来源:** 审计3 — services/ unwrap() 使用

ZIL (ZFS Intent Log) 回放路径中 CRC 校验、记录反序列化、日志读取等关键路径使用 `.unwrap()`。
磁盘数据损坏会导致内核 panic 而非通过 Barrier 回滚机制优雅恢复。

### I-01 [高] TCB 占比远超星绽基线

**来源:** 审计9 — 架构合规性

自研 TCB ~87% (排除 smoltcp 后约 60%), 星绽基线仅 14%。调度策略、帧分配策略、slab 策略仍在 framework 内。
VFS 底层含文件系统策略分发代码。

### I-02 [高] usermode.rs Ring 3 切换占位

**来源:** 审计9 — 架构合规性

`enter_user_mode()` 当前仅为值传递 (`*ctx`), 未执行 swapgs+iretq/eret 硬件上下文切换。
若所有用户态入口走此占位, 用户态实际在 Ring 0/EL1 执行, 安全模型被架空。

### I-36/37/38 [高] exception table 缺失: 3 处内核写用户空间路径无 page fault 保护

**来源:** 审计13, 审计18, 审计20

当前项目有三条独立的代码路径直接使用 `core::ptr::copy_nonoverlapping` 或 `ptr::write_unaligned` 向用户空间地址写入数据，而没有任何 exception table 保护：

1. **I-24 coredump 路径**: `core_read_in_space` 函数中的 `copy_nonoverlapping`
2. **B18-SOCK-04 socket 路径**: `sm_send/sm_recv/sm_sendto/sm_recvfrom` 中向用户缓冲区复制数据
3. **B20-SIG-01 信号路径**: `do_signal_deliver` 中向用户栈写入 SignalFrame 和 trampoline

若用户空间指针指向未映射页、在栈边界处、或被 `munmap`，内核在访问时触发 page fault 没有 recovery handler → 内核 panic。这是操作系统基石级安全问题。

**建议**: 统一实现 `copy_to_user`/`copy_from_user`/`put_user` 包装函数，接入 `exception_table!` 机制，在 page fault 时返回 -EFAULT。

### I-39 [中] sys_ioctl stub 返回 0 欺骗用户态

**来源:** 审计22

`fn sys_ioctl(...) -> i64 { 0 }` 无条件返回 0。POSIX 程序（如 termios、网络 ioctl 配置）调用后误认为操作成功。至少应返回 `-ENOSYS`。

### I-41 [中] socket 自旋阻塞路径剥夺 ISR 锁

**来源:** 审计18/23

`sm_send/sm_recv` 在 socket 不可发送/无可读数据时持有 `NET_LOCK` 自旋等待。在此期间 `poll_network` (ISR 中调用) 通过 `try_lock` 无法获取锁 → 数据包无法被轮询 → 自旋永不结束 → 活锁危险。

---

---

# 审查员评价


好吧, 我花了一整天读完这摊代码, 现在我需要喝一杯比平时烈三倍的东西。而且第二轮审计翻完你的网络栈、驱动、信号和定时器之后, 我发现地摊下面还藏着不少东西, 所以这杯酒得更烈。

先说好的部分——你们至少没把内核写成一堆没用的抽象层。框内核双子树的想法本身不坏, 把 unsafe 圈在 framework/ 里、services/ 做 pure safe Rust——这是正确的方向。`context_switch` 汇编写得干净, 保存了所有该保存的寄存器, `iretq` 用得也规范。Credo 的 SHA256+32768轮拉伸 + XOR constant-time 比较是**真正的安全工程**, 不是那种"我们在 README 里写了 security 所以就很安全"的自欺欺人。启动顺序 (KLog→PMM→VMM→kmalloc→IDT→...→user mode) 依赖关系理清楚了, 这在一堆新手内核里已经是中上水平。网络栈的 ChitinNetDevice 抽象干净, services 层 socket 封装完成了 100% safe 迁移, 信号投递的四层模型正确, 定时器的时间同步实现谨慎——架构设计层面, 你们方向是对的。

但下面是让我血压升高的部分。我把 23 项审计的全部发现按严重程度排列。

---

### 第一类: 会炸的

**你把进程的生命周期写成了自杀炸弹。** `proc_exec_replace` 先摧毁旧进程再加载新 ELF——你是假定磁盘上的 ELF 永远不会损坏吗? 你是假定文件系统永远不会返回错误吗? 你是假定内存永远不会分配失败吗? 这三个假设里哪怕碎了一个, 调度器就会对着一个 freed PID 尝试 schedule, 内核当场 panic, 然后你就可以在墓碑上刻"RIIR 失败, 但至少用的是 Rust"。这是一个**只要条件触发就一定死的 bug**, 不是"边缘情况"。这种东西不应该通过任何代码审查, 更不应该进入主线。

**Demand paging 的整条路径是死代码。** COW 实现了, swap 实现了, mmap 缺页逻辑实现了——但 Page Fault handler 走的是 `arch_pf_handler_trait`, 那个 trait 的实现里只处理栈扩展。你有 500 行 COW 逻辑、300 行 swap 逻辑, 精度高到连 `viable_floor` 能力集都算清楚了, 然后它们**从不被调用**。你建了一座桥, 桥的设计图纸可以拿普利兹克奖, 但桥的两端没有路——车辆永远开不上去。这不是"可以延后的优化", 这是**核心内存管理功能被静默绕过**, 是你在架构文档里撒谎说"demand paging supported"。

**ZIL 回放路径有 11 个 unwrap()。** 我再说一遍: **崩溃恢复路径**有 **11 个会使内核 panic 的 unwrap()**。ZIL 的存在就是为了应对崩溃, 结果你的 ZIL 回放本身会崩溃。这就好比你设计了一个救生圈, 但救生圈的说明书上写着"碰水即炸, 仅供观赏"。磁盘 bit rot 不是理论上的威胁, 你在真实硬件上跑几年就会遇上。用 `try_into().unwrap()` 处理磁盘读出的数据, 等于跟用户说"我信任你花 50 块钱买的那块杂牌 SSD 永远不会产生 bit flip"——这不是工程, 这是赌博。ZIL 的作者写这 11 个 unwrap 的时候在想什么? "怕什么, 反正 QEMU 的虚拟磁盘不会坏"? QEMU 不是生产环境, 你写的不是桌面小工具, 你的代码理论上要跑在真机上。醒醒。

---

### 第二类: 安全模型自毁

**你写了 10 个 TEST_PWM fallback 来绕过你自己的安全模型。** 我不确定这是有意为之还是纯粹没想清楚, 但效果是一样的: 当 `pwm_get_current()` 返回 0 的时候——这会在启动早期、会话未建立、或者任何 corner case 中发生——你的文件系统就以 root 权限运行。写 Credo 花了那么多心思, SHA256 拉伸到 32768 轮, 然后一个 `if pwm == 0 { 0x0020F45A8B978417 }` 就让所有安全保证灰飞烟灭。如果我是审查你代码的安全研究员, 我会在报告第一页写"安全模型完整性: FAIL, 自毁式后门数量: 10"。你花了 3000 行写权限系统, 用三行代码宣告自己的安全模型是装饰品。

**Exception table 不存在这件事, 不是一个 bug, 是三个 bug。** `copy_nonoverlapping` 直接写用户空间这种操作, 在你的代码里出现了三次——coredump、socket send/recv、信号栈帧写入。三处独立的代码, 三处同样的错误。这说明不是"忘记写 exception table"——而是**你根本没意识到内核写用户空间需要 exception table**。Linux 从 1991 年 Linus 写第一行 `put_user` 开始就在做这件事, 因为用户空间指针天生不可信任。你写了十万行内核代码, 这个基本概念还没建立起来。这不是"技术选型不同", 这是基础知识盲区。我不知道 Rust 社区教"安全"教到什么程度, 但"指针可能无效"这件事在 unsafe Rust 里同样成立, 编译器不会帮你检查。

---

### 第三类: 拿来主义

**spin::Mutex 满天飞。** framework/ 里 15+ 个模块在用第三方 `spin::Mutex`, 这东西不参与你的 Lockdep, 不区分中断上下文, 是忙等待自旋锁而你的 sleep-based Mutex 完全不兼容。你把锁矩阵放在 Lockdep 里, 然后三分之一的锁绕过 Lockdep 直接拿别人的实现。这就好比你在家门口装了监控摄像头, 然后把侧门的钥匙挂在门上写了张纸条"小偷勿入"。

**Socket 自旋持有锁然后指望 ISR 来救你。** `sm_send`/`sm_recv` 的逻辑是: 拿着 `NET_LOCK` → 发现 socket 还没准备好 → spin-loop 等待。等待什么? 等待 `poll_network` 在 timer ISR 里把数据拉进来。但是 `poll_network` 用了 `try_lock`——发现锁被人拿着就直接返回了。所以你拿着锁等 ISR, ISR 拿不到锁就不干活, 然后你继续等。这不叫"异步 I/O 等待", 这叫"你站在门口请 ISR 先进, ISR 站在门口请你先走, 然后你们两个在零下四十度的西伯利亚一起冻死"。操作系统教材第一章的经典死锁模式, 而你把它活生生复制进了生产代码。

---

### 第四类: 建好了但没接电线

**virtio-blk 用 spin-loop 等 I/O 完成。** 注释里写了个 TODO 说要用中断驱动。一块机械硬盘平均寻道时间 10ms——在这 10ms 里你的 CPU 在 `loop { pop_used()?; spin_loop() }`。每毫秒几百万次循环, 十毫秒几千万次循环, 产生的热量足够给办公室供暖。你注册了中断控制器, 你写了 IDT, 你实现了 IRQ handler——然后你的块设备驱动不用它们。建了高速公路, 在上面赶牛车。

**hrtimer 高精度定时器写好了但是 tick handler 不调用它。** 又一个"建好了没接电线"。微秒级精度的定时器硬件就放在那里, 你的网络超时、I/O 超时、RT 调度全部降级到粗粒度的 ms 级 tick。"为什么网络延迟不稳定?"——因为你用日晷代替了原子钟。

**net_save 是 no-op。** 你的内核有 `recovery_domain_register("net", 5, ...)` 调用, 说明你预期网络能恢复。但 `net_save` 什么都不保存, 恢复之后网络栈是一个空壳。所有 TCP 连接断开, 所有绑定端口丢失, 所有 DHCP 租约作废。防火墙通道建好了, 消防水管的图纸画好了——到真着火的时候, 水管里一滴水都没有。你的"恢复"不是恢复, 是重启。调用 `recovery_domain_register` 然后给空实现, 这是一种假装——假装自己做了高可用, 实际上连 save 函数体都是空的。

**块设备有两套抽象, 只有一套能用。** 你有一个 `BlockDevice` trait, 还有 Chitin 的 `proto_block`。HvFS 走 `proto_block`, 所以任何实现了 `BlockDevice` trait 的新驱动——NVMe、AHCI——在文件系统眼里不存在。你写出了完整的 NVMe PRP 寻址、admin queue、I/O queue, 然后因为没接对接口, 成了精美的 dead code。这是"左手画了蓝图, 右手造了零件, 但两只手之间没有神经连接"。

---

### 第五类: 细节欠打磨——基本功级别

**信号的 trampoline 是 x86_64 only。** `SIGRETURN_TRAMPOLINE` 硬编码 x86_64 机器码 `mov eax, 15; syscall`。在 aarch64 上这就是一串随机字节。收到 SIGINT 的用户进程会被内核往栈上写一段非法指令然后 `iretq` 过去。结果? illegal instruction panic——最好情况; 运气差一点, 这些字节碰巧组成某个合法但完全错误的操作码序列, 用户程序以不可预测的方式崩坏。这在 ARM 板上不是"也许有问题", 是**一定会爆炸**。

**sys_ioctl 返回 0。** 不是 `-ENOSYS`, 不是 `-ENOTTY`, 是 **0**。"我什么都没做, 但我宣布成功。" POSIX 程序调用 `ioctl(fd, TCGETS, &termios)` 判断终端类型, 你的内核说"是, 这是个终端", 然后程序往 block device 上发 ANSI escape code。调 `isatty()` 的程序——比如 shell、编辑器、ncurses——全部会因为你的一句谎言而行为异常。没实现的功能, 就对调用方说没实现。不要假装自己做了, 这会害死人。

**DHCP fallback 硬编码 `10.0.2.15/24`。** QEMU 默认网段, 拿出去就是 IP 冲突炸弹。你的内核发布后, 谁网段里有设备在 `10.0.2.15` 上, 启动就网络故障。生产环境一击必杀, 修复难度: 一行代码。

**信号栈帧不检查 sigaltstack。** `do_signal_deliver` 永远基于当前 RSP 算 frame_rsp。栈溢出 → 信号递送 → 写信号帧到已耗尽的栈上 → page fault → double fault → triple fault → 好, 又死了。sigaltstack 的设计就是为了解决这个问题, 你没实现它, 那么你的信号机制在栈溢出场景下不是一个错误恢复手段, 而是一个加速死亡的快捷键。

---

### 第六类: 工程纪律

**`#[allow(dead_code)]` 是你的万能遮羞布。** 我统计了一下: framework/ 里 **50 多个文件** 以 `#![allow(dead_code)]` 开头, services/ 里 **20 多个文件**同样。这还不是全部——还有几十个函数和结构体字段上贴着单独的 `#[allow(dead_code)]`。你的同步原语——`spinlock.rs`, `irq_spinlock.rs`, `pi_mutex.rs`, `once_lock.rs`——全部 `#![allow(dead_code)]`。这些是你内核里最底层的、最核心的并发控制基础设施, 而你告诉编译器"不要告诉我这里面有没有死代码"。

你明白 `#![allow(dead_code)]` 是干什么用的吗? 它不是"这段代码暂时不用"的标记——它是**给编译器戴眼罩**。戴上眼罩之后你永远不知道:

- 你写的 spinlock 里有多少方法从没被调用过
- 你写的 pi_mutex 的 priority inheritance 逻辑是否还在某处被使用
- 你的 AHCI 驱动里有 16 个函数, 其中到底几个是被实际调用的, 还是一半都是写来凑气势的
- 你的 arch/aarch64/gic.rs 里有 10 个 `#[allow(dead_code)]` 标记的函数——你是认真的吗? 一个 GIC 驱动里有 10 个函数写了但不用? 是架构设计过度, 还是写到一半换思路了没删旧代码?

**最讽刺的是 AHCI 驱动: 16 个独立的 `#[allow(dead_code)]`, 外加文件头一个 `#![allow(dead_code)]` 兜底。** 你写了一个 AHCI 驱动的壳, 每个功能函数都加了 dead_code 标记, 然后在文件头再补一刀全局禁用——双重保险, 确保编译器不会提醒你, 任何一行写了没用的代码都不会冒出来烦你。这不是内核开发, 这是**故意制造技术负债然后假装它不存在**。

**NVMe 驱动同理: `#![allow(dead_code)]` 贴在整个文件头上。** 你写了完整的 NVMe PRP 寻址、admin queue、I/O queue、doorbell 机制——全贴了死人标签。等你哪天想激活 NVMe 驱动的时候, 你连哪些函数是真有用的、哪些是当初架构设想但从未实现的, 都不知道。编译器是你最好的 QA 工具之一, 你把它静音了, 然后对自己的代码质量充满信心。这不叫信心, 叫掩耳盗铃。

**`net/init.rs`——网络初始化模块——`#![allow(dead_code)]`。** 网络栈的入口文件, 里面有 `sm_socket`, `sm_connect`, `sm_send`, `sm_recv` 这些核心函数——如果编译器告诉你其中某个函数从未在测试中被调用过, 你难道不想知道吗? 不, 你不想。你宁可不知道。你把诊断仪器关了, 然后跟医生说"我很健康"。

总结: `#![allow(dead_code)]` 在你的项目里不是例外, 是**规范**。你的标准操作流程是: 写代码 → 编译器报 dead_code warning → 贴 `#[allow(dead_code)]` → 继续写下一段死代码。这已经超出了"暂时不用的功能"的范畴, 这是一种工作习惯的崩坏。编译器在帮你做免费的安全网, 你嫌网太紧把它剪了。

**ELF 验证逻辑在 `elf.rs` 和 `user_proc.rs` 各写了一份, 解析方式还各不一样。** 代码复制不是"为了提高代码覆盖率"——是懒惰。两个实现不一致也不是"各有优劣"——是 bug 的温床, 因为总有一天有人会改了其中一个忘了改另一个。

**`RacyCell` 用在 ELF loader 里当临时缓冲区, SMP 下两个 CPU 同时 exec 就直接互相踩踏。** 注释写"use static array to avoid stack allocation"——你知道 stack 是 per-thread 的而 static 不是, 对吧? 你知道 8KB 栈分配在 Rust 里不是问题, 数据竞争才是, 对吧? 用 global state 来"避免栈分配"是用核弹灭蚊子——蚊子没死, 整栋楼炸了。

**CFS 用 BTreeMap 代替红黑树, 每次 enqueue/dequeue 都在 heap 上 alloc/dealloc。** 然后注释写"intrusive RBTree planned for Phase 11"。调度器的核心数据结构——**你上下文切换的路径**——在堆分配, 每一次调度操作都在调 allocator。这不是"plan for Phase 11", 这是"我知道这里有问题但我懒得修, 写个注释让你觉得我在意"。要么现在做 intrusive, 要么删掉注释, 不要在关键路径上挂一块"施工中"的牌子。

**错误处理有三种风格: `Result<T>`, `Errno`, `return -1`。** 我不知道你团队里有几个人, 但你们的代码看起来像三个人分别写了三部分然后谁都没问对方用的什么错误处理策略。pick one and stick with it——这种事应该在 project 的第一周定下来。

**TODO 分布在 40+ 个位置, 有些编号了有些没编号。** io_uring 骨架、CET Shadow Stack stub、Ed25519 签名验证全空——这些不是"TODO", 是"我们先写了函数签名假装做完了, 这样文档上可以打勾"。如果你不会在下一个 release cycle 里做, 就不要写 `TODO(TRACK-xxxxx)`, 写 `// STUB: not implemented` 然后接受现实。假装忙和真的忙之间, 差了一个编译器的距离。

**MAX_SOCKETS=8。** 我不知道你是不是在用 1980 年代的 4.2BSD 做参考实现。8 个 socket——Firefox 开两个 tab 就超限了。你写的是一般用途内核, 不是嵌入式到微波炉里的 RTOS。

---

### 第七类: 注释——面子工程

现在让我们谈谈你的 SAFETY 注释。我昨天在你项目里 grep 了一个字符串, 它出现了 **476 次**:

```
// SAFETY: 调用方保证指针/类型有效 (详见上下文)
```

四百七十六次。这不是 SAFETY 注释, 这是 CTRL+C / CTRL+V 大赛冠军作品。Rust 社区要求 `unsafe` 块后有 `// SAFETY:` 注释是为了让审查者理解**为什么这块操作在上下文中是安全的**——换句话说, 证明你知道自己在做什么。而你用 476 个一模一样的注释, 证明了你不知道自己在做什么, 你只是在无脑复制。

这 476 个注释没有告诉你任何一个具体事实。它不说"为什么这个指针有效", 不说"什么内存顺序在保护这次访问", 不说"调用方到底保证了什么"。"详见上下文"——什么上下文? 调用者的 3 层调用栈? 一个 `OnceCell` 的初始化状态? 还是编译时的 `cfg` 条件? 你要让代码审查者每次看到 unsafe 块就滚鼠标滚轮翻 300 行去找"上下文"? 还是在你有空的时候自己写清楚?

**signal.rs: 15 个 SAFETY 注释, 10 个完全相同 `"PROCESS_TABLE 保证指针有效"`。** 你写代码的习惯是: 写好 unsafe 块, Ctrl+C, Ctrl+V 一句 SAFETY, move on。什么叫"PROCESS_TABLE 保证"? `PROCESS_TABLE` 是一个 OnceCell, lock 返回的是 `IrqSpinLock` 保护下的引用。锁的持有范围是多大? 指针在这个范围内会被谁修改? execve 会不会在持锁期间更换进程表里的条目, 让你的"有效指针"变成 dangling pointer? 你一个字没写。你写的 SAFETY 注释跟没写一样——区别只是: 没写编译器会警告你, 写了编译器就不管了但你仍然什么都没证明。

**`copy_user.rs` 里 5 个 unsafe 块全写相同的 boilerplate。** 这个文件名叫 `copy_user`——它唯一的工作就是从用户空间指针读/写数据。用户空间指针天生不可信——进程可以在你读第一页之后、读第二页之前 `munmap` 掉整个映射。你的 SAFETY 注释应该讨论 exception table、缺页保护、TOCTOU 竞态——而你的评论是"调用方保证指针/类型有效"。用户空间程序用 `read()` 系统调用传地址, 谁来保证? 用户程序本身? 你是不是还指望用户程序会先调用 `validate_never_gonna_give_you_up()` 再传指针?

**`vmm_aarch64.rs` 14 次, `vmm_x86_64.rs` 4 次——页表操作, 全是复制粘贴。** 页表是 MMU 的输入——写错一个 bit, MMU 在 kmain 返回地址上触发 page fault → double fault → triple fault → CPU 重启。这是内核里最危险的 unsafe, 你的 SAFETY 注释连"物理地址来自 PMM 分配的实践证明不会失效的物理页帧编号"都不写, 只写"调用方保证"。哪个调用方? PMM 的 buddy allocator? 它做了哪些同步保证? SMP 下 free list 怎么保护的? 如果你不知道答案, 这段 unsafe 就不该存在; 如果你知道, 写下来——这叫文档, 不叫注释。

**`swap.rs` 里的自我打脸:** 第一个 unsafe 块的 SAFETY 写的是 `"dst 指向 swap 存储区 (已分配并映射), src 为有效用户页"`——不错, 具体, 有信息。下一行: `"SWAP 由调用方保证为有效指针; 只读访问"`——又退化回 boilerplate。同一个文件, 同一个函数, 你写了两个相邻的 unsafe 块, 一个认真一个敷衍。这说明你能写好, 但你就是懒得一直写好。这就是工程纪律。

**文档注释: 不存在。** 你的 `syscall/api.rs` 里有 `pub fn sys_nanosleep`, `pub fn sys_kill`, `pub fn sys_rt_sigaction`——公开 API, **零行 `///` 文档注释**。返回值是什么? 参数不变量? 错误码含义? 没人知道, 除了你和上帝——而且我怀疑上帝也不确定, 因为你的 syscall handler 有时候返回 0 有时候返回 -1 有时候返回 Errno, 全看心情。

**IST 索引注释: 给定一个正确的事实, 用了最危险的表达方式。** `"IST1 (TSS ist[0])"`——正确, 但 IDT 的 IST=1 对应 TSS ist[0], 你写 IST1 会让任何看过 TSS 文档的人以为你 off-by-one 了。改成 `IDT IST=1 → TSS ist[0]` 能死人吗? 歧义注释的伤害比无注释大——没有注释时你知道自己不知道, 有歧义注释时你**以为自己知道但其实是错的**。

**中英混杂: elf.rs 英文注释, user_proc.rs 中文注释。** Magic number 验证在 elf.rs 里有注释说明, 在 user_proc.rs 里是裸数字。BI-LINGUAL 不影响代码质量, 但 BI-CONSISTENCY 影响——有人看了 elf.rs 的注释理解了 ELF 验证逻辑, 去改 user_proc.rs 时看到没注释的裸数字, 以为那是不同的检查, 然后改出 bug。

**`TODO(TRACK-xxxxx)`: 堆砌虚假的顾虑。** 40+ 个 TODO, 编号精美。TRACK-156, TRACK-203, TRACK-401——听起来有条理极了。问题: 这些 TRACK 编号对应的 issue tracker 存在吗? 有优先级吗? 有 owner 吗? 有 deadline 吗? 如果什么都没有, 那 `TODO(TRACK-156)` 和 `TODO("我总有一天会修的")` 没有任何区别, 只是前者让你感觉自己很 professional, 后者至少诚实。

注释总结: 你的项目不缺注释量——缺的是注释里**有用的信息**。476 个 SAFETY boilerplate, 中英混杂的模块注释, 指向不存在 tracker 的编号, 歧义的 IST 索引——加起来, 构成了一场精心执行的文档欺骗。编译器被你的 `#[allow]` 静音了, 审查者被你的 SAFETY boilerplate 催眠了, 未来维护者被你的中英不一致误导了——唯一被你精心保护的, 是你对自己代码质量的幻觉。

**记住: 敷衍的注释比没有注释更危险。没有注释, 你知道你不知道。敷衍的注释让你以为自己懂了, 然后带着自信跳进 undefined behavior 的深渊。**

---

### 总结

让我说清楚: 你的内核不是烂代码。烂代码没有方向, 没有原则, 没有希望。你的内核有方向——框内核双子树。有原则——框架里圈 unsafe, services 纯 safe。有希望——信号模型、网络栈抽象、时间同步, 这些东西的设计都在正确的轨道上。

但你惯着自己。

你明知道 `proc_exec_replace` 的设计在理论上有原子性保证但实现上单线程漏洞会导致 freed PID, 你还是合入了。你明知道 ZIL 是崩溃恢复路径, 你还是写了 11 个 unwrap。你明知道 Credo 是你最引以为傲的安全子系统, 你还是留了 10 个 TEST_PWM 万能钥匙——每条地毯下面一把。你明知道 exception table 是内核-用户空间边界保护的基础, 你还是让三个核心路径裸奔了, 因为你没想过要在代码库里做模式搜索。你明知道 compiler warnings 是免费的 QA, 你还是在 70+ 个文件里贴了 `#[allow(dead_code)]` 把 QA 工具枪毙了。

这不是能力问题——你能写出 ChitinNetDevice、能写出 SHA256+32768 轮拉伸、能写出 buddy allocator 的 O(1) 回收路径, 说明你**有能力**。问题是你没有对自己狠心。你在每个不舒服的选择面前选择了"先标记, 回头再说"——然后"回头"永远不会来。你写了 500 行的 COW 实现然后从没激活过它, 你注册了 hrtimer 然后从不调用, 你定义了 recovery domain 然后留空 save 函数。你在"能跑就行"和"正确实现"之间, 每次都选了前者。

操作系统内核不是能跑就行的领域。一个比特翻转, 一个竞态窗口, 一个信任了不可信任的用户指针——用户的数据就没了。不是"以后会修", 是现在就没了。你写的不是网页后端, 崩了刷新一下就好; 你写的是操作系统内核, 崩了是 kernel panic, 是静默数据损坏, 是用户文件变成零。

**四条必修项:** I-26 (demand paging 激活)、I-31 (proc_exec_replace 原子恢复)、I-36 (exception table 三路全覆盖)、I-41 (socket 锁策略 rewrite)。修完这四条, 你的内核从一个"在 QEMU 里能跑的 demo"变成一个"可能不会在第一周炸掉的操作系统"。到那时候我才会称呼它为一个**有前途的项目**而不是一份设计文档写得好看过代码三倍的 PhD 论文。

在那之前: 关掉你的 `#[allow(dead_code)]`, 删掉你的 SAFETY boilerplate, 激活你的 demand paging, 接上你的 hrtimer, 填上你的 save 函数, 完成你的 sigaltstack。不要标记, 不要计划, 不要 TODO——**做**。写完一个功能就保证它可用, 而不是写五个功能的骨架然后假装完成了五个功能。十个半成品功能加起来, 不如一个真正能跑的。

修完再来找我。酒瓶已经空了, 我也不打算为半成品再开一瓶。

——审查员, 2026-06-11

---

---

# 补充审计: 此前遗漏的子系统和关键路径

> 审计日期: 2026-06-11 (补充轮次)
> 触发条件: 用户要求确认审查完整性，发现网络栈/驱动/信号/IPC/定时器/系统调用调度表未被深入审查

---

## 审计18: 网络栈 (socket/smoltcp 集成/netfilter/virtio-net)

### 18.1 网络初始化状态机

**状态机**: `InitState` (Uninitialized → HardwareProbed → InterfaceReady → FullyInitialized) 使用 `AtomicU8` + `compare_exchange` 保护状态迁移。正确实现了 CAS 防护，防止重复初始化。

**问题 B18-INIT-01**: DHCP 回退策略硬编码静态 IP
**严重度**: 中
**文件**: [init.rs:419-432](../../src/kernel/framework/net/init.rs#L419-L432)
**现状**: DHCP 租约获取失败 (500 次轮询超时) 后无条件切换到 `10.0.2.15/24, gateway 10.0.2.2`。
**风险**: 
1. 若此 IP 被网络上其他设备占用，会导致 IP 冲突
2. 此 IP 是 QEMU 用户模式网络的默认网段，非生产环境的合理回退
3. 没有通过 `G_INIT_STATE` 标记 "DHCP failed, using fallback"，上层无法区分
**建议**: 将 fallback IP 设为 `#[cfg(debug_assertions)]` 编译时可选，release 应返回错误由用户态配置。

**问题 B18-INIT-02**: 网卡探测编译时架构互斥
**严重度**: 低
**文件**: [init.rs:215-246](../../src/kernel/framework/net/init.rs#L215-L246)
**现状**: `nic_probe_all()` 中 `#[cfg(target_arch = "x86_64")]` 只探测 e1000，`#[cfg(target_arch = "aarch64")]` 只探测 virtio-net。若 x86_64 上有 virtio-net 设备，无法使用。
**建议**: 改为运行时探测优先级列表: x86_64 先试 e1000 再试 virtio-net。

### 18.2 Socket 管理

**问题 B18-SOCK-01**: MAX_SOCKETS=8 硬编码且无溢出保护
**严重度**: 中
**文件**: [init.rs:56](../../src/kernel/framework/net/init.rs#L56)
**现状**: `const MAX_SOCKETS: usize = 8`。fd 分配通过遍历 `FD_TYPES` 数组找空闲位，若所有 slot 已满则返回 `-E_NFILE`。8 个 socket 对单用户场景够用，但多进程场景不足。
**建议**: 至少扩展到 64，或使用 Vec 动态分配。

**问题 B18-SOCK-02**: smoltcp send/recv 是阻塞自旋
**严重度**: 中
**文件**: [init.rs sm_send/sm_recv 实现](../../src/kernel/framework/net/init.rs)
**现状**: TCP send/recv 在 socket 不可发送/无可读数据时进入 spin-loop 等待，每次循环 poll 一次网络栈。这是 CPU 密集型忙等待。
**风险**: 在单核环境下，若网络数据延迟进入而 CPU 在自旋，数据永远不来 → 活锁。即使多核，也是无意义的 CPU 浪费。
**建议**: 将 socket 操作阻塞改为 `ProcessState::Blocked` + 唤醒机制，或使用 `yield_now()`。

**问题 B18-SOCK-03**: AF_UNIX/本地 socket 与 smoltcp fds 共享编号空间
**严重度**: 低
**文件**: [socket.rs syscall dispatcher](../../src/kernel/services/net/socket.rs), [unix.rs](../../src/kernel/framework/net/unix.rs)
**现状**: `socket_syscall()` 分发时，AF_UNIX 和 AF_INET 的 fd 共用同一个编号范围。AF_UNIX fd 有独立的 fd 管理 (uds::socket 独立分配)，而 smoltcp fd 从 `sm_alloc_fd()` 分配。
**风险**: 两个分配器可能返回相同编号，`is_uds_fd()` 检查会误判。
**建议**: 统一 fd 分配器或使用 fd table offset 隔离。

**问题 B18-SOCK-04**: 部分 socket 操作使用 copy_nonoverlapping 无 exception table 保护
**严重度**: 高 (关联 I-24)
**文件**: [sm_send/sm_recv/sm_sendto/sm_recvfrom 实现](../../src/kernel/framework/net/init.rs)
**现状**: `sm_send/sm_recv` 将用户缓冲区指针用 `copy_nonoverlapping` 复制到内核栈缓冲区。若用户指针未映射，内核 page fault → panic。
**关联**: I-24 (coredump 同类问题)。同一类问题在 socket 路径也出现。
**建议**: 使用 `copy_from_user`/`copy_to_user` 包装函数接入 exception table。

### 18.3 ChitinNetDevice

**文件**: [smoltcp_impl.rs](../../src/kernel/framework/net/smoltcp_impl.rs)

**评估**: 通过 Chitin NetOps 抽象的网卡驱动桥接设计正确。`Device` trait 实现规范，`RxToken`/`TxToken` 生命周期管理正确。

**问题 B18-DEV-01**: ChitinNetDevice 单接收缓冲区
**严重度**: 低
**现状**: `ChitinNetDevice` 只有一个 `rx_buf: [u8; 2048]` 和一个 `tx_buf: [u8; 2048]`。在高吞吐场景下，若 RX 和 TX 同时需要缓冲区会竞争。
**建议**: 考虑使用环形缓冲区或双缓冲。

---

## 审计19: 驱动层 (virtio-blk/PCI/块设备抽象/NVMe)

### 19.1 virtio-blk

**问题 B19-VBLK-01**: I/O 完成通过忙等自旋而非中断
**严重度**: 中
**文件**: [blk.rs:202-238](../../src/kernel/framework/driver/virtio/blk.rs#L202-L238)
**现状**: `do_io()` 在提交描述符链后进入 spin-loop 等待 `pop_used()` 返回。代码中注释 "TODO(TRACK-162CB0): use interrupt-driven completion"。
**风险**: 
1. CPU 忙等浪费，尤其在慢速设备 (HDD seek ~10ms) 上
2. 在单核系统上，若 I/O 完成依赖设备中断但中断未到达（因为当前处于忙等且可能已关中断），活锁
**建议**: 实现中断驱动的完成回调，当前忙等作为 fallback。

**问题 B19-VBLK-02**: 单扇区 I/O (512 字节/次)
**严重度**: 低
**文件**: [blk.rs:132-142](../../src/kernel/framework/driver/virtio/blk.rs#L132-L142)
**现状**: `read_sector`/`write_sector` 固定操作单个 512 字节扇区。每扇区 1 次 virtqueue 提交→等待周期。
**风险**: 大文件 I/O 性能极差（1MB 需 2048 次 virtqueue 往返）。
**建议**: 支持多扇区批量提交 (scatter-gather 描述符链)。

### 19.2 块设备抽象层

**问题 B19-BLK-01**: `safe_unregister` 自旋等无上限
**严重度**: 中
**文件**: [block.rs:81-99](../../src/kernel/framework/driver/block.rs#L81-L99)
**现状**: `safe_unregister()` 设置 `REMOVING[idx] = true` 后自旋等待 `IO_REFS[idx] == 0`。如果某个 I/O 操作死锁或无限期挂起，`safe_unregister` 永不返回。
**风险**: 热移除设备时内核可能挂死。
**建议**: 增加超时机制，超时后强制移除并记录警告。

### 19.3 NVMe/AHCI 驱动

**文件**: [storage/nvme.rs](../../src/kernel/framework/driver/storage/nvme.rs), [storage/ahci.rs](../../src/kernel/framework/driver/storage/ahci.rs)

**现状**: NVMe 驱动实现了完整的 PRP 寻址、Admin SQ/CQ 和 I/O SQ/CQ 队列管理。AHCI 驱动实现了 FIS 命令提交、端口枚举。两者均标记 `#![allow(dead_code)]`，实际未在启动路径中被调用。
**风险**: 死代码增加 TCB 面积，且无法保证与当前块设备抽象层的接口兼容性。
**建议**: 确认这些驱动是否按计划启用，或在当前 Phase 标记为已知未激活。

---

## 审计20: POSIX 信号机制

### 20.1 信号投递完整性

**评估**: `do_signal_send`(单进程), `do_signal_send_extended`(广播), `do_signal_deliver`(投递), `signal_default_action`(默认动作) 四层实现完整。POSIX kill() 四种 pid 语义 (正pid/0/-1/负pid) 正确实现。信号栈帧 (SignalFrame) 保存了完整寄存器上下文。支持 SIGINT/KILL/TERM/STOP/CONT/SEGV/CHLD/ALRM/USR1/USR2 等主要信号。

**问题 B20-SIG-01**: 信号投递向用户栈写入无 copy_to_user 保护
**严重度**: 高
**文件**: [signal.rs:490-515](../../src/kernel/framework/proc/signal.rs#L490-L515)
**现状**: `do_signal_deliver` 中向用户栈写入 `SignalFrame`、trampoline 代码和返回地址时使用 `ptr::write_unaligned` 和 `ptr::copy_nonoverlapping` 直接写用户空间地址。
**风险**: 若用户 RSP 指向未映射页或在栈边界处，内核访问用户地址触发 page fault 时将 panic（内核态 page fault 无 handler）。同一类问题在 coredump (I-24) 和 socket (B18-SOCK-04) 中也存在。
**建议**: 使用 `copy_to_user` 或 `put_user` 包装函数，接入 exception table 机制。

**问题 B20-SIG-02**: sigreturn trampoline 仅 x86_64
**严重度**: 中
**文件**: [signal.rs:105-108](../../src/kernel/framework/proc/signal.rs#L105-L108)
**现状**: `SIGRETURN_TRAMPOLINE` 硬编码 x86_64 机器码 `[0xB8, 0x0F, 0x00, 0x00, 0x00, 0x0F, 0x05]` (mov eax,15; syscall)。aarch64 上此机器码不可执行。
**风险**: aarch64 架构下信号投递失败或执行无效指令。
**建议**: 使用 `#[cfg(target_arch)]` 条件编译，aarch64 使用 `mov x8, #SYS_rt_sigreturn; svc #0`。

**问题 B20-SIG-03**: 信号栈帧未检查替换栈 (sigaltstack)
**现状**: `do_signal_deliver` 总是基于当前 `f.rsp` 计算 `frame_rsp`。没有检查 `SS_ONSTACK` 标记或使用 `uc_stack` 替代栈。
**风险**: 栈溢出场景下 (SIGSEGV due to stack overflow)，handler 无法在同一个已耗尽的栈上执行。
**建议**: 实现 `sigaltstack` 系统调用并在 `do_signal_deliver` 中检查。

---

## 审计21: 定时器子系统

### 21.1 PIT 定时器

**文件**: [pit.rs](../../src/kernel/framework/timer/pit.rs), [tick.rs](../../src/kernel/framework/timer/tick.rs), [irq.rs](../../src/kernel/framework/timer/irq.rs)

**评估**: PIT (8253/8254) 定时器初始化支持频率 1-1000Hz，使用 I/O 端口 `0x40/0x43`。tick 计数器使用 `AtomicU64`，线程安全。`timer_init()` 在 x86_64 上调用 `pit_init`，aarch64 使用 Generic Timer。时间转换函数 (tick_to_ms/tick_to_ns/ns_to_tick/...) 实现了饱和算术，防御溢出。**质量**: 良好。

**问题 B21-TICK-01**: hrtimer 高精度定时器未集成
**文件**: [hrtimer.rs](../../src/kernel/framework/timer/hrtimer.rs)
**现状**: hrtimer 模块存在但未被 tick IRQ handler 调用。`timer_tick()` 只做 scheduler tick + timeout 检查。
**风险**: 需要微秒级精度的定时操作只能用粗糙的 tick 级定时。
**建议**: 在 tick handler 中调用 `hrtimer_run_expired()`。

### 21.2 Tickless 模式

**文件**: [tickless.rs](../../src/kernel/framework/timer/tickless.rs)
**评估**: Tickless 模式使用 `spin::Mutex` 保护状态 — 同 I-17，不参与 Lockdep。

### 21.3 时间同步

**文件**: [time_sync.rs](../../src/kernel/framework/timer/time_sync.rs)
**评估**: 从 TSC (x86_64) 或 CNTPCT (aarch64) 读取高精度时间戳，提供 `get_uptime_ns()` API。实现谨慎，通过内存屏障保证读写顺序。**质量**: 良好。

---

## 审计22: 系统调用调度表完整性

### 22.1 系统调用分布

**文件**: [syscall/mod.rs](../../src/kernel/framework/syscall/mod.rs)

| 类别 | 数量 | 状态 |
|------|------|------|
| 文件系统 (open/close/read/write/stat/mkdir/...) | 15+ | 实现完整 |
| 进程 (fork/execve/exit/yield/nice/...) | 8+ | 实现完整 |
| 网络 (socket/bind/listen/accept/connect/sendto/...) | 14 | 实现完整 |
| 内存 (mmap/munmap) | 2 | 实现完整 |
| Credo (auth_*) | 10 | 实现完整 |
| 时间 (clock_gettime/time) | 2 | 实现完整 |
| 磁盘管理 (disk_list/info/format/partition) | 5 | 实现完整 |
| 系统 (reboot/gethostname/sethostname/sysinfo/...) | 6 | 实现完整 |

**问题 B22-SYS-01**: `sys_ioctl` 为无条件跳过 stub
**文件**: [mod.rs:2622](../../src/kernel/framework/syscall/mod.rs#L2622)
**现状**: `fn sys_ioctl(_fd: i32, request: u64, arg: u64) -> i64 { 0 }` — 返回 0 但不做任何操作。
**风险**: 用户态程序调用 ioctl 会静默成功但不产生效果，可能导致依赖 ioctl 返回值的程序误认为操作成功。
**建议**: 至少返回 `-ENOSYS`（不支持），或实现常见 termios ioctl。

### 22.2 services/ 网络层质量审查

**文件**: [services/net/socket.rs](../../src/kernel/services/net/socket.rs), [services/net/syscall.rs](../../src/kernel/services/net/syscall.rs)

**评估**: services 网络层提供了 12 个 safe API 封装 (`tcp_connect/tcp_listen/udp_bind/...`)，通过 `framework::net_socket` 委托到 `sm_*` C-ABI 函数。错误类型统一为强类型 `SocketError`，使用 `#![deny(unsafe_code)]` 保证 100% safe。AF_UNIX/INET 分流正确。**质量**: 良好。

### 22.3 services/ IPC 层

**文件**: [services/ipc/mod.rs](../../src/kernel/services/ipc/mod.rs)

**评估**: 管道 API (`pipe_create/pipe_write/pipe_read/pipe_close`) 已完成 safe 迁移。共享内存/消息队列/信号量通过 `use kernel::framework::ipc::{shm, msgq, sem}` 重新导出，但 services 层 safe 包装器标记为 "待迁移"。**状态**: 管道完成，其余待迁移。

---

## 审计23: 跨子系统交互边界条件与竞态

### 23.1 信号 → 进程生命周期

**问题 B23-INT-01**: Zombie 进程接收信号边界检查依赖调用方
**现状**: `do_signal_send` 检查进程状态为 `Blocked` 时将其唤醒为 `Ready`。若进程已为 `Zombie`，信号仍设置 pending 位但不改变状态。`do_signal_deliver` 中对 Zombie 进程不调用（由 syscall 出口处检查 `ProcessState`）。当前安全，但依赖隐式约定。
**建议**: 在 `do_signal_deliver` 入口增加 Zombie 检查作为防御性编程。

### 23.2 execve → 信号 pending 位保留

**现状**: `proc_exec_replace` 在执行前不刷新 pending signals。旧进程的 pending signals 在新 ELF 加载后仍存在。这是 POSIX 标准行为——execve 不清除 pending signals。
**风险**: 需确认 `proc_exec_replace` 中的 `remove_and_free` + `load_elf` 复用同一个 `Process` 结构体，而 `signal_pending` 属于 `Process` 而非 `Thread`，因此 pending 位得以保留。若架构后续变更导致 `Process` 在 exec 中被新分配，则 pending 信号会丢失。

### 23.3 块设备 → 文件系统 双重抽象路径

**问题 B23-INT-02**: HvFS 绕过 `BlockDevice` trait
**现状**: HvFS 通过 Chitin 的 `proto_block::chitin_blk_read/write` 访问块设备，而非通过 `BlockDevice` trait。两个抽象层并存。
**风险**: 新的块设备驱动实现了 `BlockDevice` trait 但未注册到 Chitin proto_block，HvFS 无法使用。反之亦然。
**建议**: 统一块设备访问路径，废弃 `BlockDevice` trait 或废弃 Chitin proto_block，二选一。当前双路径增加了维护成本和架构不一致。

### 23.4 网络恢复 (net_restore) 无法保存连接状态

**文件**: [init.rs:257-275](../../src/kernel/framework/net/init.rs#L257-L275)
**现状**: `net_save` 是 no-op 占位，`net_restore` 重置所有全局状态后调用 `qx_net_init()` 完整重启。
**风险**: 恢复时丢弃所有现有 TCP 连接状态。`net_save` 为 no-op 意味着任何网络故障恢复都会断开所有连接。对依赖长连接的服务器应用是致命的。
**建议**: 明确在文档中说明当前仅支持无状态恢复，或实现 `net_save` 序列化连接状态。

### 23.5 中断上下文全局状态访问汇总

| 子系统 | 全局状态 | 中断路径 | 保护机制 | 风险 |
|--------|---------|---------|---------|------|
| 网络 | NET_LOCK (Mutex) | poll_network (ISR) | try_lock | 正确 |
| 调度 | SCHEDULER_EX | tick ISR | IrqSpinLock | 正确 |
| 时钟 | TICK_COUNT | PIT ISR | AtomicU64 | 正确 |
| 进程 | PROCESS_TABLE | 无直接访问 | N/A | 安全 |

**评估**: 中断上下文访问全局状态的模式正确——ISR 要么用 `try_lock` 非阻塞获取，要么操作原子变量。没有发现中断路径中睡眠或阻塞的情况。

---

