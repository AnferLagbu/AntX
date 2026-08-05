# clippy pedantic 全量修复工程

> **DEPRECATED 2026-08-06**: 本文档 8 个工程计划已合并到 [stage-engineering-master.md](../stage-engineering-master.md) 工程计划 A. 保留本文档作为历史快照, 不再更新.
>
> 消除全部 clippy pedantic 警告 (最初 10591 → 当前 4643 → 目标 0), 分 7 批次推进.

## 工程计划 1: 批次 1-3 已完成 (清单/自动修复/文档类)

### 背景
- **初始状态**
  - 描述: 2026-07-31 全仓 clippy pedantic 扫描发现 10591 条警告
  - 方案: 按 MachineApplicable / 文档类 / cast 类 / 指针类 / 风格类分 7 批次推进
  - 状态: [X]

### 目标
- **批次 1-3 清零**
  - 描述: 批次 1 (清单分类) + 批次 2 (MachineApplicable 自动修复) + 批次 3 (文档类) 全部完成
  - 方案: cargo clippy --fix 自动修复 + worker 并行补中文文档
  - 状态: [X]

### 现状 (2026-08-02)
- **批次 2 自动修复成果**
  - 描述: 约 3900 条 MachineApplicable 警告自动修复 (needless/closure/imports 等), 272 处 no_mangle 补 extern "C", 6 处 #[expect] 豁免
  - 方案: cargo clippy --fix 排除会破坏编译的 cast/ptr 类
  - 状态: [X]
- **批次 3 文档类清零**
  - 描述: doc_markdown 3078 条 + missing_errors_doc/missing_panics_doc 841 条全部清零
  - 方案: --fix 自动补反引号 (461 文件) + 4 组并行 worker 补中文文档
  - 状态: [X]
- **批次 4 部分 (cast_lossless)**
  - 描述: cast_lossless 170 处 --fix 自动修复清零
  - 方案: 无损转换自动修复
  - 状态: [X]
- **批次 4 部分 (ab/ac 组 expect)**
  - 描述: ab 组 92 处 + ac 组 99 处 expect 已提交 (混入 commit c7bc2d1b), 覆盖 318 个触发点
  - 方案: 函数级 #[expect(clippy::cast_*)] + 中文注释
  - 状态: [X]

### 决策记录
- **DECISION-033: cast 类采用函数级 #[expect] + 中文注释**
  - 描述: cast 截断/符号/回绕类警告无法自动修复 (涉及语义判断), 采用函数级 `#[expect(clippy::cast_*)]` + 中文注释说明收窄安全性
  - 方案: 优于全文件 `#[allow]` (精确到函数) + 优于逐个重构 (工作量过大). 放弃"逐个改用 checked_*/try_from" 方案 (部分场景确需截断, 如 u64→u32 系统调用返回值)
  - 状态: [X]
- **DECISION-035: cast expect 注释统一模板**
  - 描述: 已完成批次 (ab/ac 组 191 处) 注释格式存在 10+ 种变体, 不符合 §5.2 "Explain why, not what" 规范
  - 方案: 统一为 `// 有意窄化: <具体原因>`, 原因需说明**为什么**可以安全截断 (如"u64→u32 取低 32 位, cpuid 返回值仅低 32 位有效"), 而非套用模板 (如"调用方保证值域安全"). 放弃保留多变体 (维护成本高, 可读性差)
  - 状态: []

### 质量评估 (2026-08-02)
- **批次 2 自动修复 — 质量: 优 (8/10)**
  - 描述: no_mangle 补 extern "C" 272 处正确, 双架构编译+审计+host-tests 全通过
  - 方案: 漏修复 2 处 (barrier/api.rs `recovery_set/get_fault_rate` 在 `#[cfg(feature = "fault_injection")]` 下未触发 clippy)
  - 状态: []
- **批次 3 文档类 — 质量: 优 (9/10)**
  - 描述: # Panics / # Errors 文档语义准确 (抽样 racy_cell.rs:127, ioport.rs:86), doc_markdown 反引号 3078 条
  - 方案: doc_markdown 剩余 5 条 (数量极少, 批次 7 收尾时处理)
  - 状态: []
