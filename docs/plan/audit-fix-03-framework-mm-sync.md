# 审计修复分册 03：framework 内存、同步与中断

> 修复 framework/mm（kmalloc/swap/cow/pmm）、framework/sync（pi_mutex/audit）、framework/timer、framework/irq 与 klog 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.4 节 + 第 7 章 TOP 20 + 附录 H（H.4.2/H.4.3/H.4.5/H.5.5）+ 附录 C framework-mm/sync/timer/irq 报告。

## 工程计划 A: 内存子系统修复

### 背景

- **B03-01. 内存子系统 P0 集中**
  - 描述：kmalloc dump_stats 潜伏未定义变量、swap 16MB 泄漏、COW 物理页泄漏、pmm 无 reserve_range API、pmm 自引用读取相邻字段（LTO 脆弱）。
  - 方案：按泄漏→API→健壮性顺序修复。
  - 状态：[]

### 待办

- **B03-02. kmalloc dump_stats 修复（P0-14 降级后）**
  - 描述：[kmalloc.rs:691-707](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kmalloc.rs#L691-L707) `serial_println!` 为空宏导致 dump_stats 静默 no-op，且 `stats` 未定义（编译仅因宏吞参未报错）。
  - 方案：`let _stats` 改 `let stats`；将串口打印接入真实 klog 路径（或删除该占位宏，改用 `crate::klog_info!`）。
  - 状态：[]

- **B03-03. swap init 标记 reserved（P0-15）**
  - 描述：[swap.rs:155-194](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/swap.rs#L155-L194) 分配 4096 个 4KB 页（16MB）后未调 reserve 标记，PMM 每次 boot 永久泄漏。
  - 方案：P0-29 落地后，在 `init` 完成时调用 `pmm.reserve_range(base, size)`；swap 释放路径同步解除。
  - 状态：[]

- **B03-04. pmm reserve_range API（H.4.2 P0-29）**
  - 描述：framework/mm/pmm 没有 `reserve_range` API，swap（P0-15）等子系统无法声明自有内存。
  - 方案：实现 `pmm.reserve_range(base, size)`（含 bitmap 标记 + 边界校验），并补充 host-tests。
  - 状态：[]

- **B03-05. pmm 自引用读取相邻字段（H.4.5 P1-B）**
  - 描述：framework/mm/pmm.rs 中 `set_bit`/`clear_bit`/`test_bit`/`count_free_pages`（L808-879）用 `self as *const Self as *const u64` + 硬编码 `p.add(1)` 读相邻字段 `bitmap_size`；L793 注释记载 LTO 曾把 `self.bitmap_size.get()` 错位到 `self.failed_allocs`。
  - 方案：将 4 处替换为 `core::ptr::addr_of!(self.bitmap_size)`（与既有 `buddy_meta_ref` L928 / `buddy_heads_ref` L953 修复模式一致）；不拆分字段。
  - 状态：[]

- **B03-06. COW 物理页泄漏（H.4.3 P0-30）**
  - 描述：framework/mm/cow.rs 引用计数经 `IrqSpinLock<BTreeMap<u64,u32>>`；实测 `cow_dec_ref` 生产调用点仅 `cow_handle_fault`（L322）1 处，**exit/munmap 的 `destroy_page_table`（vmm_x86_64.rs:1062-1129）不遍历 leaf PTE、不调 dec** → 未写即退出的共享页引用永不归零，物理页泄漏属实。
  - 方案：在 `destroy_page_table` 遍历 leaf PTE，对存在于 COW_REFS 的共享页调 `cow_dec_ref`，返回 true 则 `free_page`；注意锁序（COW_REFS 与 VMM_LOCK 统一在持 VMM_LOCK 下操作）；修复 `cow_handle_fault` 锁内判定+锁外执行的 TOCTOU（判定与映射在同一临界区）；补 fork-exit 共享页计数 host-tests。
  - 状态：[]

## 工程计划 B: 同步与中断修复

### 背景

- **B03-07. 同步/中断 P0 集中**
  - 描述：pi_mutex_process_exit 空实现（永久持锁）、GLOBAL_AUDIT static mut 多核撕裂、do_softirq 全局 running、timer tick 内存序、recovery_domain_register Box::leak。
  - 方案：按持锁→内存序→泄漏顺序修复。
  - 状态：[]

### 待办

- **B03-08. pi_mutex_process_exit 实装（TOP 20 #6 / D7）**
  - 描述：[pi_mutex.rs:58-65](file:///home/anfer/Code/QueenX/src/kernel/framework/sync/pi_mutex.rs#L58-L65) 仅 `let _ = raw_usize; let _ = pid;`，进程退出时持有 PI Mutex 永久不释放，任何持锁进程退出即死锁。
  - 方案：实装 register/unregister/force_unlock 三件套；进程退出路径回调；补 pi_mutex host-tests（exit 后锁可再获取）。
  - 状态：[]

- **B03-09. audit.rs GLOBAL_AUDIT static mut（TOP 20 #11）**
  - 描述：`GLOBAL_AUDIT`（credo/audit.rs:91）为 `pub(crate) static mut`（256 项 AuditLog 数组结构，非计数器）；实测写是常态、读（dump）极少，仅 2 处 unsafe 访问（raw::log/raw::dump L109-118），调用方均在非中断的 credo 服务栈。
  - 方案：定死为 `IrqSpinLock<AuditLog>` 包装（`AtomicU64` 不适用——是结构体数组非计数器；OnceLock 不适用——需可变写）。改动面 = static 定义 + 2 个 raw 函数体（约 10-15 行），顺带消除无锁环形写的读者撕裂。
  - 状态：[]

- **B03-10. recovery_domain_register Box::leak（TOP 20 #12）**
  - 描述：framework/barrier/api.rs:44-53 `Box::leak` 泄漏 `RecoveryDomain`（单域约 10-12KB，`'static` 永不复用）；注册表天然上限 `MAX_RECOVERY_DOMAINS=32`，注册点仅 4 处且均在启动期。
  - 方案：定死为静态预分配——`static RECOVERY_DOMAINS: [IrqSpinLock<Option<RecoveryDomain>>; 32]`（约 320-400KB .bss）；`RecoveryDomain::new` 改造为 `const fn`（参照既有 `RECOVERY_MANAGER: IrqSpinLock<RecoveryManager> = IrqSpinLock::new(RecoveryManager::new())` 模式）。
  - 状态：[]

- **B03-11. do_softirq 全局 running（TOP 20 #14）**
  - 描述：framework/irq 中 `do_softirq` 用全局 running 标记，多核下仅 1 CPU 处理 softirq。
  - 方案：改 per-CPU running 标记；单开 PR（决策点 D5）。
  - 状态：[]

- **B03-12. timer tick 计数器内存序（TOP 20 #20）**
  - 描述：framework/timer tick 计数器内存序多核一致性问题。
  - 方案：核对 tick 计数器的 `Ordering`（Relaxed→Acquire/Release），补多核 tick 测试。
  - 状态：[]

- **B03-13. klog_ffi! NUL 终止（TOP 20 #4 / H.5.5 P1-H）**
  - 描述：framework/klog `klog_ffi!` 宏 256 字节栈缓冲无 NUL 终止保证，栈缓冲溢出读取风险。
  - 方案：格式化后显式 `buf[len] = 0`；长度封顶 255；补越界 host-tests。
  - 状态：[]

## 工程计划 C: framework 剩余子系统

### 背景

- **B03-14. cpu/剩余模块 P0 引用**
  - 描述：framework/cpu 单文件 1554 行 + SAFETY + 溢出；framework 顶层散文件 SMEP/SMAP、IoMem 溢出、帧验证；framework/tests 永久关中断、持锁执行、物理地址硬编码。详见 [附录 C 报告索引](./code-audit-final-summary.md) 与 [archive 子系统报告](./archive/audit-2026-08-14/)。
  - 方案：以 archive 子系统报告为准提取详细 P0/P1 条目，逐项登记后实施。
  - 状态：[]

### 待办

- **B03-15. framework/tests run_all 永久关闭中断（TOP 20 #7）**
  - 描述：[tests/mod.rs:122](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L122) `run_all` 开头 `interrupt_disable()` 后无 re-enable，测试结束后中断全关。
  - 方案：测试循环后恢复中断状态（save/restore 而非无条件 disable）。
  - 状态：[]

- **B03-16. cpu/mod.rs 1554 行单文件拆分（P0）**
  - 描述：`framework/cpu/mod.rs` 1554 行单文件，含 CPU 厂商检测/签名解析/特性收集/缓存检测/MSR/TSC/多核拓扑/FFI 导出；`cache.rs`/`topology.rs` 已声明却仅占位。
  - 方案：拆分到已声明未用子模块：`cpu/feature.rs` + `cpu/topology_impl.rs` + `cpu/cache_impl.rs`；MSR 常量集中到 `msr.rs`；`CpuVendor::Unknown` 兜底语义明确化。
  - 状态：[]

- **B03-17. cpu/msr.rs 对齐与 #GP（P0）**
  - 描述：`cpu_read_msr`（msr.rs:71-84）仅查 `is_null()` 未查对齐，非对齐指针 aarch64 data abort；`cpu_write_msr`（msr.rs:92-98）非法 MSR 触发 #GP 后仍返回 0 假设成功。
  - 方案：`cpu_read_msr` 加 `assert!(low % 4 == 0 && high % 4 == 0)`；`cpu_write_msr` 预检 MSR 合法性或配套 KPTI IST #GP handler 捕获。
  - 状态：[]

- **B03-18. cpu/cpuid.rs leaf 边界（P0）**
  - 描述：`cpuid.rs:22-37` 未处理 leaf > max_leaf（AMD 可能 panic）；`is_leaf_supported`（cpuid.rs:48-51）存在但调用方未强制使用，且不验证 subleaf。
  - 方案：公开 API 强制 `is_leaf_supported(leaf)` 校验并返回 `Option`；扩展为 `(leaf, subleaf)` 双层校验。
  - 状态：[]

- **B03-19. cpu/tsc.rs 溢出与序列化（P0/P1）**
  - 描述：`cycles_to_nanoseconds`（tsc.rs:42-50）`tsc_cycles * 1000` 可溢出 u64（4GHz 运行 24h 达 3.5×10¹⁷）；`read_tsc_serialized` 与 `read_tsc` 实现相同（tsc.rs:28-32），无 mfence/lfence 序列化，与文档"更精确"不符。
  - 方案：`checked_mul(1000).unwrap_or(u64::MAX) / freq`；实装序列化版本或修正文档。
  - 状态：[]

- **B03-20. cpu/arch.rs send_ipi 越界（P1）**
  - 描述：`send_ipi`（arch.rs:46-55）不验证 CPU 索引在 `MAX_CPUS` 范围，越界 IPI 丢失。
  - 方案：入口校验 `target_cpu < MAX_CPUS`。
  - 状态：[]

- **B03-21. framework 顶层散文件：userptr validate NULL（P0）**
  - 描述：`userptr.rs:200-211` `validate_user_buf` 在 `len == 0` 时直接 return true 不检查 ptr，`validate_user_buf(0, 0)` 返回 true 但 0 是 NULL。
  - 方案：`len == 0` 时也检查 `ptr != 0`，或单独提供 `validate_user_buf_zero_ok`。
  - 状态：[]

- **B03-22. framework 顶层散文件：IoMem 溢出（I5，P0）**
  - 描述：`iomem.rs:95-122` `IoMem::new` 不检查 `phys + len` 溢出（相加回绕 0 绕过全部范围检查）；`AliasRegistry::check_conflict` 用 saturating_add 不阻止溢出对。
  - 方案：`new` 加 `phys.checked_add(len).ok_or(...)?`；`AliasRegistry::register` 内部加 overflow 检查。
  - 状态：[]

- **B03-23. framework 顶层散文件：irqline 中断上下文约束（P0）**
  - 描述：`irqline.rs:126-156` SAFETY 声称"启动阶段单线程调用"与 `IrqSpinLock` 设计冲突；`on_interrupt` 在中断上下文执行，内部不可持锁/睡眠，纯文档约束无编译检查。
  - 方案：`dispatch_irq` 内检查当前是否中断上下文；文档显式列出"严禁持锁的同步原语名单"。
  - 状态：[]

- **B03-24. framework 顶层散文件：RacyCell Sync 无约束（P0）**
  - 描述：`racy_cell.rs:32-35` `unsafe impl<T> Sync for RacyCell<T>` 无 `T: Send` 约束，`T = *mut u8` 也可 Sync，跨线程共享裸指针 UB。
  - 方案：改 `unsafe impl<T: Send> Sync`；或删除 unsafe impl，强制走 `SpinLock<RacyCell<T>>`。
  - 状态：[]

- **B03-25. framework 顶层散文件：net_socket map_rc 恒等函数（P0）**
  - 描述：`net_socket.rs:99-103` `map_rc(rc: i32) -> i32 { rc }` 注释声称"i32 → 强类型翻译"但实际恒等，services 层必须自己判断正负。
  - 方案：实现真正的 `i32 → NetError` 翻译，或删除该函数直接处理 i32。
  - 状态：[]

- **B03-26. framework 顶层散文件：Frame::as_virt_ptr 悬挂指针（P0）**
  - 描述：`frame.rs:127-130` 返回 `*mut u8` 无 Frame 生命周期绑定，Frame Drop 后指针悬挂；SAFETY 只约束"每物理地址一个 Frame"未约束裸指针使用期。
  - 方案：返回 `&mut [u8]`（生命周期绑定 `&self`）或 `NonNull<u8>` + `PhantomData<&'a mut Frame>`。
  - 状态：[]

### 验证门槛

- **B03-27. 内存/同步回归**
  - 描述：修复后跑 host-tests 全部（含 mm/sync/timer 相关用例）+ 双架构编译。
  - 方案：`make test-host` + `./ci/build.sh all`。
  - 状态：[]

- **B03-28. 死锁回归**
  - 描述：pi_mutex/IRQ 路径修复后，用修复版 `audit_deadlock_matrix.py`（分册 01）扫描 0 新问题。
  - 方案：分册 01 完成后跑死锁矩阵 + lockbud。
  - 状态：[]

### 决策记录

- **DECISION-049**
  - 描述：pi_mutex_process_exit 实装采用"注册表 + force_unlock"方案（对应审计决策点 D7）。
  - 方案：`PI_MUTEX_REGISTRY` 已存在，补 register 写入、exit 遍历 force_unlock、unregister 清理。
  - 状态：[]
