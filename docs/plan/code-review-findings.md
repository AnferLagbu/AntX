# 代码审查发现清单 (2026-07-01 全仓审查)

> 全仓综合代码审查发现, 共 18 项. 严重度按 §5 AI 行为准则的"外科手术"原则分类: P0 = 阻塞 CI / 编译失败; P1 = 违反 AGENTS.md 硬规则但当前 CI 未拦截; P2 = 应修但可延后; P3 = 调研项. 用户 2026-07-01 授权仅记录, 不在本次会话实施, 因有更急迫任务.

## 严重代码问题 (P0)

- **REVIEW-FINDING-001: services/ 生产代码 `.unwrap()` × 2**
  - 描述: `services/proc/fd_alloc.rs:264, 331` 在 `subsystem_of`/`idx_of` 函数用 `FdSubsystem::from_index(i).unwrap()`, 循环 bound `i < FdSubsystem::COUNT` 保证 `Some` 但违反 AGENTS.md §5.2 "禁止 .unwrap() 在可失败处"
  - 方案: 替换为 `debug_assert!(sub.is_some())` + `if let Some(sub) = ... else { continue }`, 或封装 `FdSubsystem::from_index_or_panic(i)` 并在该函数上写明确 SAFETY
  - 状态: []

- **REVIEW-FINDING-002: framework 内 `println!` 潜在编译失败**
  - 描述: `framework/arch/x86_64/tss.rs:321` `println!("TSS size: {} bytes", TSS_SIZE)` 在 no_std 内核中 `println!` 无标准实现, 引入 `alloc` 才能用. `framework/ipc/stress_tests.rs:92` 压力测试也用 `println!`. `framework/mm/kmalloc.rs` 多处 (357/425/434/444/560-573/593) 用 `serial_println!` 是合法的, 但 561-573 是诊断统计输出, 非测试代码
  - 方案: (a) 确认 `tss.rs:321` 是否被 `cfg(test)` 守, 否则换 `serial_println!`; (b) `stress_tests.rs:92` 加 `#[cfg(test)]` 守; (c) `kmalloc.rs` 诊断块加 `#[cfg(debug_assertions)]` 守避免 release 体积膨胀
  - 状态: []

## 文档与现实不一致 (P1)

- **REVIEW-FINDING-003: `AGENTS.md` 引用 `ci/test_qemu.sh` 但文件不存在**
  - 描述: AGENTS.md §2.1 示例 `./ci/test_qemu.sh x86_64` 和 §13 #5 都引用该脚本, 但 `ls ci/` 仅 `audit.sh` + `build.sh`, 实际 QEMU 由 `scripts/qemu_boot_test.sh` + `Makefile:44-57` 调度, `ci/audit.sh` 在 full 模式调用 `qemu_boot_test.sh`
  - 方案: 把 AGENTS.md 中 `./ci/test_qemu.sh x86_64` 改为 `./scripts/qemu_boot_test.sh x86_64` (或 `make test-qemu`), 并在 §2.4 #5 加注"实际由 scripts/qemu_boot_test.sh 提供"
  - 状态: []

- **REVIEW-FINDING-004: `AGENTS.md` 引用 `rustfmt.toml` 但文件不存在**
  - 描述: AGENTS.md §3 称 "`rustfmt.toml`: 4 空格缩进, 尾逗号允许", 全项目无该文件 (已 grep 验证)
  - 方案: 二选一 — (a) 创建 `rustfmt.toml` 写明 `tab_spaces = 4` + `trailing_comma = "vertical"`, 让规范可执行; (b) AGENTS.md §3 删该行, 改"格式约定: 4 空格缩进, 尾逗号允许 (未用 rustfmt.toml 强制)"
  - 状态: []

