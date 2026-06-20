# AntX 维护周期工程规划 (2026-06-19)

> 本文档为维护周期的统一任务清单，整合了：
> - 新发现的硬编码/解耦/合规/质量问题
> - 旧文档中 SKIP 项的重新评估
> - 旧文档中标记完成但验收清单未闭合的项
> - 源码中 TRACK 标记的未实现项
>
> **维护原则**: 质量优先，速度不重要。严格落实解耦与避免硬编码，严格落实框内核最佳实践。
> **执行约定**: 每四项为一组工程，每完成一项将 `[ ]` 改为 `[x]`，补全完成记录。
> **验证门槛**: 双架构 0 error/0 warning + clippy 0 warning + 三审计通过 + host-tests 通过。

---

## 文档元信息

| 字段 | 值 |
|------|---|
| 起始日期 | 2026-06-19 |
| 当前 Self TCB | 50.0% (excl. smoltcp+tests) |
| 目标 Self TCB | < 30% |
| 当前 framework 跨模块引用 | 352 处 |
| 当前 services→framework 依赖 | 215 处 |
| `#[allow(dead_code)]` | 40 处 |
| 非 smoltcp TODO | 15 处 |
| TRACK 标记 | 28 处 |
| 归档文档 | tcb-reduction-plan.md, engineering-discipline.md, maintenance-2026-06-11.md, framekernel-compliance.md, delivery-summary-2026-06-13.md, deep-audit-2026-06-11.md, vfs-policy-extraction.md |

---

## 一、硬编码消除 (HARD-*)

> **原则**: 所有魔术数字必须有语义常量名，所有重复定义必须统一到权威来源。

### [x] HARD-1: KERNEL_BASE 重复定义消除

**当前**: `0xFFFF800000000000` 在 4 处独立定义
- `framework/mm/mod.rs:118` — 权威定义
- `framework/proc/process.rs:312` — 重复
- `framework/proc/user_proc.rs:9` — 重复
- `framework/dma/mod.rs:26` — 重复

**方案**: 删除 3 处重复定义，统一引用 `framework::mm::KERNEL_BASE`。若跨模块引用不便，在 `framework::config` 中 re-export。

**验收**:
- [x] `grep -rn '0xFFFF800000000000' src/kernel/` 仅剩 `mm/mod.rs` 权威定义
- [x] 双架构 0w0e + 三审计通过

**完成记录** (2026-06-19): 消除 4 处重复定义 + 6 处魔术数字，统一引用 `framework::mm::KERNEL_BASE`。

---

### [ ] HARD-2: PAGE_SIZE 硬编码消除 (framework 层)

**当前**: `4096` / `0x1000` 在 framework 层约 15+ 处硬编码
- `framework/proc/api.rs:612` — `4096`
- `framework/proc/process.rs:315` — `4096`
- `framework/mm/slab.rs:549` — `4096`
- `framework/mm/swap.rs:170` — `4096`
- `framework/syscall/api.rs:230` — `4096`
- `framework/syscall/mod.rs:1276` — `4096`
- `framework/dma_buf.rs` — `64` (cache line)
- `framework/dma/engine.rs` — `64` (cache line)
- `framework/arch/x86_64/smp_init.rs:203-215` — `0x300`/`0x310` (APIC 寄存器偏移)

**方案**:
1. `4096` → `config::PAGE_SIZE` (u64) 或 `config::PAGE_SIZE_USIZE` (usize)
2. `0x1000` → 同上
3. APIC 偏移 → 已有 `apic::APIC_ICR_LOW`/`APIC_ICR_HIGH` 常量

**验收**:
- [ ] framework 层无裸 `4096`/`0x1000` 用于表示页大小
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] HARD-3: PAGE_SIZE 硬编码消除 (services 层)

**当前**: `4096` 在 services 层约 5+ 处硬编码
- `services/ipc/shm.rs:39`
- `services/proc/madvise_mlock.rs:278`
- 其他 services 文件

**方案**: 统一引用 `services::config::memory::PAGE_SIZE`

**验收**:
- [ ] services 层无裸 `4096`/`0x1000` 用于表示页大小
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] HARD-4: Cache line size 提取为公共常量

