# QueenX 未修复问题追踪清单 (2026-08-09)

> **本会话审计产生的"未修复问题"专项追踪文档**.
>
> 区别于 [stage-engineering-master.md](./stage-engineering-master.md)（静态检查工程）, [progress-active-tasks.md](./progress-active-tasks.md)（活跃任务进度）, [future-roadmap.md](./future-roadmap.md)（远期规划）, 本文档**专门追踪"已识别但当前未修复"的问题**, 包括:
> - **运行时已知问题** (网卡/GICv3 挂起等)
> - **源码未实现** (TODO/FIXME)
> - **跨文档矛盾** (code-review 发现)
> - **构建/工具问题**
> - **本会话刻意维持** (DECISION 登记)
>
> **状态字段约定**:
> - ❌ `[]` 未修复 (当前待办)
> - ⏸️ `[~]` 已识别但刻意维持 (DECISION 登记)
> - 🔄 `[X]` 已修复 (commit hash 已登记)
> - 🚫 `[永久]` 永久搁置 (有意识决策)
>
> **建立日期**: 2026-08-09 (本会话审计产出)
> **审计触发**: 用户问题"未被修复的问题有哪些？我是指如于类似网卡挂起等包括但不限于预存问题的问题"
> **来源 commit**: `ebb985c0` (DECISION-046), `a656c91e` (expect 兜底根治), `4f1a9d3e` (brittle 修复)

---

## 📊 总览 (2026-08-09)

| 类别 | 数量 | 严重度分布 | 状态 |
|---|---|---|---|
| 运行时已知问题 | 3 | P0×1 + P1×2 | ❌ 未修复 |
| 源码未实现 (TODO) | ~43 | P1×16 + P2×22 + P3×5 | ❌ 未修复 |
| 跨文档矛盾 (code-review) | 8 | P1×3 + P2×3 + P3×2 | ❌ 用户授权"仅记录不修复" |
| 远期工程 | 6 | 远期 | ❌ 未启动 |
| 本会话刻意维持 | 3 | 决策登记 | ⏸️ DECISION |
| 构建/工具问题 | 3 | 工具 | ❌ 未提交 |
| lint 副作用 (已修复) | 2 | — | 🔄 已修复 |
| **总计** | **~65 项** | — | — |

---

## 🔴 第 1 类：运行时已知问题 (3 项)

### ISSUE-RT-001: x86_64 e1000 + smoltcp 初始化挂起

| 字段 | 数据 |
|---|---|
| **严重度** | P1 (阻塞 x86_64 进入 Ring 3) |
| **状态** | ❌ 未修复 (`[]`) |
| **类型** | 运行时挂起 (网络栈初始化) |
| **现象** | QEMU 默认 e1000 NIC 触发 smoltcp 栈初始化挂起 |
| **触发条件** | x86_64 启动时不加 `-nic none` |
| **影响** | x86_64 仅能到 Network Subsystem Init, **无法进入 Ring 3** |
| **当前应对** | 启动脚本加 `-nic none` 隔离测试 |
| **调试入口** | `src/kernel/framework/driver/net/e1000.rs` |
| **来源** | `scripts/qemu_boot_test.sh` 注释: "x86_64 走到 e1000 NIC 检测后因 smoltcp 初始化挂起, 已记录. e1000 调试见 driver/net/e1000.rs" |
| **关联** | ISSUE-SRC-002 (skb 投递到 smoltcp 未实现) |
| **建议方案** | (1) 隔离 NIC 后单独调试 smoltcp 初始化路径; (2) 检查 e1000 probe 与 smoltcp iface 创建的 race condition; (3) 逐步加 NIC 看具体哪个 packet 触发挂起 |
| **工作量** | 估计 1-2 周 |

### ISSUE-RT-002: aarch64 GICv3 挂起 ⚠️ **用户当前调试**