- **批次 4 ab/ac 组 cast expect — 质量: 中 (5/10)**
  - 描述: 0 unfulfilled expect ✓, 0 无注释 expect ✓, 抽样 (cpuid.rs:18, idt/types.rs:135) 语义正确
  - 方案: 注释格式 10+ 种变体不统一 (违反 DECISION-035); credo/storage.rs 3 处 expect 语义错误 (按字节拆分不该 expect); cast_sign_loss 643 条完全未处理
  - 状态: []

### 变更历史
- **2026-07-31**
  - 描述: 批次 1-2 完成, 10591 → ~6700
  - 方案: -
  - 状态: [X]
- **2026-08-01**
  - 描述: 批次 3 文档类清零 + 批次 4 cast_lossless 清零 + ab/ac 组 expect 提交
  - 方案: -
  - 状态: [X]
- **2026-08-02**
  - 描述: 质量评估完成, 发现 3 类问题 (注释不统一 / credo expect 错误 / sign_loss 未处理)
  - 方案: -
  - 状态: [X]

***

## 工程计划 2: 批次 4 剩余 — cast 类 (截断/符号/回绕/精度)

### 背景
- **cast 类剩余警告**
  - 描述: cast 截断/符号/回绕/精度类共 2005 条, 涉及有损转换的语义判断, 无法自动修复
  - 方案: 按 DECISION-033 采用函数级 `#[expect]` + 中文注释
  - 状态: []

### 目标
- **cast 类清零**
  - 描述: 5 个 cast 子类全部清零
  - 方案: 脚本化批量处理 + 人工审查语义
  - 状态: []

### 现状 (2026-08-02)
- **cast 类分布**
  - 描述: cast_possible_truncation 945 + cast_sign_loss 643 + cast_possible_wrap 285 + cast_ptr_alignment 89 + cast_precision_loss 43 = 2005 条
  - 方案: 按子类分批处理, truncation/sign_loss 优先 (占 79%)
  - 状态: []
- **aa/ad/ae 组中断**
  - 描述: aa 组 (framework arch/driver/mm) 63 处 expect 插入后中断; ad 组 (framework sync + services 前段) 仅完成分析; ae 组 (services driver/fs) 仅完成分析
  - 方案: 重跑 clippy 生成最新位置清单 (旧清单 3/4 行号已不匹配), 重新派 worker
  - 状态: []

### 方案
- **步骤 1: 生成最新位置清单**
  - 描述: 当前工作区 HEAD=622bf943, 重跑 clippy 获取实时位置
  - 方案: `cargo clippy --release -- -W clippy::pedantic --message-format=json` 解析 JSON 获取精确文件:行号
  - 状态: []
- **步骤 2: 按 cast 子类分批处理**
  - 描述: truncation (945) → sign_loss (643) → possible_wrap (285) → ptr_alignment (89) → precision_loss (43)
  - 方案: 每子类按文件分组, 派 worker 并行处理, 函数级 `#[expect(clippy::cast_*)]` + 中文注释
  - 状态: []
  - 详情: 注释格式 `// 有意窄化: <原因>, 调用方/上下文保证值域安全`
- **步骤 3: cast_ptr_alignment 特殊处理**
  - 描述: cast_ptr_alignment 89 条涉及裸指针类型转换对齐, 部分场景需改用 `core::ptr::addr_of!` 而非 expect
  - 方案: 逐个审查, 能重构则重构, 不能重构则 expect + SAFETY 注释
  - 状态: []

### 待办
- **重跑 clippy 生成位置清单**
  - 描述: 旧清单已陈旧, 需重新生成
  - 方案: JSON 格式解析, 输出 `文件:行号:lint名` 列表
  - 状态: []
- **完成 aa 组中断部分**
  - 描述: framework arch/driver/mm 40 文件, 63 处 expect 待验证
  - 方案: 重新跑 clippy 确认 0 unfulfilled
  - 状态: []
