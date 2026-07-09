# Cargo workspace 组件化拆分计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 framework 单体 107K 行拆分为 ~15 个独立 crate，TCB 占比从 64% 降至 <30%

**Architecture:** 按子系统拆分，每个 crate 独立 `#![deny(unsafe_code)]` 边界，通过 Cargo workspace 管理依赖

**Tech Stack:** Rust (edition 2024), Cargo workspace, 依赖图重构

**Status:** 进行中 — Phase 1 已开始，core crate 已创建

## Global Constraints

- services 层 0 unsafe，所有 unsafe 操作委托至 framework API
- 中文注释强制
- 完成后在 kernel-roadmap.md 中标记 G1 状态为 [X]
- 每个 Phase 完成后必须双架构编译验证

## 当前进度

- [x] 创建 core crate (src/kernel/framework/core/)
- [ ] Phase 1: 拆分 timer/idt/klog/boot
- [ ] Phase 2: 拆分 sync/credo/barrier
- [ ] Phase 3: 拆分 mm/proc/fs/net/driver

---

## 拆分策略

### Phase 1: 低风险模块 (独立性强)
- `queenx-timer` (4K 行) — 定时器
- `queenx-idt` (4K 行) — 中断描述符表
- `queenx-klog` (1K 行) — 日志
- `queenx-boot` (1K 行) — 引导

### Phase 2: 中风险模块 (有依赖)
- `queenx-sync` (5K 行) — 同步原语
- `queenx-credo` (3K 行) — 身份权限
- `queenx-barrier` (3K 行) — 故障恢复

### Phase 3: 高风险模块 (核心依赖)
- `queenx-mm` (13K 行) — 内存管理
- `queenx-proc` (11K 行) — 进程管理
- `queenx-fs` (2K 行) — 文件系统
- `queenx-net` (5K 行) — 网络
- `queenx-driver` (21K 行) — 设备驱动

---

## Task 1: 创建 workspace 配置

**Covers:** workspace 结构

**Files:**
- Create: `src/kernel/Cargo.toml` (workspace root)
- Modify: `src/rust/Cargo.toml` (依赖 workspace)

**Interfaces:**
- Consumes: 无
- Produces: workspace 配置

- [ ] **Step 1: 创建 workspace Cargo.toml**

```toml
[workspace]
members = [
    "framework/timer",
    "framework/idt",
    "framework/klog",
    "framework/boot",
    "framework/sync",
    "framework/credo",
    "framework/barrier",
    "framework/mm",
    "framework/proc",
    "framework/fs",
    "framework/net",
    "framework/driver",
    "framework/arch",
    "framework/lib",
    "framework/config",
    "services/*",
]

[workspace.dependencies]
spin = "0.9"
bitflags = "2.4"
zerocopy = { version = "0.8", default-features = false, features = ["derive"] }
```

- [ ] **Step 2: 修改 src/rust/Cargo.toml**

```toml
[dependencies]
queenx-timer = { path = "../kernel/framework/timer" }
queenx-idt = { path = "../kernel/framework/idt" }
queenx-klog = { path = "../kernel/framework/klog" }
queenx-boot = { path = "../kernel/framework/boot" }
queenx-sync = { path = "../kernel/framework/sync" }
queenx-credo = { path = "../kernel/framework/credo" }
queenx-barrier = { path = "../kernel/framework/barrier" }
queenx-mm = { path = "../kernel/framework/mm" }
queenx-proc = { path = "../kernel/framework/proc" }
queenx-fs = { path = "../kernel/framework/fs" }
queenx-net = { path = "../kernel/framework/net" }
queenx-driver = { path = "../kernel/framework/driver" }
queenx-arch = { path = "../kernel/framework/arch" }
queenx-lib = { path = "../kernel/framework/lib" }
queenx-config = { path = "../kernel/framework/config" }
```