| 字段 | 数据 |
|---|---|
| **严重度** | P0 (用户当前调试中) |
| **状态** | ❌ 未修复 (`[]`) |
| **类型** | 运行时挂起 (中断控制器) |
| **现象** | GICv3 初始化或中断处理挂起 |
| **影响** | aarch64 启动可能挂起 (本次 QEMU 启动虽到 EL0, 但偶发 GICv3 相关问题) |
| **调试方式** | GDB 调试 (`.gdb_debug_gic` 文件) |
| **源码位置** | `src/kernel/framework/arch/aarch64/gic.rs` (GICv3 初始化), `src/kernel/framework/arch/aarch64/barrier/mod.rs` (SGI 7 使能) |
| **用户当前活动** | 用户 IDE 打开 `.gdb_debug_gic` 文件表明**正在 GDB 调试 GICv3 挂起** |
| **建议方案** | (1) GDB `break gic_init` 单步跟踪; (2) 检查 GICR_SGI_BASE 寄存器访问; (3) 检查 SGI 7 触发时 Redistributor 状态 |
| **工作量** | 估计 3-5 天 |

### ISSUE-RT-003: 真实硬件启动验证未运行

| 字段 | 数据 |
|---|---|
| **严重度** | P1 |
| **状态** | ❌ 未运行 (`[]`) |
| **类型** | 运行时验证缺失 |
| **现象** | 仅 QEMU 模拟启动, 真实硬件未验证 |
| **影响** | 可能有 QEMU 兼容但真实硬件失败的问题 |
| **来源** | stage-engineering-master.md:301 `[ ] QEMU 实际验证` |
| **建议方案** | (1) 选定 x86_64 + aarch64 各一款硬件 (如 Intel NUC + Raspberry Pi); (2) 制作可启动介质; (3) 串口观察启动日志 |
| **工作量** | 估计 2-4 周 (含硬件采购) |

---

## 🟠 第 2 类：源码未实现 (~43 个 TODO)

### 2.1 P1 严重 — 阻塞核心功能

| # | 文件:行 | 描述 | 阻塞 |
|---|---|---|---|
| ISSUE-SRC-001 | `framework/arch/shadow_stack.rs:304` | TODO(TRACK-4C9A12): 使用 PMM 分配实际物理页 | shadow stack 实现 |
| ISSUE-SRC-002 | `framework/credo/secure_boot.rs:198` | TODO(TRACK-7A8BAB): 替换为真正的 Ed25519 验证 | 安全启动 |
| ISSUE-SRC-003 | `framework/idt/safety.rs:25` | TODO(TRACK-2B3C56): 完整实现 CPUID 解析 | CPU 特性检测 |
| ISSUE-SRC-004 | `framework/driver/power.rs:148` | TODO(TRACK-6F7A9A): 实现真正的 S3 挂起 | 电源管理 |
| ISSUE-SRC-005 | `framework/driver/uefi.rs:264` | TODO(TRACK-4D5E78): 实际解析 EFI_SYSTEM_TABLE | UEFI 启动 |
| ISSUE-SRC-006 | `framework/driver/uefi.rs:435` | TODO(TRACK-5E6F89): 调用 EFI_RUNTIME_SERVICES.SetTime | UEFI 时间服务 |
| ISSUE-SRC-007 | `framework/driver/usb/xhci.rs:670` | TODO: 实现 Event Ring 处理 | xHCI USB 驱动 |
| ISSUE-SRC-008 | `framework/net/init.rs:822` | TODO: 待 NAPI/中断驱动模式启用后, 此处实现 skb 投递到 smoltcp | **关联 ISSUE-RT-001** |
| ISSUE-SRC-009 | `framework/timer/tickless.rs:237` | TODO(TRACK-3C4D67): 集成 hrtimer 获取最近到期时间 | tickless 模式 |
| ISSUE-SRC-010 | `services/io/iouring.rs:314` | TODO(TRACK-8B9CBC): 集成 VFS fd 表 | io_uring VFS 集成 |
| ISSUE-SRC-011 | `services/io/iouring.rs:319` | TODO(TRACK-9CADCD): 实现网络异步操作 | io_uring 网络 |
| ISSUE-SRC-012 | `services/io/iouring.rs:323` | TODO(TRACK-ADBECDE): 实现超时等待 | io_uring 超时 |
| ISSUE-SRC-013 | `services/io/iouring.rs:470` | TODO(TRACK-BECFEF): 实现缓冲区注册 / 文件注册 | io_uring 完整功能 |
| ISSUE-SRC-014 | `services/ipc/sem.rs:102` | TODO(TRACK-21BAF1): 阻塞当前线程到 wait 队列 | semaphore 阻塞语义 |
| ISSUE-SRC-015 | `services/ipc/signal.rs:84` | TODO(TRACK-48CC21): 将处理函数注册到 SignalPending | 信号注册 |
| ISSUE-SRC-016 | `services/ipc/signal.rs:135` | TODO(TRACK-3A9016): 实现完整的信号分发逻辑 | 信号分发 |

