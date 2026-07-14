# 代码审查问题修复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复代码审查发现的 6 个问题：死代码清理 (4 项)、kernel_test stub 类型不匹配 (1 项)、VMM/系统调用分发文件过大 (1 项为建议性重构，本期不实施)

**Architecture:** 逐项修复，每项独立验证。修复范围限于确认的死代码和明确 bug，不涉及架构重构。

**Tech Stack:** Rust, QueenX Framekernel audit scripts

## Global Constraints

- 双架构编译 0 warning 0 error (`./ci/build.sh all`)
- 审计全部通过 (`ci/audit.sh`)
- host-tests 全部通过 (`make test-host`)
- 中文注释强制
- framework `unsafe` 块必须配 `// SAFETY:` 注释

---

## Task 1: 修复 Mutex timeout 硬编码 TSC 频率

**问题:** `mutex.rs:136` 硬编码 `2400000` cycles/ms，且 `lock_timeout` 是死代码 (零调用者)。

**决策:** 移除 `lock_timeout` 方法和 FFI 导出 `mutex_lock_timeout`。理由：
1. 零调用者 — 项目中无任何代码使用
2. 硬编码 TSC 频率在 ARM/低功耗 CPU/非 invariant TSC VM 上完全错误
3. FFI 导出 `mutex_lock_timeout` (`sync/mod.rs:530`) 是空壳 stub (忽略 timeout 参数)

**Files:**
- Modify: `src/kernel/framework/sync/mutex.rs` (移除 `lock_timeout` 方法, lines 122-160)
- Modify: `src/kernel/framework/sync/mod.rs` (移除 `mutex_lock_timeout` FFI 导出, lines ~525-540)

**Interfaces:**
- Consumes: 无
- Produces: 移除 `Mutex::lock_timeout()` 和 `mutex_lock_timeout()` FFI

**Steps:**

- [ ] **Step 1: 读取 mutex.rs 确认移除范围**

确认 `lock_timeout` 方法的精确行范围 (约 lines 122-160)，以及 `rdtsc()` 辅助函数是否仅被 `lock_timeout` 使用。

- [ ] **Step 2: 读取 sync/mod.rs 确认 FFI 移除范围**

确认 `mutex_lock_timeout` FFI 导出的精确行范围。

- [ ] **Step 3: 移除 mutex.rs 中的 lock_timeout 方法**

```rust
// 移除 lines 122-160 的 lock_timeout 方法
// 如果 rdtsc() 仅被 lock_timeout 使用, 也一并移除
```

- [ ] **Step 4: 移除 sync/mod.rs 中的 mutex_lock_timeout FFI**

```rust
// 移除 #[unsafe(no_mangle)] pub extern "C" fn mutex_lock_timeout(...)
```

- [ ] **Step 5: 验证编译**

```bash
./ci/build.sh x86_64
```
Expected: 0 error, 0 warning

- [ ] **Step 6: 运行审计**

```bash
python3 scripts/audit_services_boundary.py
python3 scripts/audit_safety_coverage.py
python3 scripts/audit_dead_code.py
```
Expected: 全部 PASS

---

## Task 2: 移除 Mutex 死代码 get()/get_mut()

**问题:** `mutex.rs:82-92` 的 `unsafe fn get()` 和 `unsafe fn get_mut()` 是死代码 (零调用者，仅在 doc comment 中引用)。

**Files:**
- Modify: `src/kernel/framework/sync/mutex.rs` (移除 lines 78-92)

**Interfaces:**
- Consumes: 无
- Produces: 移除 `Mutex::get()` 和 `Mutex::get_mut()`

**Steps:**

- [ ] **Step 1: 读取 mutex.rs 确认移除范围**

确认 `get()` 和 `get_mut()` 方法的精确行范围，以及 CondVar doc comment 中的引用是否需要更新。

- [ ] **Step 2: 移除 get() 和 get_mut() 方法**

移除 lines 78-92 (包括 doc comment)。

- [ ] **Step 3: 更新 CondVar doc comment**

如果 CondVar 的 doc comment 示例引用了 `mutex.get()` / `mutex.get_mut()`，更新为使用 `lock()` guard 模式。

- [ ] **Step 4: 验证编译**

```bash
./ci/build.sh x86_64
```

- [ ] **Step 5: 运行审计**

```bash
python3 scripts/audit_dead_code.py
```

---

## Task 3: 修复 kernel_test InitState stub 类型不匹配

**问题:** `services/net/mod.rs:30-38` 的 stub `InitState::Failed` 隐式值为 `4`，而真实 `framework/net/init.rs:34-42` 的 `Failed = 255`。如果测试代码序列化 discriminant 值，行为会不一致。

**Files:**
- Modify: `src/kernel/services/net/mod.rs` (修复 stub InitState, lines 30-38)

**Interfaces:**
- Consumes: 无
- Produces: stub `InitState::Failed` discriminant 与真实值一致 (= 255)

**Steps:**

- [ ] **Step 1: 读取真实 InitState 定义**

确认 `framework/net/init.rs` 中 `InitState` 的所有 variant 和 discriminant 值。

- [ ] **Step 2: 读取 stub InitState 定义**

确认 `services/net/mod.rs` 中 stub 的精确内容。

- [ ] **Step 3: 修复 stub 的 Failed discriminant**

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitState {
    Uninitialized = 0,
    HardwareProbed = 1,
    InterfaceReady = 2,
    FullyInitialized = 3,
    Failed = 255,  // 必须与 framework/net/init.rs 一致
}
```

- [ ] **Step 4: 验证编译**

```bash
./ci/build.sh x86_64
```

- [ ] **Step 5: 运行 host-tests**

```bash
make test-host
```

---

## Task 4: 移除 PMM FreeNodeRef 死代码

**问题:** `pmm.rs:115,121` 的 `FreeNodeRef::is_null()` 和 `FreeNodeRef::as_ptr()` 标注 `#[allow(dead_code)]` 但零调用者。

**Files:**
- Modify: `src/kernel/framework/mm/pmm.rs` (移除 is_null 和 as_ptr, lines 114-124)

**Interfaces:**
- Consumes: 无
- Produces: 移除 `FreeNodeRef::is_null()` 和 `FreeNodeRef::as_ptr()`

**Steps:**

- [ ] **Step 1: 读取 pmm.rs 确认移除范围**

确认 `is_null()` 和 `as_ptr()` 的精确行范围。

- [ ] **Step 2: 确认无调用者**

grep 确认 `FreeNodeRef.*is_null` 和 `FreeNodeRef.*as_ptr` 无调用。

- [ ] **Step 3: 移除死代码方法**

移除 lines 114-124 (is_null 和 as_ptr)。

- [ ] **Step 4: 验证编译**

```bash
./ci/build.sh x86_64
```

- [ ] **Step 5: 运行审计**

```bash
python3 scripts/audit_dead_code.py
```

---

## Task 5: 全量验证

**Steps:**

- [ ] **Step 1: 双架构编译**

```bash
./ci/build.sh all
```
Expected: 0 error, 0 warning

- [ ] **Step 2: 全量审计**

```bash
ci/audit.sh
```

- [ ] **Step 3: host-tests**

```bash
make test-host
```

- [ ] **Step 4: 记录结果**

所有验证通过后标记完成。

---

## 建议性重构 (本期不实施)

| # | 问题 | 建议 | 理由 |
|---|------|------|------|
| 3 | VMM 文件 1555 行 | 拆分页表遍历 vs VMM 管理 | 大型重构，需独立 PR |
| 4 | 系统调用分发单 match 347 行 | 按子系统拆分分发表 | 大型重构，需独立 PR |
