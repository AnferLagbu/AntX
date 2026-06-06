# 框内核 (Framekernel)

> 框内核是 Asterinas 项目 (USENIX ATC 2025) 提出的 OS 架构, 通过"语言层面的内核内特权分离", 用单一地址空间同时获得宏内核性能与微内核级安全. 本文档讲清楚它的定义、原理, 以及在 AntX/QueenX 项目中的具体落地形式.

适用读者: 维护 `framework/` 和 `services/` 两个子树的开发者, 评估子系统归属 (改哪里) 与新功能入口的贡献者.

---

## 这是什么

**框内核** (Framekernel) 是一种将整个 OS 内核放在单一地址空间 (像宏内核一样), 但在源码层将其逻辑划分为两半的 OS 架构:

| 划分 | 名称 | unsafe 权限 | 职责 | 代码量 |
|------|------|-------------|------|--------|
| 特权半 | **OS Framework** (操作系统框架) | 允许 `unsafe` | 把 MMU/DMA/中断/上下文切换等底层操作封装成内存安全的 Rust API | 小 (TCB) |
| 去特权半 | **OS Services** (操作系统服务) | 禁止 `unsafe` (仅 safe Rust) | 在 framework 提供的安全 API 之上实现 OS 功能: 系统调用, 进程管理, 文件系统, 设备驱动, 网络协议栈 | 大 |

技术要点:
- 单一地址空间: services 与 framework 共址, 互相调用走**普通函数调用 + 共享内存**, 无 IPC 上下文切换, 因此无微内核的 IPC 性能损失.
- 语言层特权分离: services 被强制 `safe Rust` (`#![deny(unsafe_code)]`); framework 唯一被允许写 `unsafe`. **整个内核的内存安全只依赖 framework 这一个 TCB 的正确性**.
- TCB (Trusted Computing Base) 最小化: framework 越小越可控. Asterinas 论文实测 OSTD ~15,000 LoC, 约占内核总量 14%.

### 起源

