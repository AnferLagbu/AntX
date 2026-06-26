# 工程纪律性规范

> 约束后续新代码的工程规范. 已有代码的解耦进度见 docs/plan/engineering-discipline.md. 所有开发者 (含 AI) 提交新代码前必须遵守本文件. 创建于 2026-06-18, 2026-06-26 按新文档规则重写.

## 这是什么
- **范围与配套**
  - 描述: 工程纪律性规范的范畴
  - 方案: 范围: 约束后续新代码的工程规范; 配套: docs/plan/engineering-discipline.md (已有代码解耦进度) / framekernel-dev-guide.md (架构开发场景) / framekernel-nature.md (安全不变式 I1-I6) / AGENTS.md (项目硬约束); 0 铁律 + 13 章节 + 1 提交检查清单 + 1 存量问题处理策略
  - 状态: [X]

## 为什么这样设计
- **0 铁律 (零容忍, 违反即拒收)**
  - 描述: 6 条零容忍规则
  - 方案: F1 services 层 0 unsafe (#![deny(unsafe_code)] + audit_services_boundary.py) / F2 services 禁止访问 framework 内部模块 (audit_services_boundary.py 黑名单) / F3 新增代码禁止引入模块间循环依赖 (audit_coupling.py) / F4 framework 任何 unsafe 块必须配 // SAFETY: 注释 (audit_safety_coverage.py) / F5 双架构编译 0 warning 0 error (make ARCH=x86_64 && make ARCH=aarch64) / F6 三审计全部通过 (audit_services_boundary + audit_safety_coverage + audit_deadlock_matrix)
  - 状态: [X]

## 如何使用
- **1 禁止耦合**
  - 描述: 4 条耦合规则
  - 方案: 1.1 跨子系统禁止直接访问内部 (子系统 A 调用 B 只能通过 B 的 mod.rs re-export 或 api.rs, 不得直接访问 B 的内部子模块) / 1.2 禁止隐式依赖传递 (A 依赖 B, B 依赖 C, A 不得直接使用 C 的类型/函数除非 A 也显式声明依赖 C) / 1.3 禁止 services 层反向依赖 (services A→services B→framework→services A 形成循环时, 必须通过 trait 注入或回调接口解耦) / 1.4 禁止跨层传递内部类型 (framework 内部类型如 Process/MmStruct 的裸字段不得作为 services 公开 API 的参数或返回值, 用 PID 或句柄)
  - 状态: [X]
- **2 禁止硬编码**
  - 描述: 3 条硬编码规则
  - 方案: 2.1 跨子系统常量: 子系统 A 中使用的、属于子系统 B 的常量, 必须在 framework::config 或 services::config 中定义, 不得在 A 内部硬编码 / 2.2 魔数: 所有数字常量必须有命名, 唯一例外 0/1/-1 等上下文明确的字面量 / 2.3 字符串路径: 文件系统路径、设备名等不得在代码中硬编码, 通过配置或参数传入
  - 状态: [X]
- **3 禁止耦合性代码**
  - 描述: 3 条耦合代码规则
  - 方案: 3.1 禁止顺手优化: 修改现有代码时只改必须改的内容, 每一行改动必须能追溯到用户请求或明确的 bug 修复; 3.2 禁止过度抽象: 不为单次使用的代码创建 trait/抽象, 不为将来可能需要预留扩展点, 三行重复代码优于一个过早抽象, 问自己"一个资深工程师会认为这太复杂了吗?" / 3.3 禁止功能膨胀: 新功能的实现范围必须严格匹配需求, 不得添加需求未提及的辅助功能/灵活性/可配置性/不可能场景的错误处理
  - 状态: [X]
- **4 代码质量**
  - 描述: 4 项代码质量规则
  - 方案: 4.1 services 层新文件首行 #![deny(unsafe_code)] + //! @SAFE: 本文件不含 unsafe 代码. 所有 unsafe 操作已委托至 framework API / 4.2 framework 层新 unsafe 块必须配 // SAFETY: <前提条件>; <调用方保证>; <硬件契约> / 4.3 中文注释强制 (audit_comment_language.py 硬阈值 0 violations) / 4.4 公共 API 必须有文档注释 (clippy missing_docs_in_crate_items 强制)
  - 状态: [X]
- **5 中断与原子**
  - 描述: 4 条中断与原子规则
  - 方案: 5.1 中断上下文禁止睡眠操作 (持锁期间禁止 schedule/yield/block) / 5.2 原子操作 Ordering 正确 (Acquire/Release/SeqCst 根据同步需求选择) / 5.3 资源获取/释放严格配对 (RAII/智能指针/ScopeGuard) / 5.4 失败路径 LIFO 反序回滚 (分配顺序反向释放)
  - 状态: [X]
- **6 trait 抽象**
  - 描述: 5 条 trait 抽象规则
  - 方案: 6.1 策略通过 trait 注入到 services (机制留 framework, 策略 trait 定义在 framework::api, 实现放 services) / 6.2 trait dispatch 优先静态分发 (impl Trait 优于 Box<dyn Trait>, 单态化 0 开销) / 6.3 trait 方法配 #[inline(always)] 强制内联 (0 开销) / 6.4 trait 方法 &mut self 而非 &mut dyn (编译期类型已知) / 6.5 避免在 poll 路径分配/锁
  - 状态: [X]
- **7 类型与内存**
  - 描述: 3 条类型与内存规则
  - 方案: 7.1 优先使用强类型 (新类型模式 NewType/Pid/Handle) / 7.2 禁止裸指针出现在 public API (&mut T/&T/智能指针) / 7.3 内存分配失败必须显式处理 (返回 Result/Option, 禁止 unwrap)
  - 状态: [X]
- **8 错误处理**
  - 描述: 3 条错误处理规则
  - 方案: 8.1 统一错误类型 (KernelError 跨子系统, 禁止传递子系统内部错误类型) / 8.2 可恢复错误用 Result (禁止 panic!() 用于业务逻辑) / 8.3 非 test 代码禁止 unwrap() (audit_safety_coverage.py 检测)
  - 状态: [X]
- **9 测试**
  - 描述: 3 条测试规则
  - 方案: 9.1 修改跨模块接口必须补充集成测试 (host-tests) / 9.2 公共 API 必须有单元测试 (no_std 单元测试 + host-tests 集成测试) / 9.3 性能基线 (host-tests/benches/baseline.json) 每次 PR 更新
  - 状态: [X]
- **10 文档**
  - 描述: 4 条文档规则
  - 方案: 10.1 修改模块接口或依赖关系时, 必须同步更新 framekernel-dev-guide.md 和 audit-*.md / 10.2 公共 API 必中文文档注释 (clippy missing_docs_in_crate_items) / 10.3 plan/ 文档按新规则 (标题+章节+条目描述+方案+状态+详情) / 10.4 CHANGELOG.md 记录"面向用户/接手人"的可见变更
  - 状态: [X]
- **11 多架构**
  - 描述: 3 条多架构规则
  - 方案: 11.1 架构特定代码 cfg 门控 (#![cfg(target_arch = "x86_64")] / 11.2 优先使用架构无关代码 (portable 类型/接口) / 11.3 双架构编译验证 (x86_64 + aarch64)
  - 状态: [X]
- **12 提交检查清单**
  - 描述: 14 项提交前确认
  - 方案: 基础 (4 项): 双架构编译 0w0e / 三审计通过 / host-tests 通过 / 文档同步更新 (CHANGELOG.md); 耦合与编码 (5 项): 无新增 services 层 unsafe / 无新增跨子系统内部访问 / 无新增循环依赖 / 无硬编码跨子系统常量 / 无顺手修改无关代码; 内核安全 (5 项): 中断上下文无 sleep 操作 / 原子操作 Ordering 正确 / 资源获取/释放严格配对 / 失败路径 LIFO 反序回滚 / framework 修改通过 I1-I6 自检
  - 状态: [X]
- **13 存量问题处理**
  - 描述: 4 步处理策略
  - 方案: (1) 触及时修复: 修改该模块时顺带修复; (2) 标记待修: 用 // TODO(TCB): 策略可提取到 services 标注; (3) 禁止忽视: 不允许以"历史遗留"为由永久搁置; (4) 新代码零容忍: 新代码必须 100% 符合本规范, 不允许"先写再改"
  - 状态: [X]

## 工作原理
- **6 层防护机制**
  - 描述: 6 层防护
  - 方案: 编译期: services/mod.rs 顶部 #![deny(unsafe_code)] 强制 services 0 unsafe; 静态检查: audit_services_boundary + audit_safety_coverage + audit_deadlock_matrix + audit_comment_language + audit_coupling + audit_tcb_ratio + audit_invariants 7 个审计脚本; 编译验证: 双架构 cargo check 0w0e + clippy 0 warning; 测试: host-tests + miri-tests + QEMU 集成测试; 人工审查: PR review 流程覆盖本规范 13 章节; 文档同步: CHANGELOG.md + plan/ + explain/ 三层文档自动维护
  - 状态: [X]

## 注意事项
- **违反铁律即拒收**
  - 描述: F1-F6 是硬约束
  - 方案: 6 条铁律零容忍, 违反即拒收 PR, 不接受"先合并再修"
  - 状态: [X]
- **新代码 100% 合规**
  - 描述: 不允许"先写再改"
  - 方案: 存量代码可按 §13 策略渐进修复, 新代码必须 100% 符合本规范
  - 状态: [X]
- **跨架构同步**
  - 描述: x86_64 + aarch64 双架构必须同时通过
  - 方案: 任何架构特定代码必须配 cfg 门控, 双架构编译验证
  - 状态: [X]
- **不为将来预留**
  - 描述: 准则 §0 严格适用
  - 方案: 不为"将来可能用"预留扩展点/抽象/配置
  - 状态: [X]

## 交叉引用
- **依赖清单**
  - 描述: 4 个依赖源
  - 方案: framekernel-dev-guide.md (架构开发场景指导) / framekernel-nature.md (安全不变式 I1-I6 定义) / engineering-discipline.md (已有代码解耦进度) / AGENTS.md (项目硬约束)
  - 状态: [X]
- **被引用清单**
  - 描述: 1 个被引用源
  - 方案: docs/CHANGELOG.md (代码变更日志, 本规范的变更也会写进去)
  - 状态: [X]

## 变更历史
- **2026-06-26**
  - 描述: 按新文档规则重写 (标题+条目(描述+方案+状态)+详情)
  - 方案: 结构重组, 保留原意
  - 状态: [X]
- **2026-06-18**
  - 描述: 初始版本
  - 方案: -
  - 状态: [X]
