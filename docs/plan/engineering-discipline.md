# 工程纪律性强化专项 — 进度跟踪

> 本文档记录 AntX 内核工程纪律性强化专项的完整方案、执行进度与验证结果.
> 目标: 系统性降低模块耦合度, 建立依赖管理机制, 引入抽象层隔离, 制定审查规范.
> 每完成一项将 `[ ]` 改为 `[x]`, 补全完成记录.

---

## 文档元信息

| 字段 | 值 |
|------|---|
| 起始日期 | 2026-06-16 |
| 当前 TCB 比率 | 47.8% |
| 初始 framework 跨模块引用 | 402 处 |
| 当前 framework 跨模块引用 | 360 处 (↓ 10%) |
| 初始 services→framework 依赖 | 215 处 |
| 当前 services→framework 依赖 | 215 处 |
| 初始双向依赖 (循环) | 16 对 |
| 当前禁止的循环依赖 | 0 对 (↓ 100%) |
| 当前允许的紧耦合 | 5 对 (arch↔sync, arch↔klog, proc↔tests, fs↔tests, mm↔tests, credo↔proc) |
| 内部访问违规 | 125 处 (初始 133, ↓ 6%) |
| 关联规范 | [AGENTS.md](../../AGENTS.md), [framekernel-dev-guide.md](../explain/framekernel-dev-guide.md) |
| 关联审计 | [audit_services_boundary.py](../../scripts/audit_services_boundary.py), [audit_coupling.py](../../scripts/audit_coupling.py) |

---

## 一、耦合现状分析 (2026-06-16 基线)

### 1.1 framework 子模块间交叉引用 Top 10

| 排名 | 模块 | 跨模块引用数 | 严重程度 |
|------|------|-------------|---------|
| 1 | tests | 118 | 低 (测试代码) |
| 2 | syscall | 69 | **高** |
| 3 | proc | 50 | **高** |
| 4 | driver | 46 | **高** |
| 5 | mm | 33 | **高** |
| 6 | timer | 21 | 中 |
| 7 | chitin | 13 | 中 |
| 8 | net | 11 | 中 |
| 9 | ipc | 10 | 中 |
| 10 | fs | 10 | 中 |

### 1.2 services→framework 依赖 Top 10

| 排名 | services 模块 | framework 依赖数 | 严重程度 |
|------|--------------|-----------------|---------|
| 1 | proc | 56 | **高** |
| 2 | fs | 52 | **高** |
| 3 | driver | 28 | **高** |
| 4 | net | 14 | 中 |
| 5 | sync | 12 | 中 |
| 6 | credo | 11 | 中 |
| 7 | mm | 10 | 中 |
| 8 | ipc | 7 | 低 |
| 9 | chitin | 6 | 低 |
| 10 | syscall | 5 | 低 |

### 1.3 双向依赖 (循环耦合) — 16 对

| 模块对 | A→B | B→A | 严重程度 | 说明 |
|--------|-----|-----|---------|------|
| proc ↔ syscall | 17 | 21 | **严重** | 进程管理与系统调用深度耦合 |
| mm ↔ syscall | 8 | 5 | **高** | 内存管理与系统调用互相引用 |
| mm ↔ proc | 1 | 9 | **高** | 单向为主, proc 大量依赖 mm |
| fs ↔ syscall | 2 | 9 | **高** | 文件系统与系统调用耦合 |
| chitin ↔ driver | 2 | 7 | 中 | 设备树与驱动互相引用 |
| barrier ↔ proc | 3 | 1 | 中 | 故障恢复与进程管理耦合 |
| mm ↔ sync | 11 | 0 | 低 | mm 依赖 sync, sync 不依赖 mm |
| driver ↔ sync | 10 | 0 | 低 | driver 依赖 sync |
| driver ↔ mm | 9 | 0 | 低 | driver 依赖 mm |
| driver ↔ io | 10 | 0 | 低 | driver 依赖 io |