**当前**: `64` 在 3 个文件中硬编码
- `framework/dma_buf.rs:183,221`
- `framework/dma/engine.rs:368,401,450`

**方案**: `cpu::CpuInfo` 已有 `cache_line_size` 字段。在 `framework::config` 中定义 `pub const CACHE_LINE_SIZE: usize = 64;` (默认值)，运行时可通过 CpuInfo 覆盖。

**验收**:
- [ ] DMA 相关代码使用 `config::CACHE_LINE_SIZE` 而非裸 `64`
- [ ] 双架构 0w0e + 三审计通过

---

### [x] HARD-5: VIRTIO_MMIO_BASE 重复定义统一

**当前**: `0x0a00_0000` 在 2 处定义
- `framework/driver/virtio/mod.rs:98`
- `services/driver/virtio/transport.rs:112`

**方案**: services 层引用 framework re-export 的常量，或在 `config` 中统一定义。

**验收**:
- [ ] `VIRTIO_MMIO_BASE` 仅 1 处权威定义
- [ ] 双架构 0w0e + 三审计通过

---

### [x] HARD-6: KERNEL_TEXT_BASE 提取

**当前**: `0xFFFFFFFF80000000` 在 4+ 处硬编码
- `framework/idt/idt.rs:152,158,800`
- `framework/idt/handlers.rs:232`
- `framework/idt/types.rs:118`
- `framework/idt/safety.rs:230,282`

**方案**: 在 `framework::config` 中定义 `pub const KERNEL_TEXT_BASE: u64 = 0xFFFFFFFF80000000;`

**验收**:
- [x] `0xFFFFFFFF80000000` 仅 1 处权威定义
- [x] 双架构 0w0e + 三审计通过

**完成记录** (2026-06-19): 新增 KERNEL_TEXT_BASE 于 framework/mm/mod.rs (cfg-gated: x86_64=0xFFFFFFFF80000000, aarch64=0x40080000)，替换 idt/ 中 7 处硬编码。

---

### [x] HARD-7: 空指针阈值语义化

**当前**: `0x1000` 作为空指针/低地址阈值在 `framework/proc/process.rs:35,50` 使用

**方案**: 定义 `pub const USER_ADDR_FLOOR: u64 = 0x1000;` 和 `pub const USER_ADDR_MIN: u64 = 0xFFFF;` 语义常量

**验收**:
- [x] 相关位置使用语义常量
- [x] 双架构 0w0e + 三审计通过

**完成记录** (2026-06-19): 新增 USER_ADDR_FLOOR (0x1000) + USER_ADDR_MIN (0xFFFF) 于 framework/mm/mod.rs, 替换 idt/ 5处 + proc/process.rs 2处硬编码。

---

## 二、解耦与边界合规 (DECOUPL-*)

> **原则**: services 只走 framework 顶层 re-export，不直接访问 arch 内部子模块。

### [ ] DECOUPL-1: services/driver/acpi.rs 边界违规修复

**当前**: 11 处 `framework::arch::x86_64::acpi::*` 直接访问内部模块

**方案**: 在 `framework/arch/mod.rs` 添加 `pub use platform::acpi::*;` re-export，services 改用 `framework::arch::acpi`

**验收**:
- [ ] services 中无 `framework::arch::x86_64::acpi` 引用
- [ ] `audit_services_boundary.py` 通过
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] DECOUPL-2: services/config/validate.rs 边界违规修复

**当前**: 直接访问 `framework::arch::apic`/`ioapic` 内部模块

**方案**: 在 `framework/arch/mod.rs` 添加 re-export，services 改用 `framework::arch::apic`/`framework::arch::ioapic`

**验收**:
- [ ] services 中无 `framework::arch::x86_64::apic`/`ioapic` 引用
- [ ] `audit_services_boundary.py` 通过
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] DECOUPL-3: services/proc/shadow_stack.rs 路径规范化

**当前**: 使用 `framework::arch::shadow_stack` 路径

**方案**: `arch/mod.rs` 已 re-export `shadow_stack::*`，services 应改用 `framework::arch::shadow_stack` → `framework::arch` 顶层 re-export

