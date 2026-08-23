# 审计修复分册 03：framework 内存、同步与中断

> 修复 framework/mm（kmalloc/swap/cow/pmm）、framework/sync（pi_mutex/audit）、framework/timer、framework/irq 与 klog 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.4 节 + 第 7 章 TOP 20 + 附录 H（H.4.2/H.4.3/H.4.5/H.5.5）+ 附录 C framework-mm/sync/timer/irq 报告。

## 工程计划 A: 内存子系统修复

### 背景

- **B03-01. 内存子系统 P0 集中**
  - 描述：kmalloc dump_stats 潜伏未定义变量、swap 16MB 泄漏、COW 物理页泄漏、pmm 无 reserve_range API、pmm 自引用读取相邻字段（LTO 脆弱）。
  - 方案：按泄漏→API→健壮性顺序修复。
  - 状态：[X]

### 待办

- **B03-02. kmalloc dump_stats 修复（P0-14 降级后）**
  - 描述：[kmalloc.rs:691-707](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/kmalloc.rs#L691-L707) `serial_println!` 为空宏导致 dump_stats 静默 no-op，且 `stats` 未定义（编译仅因宏吞参未报错）。调用方：`mm/api.rs:481/562` FFI `kmalloc_dump_stats` + `mm/mechanism.rs:67` re-export。
  - 方案：**最小修复** — `let _stats` 改 `let stats`；`serial_println!` 改 `crate::klog_info!(Memory, ...)`（保留 FFI 入口与调试能力，0 调用方破坏）。决策：用户 2026-08-23 选"最小修复"（保留 FFI），放弃"删除整个函数"（会破坏 2 个 FFI 入口 + 1 个 re-export）。
  - 状态：[X]

- **B03-03. swap init 标记 reserved（P0-15）**
  - 描述：[swap.rs:155-194](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/swap.rs#L155-L194) 分配 4096 个 4KB 页（16MB）后未调 reserve 标记，PMM 每次 boot 永久泄漏。深层隐患：swap init 现通过 4096 次 `alloc_page()` 拿页（PMM 簿记为 `allocated`），但 swap 模块**永不释放**这些页（grep 全文件无 `swap_fini` / `free_storage`），且 `slot_addr = storage_virt + slot * PAGE_SIZE` 隐含**4096 页物理连续**假设但 `alloc_page` 不保证连续——违反 PMM buddy 上限（MAX_BUDDY_ORDER=9 即 2MB 上限，16MB 分配不可能成功）。
  - 方案：**路径 B 根治（boot 预留 + find_contig_range + reserve_range）**。具体步骤：(1) 新增 `pmm.find_contig_range(size) -> Option<PhysAddr>`，扫描 bitmap 找连续 `size` 物理范围（不调 buddy，因 buddy 上限 2MB）；(2) `pmm.reserve_range(base, size)` 已实装（B03-04），本轮直接调用；(3) swap init 改写：不再调 `alloc_page()`，改为先调 `find_contig_range(SWAP_MAX_SLOTS * PAGE_SIZE)` 拿连续基址，再调 `reserve_range(base, size)` 声明 reserved，回滚路径同步 unreserve；(4) 调用方：`rust/src/lib.rs:598` `swap_init()` 在 PMM 初始化之后调用，无需新参数。决策：用户 2026-08-23 选路径 B（架构最清晰，PMM 簿记与所有权语义一致），放弃路径 A（alloc_pages 超 buddy 上限不可行）与路径 C（临时方案 unreserve API 长期需重构）。DECISION-050 详见文末。
  - 状态：[X]

- **B03-04. pmm reserve_range API（H.4.2 P0-29）**
  - 描述：framework/mm/pmm 没有 `reserve_range` API，swap（P0-15）等子系统无法声明自有内存。
  - 方案：实现 `pub fn reserve_range(&self, base: PhysAddr, size: usize) -> Result<(), &'static str>`（含页对齐校验 + 边界校验 + 拒绝与已分配页重叠 + PMM 锁内批量 set_bit + stats_alloc 更新 + klog 记录）。host-tests 留作下批 (本阶段 B03-03 调用方落地后再补,避免孤立测试)。
  - 状态：[X]

- **B03-05. pmm 自引用读取相邻字段（H.4.5 P1-B）**
  - 描述：framework/mm/pmm.rs 中 `set_bit`/`clear_bit`/`test_bit`/`count_free_pages`（L808-879）用 `self as *const Self as *const u64` + 硬编码 `p.add(1)` 读相邻字段 `bitmap_size`；L793 注释记载 LTO 曾把 `self.bitmap_size.get()` 错位到 `self.failed_allocs`。
  - 方案：将 4 处替换为 `core::ptr::addr_of!(self.bitmap_size).read_volatile().get()`（与既有 `buddy_meta_ref` L928 / `buddy_heads_ref` L953 修复模式一致）；不拆分字段。**实际施工发现 `bitmap_size: Cell<usize>`，addr_of! 返回 `*const Cell<usize>`，需 `.get()` 提取内部值**（Cell 是 repr(transparent) 但读路径仍需 .get()）。施工同时移除原 ptr_as_ptr/ref_as_ptr expect 治根。
  - 状态：[X]

- **B03-06. COW 物理页泄漏（H.4.3 P0-30）**
  - 描述：framework/mm/cow.rs 引用计数经 `IrqSpinLock<BTreeMap<u64,u32>>`；实测 `cow_dec_ref` 生产调用点仅 `cow_handle_fault`（L322）1 处，**exit/munmap 的 `destroy_page_table`（vmm_x86_64.rs:1062-1129）不遍历 leaf PTE、不调 dec** → 未写即退出的共享页引用永不归零，物理页泄漏属实。
  - 方案：在 `destroy_page_table` 遍历 leaf PTE，对存在于 COW_REFS 的共享页调 `cow_dec_ref`，返回 true 则 `free_page`；注意锁序（COW_REFS 与 VMM_LOCK 统一在持 VMM_LOCK 下操作）；修复 `cow_handle_fault` 锁内判定+锁外执行的 TOCTOU（判定与映射在同一临界区）；补 fork-exit 共享页计数 host-tests。**调研发现 unmap_page_in_table 现有架构约定"不释放物理页"（调用方负责），本轮修复严格限于 destroy_page_table 路径**；TOCTOU 修复留作下轮独立 PR（与 fork-exit host-tests 一并）。
  - 状态：[X]

## 工程计划 B: 同步与中断修复

### 背景

- **B03-07. 同步/中断 P0 集中**
  - 描述：pi_mutex_process_exit 空实现（永久持锁）、GLOBAL_AUDIT static mut 多核撕裂、do_softirq 全局 running、timer tick 内存序、recovery_domain_register Box::leak。
  - 方案：按持锁→内存序→泄漏顺序修复。
  - 状态：[]

### 待办

- **B03-08. pi_mutex_process_exit 实装（TOP 20 #6 / D7）**
  - 描述：[pi_mutex.rs:58-65](file:///home/anfer/Code/QueenX/src/kernel/framework/sync/pi_mutex.rs#L58-L65) 仅 `let _ = raw_usize; let _ = pid;`，进程退出时持有 PI Mutex 永久不释放，任何持锁进程退出即死锁。PI_MUTEX_REGISTRY 当前仅 `Vec<usize>` 强制丢失类型。
  - 方案：**返工后定稿（2026-08-23）**：采用"持有登记表 + 类型擦除函数指针"。`try_lock` 快路径与 `do_unlock` 移交路径登记 `(mutex 地址, 非泛型退出分派)` 到 `PI_MUTEX_HELD: IrqSpinLock<BTreeMap<pid, Vec<HeldLock>>>`；分派经固定布局前缀（data 之前字段与 T 无关）以 `PiMutex<u8>` 重建引用调 `force_unlock_for_exit`，对任意 T（含 ?Sized）兼容；`process_cleanup::notify_process_exit` 接线 `pi_mutex_process_exit(pid)`。DECISION-049 机制内化方案在返工中演化为注册表方案，最终实现详见 DECISION-059。
  - 状态：[X]

- **B03-09. audit.rs GLOBAL_AUDIT static mut（TOP 20 #11）**
  - 描述：`GLOBAL_AUDIT`（credo/audit.rs:91）为 `pub(crate) static mut`（256 项 AuditLog 数组结构，非计数器）；实测写是常态、读（dump）极少，仅 2 处 unsafe 访问（raw::log/raw::dump L109-118），调用方均在非中断的 credo 服务栈。
  - 方案：定死为 `IrqSpinLock<AuditLog>` 包装（`AtomicU64` 不适用——是结构体数组非计数器；OnceLock 不适用——需可变写）。改动面 = static 定义 + 2 个 raw 函数体（约 10-15 行），顺带消除无锁环形写的读者撕裂。
  - 状态：[X]

- **B03-10. recovery_domain_register Box::leak（TOP 20 #12）**
  - 描述：framework/barrier/api.rs:44-53 `Box::leak` 泄漏 `RecoveryDomain`（单域约 10-12KB，`'static` 永不复用）；注册表天然上限 `MAX_RECOVERY_DOMAINS=32`，注册点仅 4 处且均在启动期。
  - 方案：定死为静态预分配——`static RECOVERY_DOMAINS: [IrqSpinLock<Option<RecoveryDomain>>; 32]`（约 320-400KB .bss）；`RecoveryDomain::new` 改造为 `const fn`（参照既有 `RECOVERY_MANAGER: IrqSpinLock<RecoveryManager> = IrqSpinLock::new(RecoveryManager::new())` 模式）。
  - 状态：[X]

- **B03-11. do_softirq 全局 running（TOP 20 #14）**
  - 描述：framework/irq 中 `do_softirq` 用全局 running 标记，多核下仅 1 CPU 处理 softirq。
  - 方案：改 per-CPU running 标记；单开 PR（决策点 D5）。
  - 状态：[X]

- **B03-12. timer tick 计数器内存序（TOP 20 #20）**
  - 描述：framework/timer tick 计数器内存序多核一致性问题。
  - 方案：核对 tick 计数器的 `Ordering`（Relaxed→Acquire/Release），补多核 tick 测试。
  - 状态：[X]

- **B03-13. klog_ffi! NUL 终止（TOP 20 #4 / H.5.5 P1-H）**
  - 描述：framework/klog `klog_ffi!` 宏 256 字节栈缓冲无 NUL 终止保证，栈缓冲溢出读取风险。
  - 方案：格式化后显式 `buf[len] = 0`；长度封顶 255；补越界 host-tests。
  - 状态：[X]

## 工程计划 C: framework 剩余子系统

### 背景

- **B03-14. cpu/剩余模块 P0 引用**
  - 描述：framework/cpu 单文件 1554 行 + SAFETY + 溢出；framework 顶层散文件 SMEP/SMAP、IoMem 溢出、帧验证；framework/tests 永久关中断、持锁执行、物理地址硬编码。详见 [附录 C 报告索引](./code-audit-final-summary.md) 与 [archive 子系统报告](./archive/audit-2026-08-14/)。
  - 方案：以 archive 子系统报告为准提取详细 P0/P1 条目，逐项登记后实施。
  - 状态：[X]

### 待办

- **B03-15. framework/tests run_all 永久关闭中断（TOP 20 #7）**
  - 描述：[tests/mod.rs:122](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L122) `run_all` 开头 `interrupt_disable()` 后无 re-enable，测试结束后中断全关。
  - 方案：测试循环后恢复中断状态（save/restore 而非无条件 disable）。
  - 状态：[X]

- **B03-16. cpu/mod.rs 1554 行单文件拆分（P0）**
  - 描述：`framework/cpu/mod.rs` 1554 行单文件，含 CPU 厂商检测/签名解析/特性收集/缓存检测/MSR/TSC/多核拓扑/FFI 导出；`cache.rs`/`topology.rs` 已声明却仅占位。
  - 方案：**返工完成（2026-08-23）**：拆分到已声明未用子模块——新建 `cpu/feature.rs`（`CpuFeatures` 位标志集 + 特性收集 `collect_features`），缓存逻辑填入 `cpu/cache.rs`（`CacheInfo` + `detect_cache`），拓扑逻辑填入 `cpu/topology.rs`（`TopologyInfo` + `detect_topology`）；MSR 常量（IA32_EFER/EFER_SCE/IA32_STAR/IA32_LSTAR/IA32_SFMASK）集中到 `msr.rs`；`CpuVendor::Unknown` 兜底语义明确化（厂商特定 CPUID 分支跳过，检测走保守默认值，is_virtualized 对 Unknown 保守返回 true）。顶层 `pub use` re-export 保持公共 API 不变（`cpu::CpuFeatures/CacheInfo/TopologyInfo` 路径兼容）。用户 2026-08-23 选"填入现有占位模块"而非计划字面的 `*_impl.rs` 新建（避免重复模块）。
  - 状态：[X]

- **B03-17. cpu/msr.rs 对齐与 #GP（P0）**
  - 描述：`cpu_read_msr`（msr.rs:71-84）仅查 `is_null()` 未查对齐，非对齐指针 aarch64 data abort；`cpu_write_msr`（msr.rs:92-98）非法 MSR 触发 #GP 后仍返回 0 假设成功。
  - 方案：`cpu_read_msr` 加 `assert!(low % 4 == 0 && high % 4 == 0)`；`cpu_write_msr` 预检 MSR 合法性或配套 KPTI IST #GP handler 捕获。
  - 状态：[X]

- **B03-18. cpu/cpuid.rs leaf 边界（P0）**
  - 描述：`cpuid.rs:22-37` 未处理 leaf > max_leaf（AMD 可能 panic）；`is_leaf_supported`（cpuid.rs:48-51）存在但调用方未强制使用，且不验证 subleaf。
  - 方案：公开 API 强制 `is_leaf_supported(leaf)` 校验并返回 `Option`；扩展为 `(leaf, subleaf)` 双层校验。
  - 状态：[X]

- **B03-19. cpu/tsc.rs 溢出与序列化（P0/P1）**
  - 描述：`cycles_to_nanoseconds`（tsc.rs:42-50）`tsc_cycles * 1000` 可溢出 u64（4GHz 运行 24h 达 3.5×10¹⁷）；`read_tsc_serialized` 与 `read_tsc` 实现相同（tsc.rs:28-32），无 mfence/lfence 序列化，与文档"更精确"不符。
  - 方案：`checked_mul(1000).unwrap_or(u64::MAX) / freq`；实装序列化版本或修正文档。
  - 状态：[X]

- **B03-20. cpu/arch.rs send_ipi 越界（P1）**
  - 描述：`send_ipi`（arch.rs:46-55）不验证 CPU 索引在 `MAX_CPUS` 范围，越界 IPI 丢失。
  - 方案：入口校验 `target_cpu < MAX_CPUS`。
  - 状态：[X]

- **B03-21. framework 顶层散文件：userptr validate NULL（P0）**
  - 描述：`userptr.rs:200-211` `validate_user_buf` 在 `len == 0` 时直接 return true 不检查 ptr，`validate_user_buf(0, 0)` 返回 true 但 0 是 NULL。
  - 方案：`len == 0` 时也检查 `ptr != 0`，或单独提供 `validate_user_buf_zero_ok`。
  - 状态：[X]

- **B03-22. framework 顶层散文件：IoMem 溢出（I5，P0）**
  - 描述：`iomem.rs:95-122` `IoMem::new` 不检查 `phys + len` 溢出（相加回绕 0 绕过全部范围检查）；`AliasRegistry::check_conflict` 用 saturating_add 不阻止溢出对。
  - 方案：`new` 加 `phys.checked_add(len).ok_or(...)?`；`AliasRegistry::register` 内部加 overflow 检查。
  - 状态：[X]

- **B03-23. framework 顶层散文件：irqline 中断上下文约束（P0）**
  - 描述：`irqline.rs:126-156` SAFETY 声称"启动阶段单线程调用"与 `IrqSpinLock` 设计冲突；`on_interrupt` 在中断上下文执行，内部不可持锁/睡眠，纯文档约束无编译检查。
  - 方案：`dispatch_irq` 内检查当前是否中断上下文；文档显式列出"严禁持锁的同步原语名单"。
  - 状态：[X]

- **B03-24. framework 顶层散文件：RacyCell Sync 无约束（P0）**
  - 描述：`racy_cell.rs:32-35` `unsafe impl<T> Sync for RacyCell<T>` 无 `T: Send` 约束，`T = *mut u8` 也可 Sync，跨线程共享裸指针 UB。
  - 方案：**返工完成（2026-08-23）**：加 `T: Send` 约束后编译器捕获 2 处真实问题（IpcNamespace/DynIpcNamespace 含 `NonNull<Message>` 非 Send），且实测确认 Rust 1.98 nightly 中 `NonNull<T>` 显式 `!Send`。根治：`Message.next`/`MsgQueue.head/tail` 由 `NonNull<Message>` 改 `AtomicPtr<Message>`（AtomicPtr 无条件 `Send + Sync`），`RacyCell` 恢复 `unsafe impl<T: Send> Sync`。侵入式链表操作（入队/出队/销毁）适配 load/store + `NonNull::new`/`as_ptr`，队列操作均在持锁下执行，Relaxed 内存序足够。DECISION-053。
  - 状态：[X]

- **B03-25. framework 顶层散文件：net_socket map_rc 恒等函数（P0）**
  - 描述：`net_socket.rs:99-103` `map_rc(rc: i32) -> i32 { rc }` 注释声称"i32 → 强类型翻译"但实际恒等，services 层必须自己判断正负。
  - 方案：实现真正的 `i32 → NetError` 翻译，或删除该函数直接处理 i32。
  - 状态：[X]

- **B03-26. framework 顶层散文件：Frame::as_virt_ptr 悬挂指针（P0）**
  - 描述：`frame.rs:127-130` 返回 `*mut u8` 无 Frame 生命周期绑定，Frame Drop 后指针悬挂；SAFETY 只约束"每物理地址一个 Frame"未约束裸指针使用期。
  - 方案：返回 `&mut [u8]`（生命周期绑定 `&self`）或 `NonNull<u8>` + `PhantomData<&'a mut Frame>`。
  - 状态：[X]

### 验证门槛

- **B03-27. 内存/同步回归**
  - 描述：修复后跑 host-tests 全部（含 mm/sync/timer 相关用例）+ 双架构编译。
  - 方案：`make test-host` + `./ci/build.sh all`。
  - 状态：[X] **2026-08-23 返工后复跑通过**：host-tests 全绿；双架构 `cargo check`/`cargo clippy -- -D warnings` 0 error 0 warning；kernel_test feature 编译通过；`./ci/build.sh all` 5/5（含 x86_64 链接）。附带修复：Makefile 跨架构戳记 bug（原 L122 解析期无条件覆写 `.arch`，`make test-host` 等无链接 make 会清掉戳记，导致跨架构残留 boot.o 被误链接；戳记写入移至 `arch-switch-clean` 配方，仅真实切换时更新，已回归验证 aarch64→test-host→x86_64 序列）。

- **B03-28. 死锁回归**
  - 描述：pi_mutex/IRQ 路径修复后，用修复版 `audit_deadlock_matrix.py`（分册 01）扫描 0 新问题。
  - 方案：分册 01 完成后跑死锁矩阵 + lockbud。
  - 状态：[X] **2026-08-23 复跑通过**：`audit_deadlock_matrix.py` 0 违规（before=0/after=0，含 pi_mutex 持有登记表锁序——`PI_MUTEX_HELD` 仅持 IrqSpinLock 不嵌套 waiters 锁）。

### 决策记录

- **DECISION-049**
  - 描述：pi_mutex_process_exit 实装采用"PiMutex 内置 process_exit 方法"方案（机制内化，对应审计决策点 D7）。
  - 方案：放弃原文档"PI_MUTEX_REGISTRY 写入 + exit 遍历 force_unlock + unregister"路径（依赖全局 usize 指针注册表，丢失类型安全）。改为 PiMutex 自治：`PiMutex::process_exit(&self, pid: u32)` 方法内部检查 holder 后调 `force_unlock`（已有 L494）。删除全局 `PI_MUTEX_REGISTRY` static。进程退出回调由 process 层维护（遍历进程 mutex 列表）。用户 2026-08-23 评估三方案（usize + 强转 / Weak/Arc 重设计 / 机制内化）后选 C。
  - 状态：[X]

- **DECISION-050**
  - 描述：B03-03 swap init 16MB 泄漏修复采用"路径 B 根治（find_contig_range + reserve_range）"方案。
  - 方案：调研发现 swap init 现状是 `alloc_page()` 4096 次，但 PMM buddy 上限 `MAX_BUDDY_ORDER=9`（2MB），且 swap 模块无释放路径（grep 无 `swap_fini`），同时 `slot_addr = storage_virt + slot * PAGE_SIZE` 隐含4096 页物理连续假设但 `alloc_page` 不保证连续——三重隐患。放弃路径 A（alloc_pages 超 buddy 上限不可行）和路径 C（unreserve API 临时方案）。改走路径 B：(1) 新增 `pub fn find_contig_range(&self, size: usize) -> Option<PhysAddr>` 在 pmm.rs（L 紧邻 alloc_pages）——扫描 bitmap 找连续 size 物理范围（不调 buddy，因 buddy 上限 2MB），返回连续基址；(2) swap init 改为：先调 `find_contig_range(16MB)` 拿基址，再调 `reserve_range(base, 16MB)` 声明 reserved，回滚路径同步 unreserve；(3) 调用方 `rust/src/lib.rs:598` 无需新参数。用户 2026-08-23 选 B（架构最清晰，PMM 簿记与所有权语义一致，符合"机制留 framework，策略放 services" framekernel 原则）。
  - 详情：实施方案文件清单（审计追溯）：新增 `src/kernel/framework/mm/pmm.rs::find_contig_range`；修改 `src/kernel/framework/mm/swap.rs::SwapArea::init`（L155-194）+ `pub fn swap_init`（L522-527）；新增 `src/kernel/framework/mm/swap.rs::SwapArea::deinit`（unreserve 回滚，供 host-tests 与将来 swap 模块热卸载）。验收：双架构 cargo check 0w0e + pmm::find_contig_range host-tests + 回滚路径测试。
  - 状态：[X]

- **DECISION-053**
  - 描述：B03-24 RacyCell Sync 无约束根治采用"AtomicPtr 替换 NonNull"方案。
  - 方案：加 `T: Send` 约束的尝试被编译器捕获 2 处真实问题（IpcNamespace/DynIpcNamespace 含 `NonNull<Message>` 非 Send），实测确认 Rust 1.98 nightly `NonNull<T>` 显式 `!Send`。services 层 `#![deny(unsafe_code)]` 禁止 `unsafe impl Send/Sync`（F1），无法在 services 侧补 impl。根治：`Message.next`/`MsgQueue.head/tail` 由 `NonNull<Message>` 改 `AtomicPtr<Message>`（AtomicPtr 无条件 `Send + Sync`），`RacyCell` 恢复 `unsafe impl<T: Send> Sync`。侵入式链表操作均在持锁下执行，Relaxed 内存序足够。放弃方案二（删除 unsafe impl，破坏全部 `RacyCell::get_mut()` 调用点）。
  - 状态：[X]

- **DECISION-059**
  - 描述：B03-08 pi_mutex_process_exit 返工后定稿采用"持有登记表 + 类型擦除函数指针"方案（DECISION-049 机制内化方案的演进）。
  - 方案：返工审查发现 DECISION-049 的"PiMutex 内置 process_exit 方法"依赖调用方遍历进程 mutex 列表，而实际零调用方（未接线）。最终方案：`try_lock` 快路径与 `do_unlock` 移交路径登记 `HeldLock { ptr, dispatch }` 到 `PI_MUTEX_HELD: IrqSpinLock<BTreeMap<pid, Vec<HeldLock>>>`；分派为非泛型 `unsafe fn(usize, u32) -> bool`，经固定布局前缀（`data` 之前字段布局与 T 无关）以 `PiMutex<u8>` 重建引用调 `force_unlock_for_exit`，对任意 T（含 ?Sized）兼容；进程退出由 `process_cleanup::notify_process_exit` 调 `pi_mutex_process_exit(pid)` 整体 remove 清理（幂等，残留条目可接受）。放弃 fat-pointer 方案（`?Sized` upcast E0277 / `ptr_metadata` E0658 / `*const ()` 非 Send）。
  - 状态：[X]