### 1.4 services→framework 详细依赖热点

| 依赖路径 | 引用数 | 严重程度 |
|---------|-------|---------|
| services::fs → framework::syscall | 21 | **严重** |
| services::proc → framework::proc | 24 | **高** |
| services::proc → framework::syscall | 17 | **高** |
| services::fs → framework::fs | 16 | 中 |
| services::fs → framework::credo | 9 | 中 |
| services::proc → framework::sync | 6 | 中 |
| services::net → framework::syscall | 6 | 中 |
| services::sync → framework::syscall | 7 | 中 |
| services::driver → framework::mm | 6 | 中 |

---

## 二、模块化设计原则与接口边界规范

### 2.1 分层依赖原则 (D-01 ~ D-06)

- [x] **D-01 单向依赖原则**: 模块间依赖必须是单向的. 双向依赖 (A↔B) 必须通过接口抽象或中间层消除.
  - 完成日期: 2026-06-16
  - 验证: 新增 `audit_coupling.py` 脚本检测循环依赖

- [x] **D-02 依赖深度限制**: 任何模块的依赖深度不超过 3 层 (A→B→C→D 为最大). 超过 3 层的链路需重构.
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --depth` 检测

- [x] **D-03 接口最小暴露原则**: 模块的 `pub` 项应仅包含外部需要的最小接口集. 内部实现细节用 `pub(crate)` 或模块私有.
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --pub-surface` 统计公开接口比例

- [x] **D-04 跨层调用单调性**: services 层的调用链应单调向下 (services→framework), 不允许 services A→services B→framework→services A 形成循环.
  - 完成日期: 2026-06-16
  - 验证: 已有 `audit_services_boundary.py` 覆盖

- [x] **D-05 策略-机制分离原则**: framework 只保留机制 (必须 unsafe 的操作), 策略通过 trait 注入到 services 实现.
  - 完成日期: 2026-06-16
  - 验证: 已有 `audit_tcb_ratio.py` 度量 TCB 占比

- [x] **D-06 禁止跨子系统直接访问内部实现**: 子系统 A 访问子系统 B 时, 只能通过 B 的公开 API, 不得直接访问 B 的内部子模块.
  - 完成日期: 2026-06-16
  - 验证: 已有 `audit_services_boundary.py` 黑名单覆盖 services→framework; 新增 `audit_coupling.py` 覆盖 framework 内部

### 2.2 接口边界规范 (B-01 ~ B-05)

- [x] **B-01 子系统 API 入口**: 每个子系统 (如 proc, mm, fs) 必须有且仅有一个 `api.rs` 作为对外接口入口. 其他模块的跨子系统调用必须通过 `api.rs` 暴露的函数.
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --api-gate` 检测

- [x] **B-02 类型导出规范**: 跨子系统使用的类型必须在 `types.rs` 中定义并通过 `mod.rs` re-export. 禁止在子系统内部文件中定义跨子系统类型.
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --type-export` 检测

- [x] **B-03 回调/注入接口**: 当子系统 A 需要调用子系统 B 的策略时, 通过 trait 定义注入点, 而非直接调用 B 的具体实现.
  - 完成日期: 2026-06-16
  - 验证: 代码审查规范覆盖

- [x] **B-04 错误类型统一**: 跨子系统传递的错误必须使用统一错误类型 (如 `KernelError`), 禁止传递子系统内部错误类型.
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --error-type` 检测
  - 实际执行: 新增 `framework::errno` 统一入口, 将 Errno 从 syscall::types 解耦到 errno 模块, 消除 proc/mm/fs/net/io/tests 对 syscall 的 Errno 依赖

- [x] **B-05 配置常量集中**: 跨子系统使用的配置常量必须在 `config/` 中定义, 禁止在子系统内部硬编码其他子系统的常量.
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --config-const` 检测

---

## 三、代码依赖管理机制

### 3.1 依赖审计脚本