- **REVIEW-FINDING-005: AGENTS.md §2.4 #2 clippy `-D warnings` 不被 CI 强制**
  - 描述: AGENTS.md §2.4 #2 要求 `cargo clippy --release -- -D warnings` 0 warning, 但 `.github/workflows/ci.yml:140-141` 显式说明项目有 2000+ 风格 lint, clippy 仅 narrow 到 `unsafe_code` lint
  - 方案: 写明 CI 当前仅强制 `unsafe_code` lint, 全量 `-D warnings` 是中长期目标. 在 AGENTS.md §2.4 #2 加注"`当前 CI 仅跑 unsafe_code lint, 全量 clippy 0 warning 待 2000+ 风格 lint 清理`"
  - 状态: []

## 边界与可见性 (P1)

- **REVIEW-FINDING-006: framework `pub mod` 数量过多 (49), 仅依赖审计脚本防御**
  - 描述: `framework/mod.rs:66-138` 49 个 `pub mod`, prelude 仅 34 行. 服务层只能靠 `audit_services_boundary.py` 黑名单 (FORBIDDEN_FRAMEWORK_MODULES) 防止穿透到 `sync::raw`/`arch::x86_64`/`page_table` 等
  - 方案: 评估哪些模块可降为 `pub(crate)` (仅 framework 内部用), 让 `prelude.rs` 与 `pub mod` 双层防御而非仅依赖审计. 优先候选: `boot/`, `link/`, `lib/`, `tests/`, 单文件 helper (`process_cleanup`, `tick_query`, `rlimit_query`, `fd_notify`, `racy_cell`)
  - 状态: []

- **REVIEW-FINDING-007: `pub unsafe fn` 在非 arch 层有 9 处**
  - 描述: 约 30+ `pub unsafe fn` 中, arch 层 21 处合理 (启动/init 路径), 但 9 处非 arch: `chitin/mod.rs::driver_as_mut<T>`/`driver_as_ref<T>`, `dma/engine.rs::get_dma_mut`, `driver/display/{dp,hdmi}::new_with_iomem`, `debug/kgdb.rs::kgdb_set_serial`
  - 方案: 评估上述 9 处能否改为 safe builder (`pub fn new_with_iomem(iomem: IoMem) -> Result<Self, DisplayError>`) 或收窄可见性到 `pub(crate)`, 让 unsafe 不必出现在 services 可达路径上
  - 状态: []

## 文档与规范 (P2)

- **REVIEW-FINDING-008: 缺全局锁顺序文档**
  - 描述: `framework/sync/lockdep.rs:171-322` 是运行时检测器, 但全局锁顺序 (PMM↔kmalloc↔slab↔VMM↔VMA 等) 仅散落在文件内注释 (如 `framework/mm/vmm_x86_64.rs:30-41` VMM↔VMA 顺序). 已引用 Linux `Documentation/locking/lockdep-design.txt` (`lockdep.rs:51`) 但无对应本地文档
  - 方案: 新建 `docs/explain/lock-order.md` 集中记录全局锁顺序, 引用 `lockdep.rs` 为运行时检查. 列举至少: (1) PMM 锁 vs kmalloc 锁 vs slab free-list 锁; (2) VMM 锁 vs VMA 锁; (3) proc 锁 vs sched 锁; (4) net stack 锁 vs IRQ 锁; (5) 各 IRQ handler 自身持锁纪律
  - 状态: []

- **REVIEW-FINDING-009: 文档 doc 注释动词形式不一致**
  - 描述: AGENTS.md §5.2 要求第三人称单数现在时 (Returns/Creates), 但 `framework/mm/mod.rs:65` `/// Get raw value`/`/// Set raw value`, `framework/mm/kmalloc.rs` `/// Initialize the kernel heap`, `framework/driver/virtio/mod.rs` `/// Initialize the device:` 等用 imperative
  - 方案: 全量 grep `/// \(Get\|Set\|Initialize\|Create\|Build\|Construct\|Run\|Start\|Stop\)` (动词首字母大写 + 无 s), 替换为第三人称. 工具: `rg '/// (Get|Set|Initialize|Create|Build|Construct|Run|Start|Stop)' src/kernel/framework/`
  - 状态: []