**验收**:
- [ ] services 使用 `framework::arch::shadow_stack_*` 顶层路径
- [ ] `audit_services_boundary.py` 通过
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] DECOUPL-4: framework 内部跨子模块深度访问治理

**当前**: `framework/proc/api.rs` 直接访问 `fs::initramfs::unpack`, `mm::numa::numa_init`, `arch::shadow_stack::cet_init` 等 3+ 层深度路径

**方案**: 各子系统在顶层 re-export 这些入口函数

**验收**:
- [ ] framework 内部无 3+ 层深度跨子系统访问
- [ ] `audit_coupling.py --depth` 通过
- [ ] 双架构 0w0e + 三审计通过

---

## 三、代码质量 (QUAL-*)

> **原则**: 内核代码不允许随意 panic，unsafe 必须有 SAFETY 注释，死代码必须审查。

### [ ] QUAL-1: 非 test 代码 unwrap() 消除

**当前**: 约 6 处 `unwrap()` 在非 test 代码中
- `services/ipc/msgq.rs:101,135`
- `framework/proc/api.rs:1476`
- `framework/syscall/clone.rs:172`
- 其他

**方案**: 改用 `?`、`match` 或 `unwrap_or_default()`。对确信不会 panic 的位置添加 `// SAFETY:` 注释说明不变式。

**验收**:
- [ ] 非 test/非 smoltcp 代码中无 `unwrap()`
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] QUAL-2: 可恢复的 panic!() 改为 Result

**当前**: 约 10 处 `panic!()` 在非 test 代码中
- `framework/mm/vmm_aarch64.rs:209`
- `framework/mm/kpti.rs:230`
- `framework/mm/vmm_x86_64.rs:1212`
- `services/fs/hvfs/zil_persist.rs:107,113`
- `services/config/validate.rs:128`
- 其他

**方案**: 区分合理 panic (size_assert/编译期不变式) 与可恢复 panic (运行时错误)。后者改为 `Result` 或 `klog_error + return`。

**验收**:
- [ ] 每处保留的 `panic!()` 有注释说明为何不可恢复
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] QUAL-3: unsafe impl Send/Sync 补 SAFETY 注释

**当前**: 15 处 `unsafe impl Send/Sync`，部分缺 SAFETY 注释
- `framework/dma_buf.rs:256-257`
- `framework/proc/process.rs:487-488,671-672`
- `framework/mm/pmm.rs:325-326`
- `framework/mm/vma.rs:1085`
- 其他

**方案**: 逐一审查，补全 `// SAFETY:` 注释，说明为何跨线程共享安全。

**验收**:
- [ ] 所有 `unsafe impl Send/Sync` 有 SAFETY 注释
- [ ] `audit_safety_coverage.py` 通过 (100%)
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] QUAL-4: framework 层 #[allow(dead_code)] 审查

**当前**: framework 层 27 处 `#[allow(dead_code)]`
- `framework/proc/api.rs` (2)
- `framework/dma/engine.rs` (2)
- `framework/cpu/mod.rs` (2)
- `framework/mm/vmm_aarch64.rs` (4)
- `framework/idt/idt.rs` (4)
- `framework/mm/slab.rs` (4)
- `framework/mm/pmm.rs` (2)
- `framework/proc/scheduler_ex.rs` (4)
- `framework/net/init.rs` (1)
- `framework/proc/user_proc.rs` (3)

**方案**: 逐一审查：
- 真正死代码 → 删除
- 待用代码 → 补测试后移除 allow
- 编译期必需 → 添加注释说明原因

**验收**:
- [ ] 每处保留的 `#[allow(dead_code)]` 有注释说明为何保留
- [ ] 无意义的死代码已删除
- [ ] 双架构 0w0e + 三审计通过

---

### [x] QUAL-5: services 层 #[allow(dead_code)] 审查

**当前**: services 层 13 处 `#[allow(dead_code)]`
- `services/ipc/mod.rs` (11) — IPC Phase N 占位函数
- `services/syscall/mod.rs` (1)
- `services/driver/power.rs` (1)