- [ ] **Step 3: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/Cargo.toml src/rust/Cargo.toml
git commit -m "refactor: 创建 workspace 配置"
```

---

## Task 2: 拆分 queenx-timer

**Covers:** Phase 1 低风险模块

**Files:**
- Create: `src/kernel/framework/timer/Cargo.toml`
- Move: `src/kernel/framework/timer/*.rs` → `src/kernel/framework/timer/src/`

**Interfaces:**
- Consumes: 无
- Produces: `queenx-timer` crate

- [ ] **Step 1: 创建 timer/Cargo.toml**

```toml
[package]
name = "queenx-timer"
version = "0.1.0"
edition = "2024"

[lib]
name = "queenx_timer"
crate-type = ["lib"]

[dependencies]
spin = "0.9"
```

- [ ] **Step 2: 移动源文件**

将 `src/kernel/framework/timer/*.rs` 移动到 `src/kernel/framework/timer/src/`

- [ ] **Step 3: 编译验证**

Run: `cargo check -p queenx-timer --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/timer/
git commit -m "refactor: 拆分 queenx-timer crate"
```

---

## Task 3: 拆分 queenx-idt

**Covers:** Phase 1 低风险模块

**Files:**
- Create: `src/kernel/framework/idt/Cargo.toml`
- Move: `src/kernel/framework/idt/*.rs` → `src/kernel/framework/idt/src/`

**Interfaces:**
- Consumes: `queenx-timer`
- Produces: `queenx-idt` crate

- [ ] **Step 1: 创建 idt/Cargo.toml**

```toml
[package]
name = "queenx-idt"
version = "0.1.0"
edition = "2024"

[lib]
name = "queenx_idt"
crate-type = ["lib"]

[dependencies]
queenx-timer = { path = "../timer" }
spin = "0.9"
```

- [ ] **Step 2: 移动源文件**

将 `src/kernel/framework/idt/*.rs` 移动到 `src/kernel/framework/idt/src/`

- [ ] **Step 3: 编译验证**

Run: `cargo check -p queenx-idt --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/idt/
git commit -m "refactor: 拆分 queenx-idt crate"
```

---

## Task 4: 拆分 queenx-klog

**Covers:** Phase 1 低风险模块

**Files:**
- Create: `src/kernel/framework/klog/Cargo.toml`
- Move: `src/kernel/framework/klog/*.rs` → `src/kernel/framework/klog/src/`

**Interfaces:**
- Consumes: 无
- Produces: `queenx-klog` crate

- [ ] **Step 1: 创建 klog/Cargo.toml**

```toml
[package]
name = "queenx-klog"
version = "0.1.0"
edition = "2024"

[lib]
name = "queenx_klog"
crate-type = ["lib"]

[dependencies]
spin = "0.9"
```

- [ ] **Step 2: 移动源文件**

将 `src/kernel/framework/klog/*.rs` 移动到 `src/kernel/framework/klog/src/`

- [ ] **Step 3: 编译验证**

Run: `cargo check -p queenx-klog --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/klog/
git commit -m "refactor: 拆分 queenx-klog crate"
```

---

## Task 5: 拆分 queenx-boot

**Covers:** Phase 1 低风险模块

**Files:**
- Create: `src/kernel/framework/boot/Cargo.toml`
- Move: `src/kernel/framework/boot/*.rs` → `src/kernel/framework/boot/src/`

**Interfaces:**
- Consumes: `queenx-klog`
- Produces: `queenx-boot` crate

- [ ] **Step 1: 创建 boot/Cargo.toml**

```toml
[package]
name = "queenx-boot"
version = "0.1.0"
edition = "2024"

[lib]
name = "queenx_boot"
crate-type = ["lib"]

[dependencies]
queenx-klog = { path = "../klog" }
```

- [ ] **Step 2: 移动源文件**

将 `src/kernel/framework/boot/*.rs` 移动到 `src/kernel/framework/boot/src/`

- [ ] **Step 3: 编译验证**

Run: `cargo check -p queenx-boot --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/boot/
git commit -m "refactor: 拆分 queenx-boot crate"
```

---

## Task 6: Phase 1 集成验证

**Covers:** Phase 1 完成

**Files:**
- 无新增修改

**Interfaces:**
- Consumes: 所有 Phase 1 crate
- Produces: 双架构编译通过

- [ ] **Step 1: x86_64 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 2: aarch64 编译验证**

Run: `cargo check --target aarch64-unknown-none`
Expected: PASS

- [ ] **Step 3: clippy 检查**

Run: `cargo clippy --target x86_64-unknown-none -- -D warnings`
Expected: PASS

- [ ] **Step 4: 运行所有测试**

Run: `cargo test -p host-tests`
Expected: PASS

- [ ] **Step 5: 更新 kernel-roadmap.md**

更新 G1 Phase 1 状态为 [X]

- [ ] **Step 6: Commit**

```bash
git add docs/plan/kernel-roadmap.md
git commit -m "refactor: Phase 1 完成 (timer/idt/klog/boot)"
```

---

## Task 7: 拆分 queenx-sync

**Covers:** Phase 2 中风险模块

**Files:**
- Create: `src/kernel/framework/sync/Cargo.toml`
- Move: `src/kernel/framework/sync/*.rs` → `src/kernel/framework/sync/src/`

**Interfaces:**
- Consumes: 无
- Produces: `queenx-sync` crate

- [ ] **Step 1: 创建 sync/Cargo.toml**

```toml
[package]
name = "queenx-sync"
version = "0.1.0"
edition = "2024"

[lib]
name = "queenx_sync"
crate-type = ["lib"]

[dependencies]
spin = "0.9"
bitflags = "2.4"
```

- [ ] **Step 2: 移动源文件**

将 `src/kernel/framework/sync/*.rs` 移动到 `src/kernel/framework/sync/src/`

- [ ] **Step 3: 编译验证**

Run: `cargo check -p queenx-sync --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/sync/
git commit -m "refactor: 拆分 queenx-sync crate"
```

---

## Task 8: 拆分 queenx-credo

**Covers:** Phase 2 中风险模块

**Files:**
- Create: `src/kernel/framework/credo/Cargo.toml`
- Move: `src/kernel/framework/credo/*.rs` → `src/kernel/framework/credo/src/`

**Interfaces:**
- Consumes: `queenx-sync`
- Produces: `queenx-credo` crate

- [ ] **Step 1: 创建 credo/Cargo.toml**

```toml
[package]
name = "queenx-credo"
version = "0.1.0"
edition = "2024"

[lib]
name = "queenx_credo"
crate-type = ["lib"]

[dependencies]
queenx-sync = { path = "../sync" }
spin = "0.9"
```

- [ ] **Step 2: 移动源文件**

将 `src/kernel/framework/credo/*.rs` 移动到 `src/kernel/framework/credo/src/`

- [ ] **Step 3: 编译验证**

Run: `cargo check -p queenx-credo --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/credo/
git commit -m "refactor: 拆分 queenx-credo crate"
```

---

## Task 9: 拆分 queenx-barrier

**Covers:** Phase 2 中风险模块

**Files:**
- Create: `src/kernel/framework/barrier/Cargo.toml`
- Move: `src/kernel/framework/barrier/*.rs` → `src/kernel/framework/barrier/src/`

**Interfaces:**
- Consumes: `queenx-sync`
- Produces: `queenx-barrier` crate

- [ ] **Step 1: 创建 barrier/Cargo.toml**

```toml
[package]
name = "queenx-barrier"
version = "0.1.0"
edition = "2024"

[lib]
name = "queenx_barrier"
crate-type = ["lib"]

[dependencies]
queenx-sync = { path = "../sync" }
spin = "0.9"
```

- [ ] **Step 2: 移动源文件**

将 `src/kernel/framework/barrier/*.rs` 移动到 `src/kernel/framework/barrier/src/`

- [ ] **Step 3: 编译验证**

Run: `cargo check -p queenx-barrier --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/barrier/
git commit -m "refactor: 拆分 queenx-barrier crate"
```

---

## Task 10: Phase 2 集成验证

**Covers:** Phase 2 完成

**Files:**
- 无新增修改

**Interfaces:**
- Consumes: 所有 Phase 1-2 crate
- Produces: 双架构编译通过

- [ ] **Step 1: x86_64 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 2: aarch64 编译验证**

Run: `cargo check --target aarch64-unknown-none`
Expected: PASS

- [ ] **Step 3: clippy 检查**

Run: `cargo clippy --target x86_64-unknown-none -- -D warnings`
Expected: PASS

- [ ] **Step 4: 运行所有测试**

Run: `cargo test -p host-tests`
Expected: PASS

- [ ] **Step 5: 更新 kernel-roadmap.md**

更新 G1 Phase 2 状态为 [X]

- [ ] **Step 6: Commit**

```bash
git add docs/plan/kernel-roadmap.md
git commit -m "refactor: Phase 2 完成 (sync/credo/barrier)"
```

---

## Task 11: 最终验证和提交

**Covers:** workspace 拆分完成

**Files:**
- 无新增修改

**Interfaces:**
- Consumes: 所有 crate
- Produces: workspace 结构完整

- [ ] **Step 1: 统计 TCB 占比**

Run: `python3 scripts/audit_tcb_ratio.py`
Expected: TCB < 30%

- [ ] **Step 2: 更新 kernel-roadmap.md**

更新 G1 状态为 [X]

- [ ] **Step 3: 推送到远程**

```bash
git push Gitee main
```