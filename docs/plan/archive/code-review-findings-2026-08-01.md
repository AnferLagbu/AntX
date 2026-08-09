# 代码审查发现清单 (2026-08-01 全仓审查)

> 全仓综合代码审查发现, 共 8 项. 严重度分类沿用 [archive/code-review-findings-2026-07-04.md](./archive/code-review-findings-2026-07-04.md) 的约定: P1 = 违反 AGENTS.md 硬规则 / 权威文档矛盾 (当前 CI 未拦截); P2 = 应修但可延后; P3 = 调研项 / 观察. 用户 2026-08-01 授权仅记录到 `docs/plan/`, 不在本轮实施修复, 状态一律 `[]`.

## 架构与文档一致性 (P1)

- **REVIEW-FINDING-024: `CHANGELOG.md` 缺失, 但 README.md 与 AGENTS.md 多处引用**
  - 描述: 根目录与 `docs/` 下均不存在 `CHANGELOG.md` (已 `ls` 验证), 但 README.md:11/163/210 三处链接 `CHANGELOG.md`, AGENTS.md §1 表格与 §11.3 也要求维护 `docs/CHANGELOG.md`. 文档引用不存在的文件, 违反 §10.2 文档同步原则
  - 方案: 二选一 — (a) 创建 `docs/CHANGELOG.md` 并补记历史变更 (按 AGENTS.md §1 归属, AI 起草 / 用户 定稿); (b) 若项目已放弃维护变更日志, 删除 README.md 与 AGENTS.md 中的引用
  - 状态: []
  - 详情: README.md:11 `- 变更记录: [CHANGELOG.md](file:///home/anfer/Code/QueenX/CHANGELOG.md)`; README.md:163 `├── CHANGELOG.md 变更日志`; README.md:210 文档索引表; AGENTS.md:48 `docs/CHANGELOG.md` 归属表; AGENTS.md:363 §11.3 实施后必做清单

- **REVIEW-FINDING-025: syscall 编号空间立场两份权威文档互相矛盾**
  - 描述: `src/kernel/framework/syscall/mod.rs:24-35` 头注释称 "0-299 保留给未来 linuxulator (与 Linux 1:1 映射)", 原生编号用 QX_* (500-899), "Linux 兼容二进制通过 linuxulator 模块将架构特定编号翻译为 QX_* 编号"; 而 `docs/explain/ref-naming.md` §三 (2026-07-05 修订) 明确 "直接使用 Linux syscall 编号 (0-299), 无需翻译层", "无需 linuxulator". 两份文档描述同一事实却结论相反, 影响新 syscall 实现时的编号选择
  - 方案: 由用户决策正确立场后统一: (a) 若走"直接 Linux ABI" (ref-naming.md 现行立场), 更新 `framework/syscall/mod.rs:24-35` 注释为 Linux 原编号直通; (b) 若走"QX_* 原生 + linuxulator 翻译" (vision-hope.md 风险 2 缓解方案), 修订 ref-naming.md §三. 决策后需同步 `services/syscall/` 与 `framework/syscall/dispatch.rs` 的实际分发行为
  - 状态: []
  - 详情: 矛盾点: `framework/syscall/mod.rs:27` "0-299 保留给未来 linuxulator" vs `ref-naming.md:32` "标准 POSIX/Linux syscall 使用 Linux 原始编号 (0-299)"; `ref-naming.md:91` "无需 syscall 翻译层" vs `framework/syscall/mod.rs:35` "通过 linuxulator 模块将架构特定编号翻译为 QX_* 编号". 另见 `docs/explain/vision-hope.md` 风险 2 也提到翻译层方案, 三份文档需一并对齐