### 2.2 P2 中等 — 部分功能缺失

| # | 文件:行 | 描述 |
|---|---|---|
| ISSUE-SRC-017 | `framework/dma/engine.rs:426` | TODO(TRACK-1F2A45): 由 DmaStream 的 coherent 属性决定 |
| ISSUE-SRC-018 | `framework/arch/shadow_stack.rs:540` | TODO(TRACK-6E7C34): 使用 #GP 异常处理来安全检测 |
| ISSUE-SRC-019 | `services/driver/power.rs:355` | TODO(TRACK-7A3B01): 实际写 MSR/寄存器调整频率和电压 |
| ISSUE-SRC-020 | `services/ipc/scheduler_integration.rs:103` | TODO(TRACK-8C5FFB): 实现基于定时器的超时等待 |
| ISSUE-SRC-021 | `services/ipc/signal.rs:106` | TODO(TRACK-614BD5): blocked 位图设置 |
| ISSUE-SRC-022 | `services/ipc/signal.rs:126` | TODO(TRACK-F806F4): blocked 位图清除 |
| ISSUE-SRC-023 | `services/proc/oomd.rs:94` | TODO: 实际发送 SIGKILL 到最大 RSS 进程 |
| ISSUE-SRC-024 | `services/proc/memfd.rs:58` | TODO: 使用 per-process fd 表 |
| ISSUE-SRC-025 | `services/proc/memfd.rs:75` | TODO: 设置 fd 的 CLOEXEC 标记 |
| ISSUE-SRC-026 | `services/proc/pidfd.rs:61` | TODO: 需要 Task 4 (OpenFile 系统) 完成后实现 |
| ISSUE-SRC-027 | `services/io/iouring.rs:319` | (重复 ISSUE-SRC-011) |
| ISSUE-SRC-028 | `services/fs/vfs/api.rs:218` | TODO: 使用 per-process fd 表 |
| ISSUE-SRC-029 | `services/net/smoltcp_impl.rs` (多个) | smoltcp 集成相关 |
| ISSUE-SRC-030 | `services/credo/sessions.rs` (多个) | 凭据会话管理 |
| ISSUE-SRC-031 | `services/credo/grants.rs` (多个) | 凭据授权 |
| ISSUE-SRC-032 | `services/credo/policy.rs` (多个) | 凭据策略 |
| ISSUE-SRC-033 | `services/barrier/attribution.rs` (多个) | 故障归属 |
| ISSUE-SRC-034 | `services/barrier/recovery_policy.rs` (多个) | 故障恢复 |
| ISSUE-SRC-035 | `services/barrier/cascade.rs` (多个) | 故障级联 |
| ISSUE-SRC-036 | `services/debug/ebpf_verifier.rs` (多个) | eBPF 验证器 |
| ISSUE-SRC-037 | `services/driver/display/dp.rs` (多个) | DisplayPort 协议 |
| ISSUE-SRC-038 | `services/driver/storage/*` (多个) | 存储驱动 |