- **完成 ad/ae 组未落地部分**
  - 描述: framework sync + services 前段 + services driver/fs 80 文件
  - 方案: 按新清单派 worker
  - 状态: []
- **修复 credo/storage.rs 3 处错误 expect (高优先级)**
  - 描述: w32/w64/w16 函数是按字节序列化 (拆分 u32/u64 为字节数组), `v as u8` 取低 8 位是正确逻辑, 不存在截断风险, 不应使用 expect
  - 方案: 改用 `(v >> (i*8)) as u8` 或 `((v >> (i*8)) & 0xFF) as u8` 消除警告, 移除 3 处 `#[expect(clippy::cast_possible_truncation)]` + 错误注释
  - 状态: []
  - 详情: 详见 [src/kernel/framework/credo/storage.rs:44-64](src/kernel/framework/credo/storage.rs)
- **统一已提交 expect 注释格式 (DECISION-035)**
  - 描述: ab/ac 组 191 处 expect 注释存在 10+ 种变体, 不符合 §5.2 "Explain why, not what"
  - 方案: 脚本化替换为统一模板 `// 有意窄化: <具体原因>`, 原因需说明为什么可以安全截断
  - 状态: []
- **补 barrier/api.rs 2 处 no_mangle extern "C"**
  - 描述: `recovery_set_fault_rate` / `recovery_get_fault_rate` 在 `#[cfg(feature = "fault_injection")]` 下未触发 clippy, 漏修复
  - 方案: 补 `extern "C"` ABI 标注
  - 状态: []
  - 详情: 详见 [src/kernel/framework/barrier/api.rs:304-311](src/kernel/framework/barrier/api.rs)

***

## 工程计划 3: 批次 5 — 指针类

### 背景
- **指针类警告**
  - 描述: 涉及裸指针转换的安全性警告, 共 788 条
  - 方案: 逐个审查, 优先重构为 safe API, 无法重构则 expect + SAFETY 注释
  - 状态: []

### 目标
- **指针类清零**
  - 描述: 4 个指针子类全部清零
  - 方案: 重构优先, expect 兜底
  - 状态: []

### 现状 (2026-08-02)
- **指针类分布**
  - 描述: ptr_as_ptr 640 + borrow_as_ptr 83 + ptr_cast_constness 33 + ref_as_ptr 32 = 788 条
  - 方案: ptr_as_ptr 占 81%, 集中处理
  - 状态: []

### 方案
- **ptr_as_ptr (640 条)**
  - 描述: `as *const T` / `as *mut T` 裸指针转换, 推荐用 `core::ptr::from_ref` / `core::ptr::from_mut`
  - 方案: 能重构则重构; framework 层 unsafe 块内保留 + SAFETY 注释; services 层 (0 unsafe) 必须重构
  - 状态: []
  - 详情: no_std 环境 `core::ptr::from_ref` 在 nightly 稳定, 需确认工具链支持
- **borrow_as_ptr (83 条)**
  - 描述: `&x as *const T` 推荐用 `core::ptr::from_ref(&x)`
  - 方案: 同 ptr_as_ptr
  - 状态: []
- **ptr_cast_constness (33 条)**
  - 描述: `*const T as *mut T` 或反向, 涉及可变性
  - 方案: 逐个审查, framework 层 unsafe 块内可能合法 (如 MMIO), 加 SAFETY 注释
  - 状态: []
- **ref_as_ptr (32 条)**
  - 描述: `&x as *const T` 推荐用 `core::ptr::from_ref`
  - 方案: 同 ptr_as_ptr
  - 状态: []

### 待办
- **确认 nightly 工具链对 `core::ptr::from_ref` 支持**
  - 描述: 检查 rust-toolchain.toml 版本是否已稳定该 API
  - 方案: `cargo doc --open` 或查阅 nightly changelog
  - 状态: []
- **按文件分组派 worker**
  - 描述: 按模块 (framework/net, framework/mm, framework/driver 等) 分组
  - 方案: 每组 40 文件, 派 5 worker 并行
  - 状态: []