- **REVIEW-FINDING-026: framework 反向依赖 services 类型 (`userctx.rs` re-export)**
  - 描述: `src/kernel/framework/userctx.rs:6-9` 注明 "纯类型定义已于 2026-06-16 迁移到 services::userctx, 本文件仅 re-export", 即 TCB (framework) 层 re-export 非 TCB (services) 层定义的类型. 而 `framework/usermode.rs:38/58` 的 `unsafe fn enter_user_mode` 直接读取该 `UserContext` 的字段布局 (`ctx.rip`/`ctx.elr_el1` 等), 使 TCB 的 unsafe 代码依赖 services 层定义的数据布局. 违反 explain-framekernel.md "services→framework 单向数据流" 与 AGENTS.md §4.2 资源分类原则 (寄存器快照属于用户态 CPU 状态, 归 framework)
  - 方案: 将 `UserContext` 纯类型定义迁回 `framework/userctx.rs` (TCB 层), `services::userctx` 改为反向 re-export 保持调用方兼容; 或至少在 framework 层重新声明一个 `#[repr(C)]` 等价结构并加编译期布局断言 (`size_of`/`offset_of` 相等), 消除对 services 类型的运行时依赖
  - 状态: []
  - 详情: `framework/userctx.rs:9` `pub use crate::kernel::services::userctx::*;`; `framework/usermode.rs:18` `use super::userctx::UserContext;`; 受影响路径: `usermode.rs:38-51` (x86_64) 与 `:58-70` (aarch64) 的 `enter_user_mode`

## 文档失实与过期 (P2)

- **REVIEW-FINDING-027: framework 顶层文档声明 "~3000+ LoC" 与实际严重不符**
  - 描述: `src/kernel/framework/mod.rs:10` 架构图注释称 "framework/ (TCB, ~3000+ LoC, unsafe 允许)", 实际 framework 全子树约 10 万行 (2026-08-01 统计: driver 19703 + mm 13144 + proc 11066 + arch 7338 + tests 6527 + syscall 6037 + net 5121 + sync 4867 等, 合计 97,946 行). 该数字在 2026-07-04 审查时已失真 (当时亦 >3000), 属持续漂移
  - 方案: 更新 `framework/mod.rs:10` 为实际量级 (如 "~10万 LoC") 或在注释中移除具体数字改为相对表述 (如 "framework/ (TCB, unsafe 允许)"), 避免再次漂移; 同时更新 `src/kernel/mod.rs:6` 若含同类数字
  - 状态: []
  - 详情: `framework/mod.rs:10` `//! framework/ (TCB, ~3000+ LoC, unsafe 允许)`; `src/kernel/mod.rs` 架构概览注释 (未列行数, 仅列模块清单)

- **REVIEW-FINDING-028: services/net 与 services/fs 头注释状态过期**
  - 描述: `src/kernel/services/net/mod.rs:4-19` 头注释仍标 "状态 (v2.7, 2026-06-04), 已完成 1/4 子系统迁移", 且 checkbox 列表含未勾选项 "smoltcp 协议栈内部 / e1000-virtio-net / socket API (后续 Phase 2.4.x)"; `src/kernel/services/fs/mod.rs:4-19` 标 "真实状态 (v2.5, 2026-06-04), 已完成 4/4 子系统迁移". 实际两模块均已远超当时范围: net 已含 smoltcp_impl/socket/unix/dhcp_policy/wait_queue 等 11 个文件, fs 已含 ext2/exfat/overlayfs/tmpfs/vfs_manager/open_file_table 等 40+ 模块. 头注释与代码现状不一致, 违反 §10.2
  - 方案: 更新两文件头注释为当前真实状态 (模块清单 + 迁移完成度), 移除过期的 "评估日期 2026-06-04" 与未勾选 checkbox, 或删除整个迁移状态块 (迁移早已完成, 状态块已无信息量)
  - 状态: []
  - 详情: `net/mod.rs:4` `## 状态 (v2.7, 2026-06-04)`; `net/mod.rs:6` `已完成 1/4 子系统迁移`; `fs/mod.rs:4` `## 真实状态 (v2.5, 2026-06-04)`; `fs/mod.rs:6` `已完成 4/4 子系统迁移`