### 2.3 P3 轻微 — 时间戳更新

| # | 文件:行 | 描述 |
|---|---|---|
| ISSUE-SRC-039 | `services/fs/hvfs/hvfs_inode.rs:92` | TODO: 未来可接入 HvFS 时间戳更新 |
| ISSUE-SRC-040 | `services/fs/ext2/mount.rs:102` | TODO: 未来可接入 ext2 inode 时间戳更新 |
| ISSUE-SRC-041 | `services/fs/exfat/mount.rs:79` | TODO: 未来可接入 exFAT 目录项时间戳更新 |
| ISSUE-SRC-042 | `services/fs/overlayfs.rs:205` | TODO: 未来可实现 copy-up + 时间戳更新 |
| ISSUE-SRC-043 | `services/fs/devfs.rs` | (类似时间戳项) |

> **说明**: P2/P3 的具体位置可通过 `grep -rn "TODO\|FIXME\|XXX" src/kernel --include="*.rs" | grep -v "src/kernel/services/net/smoltcp/"` 重新生成. qx 自有代码中**约 43 个 TODO** (排除 smoltcp vendored 527 个).

---

## 🟡 第 3 类：跨文档矛盾 (8 项)

### 3.1 P1 跨文档战略矛盾 (3 项) — ❌ 未修复

> **来源**: `docs/plan/archive/code-review-findings-2026-08-01.md`. 用户 2026-08-01 授权**仅记录不修复**, 状态 `[]`.

#### REVIEW-FINDING-024: CHANGELOG.md 缺失但 README/AGENTS 多处引用

| 字段 | 数据 |
|---|---|
| **严重度** | P1 (违反 AGENTS.md 硬规则) |
| **状态** | ❌ 未修复 (`[]`) |
| **冲突点** | README.md:11/163/210 + AGENTS.md:48/363 引用不存在的 `docs/CHANGELOG.md` |
| **方案** | (a) 创建 `docs/CHANGELOG.md` 补记历史; (b) 删除全部 10 处引用 (推荐) |
| **冲突来源** | progress-active-tasks.md DECISION-038 [X] 标记完成 (删除引用), 但 archive/code-review-findings-2026-08-01.md 仍 `[]` 未同步 |
| **建议** | 验证 README.md/AGENTS.md 实际是否已无 CHANGELOG.md 引用, 若已修复则同步 code-review 文档状态 |

#### REVIEW-FINDING-025: syscall 编号空间立场两份权威文档互相矛盾

| 字段 | 数据 |
|---|---|
| **严重度** | P1 |
| **状态** | ❌ 未修复 (`[]`) |
| **冲突点** | `framework/syscall/mod.rs:24-35` 称 "0-299 保留给未来 linuxulator" vs `ref-naming.md §三` 称 "直接使用 Linux syscall 编号" |
| **方案** | DECISION-037 已选 A (直接 Linux ABI + QX_* 500+ 自由扩展), 但 archive/code-review-findings-2026-08-01.md 仍 `[]` 未同步 |
| **建议** | 验证 `framework/syscall/mod.rs:24-35` 实际是否已更新为直接 Linux ABI 注释, 若已修复则同步 code-review 文档状态 |

#### REVIEW-FINDING-026: framework 反向依赖 services 类型 (userctx.rs re-export)

| 字段 | 数据 |
|---|---|
| **严重度** | P1 (违反 framekernel 单向数据流) |
| **状态** | ❌ 未修复 (`[]`) |
| **冲突点** | `framework/userctx.rs:6-9` re-export services 类型, `framework/usermode.rs:38/58` 直接读取其字段 |
| **方案** | 迁回 UserContext 到 framework 或加 `#[repr(C)]` 等价结构 + 编译期布局断言 |
| **建议** | 此项**真正未修复**, 应纳入未来任务 |