**方案**: services 层不应有大量死代码。IPC 占位函数应补全功能或删除。power.rs 审查是否真正待用。

**验收**:
- [ ] services 层 `#[allow(dead_code)]` 降至 0 或每处有充分理由
- [ ] 双架构 0w0e + 三审计通过

---

### [ ] QUAL-6: 非 smoltcp TODO 评估与标记

**当前**: 15 处非 smoltcp TODO
- `framework/dma/engine.rs:366` — DmaStream coherent 属性
- `services/io/iouring.rs:300,308,312,453` — VFS fd 表集成/网络异步/超时/缓冲区注册
- `services/driver/power.rs:322` — DVFS MSR/寄存器操作
- `services/syscall/types.rs` — 8 个 TRACK Phase N 占位
- `services/ipc/scheduler_integration.rs:97` — 定时器超时等待
- `services/ipc/signal.rs:71,90,107,116` — 信号处理函数注册/掩码/分发
- `services/ipc/sem.rs:88` — 阻塞等待队列
- `framework/timer/tickless.rs:227` — hrtimer 集成

**方案**: 逐一评估：
- 能做的做，补全功能
- 不能做的加 TRACK 标记，纳入后续计划
- 已过时的 TODO 删除

**验收**:
- [ ] 每处 TODO 有明确状态 (已实现/已标记 TRACK/已删除)
- [ ] 双架构 0w0e + 三审计通过

---

## 四、旧 SKIP 项重新评估 (REVAL-*)

> 以下项目在 TCB 缩减工程中标记 SKIP，现重新评估是否有条件推进。

### [x] REVAL-1: T1-2 信号投递策略提取 (原 SKIP)

**原 SKIP 原因**: 策略函数被 unsafe 核心函数内部调用，提取会导致 framework→services 反向依赖

**重新评估方向**:
- 可否通过 trait 注入模式 (类似 SchedDecision/PmmPolicy) 解耦？
- `signal_default_action`/`is_uncatchable`/`signal_pick_next` 是否可定义为 trait，services 实现？
- unsafe 核心函数调用策略函数是否可通过函数指针/回调解耦？

**验收**:
- [ ] 评估结论记录 (可行/仍 SKIP + 理由)
- [ ] 若可行，制定提取方案

---

### [ ] REVAL-2: T1-7 posix_timer 策略迁移 (原 SKIP)

**原 SKIP 原因**: 含 unsafe 回调指针转换，策略与机制深度耦合

**重新评估方向**:
- T5-1 已通过 `ServicesSyscallDispatch` trait 将 POSIX Timer 分发迁移到 services
- 剩余 unsafe 回调是否可通过 trait 注入模式封装？
- hrtimer 回调在中断上下文，是否可通过 `IrqDecision` trait 桥接？

**验收**:
- [ ] 评估结论记录
- [ ] 若可行，制定提取方案

---

### [ ] REVAL-3: T2-5 pcache 策略迁移 (原 SKIP)

**原 SKIP 原因**: 含 14 处 unsafe (UnsafeCell 裸指针/用户态拷贝/zeroed 初始化)

**重新评估方向**:
- 14 处 unsafe 是否可封装为 safe API (类似 `raw::MessageRef` 模式)？
- `pcache_copy_to_user` 是否已通过 `copy_to_user` 异常安全变体替代 (I-36/37/38 已修复)？
- UnsafeCell 操作是否可通过 `RefCell`/`OnceCell` 替代？

**验收**:
- [ ] 评估结论记录
- [ ] 若可行，制定提取方案

---

### [x] REVAL-4: T3-1 网络初始化策略提取 (原 SKIP)

**原 SKIP 原因**: 含 55 处 unsafe (smoltcp Interface/MMIO/DMA/中断)

**重新评估方向**:
- DHCP 配置策略/接口配置策略是否可独立提取 (不含硬件操作)？
- 协议栈初始化顺序策略是否可通过配置表驱动？

**验收**:
- [ ] 评估结论记录
- [ ] 若可行，制定提取方案

---

### [ ] REVAL-5: T4-1/T4-2/T4-3 credo/eBPF 策略提取 (原 SKIP)

