# 审计修复分册 03：framework 内存、同步与中断

> 修复 framework/mm（kmalloc/swap/cow/pmm）、framework/sync（pi_mutex/audit）、framework/timer、framework/irq 与 klog 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.4 节 + 第 7 章 TOP 20 + 附录 H（H.4.2/H.4.3/H.4.5/H.5.5）+ 附录 C framework-mm/sync/timer/irq 报告。

## 工程计划 A: 内存子系统修复

### 背景

- **内存子系统 P0 集中**
  - 描述：kmalloc dump_stats 潜伏未定义变量、swap 16MB 泄漏、COW 物理页泄漏、pmm 无 reserve_range API、pmm 自引用读取相邻字段（LTO 脆弱）。
  - 方案：按泄漏→API→健壮性顺序修复。
  - 状态：[]

### 待办

- **kmalloc dump_stats 修复（P0-14 降级后）**
  - 描述：[kmalloc.rs:691-707](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kmalloc.rs#L691-L707) `serial_println!` 为空宏导致 dump_stats 静默 no-op，且 `stats` 未定义（编译仅因宏吞参未报错）。
  - 方案：`let _stats` 改 `let stats`；将串口打印接入真实 klog 路径（或删除该占位宏，改用 `crate::klog_info!`）。
  - 状态：[]

- **swap init 标记 reserved（P0-15）**
  - 描述：[swap.rs:155-194](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/swap.rs#L155-L194) 分配 4096 个 4KB 页（16MB）后未调 reserve 标记，PMM 每次 boot 永久泄漏。
  - 方案：P0-29 落地后，在 `init` 完成时调用 `pmm.reserve_range(base, size)`；swap 释放路径同步解除。
  - 状态：[]

- **pmm reserve_range API（H.4.2 P0-29）**
  - 描述：framework/mm/pmm 没有 `reserve_range` API，swap（P0-15）等子系统无法声明自有内存。
  - 方案：实现 `pmm.reserve_range(base, size)`（含 bitmap 标记 + 边界校验），并补充 host-tests。
  - 状态：[]

- **pmm 自引用读取相邻字段（H.4.5 P1-B）**
  - 描述：framework/mm/pmm.rs 中 `set_bit`/`clear_bit`/`test_bit`/`count_free_pages`（L808-879）用 `self as *const Self as *const u64` + 硬编码 `p.add(1)` 读相邻字段 `bitmap_size`；L793 注释记载 LTO 曾把 `self.bitmap_size.get()` 错位到 `self.failed_allocs`。
  - 方案：将 4 处替换为 `core::ptr::addr_of!(self.bitmap_size)`（与既有 `buddy_meta_ref` L928 / `buddy_heads_ref` L953 修复模式一致）；不拆分字段。
  - 状态：[]

- **COW 物理页泄漏（H.4.3 P0-30）**
  - 描述：framework/mm/cow.rs 引用计数经 `IrqSpinLock<BTreeMap<u64,u32>>`；实测 `cow_dec_ref` 生产调用点仅 `cow_handle_fault`（L322）1 处，**exit/munmap 的 `destroy_page_table`（vmm_x86_64.rs:1062-1129）不遍历 leaf PTE、不调 dec** → 未写即退出的共享页引用永不归零，物理页泄漏属实。
  - 方案：在 `destroy_page_table` 遍历 leaf PTE，对存在于 COW_REFS 的共享页调 `cow_dec_ref`，返回 true 则 `free_page`；注意锁序（COW_REFS 与 VMM_LOCK 统一在持 VMM_LOCK 下操作）；修复 `cow_handle_fault` 锁内判定+锁外执行的 TOCTOU（判定与映射在同一临界区）；补 fork-exit 共享页计数 host-tests。
  - 状态：[]

## 工程计划 B: 同步与中断修复

### 背景

- **同步/中断 P0 集中**
  - 描述：pi_mutex_process_exit 空实现（永久持锁）、GLOBAL_AUDIT static mut 多核撕裂、do_softirq 全局 running、timer tick 内存序、recovery_domain_register Box::leak。
  - 方案：按持锁→内存序→泄漏顺序修复。
  - 状态：[]

### 待办

- **pi_mutex_process_exit 实装（TOP 20 #6 / D7）**
  - 描述：[pi_mutex.rs:58-65](file:///home/anfer/Code/QueenX/src/kernel/framework/sync/pi_mutex.rs#L58-L65) 仅 `let _ = raw_usize; let _ = pid;`，进程退出时持有 PI Mutex 永久不释放，任何持锁进程退出即死锁。
  - 方案：实装 register/unregister/force_unlock 三件套；进程退出路径回调；补 pi_mutex host-tests（exit 后锁可再获取）。
  - 状态：[]

- **audit.rs GLOBAL_AUDIT static mut（TOP 20 #11）**
  - 描述：`GLOBAL_AUDIT`（credo/audit.rs:91）为 `pub(crate) static mut`（256 项 AuditLog 数组结构，非计数器）；实测写是常态、读（dump）极少，仅 2 处 unsafe 访问（raw::log/raw::dump L109-118），调用方均在非中断的 credo 服务栈。
  - 方案：定死为 `IrqSpinLock<AuditLog>` 包装（`AtomicU64` 不适用——是结构体数组非计数器；OnceLock 不适用——需可变写）。改动面 = static 定义 + 2 个 raw 函数体（约 10-15 行），顺带消除无锁环形写的读者撕裂。
  - 状态：[]

- **recovery_domain_register Box::leak（TOP 20 #12）**
  - 描述：framework/barrier/api.rs:44-53 `Box::leak` 泄漏 `RecoveryDomain`（单域约 10-12KB，`'static` 永不复用）；注册表天然上限 `MAX_RECOVERY_DOMAINS=32`，注册点仅 4 处且均在启动期。
  - 方案：定死为静态预分配——`static RECOVERY_DOMAINS: [IrqSpinLock<Option<RecoveryDomain>>; 32]`（约 320-400KB .bss）；`RecoveryDomain::new` 改造为 `const fn`（参照既有 `RECOVERY_MANAGER: IrqSpinLock<RecoveryManager> = IrqSpinLock::new(RecoveryManager::new())` 模式）。
  - 状态：[]

- **do_softirq 全局 running（TOP 20 #14）**
  - 描述：framework/irq 中 `do_softirq` 用全局 running 标记，多核下仅 1 CPU 处理 softirq。
  - 方案：改 per-CPU running 标记；单开 PR（决策点 D5）。
  - 状态：[]

- **timer tick 计数器内存序（TOP 20 #20）**
  - 描述：framework/timer tick 计数器内存序多核一致性问题。
  - 方案：核对 tick 计数器的 `Ordering`（Relaxed→Acquire/Release），补多核 tick 测试。
  - 状态：[]

- **klog_ffi! NUL 终止（TOP 20 #4 / H.5.5 P1-H）**
  - 描述：framework/klog `klog_ffi!` 宏 256 字节栈缓冲无 NUL 终止保证，栈缓冲溢出读取风险。
  - 方案：格式化后显式 `buf[len] = 0`；长度封顶 255；补越界 host-tests。
  - 状态：[]

## 工程计划 C: framework 剩余子系统

### 背景

- **cpu/剩余模块 P0 引用**
  - 描述：framework/cpu 单文件 1554 行 + SAFETY + 溢出；framework 顶层散文件 SMEP/SMAP、IoMem 溢出、帧验证；framework/tests 永久关中断、持锁执行、物理地址硬编码。详见 [附录 C 报告索引](./code-audit-final-summary.md) 与 [archive 子系统报告](./archive/audit-2026-08-14/)。
  - 方案：以 archive 子系统报告为准提取详细 P0/P1 条目，逐项登记后实施。
  - 状态：[]

### 待办

- **framework/tests run_all 永久关闭中断（TOP 20 #7）**
  - 描述：[tests/mod.rs:122](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L122) `run_all` 开头 `interrupt_disable()` 后无 re-enable，测试结束后中断全关。
  - 方案：测试循环后恢复中断状态（save/restore 而非无条件 disable）。
  - 状态：[]

- **framework/cpu 大文件拆分与 SAFETY 补齐**
  - 描述：`framework/cpu/mod.rs` 1554 行单文件（违反简单优先）；`tools/audit_unsafe.py` 报告的 52 处 MISSING 中属 cpu 子系统的需补齐（先修分册 01 窗口误报再处理真实缺失）。
  - 方案：拆分 scheduler/percpu/feature 子模块；按 `tools/audit_unsafe.py --missing-only` 补齐真实缺 SAFETY 块。
  - 状态：[]

- **framework 顶层散文件（IoMem 溢出/帧验证/SMEP）**
  - 描述：`subsystem-framework-toplevel.md` 报告 IoMem 边界溢出、帧验证、SMEP/SMAP 缺失等 7 项 P0。
  - 方案：按 archive 报告逐项登记到本分册待办并实施。
  - 状态：[]

### 验证门槛

- **内存/同步回归**
  - 描述：修复后跑 host-tests 全部（含 mm/sync/timer 相关用例）+ 双架构编译。
  - 方案：`make test-host` + `./ci/build.sh all`。
  - 状态：[]

- **死锁回归**
  - 描述：pi_mutex/IRQ 路径修复后，用修复版 `audit_deadlock_matrix.py`（分册 01）扫描 0 新问题。
  - 方案：分册 01 完成后跑死锁矩阵 + lockbud。
  - 状态：[]

### 决策记录

- **DECISION-049**
  - 描述：pi_mutex_process_exit 实装采用"注册表 + force_unlock"方案（对应审计决策点 D7）。
  - 方案：`PI_MUTEX_REGISTRY` 已存在，补 register 写入、exit 遍历 force_unlock、unregister 清理。
  - 状态：[]