- **REVIEW-FINDING-029: README.md remote 命名与 kernel-roadmap 链接过期**
  - 描述: 两处过期: (1) README.md:21 `git remote rename origin Gitee` 与 AGENTS.md §8.4 (2026-07-31 起统一 remote 名为 `origin`, 示例为 `git remote add origin git@gitee.com:...`) 矛盾, 按新约定 README 的 rename 命令会破坏 remote 配置; (2) README.md:71 链接 `docs/plan/kernel-roadmap.md`, 该文件已归档至 `docs/plan/archive/2026-07-08-kernel-roadmap.md`, 原路径不存在 (已 `glob` 验证), 链接失效
  - 方案: (1) README.md:21 改为与 AGENTS.md §8.4 一致的 remote 添加/推送示例, 删除 `rename origin Gitee`; (2) README.md:71 链接改为 `docs/plan/archive/2026-07-08-kernel-roadmap.md` (保留归档), 或改为指向 `docs/plan/future-roadmap.md` (现行路线图)
  - 状态: []
  - 详情: README.md:21 `git remote rename origin Gitee`; README.md:71 `详细路线图与各 Phase 进度见 [docs/plan/kernel-roadmap.md](file:///home/anfer/Code/QueenX/docs/plan/kernel-roadmap.md)`

## 已知未完成与观察 (P3)

- **REVIEW-FINDING-030: framework/sched task 抽象 Phase 1.4.2 未开工, 阻塞 services/proc 迁移**
  - 描述: `src/kernel/framework/sched/mod.rs:8` 注释: "task 抽象在 Phase 1.4.2 计划中但尚未实现, 见 services/proc/mod.rs 占位说明. 任务书估时 5d, 实际未开工. 阻塞 services/proc 迁移." 属已知未完成项, 已在源码中明确登记, 无隐藏风险, 但无对应 plan 文档追踪
  - 方案: 在 `docs/plan/` 下为该未开工任务补一条计划条目 (或并入 future-roadmap.md 的 F 系列), 登记阻塞关系与估时, 避免该注释成为永久性 TODO; 若已放弃该抽象, 更新注释说明
  - 状态: []
  - 详情: `framework/sched/mod.rs:8` 注: task 抽象在 Phase 1.4.2 计划中但尚未实现; 关联: `services/proc/mod.rs` 占位说明 (未在本轮逐一核对 services/proc 内对应占位, 修复时需同步确认)

- **REVIEW-FINDING-031: IoMem 边界检查失败走 expect panic + 固定上限硬编码**
  - 描述: 两点观察: (1) `src/kernel/framework/iomem.rs:194/200/206/212` 等 `read_u*/write_u*` 在 `check_offset` 失败时用 `.expect("IoMem: ... 越界 (构造函数保证合法范围)")` 直接 panic; 注释声明这是编程错误, 但内核 panic 代价高, 且 services 层若在可恢复路径传入坏 offset 即整体崩溃; (2) 固定上限 `MAX_MMIO_MAPPINGS = 64` (iomem.rs:26) 与 `MAX_LOCK_CLASSES = 64` / `MAX_HELD_LOCKS = 8` (lockdep.rs:66/69) 均为硬编码, 超限时 IoMem 返回 Err 而 lockdep 静默截断
  - 方案: (1) 评估 `read_u*` 是否改返回 `Result<_, &'static str>` 让调用方处理, 或保持 expect 但在 SAFETY/注释中明确"仅限编程错误路径"并加 `debug_assert!` 前置; (2) 上限常量集中到 `framework/config/` 并注释超限行为 (lockdep 超限策略: 是跳过检测还是 panic)
  - 状态: []
  - 详情: `iomem.rs:194` `self.check_offset(offset, 1).expect("IoMem: read_u8 offset+1 越界 (构造函数保证合法范围)");`; `iomem.rs:26` `const MAX_MMIO_MAPPINGS: usize = 64;`; `lockdep.rs:66` `pub const MAX_LOCK_CLASSES: usize = 64;`; `lockdep.rs:69` `pub const MAX_HELD_LOCKS: usize = 8;`