- 2022 年中, 蚂蚁集团 (Ant Group) 田洪亮团队构想出"用 Rust 写完整 OS, 把 unsafe 关进最小 TCB"的路线, 这是框内核的最初动机.
- 2022-10, 中关村实验室 / 蚂蚁集团 / 北京大学 / 南方科技大学联合发布 [Asterinas](https://github.com/asterinas/asterinas) 项目作为首个工业级实现.
- 2024-10, 星绽 OS 在 HackerNews 公开发布, 引发 Rust OS 社区关注.
- 2025-06, USENIX ATC 2025 收录论文 *Asterinas: A Linux ABI-Compatible, Rust-Based Framekernel OS with a Small and Sound TCB* (DOI 见 [arxiv:2506.03876](https://arxiv.org/abs/2506.03876)), 正式命名 "Framekernel" 架构.
- 2025-07, 北大的 CortenMM (Asterinas 内存管理子系统) 获 SOSP 2025 最佳论文, 验证了框内核的 TCB 可形式化验证路径.

### 范畴边界

- 框内核**不是**第三种"微内核变体"——它和微内核共享"小 TCB"思想, 但用**单地址空间 + 函数调用**替代了微内核的 IPC.
- 框内核**不是**"全 safe Rust 内核"——它承认必须有 unsafe, 只是把 unsafe 集中到 TCB.
- 框内核**与宏内核的关键差异**不在性能, 而在"内核内分层": 单地址空间依旧是宏内核做法, 但多了源码层的特权/去特权边界.

---

## 为什么这样设计

### 问题: Rust OS ≠ Safe OS

2024-07-19 的 CrowdStrike 事件 (全球数百万 Windows 蓝屏) 是一次**内核驱动 OOB 访问**, 证明: 即使商业 OS 巨头也无法靠工程规范根除内存安全漏洞. 统计显示, 系统软件 60–70% 的高危漏洞根因是内存安全.

对 Rust 内核, USENIX ATC 2025 论文统计了 unsafe 比例:
- Linux + RFL: 55% (6/111 crate)
- Tock: 93% (91/98)
- RedLeaf: 62% (36/58)
- Theseus: 32% (54/171)

**结论**: 仅仅"用 Rust 写 OS" 并不够, 必须**系统性地控制 unsafe 的扩散范围**.

### 替代方案与放弃原因

| 方案 | 优点 | 放弃原因 |
|------|------|----------|
| 全 C 内核 + 沙箱 (gVisor) | 与现有生态兼容 | 性能损耗大, 沙箱本身仍需验证 |
| 全 safe Rust + inline asm | 无 unsafe 块 | 编译器无法验证硬件交互, 等价于"信任所有代码" |
| 微内核 (seL4) | TCB 最小, 已形式化验证 | IPC 性能损失; 不支持富功能 OS |
| Rust for Linux (混合) | 渐进式引入 Rust | unsafe 散落, TCB ≈ Linux 全量, 失去"安全"卖点 |
| **框内核 (Framekernel)** | **宏内核性能 + 微内核级 TCB** | **— 选定 —** |

### 设计契约 (OSTD 四准则)

Asterinas 在 OSTD 文档中明确, framework 必须同时满足:
1. **Soundness (安全性)**: 任何用 framework 安全 API 写的 safe Rust 都不可触发 UB.
2. **Expressiveness (表达力)**: 必须支持写设备驱动等"内核主体"功能.
3. **Minimalism (极小性)**: 能放在 services 层的就不放 framework.
4. **Efficiency (效率)**: 安全 API 应是 zero-cost abstraction, 几乎无运行时开销.

---

## 在 AntX/QueenX 的具体应用

AntX 项目 (`/home/anfer/Code/AntX`) 严格采用 Asterinas 框内核范式, 并在仓库代码中以 `framework/` (TCB) 与 `services/` (去特权) 两个子树落实.

### 目录落地

| 角色 | 路径 | unsafe 权限 | 关键模块 | 大致 LoC |
|------|------|-------------|----------|----------|
| TCB (framework) | [src/kernel/framework/](file:///home/anfer/Code/AntX/src/kernel/framework/) | 允许 | arch/ (GDT/IDT/APIC/MMU/GIC), boot/, mm/, irq/, idt/, dma/, driver/, net/, fs/, ipc/, sync/, sched/, frame.rs, vmspace.rs, usermode.rs, userctx.rs, page_table.rs 等 | ~3000+ |
| 去特权 (services) | [src/kernel/services/](file:///home/anfer/Code/AntX/src/kernel/services/) | 禁止 (`#![deny(unsafe_code)]`) | syscall/, proc/, fs/, net/, ipc/, chitin/, driver/, credo/, sync/, barrier/, wasm/ | 占内核主体 |

入口模块声明:
- 特权方入口: [src/kernel/framework/mod.rs:1-3](file:///home/anfer/Code/AntX/src/kernel/framework/mod.rs#L1-L3) `//! QueenX Framekernel — 特权 OS Framework (TCB)`
- 去特权方入口: [src/kernel/services/mod.rs:1-2](file:///home/anfer/Code/AntX/src/kernel/services/mod.rs#L1-L2) `#![deny(unsafe_code)]` + `//! QueenX Services 层 — 100% safe Rust (去特权)`

### 强制安全契约

services/ 目录的"零 unsafe" 由两层强制:
1. **编译期** ([src/kernel/services/mod.rs:1](file:///home/anfer/Code/AntX/src/kernel/services/mod.rs#L1)): 顶层 `#![deny(unsafe_code)]`, 任何 services 下的 `unsafe` 块或函数都会**编译失败**.
2. **CI 脚本** ([src/kernel/services/mod.rs:23-24](file:///home/anfer/Code/AntX/src/kernel/services/mod.rs#L23-L24)): `tools/check_tcb.sh` 与 `scripts/audit_services_boundary.py` 静态扫描, 确认 services 不能"绕道"访问 framework 的内部模块 (如 `framework::sync::raw`, `framework::arch::x86_64`).

### "unsafe 安全 API" 在 QueenX 中的具体形态

`framework/` 暴露给 `services/` 的安全 API (即 OSTD 在本项目中的等价物) 至少包括:
- `framework::frame` ([src/kernel/framework/frame.rs](file:///home/anfer/Code/AntX/src/kernel/framework/frame.rs)): 物理页 `Frame` 抽象, 引用计数.
- `framework::vmspace` ([src/kernel/framework/vmspace.rs](file:///home/anfer/Code/AntX/src/kernel/framework/vmspace.rs)): 用户地址空间句柄.
- `framework::usermode` ([src/kernel/framework/usermode.rs](file:///home/anfer/Code/AntX/src/kernel/framework/usermode.rs)): 进入 Ring 3 / EL0 的安全入口.
- `framework::userctx` ([src/kernel/framework/userctx.rs](file:///home/anfer/Code/AntX/src/kernel/framework/userctx.rs)): 用户态寄存器安全操纵.
- `framework::userptr` ([src/kernel/framework/userptr.rs](file:///home/anfer/Code/AntX/src/kernel/framework/userptr.rs)): 用户指针 (用于 copy_from_user / copy_to_user).
- `framework::iomem` / `framework::ioport` / `framework::irqline` / `framework::dma_buf`: MMIO / PIO / 中断线 / DMA 的安全代理.
- `framework::mm::api` ([src/kernel/framework/mm/api.rs](file:///home/anfer/Code/AntX/src/kernel/framework/mm/api.rs)): `kfree` 等内存释放的安全入口 (供 services 释放 framework 分配的对象).

### "安全 API + safe Rust 失败回滚" 的工作示例

Issue1 修复 (2026-05, DECISION-027) 是一个完整示范: 进程创建失败路径上, framework 暴露的 `free_kernel_process` / `free_user_process` 让上层 (同样在 framework 内) 能以 safe API 释放先前 framework 自己分配的资源, 避免 unsafe 扩散到业务层.

- 分配入口 (safe API, 公开给本模块的 create 流程): [src/kernel/framework/proc/user_proc.rs:485-494](file:///home/anfer/Code/AntX/src/kernel/framework/proc/user_proc.rs#L485-L494) `alloc_kernel_process` / [src/kernel/framework/proc/user_proc.rs:469-484](file:///home/anfer/Code/AntX/src/kernel/framework/proc/user_proc.rs#L469-L484) `alloc_user_process`.
- 释放入口 (safe API, 失败回滚专用): [src/kernel/framework/proc/user_proc.rs:502-510](file:///home/anfer/Code/AntX/src/kernel/framework/proc/user_proc.rs#L502-L510) `free_kernel_process` 与 [src/kernel/framework/proc/user_proc.rs:519-527](file:///home/anfer/Code/AntX/src/kernel/framework/proc/user_proc.rs#L519-L527) `free_user_process`. 两者内部 `unsafe { kfree(...) }` 各自带 `// SAFETY:` 注释.
- 调用点 (LIFO 反序, 先 UserProcess 再 Process, 避免 `NonNull<Process>` 悬挂): [src/kernel/framework/proc/user_proc.rs:843-901](file:///home/anfer/Code/AntX/src/kernel/framework/proc/user_proc.rs#L843-L901) `UserProcManager::create` 的三个失败分支.

这一模式就是 Asterinas 论文 §4.3 "Privilege Separation" 的工程落地: "unsafe 只在 framework 一次出现, 业务层只看到 safe API"。

### 与 Asterinas 原版的差异 (本地化)

- 项目未引入 OSTD 完整 crate, 而是把 framework 直接做成 `src/kernel/framework/`, 公共 API 集中体现在 [src/kernel/framework/mod.rs:23-55](file:///home/anfer/Code/AntX/src/kernel/framework/mod.rs#L23-L55) 列出的顶层模块.
- `services/mod.rs` 顶部的 "Safe Rust 契约" 段落 (本文件头注解) 把"每文件头声明 `@SAFE`"写成显式规范, 方便人工/脚本审计.
- "TCB 公开 API 白名单 + 内部模块黑名单" 落在 [scripts/audit_services_boundary.py](file:///home/anfer/Code/AntX/scripts/audit_services_boundary.py), 是 Asterinas 论文未单列但本项目新增的工程约束.

---

## 工作原理

### 编译期强制

```
src/kernel/services/
└── mod.rs      ← #![deny(unsafe_code)]  (本模块根)
    ├── syscall/
    ├── proc/
    ├── fs/
    └── ...     ← 任何子模块中写 `unsafe` 即触发编译错误
```

效果: `cargo build` 阶段即排除 services 层的 unsafe 代码.

### 调用边界

```
[服务层 safe Rust 代码]
        │  调用 framework::* 的安全 API
        ▼
[framework::frame  / vmspace / usermode / ...]
        │  内部 unsafe 块, 配套 // SAFETY: 注释
        ▼
[MMU / DMA / 中断控制器 / CPU 寄存器]
```

- 跨层接口必须**全部**走 framework 的 `pub fn` (无 `pub unsafe fn`).
- framework 内部子模块之间**不受** `#![deny(unsafe_code)]` 限制, 允许 `unsafe`, 但要写 `// SAFETY:`.

### 内存安全 TCB 的定义

- **TCB = framework 的所有 `unsafe` 块 + 围绕它们的 safe API 类型/函数签名**.
- 只要 (a) framework 的 `unsafe` 块全部遵守 SAFETY 契约, (b) framework 的 safe API 形式化 (或经严格审计) 正确, 则整个 OS 内核的内存安全都成立.
- services 层出 bug, 顶多产生"逻辑错误" (如 VFS 返回错误码), 不会导致 UAF/OOB 等内存安全漏洞.

### 与宏内核/微内核的对比

| 维度 | 宏内核 (Linux) | 微内核 (seL4) | **框内核 (Asterinas/QueenX)** |
|------|----------------|---------------|--------------------------------|
| 地址空间 | 单一 | 多个 (进程化服务) | 单一 |
| 跨组件通信 | 函数调用 | IPC (高开销) | 函数调用 |
| TCB 大小 | 整个内核 (千万 LoC) | 微内核 (10K LoC) | framework (~15K LoC, Asterinas 实测) |
| 设备驱动位置 | 内核态 | 用户态 | 内核态 (safe Rust) |
| 富功能支持 | 强 | 弱 | 强 (Linux ABI 兼容) |
| 内存安全保证 | 无 | 强 (可验证) | 强 (TCB 内可验证) |

---

## 注意事项

- **不要在 services 写 `unsafe`**. 即便"只用一行", 一旦放行, 后续开发者会以"先例"继续扩散, 几天后 TCB 边界形同虚设. 正确做法是把 unsafe 移到 framework 并暴露为 safe API.
- **不要让 services 直接 `pub use framework::sync::raw` 或 `framework::arch::x86_64`**. 这些是 TCB 内部细节, 只能通过 framework 顶层 API 调用. CI 脚本会拦截.
- **改 framework 是大改**. 任何 framework 的 `unsafe` 块改动, 都要重新审视下游 services 是否仍依赖其 SAFETY 契约. 提交前在 `docs/CHANGELOG.md` 写明并补 `docs/plan/audit-*.md` 审计.
- **不要把"内层抽象"暴露成 framework 公开 API**. 例如 `PageTableChecker` (在 framework) 之于 `services::fs::page_cache` 的关系应是: services 拿到 `VmSpace` / `Frame`, 不直接拿 checker. 见 [src/kernel/framework/mod.rs:55-60](file:///home/anfer/Code/AntX/src/kernel/framework/mod.rs#L55-L60) 列出的公开 API 范围.
- **不要为"将来可能用"预留 framework 模块**. 准则 §0 "不为将来可能用到预留章节" 在这里同样适用, 否则 framework 不可避免地膨胀, TCB 失控.
- **新增框架抽象时, 严格按 OSTD 四准则 (Soundness/Expressiveness/Minimalism/Efficiency) 评审**. 通过评审再合并.

---

## 交叉引用

- 依赖:
  - [docs/README.md §6 explain 文档格式](file:///home/anfer/Code/AntX/docs/README.md) — 写本文所遵循的格式规范.
  - [src/kernel/framework/mod.rs](file:///home/anfer/Code/AntX/src/kernel/framework/mod.rs) — framework 子树入口, 含完整模块清单与 SAFETY 注释规范.
  - [src/kernel/services/mod.rs](file:///home/anfer/Code/AntX/src/kernel/services/mod.rs) — services 子树入口, 包含 `#![deny(unsafe_code)]` 与 Safe Rust 契约.
- 被引用:
  - [docs/CHANGELOG.md](file:///home/anfer/Code/AntX/docs/CHANGELOG.md) `## 2026-05-21` — 移除项目版本号; 子树"框内核化"迭代.
  - [docs/plan/fix-report-issue1.md](file:///home/anfer/Code/AntX/docs/plan/) (若存在) — 进程分配失败回滚修复报告, 是框内核"safe API 失败回滚"模式的样例.
- 外部参考:
  - [Asterinas 框内核架构 (官方书)](https://asterinas.github.io/book/kernel/the-framekernel-architecture.html)
  - [USENIX ATC 2025 论文 PDF](https://www.usenix.org/system/files/atc25-peng-yuke.pdf)
  - [arXiv 预印本: 2506.03876](https://arxiv.org/abs/2506.03876)
  - [Kernel Memory Safety: Mission Accomplished (Asterinas 博客)](https://asterinas.github.io/2025/06/04/kernel-memory-safety-mission-accomplished.html)
  - [Asterinas: A Rust-Based Framekernel to Reimagine Linux (login; 2025-06-17)](https://www.usenix.org/publications/loginonline/asterinas-rust-based-framekernel-reimagine-linux-2020s)
  - [星绽 OS 登顶 SOSP: 框内核的技术解析 (今日头条, 2025)](http://m.toutiao.com/group/7567348017276240425/)

---

## 变更历史

- 2026-06-06: 初始版本, 解释框内核概念及在 AntX/QueenX 的具体落地 (framework/ + services/ 双子树, safe API 失败回滚样例, 强制契约与 CI 审计).

## 元数据

- 创建: 2026-06-06
- 最后更新: 2026-06-06
- 适用范围: 内核 (framework + services 两层)
- 状态: 已审
- 主要参考: Asterinas USENIX ATC 2025 论文 (Peng et al., 2025-06) + OSTD 官方书