**原 SKIP 原因**:
- T4-1: 深度依赖 PROCESS_TABLE 和 credo 内部模块
- T4-2: 含 unsafe 全局表裸指针访问
- T4-3: 含 30 处 unsafe (BpfInterpreter/用户态指针/bpf_map)

**重新评估方向**:
- T4-1/T4-2: 全局表裸指针是否可通过 `OnceLock`/`Mutex` 封装为 safe API？
- T4-3: 验证器与解释器是否可拆分？验证器 (策略) 是否 0 unsafe？

**验收**:
- [ ] 评估结论记录
- [ ] 若可行，制定提取方案

---

### [ ] REVAL-6: T5-3 epoll 策略迁移 (原 SKIP)

**原 SKIP 原因**: 含 3 处 unsafe (用户态指针读写)，深度依赖 VFS/scheduler/eventfd 等

**重新评估方向**:
- 3 处 unsafe 是否已通过 `copy_from/to_user` 替代 (I-36/37/38 已修复)？
- VFS 依赖是否可通过 `FsBackend` trait 解耦？

**验收**:
- [ ] 评估结论记录
- [ ] 若可行，制定提取方案

---

## 五、文档与验收闭合 (DOC-*)

### [ ] DOC-1: T6-1 验收清单闭合

**当前**: `tcb-reduction-plan.md` T6-1 验收清单 3 项 `[ ]` 未改 `[x]`
- `[ ] services/ipc/*.rs #![deny(unsafe_code)]`
- `[ ] framework/ipc/ 仅保留 unsafe 边界`
- `[ ] 双架构 0w0e + 三审计 + host-tests`

**方案**: 确认实际已满足，标记闭合

**验收**:
- [ ] 3 项验收标记为 `[x]`

---

### [ ] DOC-2: tcb-reduction-plan.md 进度总表更新

**当前**: 进度总表与实际不符
- T2 标"进行中 (2/6 完成, 1 SKIP)"，实际 5/6 完成, 1 SKIP
- T5 标"完成 (2/3 完成, 1 SKIP)"，实际 3/3 完成, 1 SKIP
- T6 标"进行中 (6/8 完成, 1 SKIP)"，实际 7/8 完成, 1 SKIP
- 合计标"20 完成, 7 SKIP, 6 待做"，实际 25 完成, 7 SKIP, 1 待做

**方案**: 更新进度总表为实际状态

**验收**:
- [ ] 进度总表与实际一致

---

### [x] DOC-3: engineering-discipline.md TCB 比率注释更新

**当前**: TCB 比率注释仍写 65.7%，且列出的"剩余候选" (T2-2/T2-3/T2-4/T5-1/T6-1) 已全部完成

**方案**: 更新为当前 50.0%，更新剩余候选列表

**验收**:
- [ ] TCB 比率注释与实际一致
- [ ] 剩余候选列表与实际一致

---

### [ ] DOC-4: deep-audit-2026-06-11.md 状态更新

**当前**: 审计文档中多项仍标"待修复"，但实际已在 maintenance-2026-06-11.md 中全部修复

**方案**: 更新 deep-audit 文档中的状态标记

**验收**:
- [ ] 所有已修复项标记为"已修复"

---

### [ ] DOC-5: engineering-progress.md E6 VFS 策略提取状态更新

**当前**: `engineering-progress.md` §二 E6 行标"进行中 (2/9)"，实际所有 E6 子项 (E6-1~E6-9) 均已完成

**方案**: 更新为"已完成 (9/9)"，补全完成日期与关键产出

**验收**:
- [ ] E6 行状态与实际一致
- [ ] 变更历史追加条目

---

### [ ] DOC-6: pi-mutex-design.md 状态更新

**当前**: 标记"待实施 (P1 插班 #3)"，实际 PI Mutex 已于 2026-06-08 完成

**方案**: 更新状态为"已完成 (2026-06-08)"，补全实际产出摘要

**验收**:
- [ ] 状态标记与实际一致

---

### [ ] DOC-7: uds-design.md 状态更新

**当前**: 标记"待实施 (Phase C.3)"，实际 UDS 已于 2026-06-08 完成