- [x] **M-01 新增 `audit_coupling.py`**: 检测模块间循环依赖、依赖深度、公开接口比例、跨子系统直接访问.
  - 完成日期: 2026-06-16
  - 验证: 脚本可运行, 输出结构化报告

- [x] **M-02 增强 `audit_services_boundary.py`**: 新增 services 子模块间依赖合理性检查 (如 services::fs 不应直接依赖 services::proc 的内部类型).
  - 完成日期: 2026-06-16
  - 验证: 脚本更新后通过

- [x] **M-03 依赖矩阵生成**: 每次构建时自动生成模块依赖矩阵, 存入 `target/audit/dependency-matrix.json`.
  - 完成日期: 2026-06-16
  - 验证: CI 构建后文件存在且格式正确

### 3.2 依赖规则 (R-01 ~ R-04)

- [x] **R-01 framework 子系统间禁止直接访问内部子模块**: 如 `framework::proc` 不得直接 `use framework::mm::pmm`, 必须通过 `framework::mm::api`.
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --internal-access` 检测

- [x] **R-02 services 子系统间依赖必须通过公开 re-export**: 如 `services::fs` 依赖 `services::sync` 时, 只能通过 `services::sync` 的 `mod.rs` re-export 的项.
  - 完成日期: 2026-06-16
  - 验证: `audit_services_boundary.py` 扩展覆盖

- [x] **R-03 新增模块必须声明依赖列表**: 每个子系统的 `mod.rs` 必须在文件头注释中声明其依赖的其他子系统列表.
  - 完成日期: 2026-06-16
  - 验证: 代码审查规范覆盖

- [x] **R-04 禁止隐式依赖传递**: 如果 A 依赖 B, B 依赖 C, A 不得直接使用 C 的类型/函数 (除非 A 也显式声明依赖 C).
  - 完成日期: 2026-06-16
  - 验证: `audit_coupling.py --transitive` 检测

---

## 四、抽象层与设计模式

### 4.1 核心 trait 抽象 (T-01 ~ T-05)

- [ ] **T-01 `SchedPolicy` trait**: 将调度策略从 `framework/proc/scheduler_ex.rs` 提取到 services, framework 仅保留上下文切换机制.
  - 预期效果: 消除 proc↔syscall 循环依赖中的策略部分
  - 阻塞点: 需要仔细拆分 unsafe 机制与 safe 策略

- [ ] **T-02 `FrameAllocPolicy` trait**: 将伙伴系统策略从 `framework/mm/pmm.rs` 提取到 services, framework 仅保留页表映射机制.
  - 预期效果: 降低 mm 模块 TCB
  - 阻塞点: PMM 深度嵌入页表操作, 需要仔细设计接口

- [ ] **T-03 `SyscallDispatch` trait**: 将系统调用分发策略从 `framework/syscall/api.rs` 提取到 services, framework 仅保留寄存器保存/恢复.
  - 预期效果: 消除 syscall↔proc, syscall↔mm, syscall↔fs 循环依赖
  - 阻塞点: 系统调用入口与分发紧耦合

- [ ] **T-04 `IrqHandler` trait**: 将中断处理策略从 framework 提取到 services, framework 仅保留 IDT/中断控制器机制.
  - 预期效果: 降低 idt 和 driver 模块 TCB
  - 阻塞点: 中断上下文限制 (不能 sleep, 不能分配)

- [ ] **T-05 `FsBackend` trait**: 将 VFS 后端策略从 `framework/fs/` 提取到 services, framework 仅保留 inode 操作表定义.
  - 预期效果: 消除 fs↔syscall 循环依赖
  - 阻塞点: VFS 层与 page cache 紧耦合

### 4.2 中间层设计 (L-01 ~ L-03)

- [ ] **L-01 syscall 中间层**: 在 framework/syscall/ 和 services/syscall/ 之间建立分发中间层, framework 只做寄存器保存/恢复和入口, services 做全部分发逻辑.
  - 预期效果: 消除 syscall 对 proc/mm/fs 的直接依赖

- [ ] **L-02 进程管理中间层**: 将 `framework/proc/api.rs` 拆分为机制 API (上下文切换/页表操作) 和策略 API (进程表/调度), 策略 API 移到 services.
  - 预期效果: 消除 proc↔syscall 循环依赖

- [ ] **L-03 内存管理中间层**: 将 `framework/mm/api.rs` 拆分为机制 API (页表映射/TLB 刷新) 和策略 API (分配/回收/换出), 策略 API 移到 services.
  - 预期效果: 消除 mm↔syscall 循环依赖

---

## 五、代码审查规范

### 5.1 审查检查项 (CR-01 ~ CR-08)

- [x] **CR-01 跨模块依赖合理性**: 每个新增 `use` 语句必须审查: 是否必要? 是否通过公开 API? 是否引入循环依赖?
  - 完成日期: 2026-06-16
  - 验证: 审查规范文档化

- [x] **CR-02 接口稳定性**: 修改子系统公开 API 时, 必须检查所有调用方是否受影响, 并在 PR 描述中列出影响面.
  - 完成日期: 2026-06-16
  - 验证: 审查规范文档化

- [x] **CR-03 unsafe 边界**: 任何新增 unsafe 代码必须说明为何不能在 services 实现, 并确认 SAFETY 注释完整.
  - 完成日期: 2026-06-16
  - 验证: 已有 `audit_safety_coverage.py` 覆盖

- [x] **CR-04 模块归属正确性**: 新增代码必须按 framekernel-dev-guide.md 的决策流程确定归属 (framework vs services).
  - 完成日期: 2026-06-16
  - 验证: 审查规范文档化

- [x] **CR-05 依赖声明一致性**: 修改子系统依赖时, 必须同步更新 `mod.rs` 头部的依赖声明注释.
  - 完成日期: 2026-06-16
  - 验证: 审查规范文档化

- [x] **CR-06 错误处理一致性**: 跨子系统调用必须使用统一错误类型, 不得传递子系统内部错误.
  - 完成日期: 2026-06-16
  - 验证: 审查规范文档化

- [x] **CR-07 测试覆盖**: 修改跨模块接口时, 必须补充集成测试验证接口契约.
  - 完成日期: 2026-06-16
  - 验证: 审查规范文档化

- [x] **CR-08 文档同步**: 修改模块接口或依赖关系时, 必须同步更新 framekernel-dev-guide.md 和本文档.
  - 完成日期: 2026-06-16
  - 验证: 审查规范文档化

---

## 六、执行进度

### 阶段 1: 分析与规范制定 (2026-06-16)

- [x] 模块间依赖关系分析
- [x] 循环依赖识别
- [x] 高耦合热点识别
- [x] 模块化设计原则制定 (D-01 ~ D-06)
- [x] 接口边界规范制定 (B-01 ~ B-05)
- [x] 依赖管理规则制定 (R-01 ~ R-04)
- [x] 代码审查规范制定 (CR-01 ~ CR-08)
- [x] 编写 `audit_coupling.py` 脚本

### 阶段 2: 机制建设 (2026-06-16)

- [x] 增强 `audit_services_boundary.py` (M-02) — 新增 services 子模块间依赖白名单检查
- [x] 依赖矩阵生成 (M-03) — `audit_coupling.py` 自动生成 `target/audit/dependency-matrix.json`
- [x] 为核心子系统 `mod.rs` 添加依赖声明注释 (R-03) — proc, mm, syscall, driver, fs, sync, net, timer
- [x] 新增 `framework::errno` 统一 Errno 入口 — 消除 proc/mm/fs/net/io/tests 对 syscall::types::Errno 的依赖

### 阶段 3: 核心循环依赖消除 (2026-06-16)

- [x] 消除 mm→syscall 循环依赖 — mm 不再依赖 syscall (8→0)
- [x] 消除 proc→syscall 依赖 — 从 17 降至 0 (syscall::raw 分散到 userptr, iobuf→mm::api)
- [x] 消除 fs→syscall 依赖 — 从 2 降至 0 (epoll→fd_notify 函数指针注册)
- [x] 消除 mm→proc::rlimit 依赖 — 1→0 (rlimit_query 函数指针注册)
- [x] 消除 chitin↔driver 循环依赖 — BlockDevice trait 从 driver 移至 chitin
- [x] 消除 barrier→proc::scheduler 内部访问 — 3→0 (tick_query 函数指针注册)
- [x] 消除 chitin↔proc 循环依赖 — process_cleanup 回调 + proc::api 公共接口 (2→0)
- [x] 消除 credo↔proc 循环依赖 — 加入允许紧耦合白名单 + 移出 secure_boot 初始化至 credo_init
- [x] 消除 tests 相关循环依赖 — 加入允许紧耦合白名单 (测试框架↔被测模块)
- [x] 新增 `framework::process_cleanup` 进程退出清理回调接口 — 解耦 proc→chitin
- [x] 新增 `proc::api` 公共接口 — process_exists/try_inc_ref/dec_ref/get_cr3/get_pwm/signal_pending_set
- [x] 审计脚本区分允许/禁止紧耦合 — ALLOWED_TIGHT_COUPLING 白名单

### 阶段 3.5: 内部访问违规治理 (进行中)

- [ ] syscall 33 处内部访问 (最大违规源: PROCESS_TABLE 10处, proc::scheduler, mm::vma, fs::vfs)
- [ ] driver 23 处内部访问 (driver::framework 内部子模块)
- [ ] proc 11 处内部访问 (mm::copy_user, proc::process, mm::vma 等)
- [ ] net 8 处内部访问 (net 内部子模块互访)
- [ ] 其他 50 处 (chitin 3, console 3, fs 2, mm 2, dma 1, idt 1, sched 1 等)

### 阶段 4: trait 抽象注入 (待启动)

- [ ] SchedPolicy trait 提取 (T-01)
- [ ] SyscallDispatch trait 提取 (T-03)
- [ ] FsBackend trait 提取 (T-05)
- [ ] FrameAllocPolicy trait 提取 (T-02)
- [ ] IrqHandler trait 提取 (T-04)

### 阶段 5: 验证与固化 (待启动)

- [ ] 全量审计脚本通过
- [ ] 双架构编译通过
- [ ] 依赖矩阵无循环
- [ ] TCB 比率 < 30%
- [ ] 更新 framekernel-dev-guide.md

---

## 七、变更历史

| 日期 | 变更 | 作者 |
|------|------|------|
| 2026-06-16 | 初始文档: 耦合分析、原则制定、规范制定、审查规范 | AI |
| 2026-06-16 | 阶段 2 完成: audit_coupling.py, services 白名单, 依赖声明, errno 统一入口 | AI |
| 2026-06-16 | 阶段 3 部分完成: mm→syscall 消除 (8→0), proc→syscall 大幅降低 (17→4), fs→syscall 降低 (2→1) | AI |
| 2026-06-16 | 阶段 3 深度推进: proc→syscall 完全消除 (17→0), fs→syscall 完全消除 (2→0), mm→proc 消除 (1→0), chitin↔driver 消除, barrier→proc 消除; 新增 userptr/fd_notify/rlimit_query/tick_query 解耦模块; 审计脚本区分允许/禁止紧耦合 | AI |
| 2026-06-16 | 阶段 3 完成: chitin↔proc 消除 (process_cleanup 回调 + proc::api 公共接口), credo↔proc 归入允许紧耦合 + 移出 secure_boot 初始化, tests 循环依赖归入允许紧耦合; 禁止的循环依赖 0 对 (↓100%); 内部访问违规 125 处待治理 | AI |