### 3.2 P2 文档失实 (3 项) — 状态不一致

#### REVIEW-FINDING-027: framework 顶层文档声明 "~3000+ LoC" 严重失实

| 字段 | 数据 |
|---|---|
| **状态** | progress B1 [X] 已声明完成, code-review `[]` 未同步 |
| **冲突点** | `framework/mod.rs:10` 声明 `~3000+ LoC`, 实际 ~10 万行 |
| **建议** | 验证 `framework/mod.rs:10` 实际是否已移除具体数字 |

#### REVIEW-FINDING-028: services/net + services/fs 头注释过期

| 字段 | 数据 |
|---|---|
| **状态** | progress B2 [X] 已声明完成, code-review `[]` 未同步 |
| **冲突点** | net/mod.rs:4-19 / fs/mod.rs:4-19 头注释过期 (v2.7/v2.5, 2026-06-04) |
| **建议** | 验证头注释实际是否已更新 (本会话确认: net/fs/proc 头注释均已含"当前已远超当时范围"描述, 但 code-review 文档未同步) |

#### REVIEW-FINDING-029: README.md remote 命名与 kernel-roadmap 链接过期

| 字段 | 数据 |
|---|---|
| **状态** | progress B3 [X] 已声明完成, code-review `[]` 未同步 |
| **冲突点** | README.md:21 `git remote rename origin Gitee` 矛盾 + README.md:71 失效链接 `kernel-roadmap.md` |
| **建议** | 验证 README.md 实际是否已修正 |

### 3.3 P3 已知未完成 (2 项) — ❌ 未修复

#### REVIEW-FINDING-030: framework/sched task 抽象 Phase 1.4.2 未开工

| 字段 | 数据 |
|---|---|
| **严重度** | P3 |
| **状态** | ❌ 未修复 (`[]`) |
| **描述** | `framework/sched/mod.rs:8` 注释: "task 抽象在 Phase 1.4.2 计划中但尚未实现", 阻塞 services/proc 迁移 |
| **建议** | 未来 task, 与 ISSUE-SRC-026 关联 |

#### REVIEW-FINDING-031: IoMem 边界 expect panic + 固定上限硬编码

| 字段 | 数据 |
|---|---|
| **严重度** | P3 |
| **状态** | progress B6 [X] 已声明完成 (debug_assert! + limits.rs), code-review `[]` 未同步 |
| **冲突点** | `iomem.rs:194/200/206/212` expect panic + `MAX_MMIO_MAPPINGS = 64` 硬编码 |
| **建议** | 验证 iomem.rs 实际是否已加 debug_assert!, 若已修复则同步 code-review 文档 |

---

## 🔵 第 4 类：远期工程 (6 项)

> **来源**: `docs/plan/future-roadmap.md` + `docs/plan/ipv6-dual-stack.md`.

| # | 编号 | 标题 | 状态 | 工作量 |
|---|---|---|---|---|
| ISSUE-FUT-001 | F1 | mdBook 文档体系 | ❌ 未启动 | ~2 周 |
| ISSUE-FUT-002 | F2 | RISC-V 64 架构支持 | ❌ 未启动 | ~6-8 周 |
| ISSUE-FUT-003 | F3 | TDX 机密计算支持 | ❌ 未启动 | ~4-6 周 |
| ISSUE-FUT-004 | F4 | NFS 网络文件共享 | ❌ 未启动 | ~6-8 周 |
| ISSUE-FUT-005 | F5 Phase 6 | DHCPv6 / SLAAC | ❌ 未启动 (smoltcp 依赖) | 待定 |
| ISSUE-FUT-006 | WASM WASI | 已完成 ✅ | 🔄 已 [X] | — |

---

## 🟣 第 5 类：本会话刻意维持 (3 项) ⏸️