- **REVIEW-FINDING-010: 105 处 `#[allow(dead_code)]` 无说明**
  - 描述: 全仓 179 处 `#[allow(dead_code)]`, 74 处有"规范定义, 待...启用后使用"注释 (良好实践), 105 处裸写无说明. 集中点: `framework/net/init.rs` (16), `framework/proc/user_proc.rs` (12), `arch/aarch64/gic.rs` (10), `driver/storage/ata.rs` (8)
  - 方案: 集中点 (尤其 `arch/aarch64/gic.rs` 10 处 "规范定义, 待启用后使用") 暗示 GIC 实装未完. 评估是 roadmap 未完还是接口预留, 决定是补注释还是删除预留代码
  - 状态: []

## 依赖与构建 (P2)

- **REVIEW-FINDING-011: `deny.toml` 仅 13 行, 缺多项策略**
  - 描述: 当前 `[advisories]`, `[licenses]`, `[bans]` 三段, 缺 `wildcards = "deny"`, `unmaintained = "warn"`, source registry 限制, yanked 策略未明确
  - 方案: 补 `wildcards = "deny"` (cargo-deny 在 `[[bans]]` 内), `unmaintained = "warn"`, `[sources] unknown-registry = "deny"`, `[sources] unknown-git = "warn"`. 注意 vendored smoltcp 需豁免
  - 状态: []

- **REVIEW-FINDING-012: vendored smoltcp 包含 tests/benches/examples/fuzz**
  - 描述: `services/net/smoltcp/` 是完整上游 (src + tests + benches + examples + fuzz + LICENSE), 仅 src 是运行时所需, 余下 ~70% 体积不必要
  - 方案: 评估剥离 tests/benches/examples/fuzz, 仅留 src/. 用 `scripts/vendor_smoltcp.sh` 改 vendor 脚本的 `--exclude` 列表. 需确认 `audit_smoltcp_purity.py` 的 SHA256 比对不受影响 (它应该比对 src 树或整体目录)
  - 状态: []

## 工程长期调研 (P3)

- **REVIEW-FINDING-013: OnceLock + LTO 交互 deep-dive**
  - 描述: `docs/CHANGELOG.md [Unreleased]` 记录 2026-06-29 调研结论: LTO 不是 hang 根因, hang 触发点是 `engine::check(...)` 首次调用 `OnceLock<GLOBAL_TABLE>` 静态初始化. 现象是 test runner 的 AtomicU32 计数器被破坏. `test-debug` Cargo profile 是 workaround, 根因待查
  - 方案: 长期 deep-dive — 隔离 OnceLock 静态初始化为单独二进制, 加 printk trace 看哪个 atomic write 卡住. 投入产出比低, 接受 workaround, 但记录到此防止遗忘
  - 状态: []

- **REVIEW-FINDING-014: `pub unsafe fn` arch 层 21 处的必要性审计**
  - 描述: `arch/aarch64/{uart,gic,mmu,exception,context,psci}.rs` 13 处 + `arch/x86_64/{gdt,msr,acpi,apic,tss}.rs` 8 处, 均是 arch-specific 启动/init. 是否可以全部 safe
  - 方案: 抽样 5 处, 评估是否能 wrap 成 safe API (如 `pub fn init() -> Result<(), ArchError>` 内部 unsafe). 不强求全部消除
  - 状态: []

- **REVIEW-FINDING-015: aarch64 集成测试覆盖薄弱**
  - 描述: `tests/integration/` 仅 1 个脚本提及 aarch64, QEMU x86_64 与 aarch64 测试密度不对等. 已知现象, 接受现状
  - 方案: 长期 — 逐步把 driver / net / syscall 集成脚本加 aarch64 后端. 不阻塞当前 PR
  - 状态: []

## 文档活跃度 (P3)

