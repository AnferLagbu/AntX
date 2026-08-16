# 审计修复分册 01：审计脚本与 CI 门禁

> 审计工具链自身存在门禁级缺陷（元审计结论），本分册负责修复全部审计脚本、CI 接线与验证门槛依赖，恢复硬规则 F1-F13 的检测可信度。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3 章 + 附录 I。

## 工程计划 A: 审计脚本缺陷修复

### 背景

- **审计工具链门禁失效**
  - 描述：元审计（附录 I）发现 9 项 P0 门禁失效——F2 违规不阻断退出码、F8 死锁检测漏检核心对象、F4 仅扫 8 文件、F7 行尾注释漏检等；CI 实际只接线 4 个脚本，F3/I1-I6/F8/F11-F13 未接入。
  - 方案：按本分册待办逐项修复，修复后补"负测试"（构造违规样例验证脚本能拦截）。
  - 状态：[]

- **验证门槛依赖失效脚本**
  - 描述：AGENTS.md §2.3 门槛 4 与 stage-engineering-master.md 工程计划 D 门槛 3 均以"三审计通过"为交付标准，但三审计脚本自身有漏洞，"全过"结果不可信。
  - 方案：本分册完成后，门槛才恢复可信；完成前不使用三审计作为放行依据。
  - 状态：[]

### 待办