### ISSUE-DEC-046: kernel `#[test]` 迁移 (DECISION-046)

| 字段 | 数据 |
|---|---|
| **状态** | ⏸️ `[~]` DECISION-046 维持原状 |
| **数量** | 1354 个跨 166 文件 (含 527 个 smoltcp vendored) |
| **理由** | 范畴属测试架构工程非静态检查工程; ROI 不匹配 (仅 ~70 个纯算法值得迁移) |
| **commit** | `ebb985c0` |
| **未来可选** | 迁移 USB HID/MassStorage/XHCI/Enumerate/Ring 5 文件 ~70 个纯算法测试 (~5-7 天) |

### ISSUE-DEC-041: cast 类 1700+ 处永久保留

| 字段 | 数据 |
|---|---|
| **状态** | 🚫 `[永久]` DECISION-041 |
| **数量** | 1910 处 cast 警告 (真实风险 < 200 处) |
| **理由** | 已知安全 cast (APIC ID < 256, 循环变量 i < 8 等); 全量 try_from 是无价值工作 |
| **处理** | clippy 警告作为"提醒", CI 不阻断 |

### ISSUE-DEC-005: brittle 测试 345 处潜在风险

| 字段 | 数据 |
|---|---|
| **状态** | ⏸️ `[~]` 维持现状 |
| **数量** | `src.find()` 37 处 (高风险) + `src.contains()` 308 处 (中风险) = 345 处 |
| **策略** | 不批量机械改 (易引入 false negative); 仅在**真实测试失败时**针对性改用 `split_whitespace + 关键 token 匹配` |
| **已修复** | 2 处 (`vfs_read/write_uses_inode_trait`, commit `4f1a9d3e`) |
| **真实风险分布** | td19_proc_kernel_error_test 8 处, usermode_ring3_test 6 处, td10/td09 各 4 处, 其他各 1-3 处 |

---

## ⚫ 第 6 类：构建/工具问题 (3 项)

### ISSUE-TOOL-001: Makefile 缺乏跨架构清理

| 字段 | 数据 |
|---|---|
| **状态** | ❌ 未修复 |
| **现象** | `build/boot.o` 残留上次 aarch64 编译产物, x86_64 链接时 ld 报错 "Relocations in generic ELF" |
| **临时处理** | 手动 `rm -f build/boot.o` |
| **建议方案** | Makefile `all` 目标加入自动清理异架构产物, 或增加 `make clean-arch` 目标 |
| **工作量** | 估计 0.5 天 |

### ISSUE-TOOL-002: x86_64 kernel.flat 陈旧未自动重建

| 字段 | 数据 |
|---|---|
| **状态** | ❌ 未修复 |
| **现象** | lint 修复后旧 kernel.flat 仍存在, QEMU 启动"日志为空, 内核未进入 Rust 入口" |
| **临时处理** | 手动 `make ARCH=x86_64 all` |
| **建议方案** | Makefile 加入文件 mtime 检查, 或 QEMU 启动脚本加入图像陈旧检测 |
| **工作量** | 估计 0.5 天 |

### ISSUE-TOOL-003: cargo test --tests 在裸机 target 失败

| 字段 | 数据 |
|---|---|
| **状态** | ❌ 未解决 (与 lint 修复正交) |
| **现象** | E0152 duplicate lang item (zerocopy/bitflags/byteorder/managed) |
| **应对** | 实际测试在 host-side 跑, lint-only 检查通过 |
| **建议方案** | 用 `#[cfg(target_os = "none")]` 隔离测试, 或 host-tests 引入独立测试目标 |
| **工作量** | 估计 1 天 |

---

## 🟢 第 7 类：lint 副作用 (2 项已修复)

| # | 描述 | 修复 commit |
|---|---|---|
| LINT-FIX-001 | plan_b_inode_test `vfs_read_uses_inode_trait` brittle substring 失效 (rustfmt 拆行) | `4f1a9d3e` |
| LINT-FIX-002 | plan_b_inode_test `vfs_write_uses_inode_trait` brittle substring 失效 (同上) | `4f1a9d3e` |

