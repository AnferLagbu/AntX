# 多 IOAPIC 稳健实现计划

> **实施状态**: ✅ 已完成 (2026-07-22)
> **变更摘要**: 3 个 commit, ~200 行变更
> **验证结果**: clippy 0 warning, 双架构编译通过, 审计全 PASS (boundary/safety/deadlock/coupling/invariants), host-tests PASS
> **关联 commit**: 6756cdf5 (ACPI MADT), ed0a356a (IOAPIC driver), aab40532 (IDT + services)

> **目标**: 将 IOAPIC 驱动从单例模式重构为支持多 IOAPIC 控制器的稳健实现
>
> **前置条件**: MSI/MSI-X 中断基础设施已完成 (docs/plan/archive/2026-07-21-hardware-compatibility-tasks.md)
>
> **创建时间**: 2026-07-21

---

## 一、问题描述

当前 IOAPIC 驱动 (`framework/arch/x86_64/ioapic.rs`) 使用全局单例模式：

```rust
static IOAPIC_BASE: AtomicU64 = AtomicU64::new(0);       // 单一基址
static IOAPIC_INITIALIZED: AtomicBool = AtomicBool::new(false); // 单一初始化标志
static IOAPIC_MAX_IRQ: AtomicU64 = AtomicU64::new(0);    // 单一最大 IRQ 数
```

所有 MMIO 操作 (`ioapic_read`/`ioapic_write`) 直接使用 `IOAPIC_BASE`，无法区分多个 IOAPIC 控制器。

**影响**: 多路 CPU 服务器可能有 2-4 个 IOAPIC 控制器，当前实现只能使用最后一个被 ACPI MADT 枚举的 IOAPIC。

---

## 二、当前架构分析

### 2.1 调用链路

```
设备驱动 → IrqLine::enable(irq=5) → ioapic::unmask_irq(5)
                                    → 操作单一 IOAPIC_BASE 的重定向表 IRQ 5

IdtManager::enable_irq(irq=5) → ioapic::unmask_irq(5)
                               → 同上

IdtManager::handle_irq(vector=37) → irq = vector - IRQ_BASE = 37 - 32 = 5
                                   → handler(irq_descriptors[5])
                                   → dispatch_irq(vector) → ISR_TABLE[37]
```

### 2.2 缺失的映射层

| 映射 | 说明 | 当前状态 |
|------|------|----------|
| GSI → IOAPIC | 全局中断号到 IOAPIC 控制器的映射 | **缺失** |
| ISA IRQ → GSI | ISA 中断号到全局中断号的映射 (Interrupt Source Override) | **缺失** |
| IOAPIC 基址表 | 维护所有 IOAPIC 的 `{base_addr, id, gsi_base, max_irq}` | **缺失** |

### 2.3 ACPI MADT 解析现状

```rust
// acpi.rs 行 353-361
MADT_TYPE_IOAPIC => {
    let ioapic = unsafe { &*(offset as *const MadtIoApic) };
    IOAPIC_ADDR.store(ioapic.io_apic_addr as u64, Ordering::Release);   // 覆盖旧值
    IOAPIC_GSIB.store(ioapic.global_sys_int_base, Ordering::Release);   // 覆盖旧值
}
```

**问题**: `io_apic_id` 字段被丢弃，多个 IOAPIC 信息互相覆盖。

---

## 三、设计方案

### 3.1 数据结构

```rust
/// 单个 IOAPIC 控制器的描述信息
#[derive(Debug, Clone, Copy)]
pub struct IoApicInfo {
    /// IOAPIC 硬件 ID (MADT 中的 io_apic_id)
    pub id: u8,
    /// MMIO 基址 (从 MADT 解析)
    pub base_addr: u64,
    /// 全局系统中断基址 (GSI base)
    pub gsi_base: u32,
    /// 最大 IRQ 数 (从 IOAPICVER 寄存器读取)
    pub max_irq: u8,
}

/// IOAPIC 实例 (封装每个控制器的状态)
pub struct IoApic {
    info: IoApicInfo,
    initialized: bool,
}
```

### 3.2 全局状态

```rust
/// 所有 IOAPIC 控制器 (启动期由 ACPI MADT 填充)
static IOAPICS: SpinLock<[Option<IoApic>; MAX_IOAPICS]> = SpinLock::new([None; MAX_IOAPICS]);
static IOAPIC_COUNT: AtomicU32 = AtomicU32::new(0);

const MAX_IOAPICS: usize = 8;  // 支持最多 8 个 IOAPIC
```

### 3.3 GSI 路由

```rust
/// GSI → IOAPIC 路由表
///
/// 每个 IOAPIC 的 GSI 范围: [gsi_base, gsi_base + max_irq)
/// 查找时遍历所有 IOAPIC，找到 GSI 落在哪个范围内
fn gsi_to_ioapic(gsi: u32) -> Option<(usize, u8)> {
    let ioapics = IOAPICS.lock();
    for (i, ioapic) in ioapics.iter().enumerate() {
        if let Some(ref info) = ioapic {
            if gsi >= info.gsi_base && gsi < info.gsi_base + info.max_irq as u32 {
                return Some((i, (gsi - info.gsi_base) as u8));
            }
        }
    }
    None
}
```