**方案**: 更新状态为"已完成 (2026-06-08)"，补全实际产出摘要

**验收**:
- [ ] 状态标记与实际一致

---

## 六、旧维护文档遗留未完成项 (LEGACY-*)

> 以下项目来自 maintenance-2026-06-11.md 中 `[ ]` 未闭合项，经源码验证后纳入。

### [ ] LEGACY-1: 用户态进程可正常运行 axsh

**来源**: maintenance-2026-06-11.md I-29 验收清单
**当前**: 用户态 Ring 3 切换已实现，但 axsh 集成运行尚未验证

**验收**:
- [ ] QEMU 启动后 axsh 可正常执行基本命令
- [ ] 双架构验证

---

### [ ] LEGACY-2: Socket 并发性能测试

**来源**: maintenance-2026-06-11.md I-42 验收清单
**当前**: SocketWaitQueue 基础设施已实现，但性能测试未补

**验收**:
- [ ] 单核 1000 个并发 send 延迟 < 1ms (QEMU 环境验证)

---

### [ ] LEGACY-3: virtio-blk I/O 中断路径实测

**来源**: maintenance-2026-06-11.md I-43 验收清单 + delivery-summary-2026-06-13.md
**当前**: ISR acknowledge + IoCompletionArray + 多实例已实现，但未在 QEMU + virtio 设备上实测

**验收**:
- [ ] QEMU virtio-blk I/O 中断路径实测通过
- [ ] 4K 写延迟 < 100μs (QEMU 环境)

---

### [ ] LEGACY-4: BlockOps thunk 移除优化

**来源**: maintenance-2026-06-11.md I-43 剩余工作
**当前**: 内核全部为内部 trait dispatch，BlockOps thunk 可在未来移除

**验收**:
- [ ] 评估是否可移除 BlockOps thunk
- [ ] 若可移除，完成移除并补测试

---

### [ ] LEGACY-5: HvFS 全部子系统 trait 化

**来源**: maintenance-2026-06-11.md I-04 验收清单
**当前**: 仅 Checksum trait 已完成，其余子系统 (SPA/DMU/ZAP/TXG/ZIL/ARC/RAID-Z) 待按需扩展

**验收**:
- [ ] 至少再完成 1 个子系统 trait 化 (如 SPA 或 DMU)
- [ ] host-test 验证

---

### [ ] LEGACY-6: sysctl 框架实现

**来源**: maintenance-2026-06-11.md I-46 验收清单
**当前**: 运行时调参 API 已有 (AtomicUsize)，但无 sysctl 框架

**验收**:
- [ ] 评估 sysctl 框架需求范围
- [ ] 若本期实施，完成基础 sysctl 注册/读取/修改 API

---

## 七、TRACK 标记项 (DRIVER-*)

> 以下为 USB/Display 驱动占位 TODO，当前标记为"保留占位"。维护周期应评估是否可推进。

### [ ] DRIVER-1: USB 驱动占位评估

**当前**: 6 处 TRACK 标记
- `TRACK-558BA7`: 扫描 PCI 总线查找 xHCI 控制器
- `TRACK-AE516E`: 初始化找到的控制器
- `TRACK-832FCE`: 枚举 USB 设备
- `TRACK-688EA7`: 实现 URB 提交
- `TRACK-2E0EB0`: 实现地址分配
- `TRACK-1F75C1`: 实现地址释放

**方案**: 评估是否有条件实现基础 USB 支持，或确认保留占位

**验收**:
- [ ] 评估结论记录 (可推进/保留占位 + 理由)

---

### [ ] DRIVER-2: Display 驱动占位评估

**当前**: 8 处 TRACK 标记
- `TRACK-599EDA`: 读取 HPD 引脚状态 (DP)
- `TRACK-B61830`: 实现实际的 AUX 通道读取 (DP)
- `TRACK-9B691E`: 实现实际的 AUX 通道写入 (DP)
- `TRACK-0350FE`: 轮询 LANE0_1_STATUS 寄存器 (DP)
- `TRACK-3C1169`: 轮询 LANE_ALIGN_STATUS_UPDATED 寄存器 (DP)
- `TRACK-CD5DA5`: 读取 HPD 引脚状态 (HDMI)
- `TRACK-7CCB60`: 通过 I2C/DDC 读取 EDID (HDMI)
- `TRACK-1BDEF6`: 配置 HDMI 控制器寄存器 (HDMI)