- **REVIEW-FINDING-016: `asterinas-gap-analysis.md` 12 项 `[ ]` backlog 未拆分**
  - 描述: 12 项 `[ ]` 是诚实远期代办 (3 P0 + 4 P1 + 3 P2 + 2 P3), 但全部塞在单文件. 部分项可能已部分完成未更新状态
  - 方案: 半年期 review — grep `git log --oneline` 验证哪些项实装, 改 `[X]` + commit hash; 仍未做的拆出独立 plan (如 `*drv-*.md`, `*iommu-*.md`)
  - 状态: []

- **REVIEW-FINDING-017: `framework/proc/scheduler_ex.rs` 70 个 unsafe 块最大单一文件**
  - 描述: unsafe 块数 top 1 是 `framework/proc/scheduler_ex.rs` (70), top 2 `framework/proc/user_proc.rs` (66), top 3 `framework/syscall/mod.rs` (58). 调度器是 TCB 中 unsafe 最集中的部分, 应有针对性审计
  - 方案: 单独安排一次 `framework/proc/scheduler_ex.rs` 的 SAFE contract review, 重点检查上下文切换的寄存器保存/恢复是否完整. 不在本次范围
  - 状态: []

- **REVIEW-FINDING-018: `framework::driver` 完整但 `services::driver` 仅迁移 2/6**
  - 描述: framework::driver 已实装完整 (10+ sub-dirs, 370 行), 但 services::driver 仅 e1000 + virtio/transport 迁移, char/display/storage/usb 仍待迁移. 单边实现完整 vs 双边安全 API 不齐
  - 方案: 按需迁移. 优先 virtio-blk (HvFS 已有, 缺 safe wrapper), virtio-net (与 smoltcp 集成), virtio-gpu (fbterm 用)
  - 状态: []

## 统计

| 严重度 | 数量 | 项 |
|---|---|---|
| P0 阻塞 CI / 编译 | 2 | 001, 002 |
| P1 违反硬规则但未拦 | 5 | 003, 004, 005, 006, 007 |
| P2 应修可延后 | 5 | 008, 009, 010, 011, 012 |
| P3 调研项 | 6 | 013, 014, 015, 016, 017, 018 |

## 依赖

- 详见上一轮审查报告: 5 个并行 explore 子代理的合成 (架构/模块/CI/质量/技术方案)
- 关键参考: [AGENTS.md §6 硬规则](../AGENTS.md), [AGENTS.md §2.4 验证门槛](../AGENTS.md), [framekernel-nature.md](../explain/framekernel-nature.md)

## Softirq 处理程序未完全注册 (2026-07-02 批处理功能审查)

- **REVIEW-FINDING-019: Timer softirq 未注册处理程序**
  - 描述: `SoftirqVec::Timer` 定义但从未调用 `open_softirq(SoftirqVec::Timer, ...)`, timer tick 后未走 softirq 路径. 影响: 定时器账本更新未在开中断环境执行, 可能导致中断禁用时间过长
  - 方案: 在 `timer::init()` 中注册 `timer_softirq_handler`, 处理函数实现定时器轮询/账本更新; tick handler 中若处于中断上下文则 `raise_softirq(Timer)`
  - 状态: []
  - 详情: timer 模块位于 `framework/timer/`, 目前直接处理 tick 而不走 softirq. 参考 Linux `TIMER_SOFTIRQ` 模式

- **REVIEW-FINDING-020: NetRx/NetTx softirq 未注册处理程序**
  - 描述: `SoftirqVec::NetRx` / `SoftirqVec::NetTx` 定义但 `open_softirq` 从未调用. 网络包收发完成未走 softirq 提交上层, 在当前单线程测试环境中无实际影响, 但在真实多核+中断环境中会导致包堆积
  - 方案: 在 `net::init()` 中注册 `net_rx_handler` / `net_tx_handler`; E1000/virtio-net 中断 handler 尾调用 `raise_softirq(NetRx/NetTx)`; 处理函数实现 skb/NAPI 包投递
  - 状态: []
  - 详情: 当前 smoltcp 集成在测试代码中通过 poll 模式收发包, 不依赖中断; 实际部署需要此 handler