### 3.4 API 变更

**当前 API (单例)**:
```rust
pub fn set_irq(irq: u8, vector: u8, apic_id: u8, masked: bool)
pub fn mask_irq(irq: u8)
pub fn unmask_irq(irq: u8)
```

**新 API (多 IOAPIC)**:
```rust
/// 按 GSI 设置 IRQ (自动路由到正确的 IOAPIC)
pub fn set_irq(gsi: u32, vector: u8, apic_id: u8, masked: bool)
pub fn mask_irq(gsi: u32)
pub fn unmask_irq(gsi: u32)

/// 按 IOAPIC 索引 + 本地 IRQ 设置
pub fn set_irq_on(ioapic_idx: usize, local_irq: u8, vector: u8, apic_id: u8, masked: bool)
pub fn mask_irq_on(ioapic_idx: usize, local_irq: u8)
pub fn unmask_irq_on(ioapic_idx: usize, local_irq: u8)
```

---

## 四、实施任务

### Task 1: ACPI MADT 解析重构 **状态: [X]** (commit 6756cdf5)

**文件**: `framework/arch/x86_64/acpi.rs`

**改动**:
1. 添加 `IoApicInfo` 结构体
2. 将 `IOAPIC_ADDR`/`IOAPIC_GSIB` 替换为 `Vec<IoApicInfo>`
3. 在 MADT 解析中收集所有 IOAPIC 条目 (保留 `io_apic_id`)
4. 实现 `_MADT_TYPE_ISO` (Interrupt Source Override) 解析
5. 更新 `get_ioapic_addr()`/`get_ioapic_gsib()` 为返回列表的 API

**验证**: 双架构编译 0 warning 0 error

---

### Task 2: IOAPIC 驱动重构 **状态: [X]** (commit ed0a356a)

**文件**: `framework/arch/x86_64/ioapic.rs`

**改动**:
1. 移除全局单例 `IOAPIC_BASE`/`IOAPIC_INITIALIZED`/`IOAPIC_MAX_IRQ`
2. 实现 `IoApic` 结构体，封装 per-IOAPIC 状态
3. 添加 `ioapic_read_on(base, reg)` / `ioapic_write_on(base, reg, val)` 参数化函数
4. 重构公共 API 支持 GSI 参数
5. 保持 `extern "C"` FFI 接口向后兼容

**验证**: 双架构编译 0 warning 0 error

---

### Task 3: IrqLine 适配 **状态: [X]** (commit aab40532)

**文件**: `framework/irqline.rs`

**改动**:
1. `IrqLine` 结构体增加 `gsi: u32` 字段 (替代或补充 `irq: u32`)
2. `enable()`/`disable()` 调用新的 GSI 路由 API
3. 保持旧 API 兼容 (默认 GSI = IRQ)

**验证**: 双架构编译 0 warning 0 error

---

### Task 4: IDT 管理器适配 **状态: [X]** (commit aab40532)

**文件**: `framework/idt/idt.rs`

**改动**:
1. `enable_irq()` 移除 `irq >= 16` 硬编码限制
2. `disable_irq()` 增加 IOAPIC 路径
3. `handle_irq()` 的 IRQ 编号范围扩展

**验证**: 双架构编译 0 warning 0 error + host-tests 通过

---

### Task 5: Services 层适配 **状态: [X]** (commit aab40532)

**文件**: `services/driver/acpi.rs`

**改动**:
1. `ioapic_addr()`/`ioapic_gsib()` 改为返回列表接口
2. 添加 `ioapic_count()` 查询函数

**验证**: 双架构编译 0 warning 0 error

---

### Task 6: 全量验证 **状态: [X]** (2026-07-22, clippy 0W / 双架构 / 5 审计 PASS / host-tests PASS)

**Steps**:
1. 双架构编译 0 warning 0 error
2. 全量审计通过
3. host-tests 通过
4. QEMU 启动测试通过

---

## 五、风险评估

| 风险 | 等级 | 缓解措施 |
|------|------|----------|
| IRQ 号全局/本地映射错误 | 高 | GSI 路由表单元测试 + QEMU 验证 |
| 现有驱动兼容性破坏 | 中 | 保持旧 API 兼容 (默认 GSI = IRQ) |
| 多 IOAPIC 无法 QEMU 验证 | 低 | 单 IOAPIC 场景完整测试 |
| 引入新 unsafe | 低 | 仅在 IOAPIC MMIO 操作中，已有 SAFETY 注释 |

---

## 六、依赖关系

```
Task 1 (ACPI) ──→ Task 2 (IOAPIC) ──→ Task 3 (IrqLine)
                                    ──→ Task 4 (IDT)
                                    ──→ Task 5 (Services)
                                         ↓
                                    Task 6 (验证)
```

---

## 七、预估工作量

| Task | 工作量 |
|------|--------|
| Task 1: ACPI MADT | ~50 行 |
| Task 2: IOAPIC 驱动 | ~150 行 |
| Task 3: IrqLine | ~30 行 |
| Task 4: IDT | ~40 行 |
| Task 5: Services | ~20 行 |
| Task 6: 验证 | ~30 行 |
| **总计** | **~320 行** |