**方案**: 评估是否有条件实现基础 Display 支持，或确认保留占位

**验收**:
- [ ] 评估结论记录 (可推进/保留占位 + 理由)

---

## 八、执行分组

### 第 1 组 — 硬编码常量统一 (低风险，高收益)

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 1 | HARD-1 | KERNEL_BASE 重复定义消除 | 低 |
| 2 | HARD-4 | APIC 寄存器偏移用已有常量替换 (含在 HARD-2 中) | 低 |
| 3 | HARD-6 | KERNEL_TEXT_BASE 提取 | 低 |
| 4 | HARD-2 | framework 层 PAGE_SIZE 硬编码消除 | 低 |

### 第 2 组 — 硬编码常量统一 (续) + 解耦

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 5 | HARD-3 | services 层 PAGE_SIZE 硬编码消除 | 低 |
| 6 | HARD-5 | VIRTIO_MMIO_BASE 统一 | 低 |
| 7 | HARD-7 | 空指针阈值语义化 | 低 |
| 8 | DECOUPL-1 | services/driver/acpi.rs 边界违规修复 | 中 |

### 第 3 组 — 解耦与边界合规

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 9 | DECOUPL-2 | services/config/validate.rs 边界违规修复 | 中 |
| 10 | DECOUPL-3 | services/proc/shadow_stack.rs 路径规范化 | 低 |
| 11 | DECOUPL-4 | framework 内部跨子模块深度访问治理 | 中 |
| 12 | DOC-1 | T6-1 验收清单闭合 | 低 |

### 第 4 组 — 代码质量

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 13 | QUAL-1 | 非 test 代码 unwrap() 消除 | 中 |
| 14 | QUAL-3 | unsafe impl Send/Sync 补 SAFETY 注释 | 低 |
| 15 | QUAL-2 | 可恢复的 panic!() 改为 Result | 中 |
| 16 | QUAL-4 | framework 层 #[allow(dead_code)] 审查 | 中 |

### 第 5 组 — 代码质量 (续) + 文档

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 17 | QUAL-5 | services 层 #[allow(dead_code)] 审查 | 中 |
| 18 | QUAL-6 | 非 smoltcp TODO 评估与标记 | 低 |
| 19 | DOC-2 | tcb-reduction-plan.md 进度总表更新 | 低 |
| 20 | DOC-3 | engineering-discipline.md TCB 比率注释更新 | 低 |

### 第 6 组 — SKIP 重新评估

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 21 | REVAL-1 | T1-2 信号投递策略提取重新评估 | 评估 |
| 22 | REVAL-2 | T1-7 posix_timer 策略迁移重新评估 | 评估 |
| 23 | REVAL-3 | T2-5 pcache 策略迁移重新评估 | 评估 |
| 24 | REVAL-4 | T3-1 网络初始化策略提取重新评估 | 评估 |

### 第 7 组 — SKIP 重新评估 (续) + 驱动评估 + 文档

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 25 | REVAL-5 | T4-1/T4-2/T4-3 credo/eBPF 策略提取重新评估 | 评估 |
| 26 | REVAL-6 | T5-3 epoll 策略迁移重新评估 | 评估 |
| 27 | DRIVER-1 | USB 驱动占位评估 | 评估 |
| 28 | DRIVER-2 | Display 驱动占位评估 | 评估 |

### 第 8 组 — 文档收尾

| 序号 | 任务 ID | 任务 | 风险 |
|------|---------|------|------|
| 29 | DOC-4 | deep-audit-2026-06-11.md 状态更新 | 低 |
| 30 | HARD-4 | Cache line size 提取为公共常量 | 低 |

---

## 变更历史

| 日期 | 变更 |
|------|------|
| 2026-06-19 | 初始版本: 整合硬编码(7项)、解耦(4项)、代码质量(6项)、SKIP重新评估(6项)、文档(4项)、驱动评估(2项)，共 29 项任务 |