***

## 工程计划 4: 批次 6 — 风格类

### 背景
- **风格类警告**
  - 描述: 涉及代码风格和可读性, 共 1817 条, 部分需人工语义判断
  - 方案: 按可自动修复性分三类处理
  - 状态: []

### 目标
- **风格类清零**
  - 描述: 32 个风格子类全部清零
  - 方案: 自动修复 + 语义重构 + expect 兜底
  - 状态: []

### 现状 (2026-08-02)
- **风格类分布 (前 10 大类)**
  - 描述: unreadable_literal 408 + inline_always 333 + manual_let_else 307 + trivially_copy_pass_by_ref 187 + unused_self 107 + similar_names 73 + match_same_arms 72 + unnecessary_wraps 71 + used_underscore_binding 63 + items_after_statements 60 = 1681 条 (占 92%)
  - 方案: 按数量降序处理
  - 状态: []

### 方案
- **A 类: 可自动修复**
  - 描述: unreadable_literal (408, 加下划线) + items_after_statements (60, 移动声明) 等
  - 方案: `cargo clippy --fix` 自动修复, 人工验证编译
  - 状态: []
- **B 类: 需语义重构**
  - 描述: manual_let_else (307, 改用 let-else) + unused_self (107, 重构为关联函数) + match_same_arms (72, 合并分支) + trivially_copy_pass_by_ref (187, 改用引用) 等
  - 方案: 逐个审查语义, 人工重构
  - 状态: []
  - 详情: manual_let_else 需确认 Rust 2024 edition 对 `let ... else` 支持
- **C 类: 需 expect 兜底**
  - 描述: inline_always (333, 热路径强制内联) + unnecessary_wraps (71, API 兼容) + too_many_lines (35, 复杂函数) 等
  - 方案: 函数级 `#[expect]` + 中文注释说明保留原因
  - 状态: []

### 待办
- **A 类自动修复**
  - 描述: cargo clippy --fix 处理 unreadable_literal 等
  - 方案: 排除会破坏编译的类别后 --fix
  - 状态: []
- **B 类语义重构**
  - 描述: 按模块分组派 worker
  - 方案: 每组 40 文件, worker 内部逐个审查
  - 状态: []
- **C 类 expect 兜底**
  - 描述: inline_always 等热路径相关
  - 方案: 脚本化批量插入 expect
  - 状态: []

***

## 工程计划 5: 批次 7 — 最终验证

### 背景
- **全量验证**
  - 描述: 批次 4-6 完成后, pedantic 警告应降至 0
  - 方案: 全量验证 + 提交
  - 状态: []

### 目标
- **pedantic 0 警告**
  - 描述: `cargo clippy --release -- -W clippy::pedantic -D warnings` 0 警告
  - 方案: 全量验证
  - 状态: []