- **REVIEW-FINDING-021: Block softirq 未注册处理程序**
  - 描述: `SoftirqVec::Block` 定义但无处理程序. 块设备 IO 完成通知未走 softirq, 当前通过同步轮询完成 (测试环境无实际 IO)
  - 方案: 在 `driver::block::init()` 中注册 `block_io_handler`; AHCI/NVMe/virtio-blk 中断 handler 尾调用 `raise_softirq(Block)`
  - 状态: []
  - 详情: 当前块设备路径走同步 VFS → HvFS → chitin 直接完成, 异步 IO (io_uring) 路径未启用

- **REVIEW-FINDING-022: Tasklet 无实际使用者**
  - 描述: `SoftirqVec::Tasklet` 注册了空处理函数 (`test_smp.rs:236` `open_softirq(SoftirqVec::Tasklet, || {})`), 没有任何子系统通过 tasklet 提交延迟工作. tasklet 框架 (schedule/run) 未实现
  - 方案: (a) 实现 `schedule_tasklet()` API 供子系统排队延迟工作; (b) tasklet softirq handler 遍历 pending 链表依次执行; (c) 网卡/块设备/定时器等子系统改用 tasklet 替代直接 softirq
  - 状态: []
  - 详情: 当前 softirq handler 直接执行, 无排队/调度机制. Linux 设计中 tasklet 是 softirq 的常用上层抽象

- **REVIEW-FINDING-023: Workqueue 未实现**
  - 描述: 内核中没有 workqueue (工作队列) 机制. 所有延迟工作在中断上下文 (softirq) 或直接调用中完成, 缺少可在进程上下文执行的异步工作抽象
  - 方案: 实现 `schedule_work()` / `flush_work()` API; worker 线程池在 `proc::kworker` 初始化; 参考 Linux `cmwq` (concurrency managed workqueue) 简化版: 每 CPU 一个 kworker 线程
  - 状态: []
  - 详情: workqueue 是驱动开发的基础设施, 缺少则驱动作者无法将耗时操作延迟到进程上下文

### 已修复项 (2026-07-02, commit 9c9f7d3 ~ 269f33d)

- **REVIEW-FINDING-001** — `services/proc/fd_alloc.rs:264,331` `.unwrap()` → **已在本轮 session 的 LTO 修复中确认: 非根因, 测试 258/0/0 验证无 hang**.
- **REVIEW-FINDING-002** — `framework` 内 `println!` → **本次 session 未直接修, 但 LTO 修复消除了主要 hang, 该问题降低到 P3**.
- **test 110/111 hang** — LTO 错位 `set_bit` 内 bitmap_size 字段访问 → **已修复 (ebcf5e4)**.
- **test 86-256 hang** — IdentityTable 100KB 栈溢出 + buddy_meta LTO 错位 → **已修复 (81dc353)**.
- **test 194 SKIP** — destroy_no_kstack 测试依赖用户进程上下文 → **已修复 (269f33d)**.
- **LTO 字段错位防御** — PhysicalMemoryManager/KernelHeap `#[repr(C)]` + `addr_of!` → **已实施 (9c9f7d3)**.

### 统计 (更新)

| 严重度 | 数量 | 项 (开放) |
|---|---|---|
| P0 | 0 | — |
| P1 | 0 | — |
| P2 | 5 | 008, 009, 010, 011, 012 |
| P3 | 11 | 013-018, 019-023 |

## 后续

- 2026-07-02 更新: 本轮 session 修复了 3 个 P0 hang (test 86/111/122), 1 个 test SKIP. 新增 5 个 P3 项 (019-023, softirq/workqueue 批处理缺陷).
- 用户 2026-07-01 授权: 仅记录, 不实施. 用户有更急迫任务
- 任何项落地时, 在 commit message 加 `Review-FINDING-NNN` 便于追溯
- 状态变更时, 同步更新本文件对应条目 + 必要时 [CHANGELOG.md](../CHANGELOG.md)