- **services_boundary.py 退出码纳入 HIGH/MEDIUM（META-P0-01）**
  - 描述：[audit_services_boundary.py:460-466](file:///home/anfer/Code/QueenX/scripts/audit_services_boundary.py) 仅 `critical > 0` 才 `exit(1)`；`FORBIDDEN_FRAMEWORK_IMPORT`（HIGH）、`RAW_POINTER_DEREF`（HIGH）、`UNLISTED_INTER_MODULE_DEP`（MEDIUM）全部放行。
  - 方案：`issues` 按严重度分级，HIGH 及以上违规即 `exit(1)`；MEDIUM 默认警告，可用 `--strict-medium` 开关。
  - 状态：[]

- **services_boundary.py 黑名单补全 + allow-list 落地（META-P0-02）**
  - 描述：FORBIDDEN_FRAMEWORK_MODULES 缺 `ipc::msgq::raw`、`syscall::types`、`proc::coredump` 等，实测 services 穿透访问未被报；`SAFE_FRAMEWORK_APIS`（allow-list）定义后零引用。
  - 方案：将 F2 改为基于 SAFE_FRAMEWORK_APIS 的 allow-list（services 只允许 import 白名单内的 framework 顶层 API），或补全 deny-list 至与 `audit_coupling.py` 的 INTERNAL_PATTERNS 对齐。
  - 状态：[]

- **services_boundary.py 匹配 pub use（META-P0-03）**
  - 描述：`use_pattern = re.compile(r'^\s*use\s+')` 不匹配 `pub use`，实测 30 处 `pub use framework::...` 穿透全部绕过。
  - 方案：正则改为 `^\s*(pub(\([^)]*\))?\s*)?use\s+`。
  - 状态：[]

- **deadlock_matrix.py 裸类型名检测 + 锁顺序矩阵（META-P0-04）**
  - 描述：[audit_deadlock_matrix.py:128-136](file:///home/anfer/Code/QueenX/scripts/audit_deadlock_matrix.py) 仅识别 `spin::`/`crate::spin::` 字面量；实测唯一第三方锁 `SpinMutex`（smp_init.rs:42）完全逃逸（366 文件 0 问题）；docstring 声称的"锁顺序 AB-BA 矩阵/原子上下文 sleep 锁/不可重入"检测代码中未实现。
  - 方案：阶段 A 增加"import 解析"收集裸类型名；实现锁顺序矩阵构建与 AB-BA 环检测；未实现的声明改为如实降级。
  - 状态：[]

- **block_registration.py 检测函数名修正（META-P0-05）**
  - 描述：[audit_block_registration.py:38](file:///home/anfer/Code/QueenX/scripts/audit_block_registration.py) 匹配 `chitin_register_block(`，但真实函数为 `chitin_register_block_dev`（[chitin/mod.rs:353](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/mod.rs#L353)），门禁恒 0 空转。
  - 方案：PATTERN 改为 `chitin_register_block_dev\s*\(`，并重跑验证 I-43 桥接单入口。
  - 状态：[]

- **once_cell.py 正则绕过修复（META-P0-06）**
  - 描述：[audit_once_cell.py:29,32](file:///home/anfer/Code/QueenX/scripts/audit_once_cell.py) 双正则均无法命中 `pub use spin::once::Once`（锚定 `^\s*use`）与 `use spin::OnceCell`（`Once\b` 词边界不成立）。
  - 方案：`USE_SPIN_ONCE` 支持 `pub use`；`SPIN_ONCE_IN_CODE` 增加 `once::` 前缀与 `OnceCell` 分支。
  - 状态：[]

- **comment_language.py 行尾注释检测（META-P0-07）**
  - 描述：[audit_comment_language.py:601](file:///home/anfer/Code/QueenX/scripts/audit_comment_language.py) 的 `iter_comments` 仅对行首 `//` 产出行，`let x = f(); // English text`（行尾注释）对 F7 透明。
  - 方案：对非注释行做"行内 `//` 拆分"，提取行尾注释进入检测流；需避免误判 URL/字符串内 `//`。
  - 状态：[]

- **audit_unsafe.sh 修复或废弃（META-P0-08）**
  - 描述：[tools/audit_unsafe.sh:102](file:///home/anfer/Code/QueenX/tools/audit_unsafe.sh) `xargs bash -c 'scan_unsafe ...'` 新进程不继承函数，实测零输出、退出 123，工具不可用。
  - 方案：加 `export -f scan_unsafe`；或废弃该脚本，统一使用 `tools/audit_unsafe.py`。
  - 状态：[]

- **public_api_docs.py 接入 CI（META-P0-09）**
  - 描述：`scripts/audit_public_api_docs.py`（F8）未接入任何 CI，门禁形同虚设。
  - 方案：修复其自身缺陷（`pub async fn` 漏检、块注释文档误报）后，在 ci-lint.yml 增加调用；修复前先跑出当前违规清单。
  - 状态：[]

- **tcb_ratio.py 退出码恢复（P0-01）**
  - 描述：[audit_tcb_ratio.py:215-221](file:///home/anfer/Code/QueenX/scripts/audit_tcb_ratio.py) `sys.exit(1)` 被注释，TCB 58.1% 超标仍 exit 0。
  - 方案：恢复 `sys.exit(1)`；同时修正 smoltcp 路径检查（实际在 `services/net/smoltcp`，非 `framework/net/smoltcp`）。
  - 状态：[]

- **safety_coverage.py 全量扫描（P0-02）**
  - 描述：[audit_safety_coverage.py:18](file:///home/anfer/Code/QueenX/scripts/audit_safety_coverage.py) 仅硬编码 8 个顶层文件，报告 53/53=100% 掩盖其余模块 unsafe；且文件缺失静默跳过（删文件 → 假 PASS）。
  - 方案：改为从 `framework/mod.rs` 的 `mod` 声明动态发现全部模块；删除"文件不存在静默跳过"。
  - 状态：[]

- **ci_check_services_unsafe.py vendored 排除（P0-04）**
  - 描述：[ci_check_services_unsafe.py](file:///home/anfer/Code/QueenX/scripts/ci_check_services_unsafe.py) 无 vendored 排除，实测对 smoltcp 18 处 unsafe 误报 exit 1。
  - 方案：复制 `audit_services_boundary.py` 的 VENDORED_EXCLUDE 列表。
  - 状态：[]

- **audit_unsafe.py SAFETY 窗口修复（META-P1）**
  - 描述：[tools/audit_unsafe.py:79,81](file:///home/anfer/Code/QueenX/tools/audit_unsafe.py) 8 行窗口过窄导致误报（属性堆叠推出 SAFETY）；窗口内任意行含 "SAFETY" 子串即判 OK 导致漏报。
  - 方案：窗口改为"跳过属性行/空行后向上找最近注释块"，且匹配严格 `SAFETY\s*:`。
  - 状态：[]

- **ci/audit.sh 门禁补漏（META-P1）**
  - 描述：[ci/audit.sh:62-69](file:///home/anfer/Code/QueenX/ci/audit.sh) audit_unsafe 输出为空时静默通过；[L135-141](file:///home/anfer/Code/QueenX/ci/audit.sh) clippy 未加 `-D warnings` 且失败仅警告；qemu 联动 `FAIL_OK` 默认 1 报虚假通过。
  - 方案：补 MISSING 为空时的 err 分支；clippy 命令加 `-D warnings` 且 else 改 err；qemu 调用处设 `FAIL_OK=0`。
  - 状态：[]

- **static_mut/repr_c/volatile fail-open 改 fail-closed（META-P1）**
  - 描述：[audit_static_mut.py:94](file:///home/anfer/Code/QueenX/scripts/audit_static_mut.py) `pub static mut` 漏检、SAFE_PATTERNS 子串豁免过宽；[audit_repr_c.py](file:///home/anfer/Code/QueenX/scripts/audit_repr_c.py) / [audit_volatile_access.py](file:///home/anfer/Code/QueenX/scripts/audit_volatile_access.py) err 分支"无法检查 = 通过"。
  - 方案：err 分支统一视为违规（fail-closed）；static_mut 正则支持 `pub(\(crate\))? static mut`；SAFE_PATTERNS 改精确匹配。
  - 状态：[]

- **coupling/invariants 接入 CI（META-P1 + CI 缺口）**
  - 描述：`audit_coupling.py`（F3）未接入 CI、services 层循环依赖不检测、`total > 20` 阈值放行轻量环；`audit_invariants.py`（I1-I6）仅本地执行。
  - 方案：修复 coupling 阈值/白名单字典序 bug/相对路径后接入 ci-lint.yml；invariants 排除 vendored 后接入。
  - 状态：[]

- **unwired_pub_fn.py 深度状态重置（META-P1）**
  - 描述：[audit_unwired_pub_fn.py:139](file:///home/anfer/Code/QueenX/scripts/audit_unwired_pub_fn.py) `in_trait_impl_depth` 置位后永不重置；`count_refs` 全词匹配把同名局部变量计入，高频词 fn 死代码逃检。
  - 方案：impl 块结束重置深度；引用计数改为符号级（排除同名局部变量）。
  - 状态：[]

### 验证门槛

- **负测试用例**
  - 描述：为每个修复后的脚本构造"违规样例 + 合法样例"各一，验证能拦截/不误报。
  - 方案：在 `host-tests/` 或 `scripts/` 下新增 fixture；CI job 内对样例文件运行脚本断言退出码。
  - 状态：[]

- **全量回归**
  - 描述：修复后全量运行 14 个审计脚本 + ci-lint.yml 对应 job，确认 0 误报且退出码语义正确。
  - 方案：本地跑 `bash ci/audit.sh quick` + `.github/workflows/ci-lint.yml` 涉及的脚本逐个验证。
  - 状态：[]

### 决策记录

- **DECISION-047**
  - 描述：审计脚本修复采用"fail-closed"原则——无法检查 = 违规，err 分支不再放行。
  - 方案：统一应用到所有审计脚本；避免"文件缺失/模式不匹配 → 通过"的隐蔽路径。
  - 状态：[]