### 方案
- **验证步骤**
  - 描述: 按 §2.4 验证门槛 5 条全量验证
  - 方案: 双架构编译 + clippy + 三审计 + host-tests + QEMU
  - 状态: []
  - 详情: clippy 命令升级为 `-D warnings` (当前 CI 仅强制 `unsafe_code`, 全量 `-D warnings` 是中长期目标, 见 §2.4 #2)

### 待办
- **双架构 release 0w0e**
  - 描述: `./ci/build.sh all`
  - 方案: x86_64 + aarch64
  - 状态: []
- **clippy pedantic 0 警告**
  - 描述: `cargo clippy --release -- -W clippy::pedantic -D warnings`
  - 方案: 全量 pedantic 强制 0
  - 状态: []
- **三审计通过**
  - 描述: services_boundary + safety_coverage + deadlock_matrix + comment_language
  - 方案: `ci/audit.sh`
  - 状态: []
- **host-tests 全通过**
  - 描述: `make test-host`
  - 方案: 全量
  - 状态: []
- **更新 CI 配置**
  - 描述: 将 clippy 配置从仅 `unsafe_code` 升级为 `-D warnings` + pedantic
  - 方案: 修改 `clippy.toml` 或 CI 脚本
  - 状态: []

### 决策记录
- **DECISION-034: pedantic 0 警告后 CI 强制 `-D clippy::pedantic`**
  - 描述: 批次 7 完成后, CI clippy 从仅强制 `unsafe_code` lint 升级为 `-D clippy::pedantic` (排除 cast_* 4 子 lint, DECISION-041 已知安全保留)
  - 方案: 防止 pedantic 警告回归. 放弃"仅强制部分 lint" (维护成本高, 易遗漏)
  - 状态: [X]
- **DECISION-042: DECISION-034 推迟到 macro 改造后实施 (历史, 已不再适用)**
  - 描述: 实施 DECISION-034 时发现 `klog_fmt` 等 macro 内部触发 ptr_as_ptr 等 pedantic lint, `#[expect]` 不能从外部施加到宏展开内部. 1598 处 macro 内 lint 无法 expect 兜底
  - 方案: 推迟 CI 升级到 macro 改造后 (klog_fmt 重写为非 macro 形式 + 在 macro 内部加 `#[allow(...)]` 或宏参数 `allow_internal_unstable`). 当前 CI 保留 cargo check + 三审计 + host-tests 验证
  - 状态: [X]
- **DECISION-043: pedantic 全强制 — 治根路径**
  - 描述: 实施 DECISION-034 的实际路径. 关键问题解决:
    - klog_fmt macro 内 ptr_as_ptr: 改为 `.cast::<u8>()` 根治 (1 处)
    - 剩余 pedantic 1100+ 处: 按 lint 性质分类处理
      - 4 处真实 fn lint 手工改 (borrow_as_ptr undo_log/coredump/e1000, ref_as_ptr kgdb, manual_let_else policy, needless_continue policy)
      - 8 类结构/装饰性 lint 全局 allow (unreadable_literal, inline_always, large_stack_arrays, struct_field_names, pub_underscore_fields, struct_excessive_bools, doc_markdown, ptr_as_ptr, cast_ptr_alignment, zero_sized_map_values, missing_fields_in_debug, ptr_cast_constness, cast_lossless, duplicated_attributes, wildcard_imports)
      - 15+ 类 fn 级 lint expect 兑底 (aarch64 92 处 + x86_64 累计 ~700 处)
    - aarch64 与 x86_64 独立 lint 集合: 各跑 expect 兑底
    - 6 处 unfulfilled expect 清理 (aarch64 e1000_probe 在 cfg(x86_64) 内不触发 aarch64 borrow_as_ptr 等)
  - 方案: 根治 macro 内 lint + 全局 allow 装饰性 lint + expect 兑底 fn lint + 手工治根关键处. 比推迟 DECISION-034 更稳健
  - 状态: [X]

***

## 工程计划 6: 已完成批次历史记录

### 变更历史
- **2026-07-31**
  - 描述: 批次 1-2 完成, 10591 → ~6700 (MachineApplicable 自动修复 ~3900 条)
  - 方案: -
  - 状态: [X]
- **2026-08-01**
  - 描述: 批次 3 文档类清零 (doc_markdown 3078 + missing_errors/panics_doc 841) + 批次 4 cast_lossless 170 清零
  - 方案: -
  - 状态: [X]
- **2026-08-01**
  - 描述: ab 组 (framework mm/net/proc) 92 处 expect + ac 组 (framework proc/sync/syscall) 99 处 expect 提交
  - 方案: -
  - 状态: [X]
- **2026-08-02**
  - 描述: 核对汇报进度, 确认 4643 条剩余 (cast 2005 + 指针 788 + 风格 1817 + 其他 33), 编写本工程计划
  - 方案: -
  - 状态: [X]

***

## 工程计划 7: 已完成批次质量评估与修复

### 背景
- **质量评估需求**
  - 描述: 已完成批次 1-3 + 批次 4 部分 (ab/ac 组 191 处 expect) 需审查修复质量, 确保不引入语义错误或注释规范问题
  - 方案: 深入源码抽样审查 + 编译期 unfulfilled 检测 + 注释规范性检查
  - 状态: [X]

### 目标
- **识别并修复已完成批次的质量问题**
  - 描述: 3 类问题 (注释不统一 / credo expect 错误 / no_mangle 漏修复) 需修复
  - 方案: 按优先级分批修复
  - 状态: []

### 现状 (2026-08-02)
- **质量评估总体结论: 中等偏上 (6/10)**
  - 描述: 批次 2/3 质量优 (8-9/10), 批次 4 cast expect 质量中 (5/10)
  - 方案: 修复 3 类问题后批次 4 质量可提升至良
  - 状态: [X]
- **已确认正确的修复**
  - 描述: 0 unfulfilled expect ✓ (所有 expect 实际触发), 0 无注释 expect ✓, 抽样 cpuid.rs:18 `ebx_val as u32` (u64→u32 取低 32 位, cpuid 返回值语义正确), idt/types.rs:135 `err_code as u32` (u64→u32 错误码截断正确)
  - 方案: 无需修改
  - 状态: [X]

### 方案
- **步骤 1: 修复 credo/storage.rs 3 处错误 expect (高优先级)**
  - 描述: w32/w64/w16 函数按字节序列化逻辑, `v as u8` 取低 8 位是正确逻辑, 不存在截断风险, expect 掩盖了正确逻辑为"截断"
  - 方案: 移除 3 处 `#[expect(clippy::cast_possible_truncation)]` + 错误注释, 改用 `(v >> (i*8)) as u8` 消除警告
  - 状态: []
  - 详情: 详见 [src/kernel/framework/credo/storage.rs:44-64](src/kernel/framework/credo/storage.rs)
- **步骤 2: 统一已提交 expect 注释格式 (DECISION-035)**
  - 描述: ab/ac 组 191 处 expect 注释存在 10+ 种变体, 前 5 种: "显式收窄转换, 调用方/上下文保证值域安全" (111 处) / "长度/计数值域受调用方约束, 有意窄化" (40 处) / "内核寄存器/硬件字段宽度, 调用方保证值域" (38 处) / "尺寸/地址转换, 调用方保证值域" (27 处) / "fd/错误码/字节数 i32 约定, 调用方保证值域" (15 处)
  - 方案: 脚本化替换为统一模板 `// 有意窄化: <具体原因>`, 原因需说明为什么可以安全截断, 而非套用模板
  - 状态: []
- **步骤 3: 补 barrier/api.rs 2 处 no_mangle extern "C"**
  - 描述: `recovery_set_fault_rate` / `recovery_get_fault_rate` 在 `#[cfg(feature = "fault_injection")]` 下未触发 clippy missing_abi lint, 漏修复
  - 方案: 补 `extern "C"` ABI 标注
  - 状态: []
  - 详情: 详见 [src/kernel/framework/barrier/api.rs:304-311](src/kernel/framework/barrier/api.rs)

### 待办
- **cred/storage.rs 3 处 expect 修复**
  - 描述: 移除错误 expect, 改用位移消除警告
  - 方案: `(v >> (i*8)) as u8` 替代 `v as u8`
  - 状态: []
- **191 处 expect 注释规范化**
  - 描述: 按 DECISION-035 统一为具体原因说明
  - 方案: 脚本化批量替换 + 人工审查语义
  - 状态: []
- **barrier/api.rs 2 处 extern "C" 补齐**
  - 描述: 2 处 `#[no_mangle]` 补 `extern "C"`
  - 方案: 直接编辑
  - 状态: []

### 决策记录
- **DECISION-036: 按字节序列化场景禁用 cast expect**
  - 描述: 按字节拆分 u32/u64 为字节数组的场景 (如 `w32`/`w64`/`w16` 函数), `v as u8` 取低 8 位是正确逻辑, 不存在截断风险, 禁止使用 `#[expect]` 掩盖警告
  - 方案: 必须改用 `(v >> (i*8)) as u8` 或 `((v >> (i*8)) & 0xFF) as u8` 消除警告. 放弃保留 expect (掩盖正确逻辑为"截断", 注释误导)
  - 状态: [X]

### 变更历史
- **2026-08-02**
  - 描述: 质量评估完成, 识别 3 类问题, 添加工程计划 7
  - 方案: -
  - 状态: [X]

***

## 工程计划 8: 阶段 8.3-8.8 expect 兜底 + 阶段 8.9 cast 类治根决策

### 背景
- **2026-08-04 阶段 8.3-8.8**
  - 描述: 用户决策"按序修复" 推进 6 类 lint 全部 expect 兜底
  - 方案: unused_self (107) + items_after_statements (58) + similar_names (73) + unnecessary_wraps (71) + used_underscore_binding (63) + too_many_lines (35) = 407 处 expect
  - 状态: [X]
- **2026-08-04 阶段 8.9 cast 类调研**
  - 描述: cast_possible_truncation 939 + cast_sign_loss 643 + cast_possible_wrap 285 + cast_precision_loss 43 = 1910 处
  - 方案: 用户决策"稳健且治根" = D 路径 (仅手工 try_from 真危险 < 200 处, 保留已知安全 cast)
  - 状态: [~]

### 决策记录
- **DECISION-040: expect 兜底批量处理 6 类 lint (阶段 8.3-8.8)**
  - 描述: 6 类 lint (unused_self / items_after_statements / similar_names / unnecessary_wraps / used_underscore_binding / too_many_lines) 涉及 407 处警告, 通过 `#[expect(clippy::*)]` attribute 兜底
  - 方案: 函数级 expect (不全局 allow) + 中文注释说明取舍. 比全局 allow 精确 (仅覆盖触发 lint 的 fn); 比手工重构工作量小. 部分 expect 出现 unfulfilled (脚本未去重 fn 级别), 手工删除冗余 expect
  - 状态: [X]
- **DECISION-041: cast 类已知安全保留, 仅真危险手工 try_from**
  - 描述: cast 类 1910 处中 < 200 处为真实风险 (数据 size 截断, 字段值越界), 其余 1700+ 处是已知安全 cast
  - 方案: **不**全局 allow (失去 lint 价值) + **不**expect 兜底 (fn 级 expect 变隐性 allow) + **不**全手工 try_from (1700+ 处无价值工作). 仅手工 try_from 改造 < 200 处真危险. 已知安全 cast 保留原状, 让 clippy 警告作为"提醒"
  - 已知安全 cast 分类 (无需 try_from):
    1. APIC ID 协议保证 < 256 → `apic_id as u8` 安全
    2. 循环变量 i 且 i < 8 比较 → `i as u8` 安全
    3. sizeof<T>() 已知 < u32 → `size_of as u32` 安全
    4. 常量字符串长度 → `NOTE_NAME.len() as u32` 安全
    5. u32 → usize (64 位系统无损)
    6. syscall ABI 协议层 cast (如 `args[0] as u64`)
  - 真实风险 cast 需手工 try_from (典型示例):
    1. 用户数据 size → u8 (如 `value_size: size as u8`)
    2. ELF 段数 → u16 (如 `phnum = ... as u16`)
    3. 文件大小 → u32 (如 `device.len() as u32` 当 device 可能 > u32::MAX)
    4. 用户态指针 → 内核指针 (需 check_user_ptr 前置检查, try_from 仅辅助)
  - 状态: [X]

### 变更历史
- **2026-08-04 阶段 8.3-8.8**
  - 描述: 6 类 lint expect 兜底批量完成 (407 处), commit 0ec1bc24 / f56c4390 / 08c7e0d6 / 27345598 / dd0f427f / fc945743
  - 方案: 脚本化 + 手工修正 unfulfilled expect
  - 状态: [X]
- **2026-08-04 阶段 8.9 cast 类**
  - 描述: cast 类 1910 处调研 + 决策登记 (DECISION-041), 实质 try_from 改造推未来阶段
  - 方案: D 路径 (仅手工真危险 < 200 处)
  - 状态: [~]

***
