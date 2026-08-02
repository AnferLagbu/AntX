# clippy pedantic 全量修复工程

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

### 变更历史
- **2026-07-31**
  - 描述: 批次 1-2 完成, 10591 → ~6700
  - 方案: -
  - 状态: [X]
- **2026-08-01**
  - 描述: 批次 3 文档类清零 + 批次 4 cast_lossless 清零 + ab/ac 组 expect 提交
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
- **DECISION-034: pedantic 0 警告后 CI 强制 `-D warnings`**
  - 描述: 批次 7 完成后, CI clippy 从仅强制 `unsafe_code` 升级为全量 `-D warnings` + `-W clippy::pedantic`
  - 方案: 防止 pedantic 警告回归. 放弃"仅强制部分 lint" (维护成本高, 易遗漏)
  - 状态: []

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