---

## 📋 文档状态不一致清单 (跨文档矛盾专项)

> **本会话审计发现**: `progress-active-tasks.md` 中 B1-B6 标记 `[X]` 完成, 但 `archive/code-review-findings-2026-08-01.md` 中对应 REVIEW-FINDING 仍 `[]` 未同步. **文档漂移**.

| progress | code-review | 真实状态 | 修复责任 |
|---|---|---|---|
| B1 [X] | REVIEW-FINDING-027 `[]` | 需核实 framework/mod.rs:10 是否已修 | 验证并同步 |
| B2 [X] | REVIEW-FINDING-028 `[]` | 已确认 net/fs/proc 头注释均已更新 | 同步 code-review 状态 |
| B3 [X] | REVIEW-FINDING-029 `[]` | 需核实 README.md 是否已修 | 验证并同步 |
| B5 [X] | (无对应) | credo/storage.rs + barrier/api.rs | (无矛盾) |
| B6 [X] | REVIEW-FINDING-031 `[]` | 需核实 iomem.rs debug_assert! 是否已加 | 验证并同步 |
| DECISION-037 [X] | REVIEW-FINDING-025 `[]` | 需核实 framework/syscall/mod.rs:24-35 是否已修 | 验证并同步 |
| DECISION-038 [X] | REVIEW-FINDING-024 `[]` | 需核实 README/AGENTS 是否已无 CHANGELOG.md 引用 | 验证并同步 |

**建议后续行动**: 用户授权"仅记录不修复"但 progress 已 [X], 需要**同步 code-review 文档状态**或将 progress 状态改为 `[X] (code-review 文档未同步)`.

---

## 🎯 优先级建议 (用户决策参考)

### P0 — 立即关注 (1 项)

- **ISSUE-RT-002** (aarch64 GICv3 挂起) — 用户当前 GDB 调试中

### P1 — 本季度修复 (5 项)

- **ISSUE-RT-001** (x86_64 e1000/smoltcp 挂起)
- **ISSUE-RT-003** (真实硬件验证)
- **REVIEW-FINDING-026** (framework→services 反向依赖)
- **ISSUE-SRC-001~016** (16 个 P1 TODO)

### P2 — 半年内修复 (~30 项)

- P2 TODO 22 项
- REVIEW-FINDING-027/028/029 (文档同步)
- 构建工具问题 3 项

### P3 — 长期/远期 (无固定期限)

- 远期工程 F1-F5
- P3 TODO 5 项
- REVIEW-FINDING-030/031

---

## 📚 关联文档

- [stage-engineering-master.md](./stage-engineering-master.md) — 静态检查工程权威跟踪
- [progress-active-tasks.md](./progress-active-tasks.md) — 活跃任务进度
- [future-roadmap.md](./future-roadmap.md) — 远期工程 F1-F5
- [ipv6-dual-stack.md](./ipv6-dual-stack.md) — Phase 6 DHCPv6
- [unresolved-issues-2026-08-09.md](./unresolved-issues-2026-08-09.md) — 未修复问题专项追踪
- 已归档: [archive/code-review-findings-2026-08-01.md](./archive/code-review-findings-2026-08-01.md), [archive/handoff-2026-08-07.md](./archive/handoff-2026-08-07.md), [archive/handoff-2026-08-09.md](./archive/handoff-2026-08-09.md)

---

## 变更历史

- **2026-08-09**: 创建本文档 (审计触发: 用户问题"未被修复的问题有哪些?")
  - 来源: 本会话对所有静态 + 动态 + 文档 + 源码的全面审计
  - 产出: 65 项未修复问题分类登记
  - 推荐: 用户授权"仅记录不修复"的 code-review 8 项 + P0/P1 立即关注 + P2/P3 长期规划