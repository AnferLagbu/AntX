# 框内核开发与维护指导

> 本文档给本项目 (AntX/QueenX) 维护者与贡献者: 在框内核 (framework/ + services/) 双子树架构下, **新代码放哪里, 改代码先改哪里, 何时两边都改**. 配套 [framekernel-nature.md](./framekernel-nature.md) 阅读.

适用读者: 维护 `framework/` 与 `services/` 的内核开发者, 以及首次提 PR 的新贡献者.

---

## 这是什么

- **范围**: 本项目内核代码的**归属决策**与**变更流程**; 不讨论 OSTD/Asterinas 理论.
- **不涵盖**: 用户态工具链, 构建系统, 测试工具 (另有文档).
- **配套**:
  - [framekernel-nature.md](./framekernel-nature.md) — 框内核是什么, 为什么这样设计.
  - [README.md §1 文件归属规则](../README.md) — 文档归属 (本文是代码归属).
  - [scripts/audit_services_boundary.py](file:///home/anfer/Code/AntX/scripts/audit_services_boundary.py) — 边界审计脚本的精确白/黑名单.

---

## 决策原则: 新代码放哪里

### 一句话判据

> **要 unsafe 吗? 要 → framework. 不要 → services.** 进一步: **涉及硬件/MMU/中断/上下文切换? → framework. 纯算法/策略/业务? → services.**

### 决策流程

```
新功能/补丁 → 问 3 个问题:

Q1. 是否需要直接操作硬件 (MMU/寄存器/中断控制器/DMA)?
    ├─ 是 → framework/ (新增或扩展现有子模块)
    └─ 否 → 继续

Q2. 是否必须 unsafe Rust (裸指针解引用/FFI/inline asm)?
    ├─ 是 → framework/ (暴露为 safe API 给 services)
    └─ 否 → 继续

Q3. 现有 framework 公开 API 是否够用?
    ├─ 不够 → framework/ 新增 safe API, services/ 调用
    └─ 够   → services/ 实现

最终位置: services/ (90% 的新功能) 或 framework/ (10%, 几乎只在"新增设备/新增架构"时)
```

### 决策速查表

| 场景 | 位置 | 理由 |
|------|------|------|
| 新增系统调用号处理 | services/syscall/ | 业务策略, 不碰硬件 |
| 新增 POSIX 文件系统 (如 ext4) | services/fs/ | 纯算法, 用 framework::frame 拿物理页 |
| 新增网卡驱动 (e1000) | **框架与服务都有**: framework/driver/ (寄存器+中断注册) + services/driver/net/ (协议栈对接) | 硬件层在 framework, 协议集成在 services |
| 新增同步原语 (如 seqlock) | framework/sync/ | 涉及原子指令, 需 unsafe |
| 新增 IRQ 安全自旋锁封装 (RAII) | services/sync/ | 把 framework 原子操作包装成类型安全 API |
| 新增 ELF 段类型支持 | framework/proc/elf.rs 加载器 + services/proc/elf.rs 应用策略 | 加载是硬件/MMU 操作, 解释是策略 |
| 新增系统调用表项 | services/syscall/mod.rs | 业务分发, 与硬件无关 |
| 新增 CPU 架构 (riscv64) | framework/arch/riscv64/ 全套 | 整架构都是 TCB |

### 子系统归属索引 (现状)

| 子系统 | framework/ | services/ |
|--------|-----------|-----------|
| 进程管理 | [framework/proc/](file:///home/anfer/Code/AntX/src/kernel/framework/proc/) (调度器/页表/上下文切换) | [services/proc/](file:///home/anfer/Code/AntX/src/kernel/services/proc/) (进程表/ELF 应用/signal) |
| 内存 | [framework/mm/](file:///home/anfer/Code/AntX/src/kernel/framework/mm/) (PMM/VMM/Slab/分配器) | (无, 全在 framework) |
| 文件系统 | [framework/fs/](file:///home/anfer/Code/AntX/src/kernel/framework/fs/) (VFS 抽象底) | [services/fs/](file:///home/anfer/Code/AntX/src/kernel/services/fs/) (ramfs/hvfs/devfs/procfs) |
| 设备驱动 | [framework/driver/](file:///home/anfer/Code/AntX/src/kernel/framework/driver/) (寄存器/时序) | [services/driver/](file:///home/anfer/Code/AntX/src/kernel/services/driver/) (驱动框架集成) |
| 同步 | [framework/sync/](file:///home/anfer/Code/AntX/src/kernel/framework/sync/) (TCB 11 子模块) | [services/sync/](file:///home/anfer/Code/AntX/src/kernel/services/sync/) (RAII 代理) |
| 网络 | [framework/net/](file:///home/anfer/Code/AntX/src/kernel/framework/net/) (硬件+协议栈底) | [services/net/](file:///home/anfer/Code/AntX/src/kernel/services/net/) (socket 应用层) |
| 系统调用 | [framework/syscall/](file:///home/anfer/Code/AntX/src/kernel/framework/syscall/) (入口/寄存器保存) | [services/syscall/](file:///home/anfer/Code/AntX/src/kernel/services/syscall/) (分发表) |
| 身份/权限 | [framework/credo/](file:///home/anfer/Code/AntX/src/kernel/framework/credo/) (PWM 硬件) | [services/credo/](file:///home/anfer/Code/AntX/src/kernel/services/credo/) (能力/会话) |
| 故障恢复 | [framework/barrier/](file:///home/anfer/Code/AntX/src/kernel/framework/barrier/) (snapshot/undo log) | [services/barrier/](file:///home/anfer/Code/AntX/src/kernel/services/barrier/) (策略/级联) |

---

## 维护场景: 何时两边都改

> 关键原则: **framework 是"机制", services 是"策略"**. 修机制 → framework; 改策略 → services. 两者都需变 → 先 framework 加 API, 再 services 用 API.

### 场景 1: 修 bug — bug 在 framework

例: 调度器在多核下偶发死锁.

```
1. 复现 + 定位 → framework/sched/
2. 修 framework/sched/scheduler.rs (含 unsafe 的修改)
3. 同步检查: services/proc/, services/syscall/ 是否依赖原行为?
   - 若 API 签名变化 → 同步改 services 调用点
   - 若仅行为修复 → services 无需改
4. CHANGELOG.md: "修复 [具体现象] (commit xxx)"
5. CI: cargo build + services 边界审计 + Miri (若涉及 unsafe)
```

### 场景 2: 修 bug — bug 在 services

例: ramfs 删除文件时未释放 dentry.

```
1. 定位 → services/fs/ramfs.rs (100% safe Rust, 编译期已保证内存安全)
2. 修 services/fs/ramfs.rs
3. framework 无需改
4. CHANGELOG.md: "修复 ramfs 资源泄漏"
5. CI: cargo build + 单元测试
```

### 场景 3: 维护子系统 — 两边都有

例: 给 VFS 加新文件类型 (如 fifo).

```
1. 评估: 新文件类型的"机制" (inode 操作表) → services/fs/
   (纯算法, 不碰硬件) → 单边修改.
2. 若新类型需要"内核态辅助结构" (如 pipe 缓冲区需物理页) →
   framework/mm/api.rs 加分配 API → services/fs/fifo.rs 用新 API.
3. CHANGELOG.md: "新增 [类型]" (变更/新增子节)
```

**结论**: 90% 的维护是单边 (仅 services 或仅 framework). 双边同时改的触发条件是**机制 + 策略都要调整**, 例如:
- 新增设备类型 (framework 加驱动接口 + services 加协议)
- 新增 OS 原语 (framework 加底层原语 + services 加 RAII 封装)
- 新增 CPU 架构 (framework/arch/ 新增 + 各 services 编译条件分支)

### 场景 4: 添加新功能 — 端到端流程

例: 新增 `mlock()` 系统调用 (锁定进程页不换出).

```
[需求] mlock 语义 (POSIX)
        ↓
[机制层 — framework/]
  - framework/mm/api.rs: 新增 `mlock_user_pages(vmspace, range) -> Result`
  - framework/proc/: 在 process metadata 加 "locked pages count"
  - framework/vmspace.rs: 若现有 API 不够, 扩展 VmSpace 接口
        ↓
[策略层 — services/]
  - services/syscall/mod.rs: 注册 syscall 号 → 分发到 mlock handler
  - services/proc/table.rs: 调用 framework 新 API, 维护统计
  - services/proc/elf.rs: 进程退出时, 沿 process table 反向释放所有 locked 页
        ↓
[测试]
  - miri-tests/: 模拟 mlock 失败路径 (页不足)
  - services 边界审计: 确认 services/proc/ 未触碰 framework::sync::raw 等禁止路径
        ↓
[文档]
  - CHANGELOG.md "## [Unreleased] / 新增" 写一条
  - 如有设计取舍 → docs/plan/ (DECISION-NNN)
  - 如有 API 变更 → docs/explain/syscall-api.md (若存在) 同步更新
        ↓
[CI 闸]
  - cargo build (含 services #![deny(unsafe_code)])
  - audit_services_boundary.py (白名单匹配)
  - check_tcb.sh (services 0 unsafe 扫描)
  - miri (unsafe 路径)
```

### 场景 5: 重构 (内部结构调整, 不改 API)

例: 把 `framework/sync/` 11 个子模块物理上合并 (v2.22 已做过).

```
1. 写 docs/plan/refactor-xxx.md (背景/目标/方案/待办)
2. 改代码 — 仅 framework/, services/ 不动 (若 API 稳定)
3. CI 跑全套 (因 TCB 改动, 必须跑 miri)
4. 完成后 CHANGELOG 写一条; plan 文档"决策记录"标完成
```

---

## 边界规则 (CI 强制)

[scripts/audit_services_boundary.py](file:///home/anfer/Code/AntX/scripts/audit_services_boundary.py) 与 [tools/check_tcb.sh](file:///home/anfer/Code/AntX/tools/check_tcb.sh) 在 PR 检查时强制以下规则. 写代码前先看, 避免 PR 被打回.

### 规则 1: services 0 unsafe

- 顶层 [src/kernel/services/mod.rs:1](file:///home/anfer/Code/AntX/src/kernel/services/mod.rs#L1) `#![deny(unsafe_code)]` 编译期拒绝.
- 例外: `extern "C"` 声明允许, 但**调用点必须在 framework**; services 只持有函数指针.
- 检查命令: `grep -rn 'unsafe ' src/kernel/services/` 必须为空 (注释除外).

### 规则 2: services 只能访问 framework 公开 API (白名单)

[services 可直接调用的 framework 模块](file:///home/anfer/Code/AntX/scripts/audit_services_boundary.py#L43-L75) 至少包括:
- 8 个安全代理: `framework::frame`, `vmspace`, `usermode`, `userctx`, `iomem`, `ioport`, `irqline`, `dma_buf`
- FFI 代理: `credo_pwm`, `net_socket`, `proc_elf`
- 子系统顶层: `framework::mm`, `proc`, `fs`, `net`, `ipc`, `credo`, `chitin`, `barrier`, `driver`, `pci`, `dma`, `irq`, `syscall`, `timer`, `wasm`, `sched`
- 基础: `framework::cpu`, `sync` (顶层 re-export), `alloc`, `klog`, `console`, `config`, `boot`, `lib`

### 规则 3: services 禁止访问 framework 内部模块 (黑名单)

[禁止直接访问的 framework 内部模块](file:///home/anfer/Code/AntX/scripts/audit_services_boundary.py#L30-L66) 至少包括:
- 同步内部: `framework::sync::raw`, `sync::arch`, `sync::atomic` (应通过 services/sync 代理), `sync::types`, `seqlock::raw`, `rcu::raw`
- 架构底层: `framework::arch::x86_64`, `arch::aarch64`, `arch::CurrentArch`
- IDT 实现: `framework::idt::statistics`, `idt::handlers`, `idt::IdtManager`, `idt::types`
- 8 API 的 raw 实现: `framework::frame::raw`, `vmspace::raw`, `iomem::raw`, `ioport::raw`, `irqline::raw`, `dma_buf::raw`, `userptr::raw`
- 其他底层: `framework::page_table`, `cpu_local`, `racy_cell`, `alloc::raw`, `boot::raw`, `barrier::undo_log` 等

### 规则 4: services 禁止裸指针解引用

`raw_ptr_deref` 模式 (例如 `*mut T` 直接 `(*ptr).field` 访问) 在 services 触发. 即便用 `as *mut T` 转型再解引用, 也会被审计脚本捕获.

正确做法: 让 framework 提供 `&mut T` / `&T` 引用, services 直接用.

### 规则 5: 改动 framework 必须更新 SAFETY 注释

framework 任何 `unsafe` 块修改后:
- 重读相邻 `// SAFETY:` 注释, 确认前提/调用方保证/硬件契约仍成立.
- 若 API 签名变化, 更新 doc-comment 中的 `# Safety` 段.
- 提交信息中加 `[TCB]` 标记, 提示 reviewer 重点审 unsafe 块.

---

## 常见反例 (不要这样做)

### 反例 1: services 里"只用一行 unsafe"

```rust
// services/fs/ramfs.rs  ← CI 拒收
pub fn read(...) {
    unsafe { (*ptr).field = ... }  // "只是临时用一下"
}
```

**正解**: 移 `(*ptr).field = ...` 到 framework 暴露的 `pub fn set_field(p: &mut Foo, v: Value)`, services 调 set_field.

### 反例 2: services 直接 include framework 内部模块

```rust
// services/sync/mod.rs  ← CI 拒收
use crate::kernel::framework::sync::raw;
```

**正解**: 在 framework/sync/mod.rs 顶层 `pub use` 出 `raw::*`, 或更优: 让 framework 暴露高层 safe API, services/sync 调高层 API.

### 反例 3: framework 模块膨胀

```rust
// framework/fs/ext4.rs  ← 审查拒收
// 在 framework 里实现 ext4 文件系统 (百万行)
```

**正解**: ext4 是"策略 + 业务", 应在 services/fs/ext4/. framework/fs/ 只放"机制": `pub trait FsBackend`, `pub trait InodeOp` 这类 trait 定义, 供 services 实现.

### 反例 4: framework 安全 API 暴露内部细节

```rust
// framework/memory.rs  ← 审查拒收
pub fn alloc_page_raw() -> *mut u8 { ... }  // 暴露裸指针给 services
```

**正解**: 暴露 `pub fn alloc_page() -> Option<Frame> { ... }`, `Frame` 是引用计数类型, services 用 RAII 释放, 永远拿不到裸指针.

### 反例 5: services 改 framework 的算法

例: 想给 ramfs 加新功能, 但 ramfs 在 services, 改 services/fs/ 即可. **不要**为图省事改 framework/fs/ 的 VFS 抽象, 那是机制层, 一改全内核受影响.

---

## 注意事项

- **TCB 改动是重大事件**. framework 任何 `unsafe` 块或 `pub` 安全 API 签名变化, 必须在 PR 描述里写明影响面, 并跑全套 miri. 评审需至少 1 名熟悉子系统的 reviewer.
- **services 跨层调用要单调**. services A 调 services B, services B 调 framework — 合法. services A 直接调 framework — 合法. **禁止** services A 调 services B 时把 framework 类型当业务数据传来传去形成循环 (会增加 TCB 的"被使用面").
- **命名空间敏感**: `framework::mm` 和 `services::mm` 风格上要统一, 避免"功能相似但 API 形状不同"导致误用. 例如 framework 暴露 `Frame::new()`, services 不要再造一个 `Frame::new()`.
- **新文件开头声明 `@SAFE`**: services 下每个新 .rs 文件首行写:
  ```rust
  //! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
  ```
  便于 [services/mod.rs:20-22](file:///home/anfer/Code/AntX/src/kernel/services/mod.rs#L20-L22) 规范的人工审计.
- **跨 commit 改动两半**: 如果一个 PR 同时改 framework 和 services, 先合 framework 的 API 变更 (单独 commit), 再合 services 的对接. 避免"半生不熟"状态卡住其他开发者.
- **删除 services 子模块前先审计**: 删一个 services 子模块前, 跑一次 `audit_services_boundary.py` 与 `check_tcb.sh` 确认无引用; 同时更新 docs/CHANGELOG.md 移除条目.

---

## 交叉引用

- 依赖:
  - [framekernel-nature.md](./framekernel-nature.md) — 框内核定义与原理, 必读背景.
  - [docs/README.md](../README.md) — 文档格式规范.
  - [src/kernel/framework/mod.rs](file:///home/anfer/Code/AntX/src/kernel/framework/mod.rs) — framework 入口与 SAFETY 规范.
  - [src/kernel/services/mod.rs](file:///home/anfer/Code/AntX/src/kernel/services/mod.rs) — services 入口与 Safe Rust 契约.
  - [scripts/audit_services_boundary.py](file:///home/anfer/Code/AntX/scripts/audit_services_boundary.py) — 边界审计, 白/黑名单权威源.
- 被引用:
  - [docs/CHANGELOG.md](file:///home/anfer/Code/AntX/docs/CHANGELOG.md) — 代码变更日志, 本文档的变更也会写进去.
- 外部:
  - [OSTD 官方书 — The Framekernel Architecture](https://asterinas.github.io/book/kernel/the-framekernel-architecture.html) — 原始定义.
  - [Asterinas USENIX ATC 2025 论文](https://www.usenix.org/system/files/atc25-peng-yuke.pdf) — §3 Framekernel 详细架构.

