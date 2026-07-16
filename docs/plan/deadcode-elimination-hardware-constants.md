# 硬件规范常量死代码消除计划

> 2026-07-14 最终轮次：8 项 `#[allow(dead_code)]` 均为硬件规范常量/函数预留。
> 通过实现对应平台特性来消除 `allow` 注解，使代码路径真正被使用。
>
> **前提**: 此前 dead code 消除工程已将违规数从 139 降至 8 (94.2%)。
> 本文档规划剩余 8 项的消除方案。

---

## 一、分类总览

| # | 文件 | 常量/函数 | 平台 | 消除方式 | 预估工作量 |
|---|------|----------|------|---------|-----------|
| S1 | shadow_stack.rs:62 | `IA32_U_CET` (0x6A0) | x86_64 | 实现用户态 Shadow Stack MSR 配置 | 1-2 天 |
| S2 | shadow_stack.rs:67 | `IA32_PL3_SSP` (0x6A4) | x86_64 | 同上 (用户态 SSP 指针) | 与 S1 合并 |
| S3 | shadow_stack.rs:70 | `IA32_INTERRUPT_SSP_TABLE` (0x6A8) | x86_64 | 实现中断 Shadow Stack 表 | 1 天 |
| A1 | vmm_aarch64.rs:29 | `DESC_VALID` | aarch64 | 实现页表诊断路径 | 0.5 天 |
| A2 | vmm_aarch64.rs:36 | `MAIR_DEVICE_nGnRnE` | aarch64 | 实现设备内存映射路径 | 0.5 天 |
| A3 | vmm_aarch64.rs:39 | `MAIR_NORMAL_NC` | aarch64 | 实现非缓存内存映射路径 | 0.5 天 |
| A4 | aarch64/mmu.rs:27 | `PT_AP_ALL_RW` | aarch64 | 实现用户态页映射路径 | 1 天 |
| G1 | gdt.rs:331 | `PerCpuGdt::new()` | x86_64 | GDT 动态分配重构 | 1-2 天 |

**总预估**: 5-7 天

---

## 二、分组实施

### Group S: Intel CET 用户态 Shadow Stack (3 项)

**目标**: 启用用户态 Shadow Stack，消除 `IA32_U_CET`、`IA32_PL3_SSP`、`IA32_INTERRUPT_SSP_TABLE` 三项 dead code。

**现状**:
- 内核态 Shadow Stack 已实现: `enable_kernel_shadow_stack()` 写 `IA32_S_CET` + `IA32_PL0_SSP`
- 用户态 Shadow Stack 仅创建描述符 (`create_user_shadow_stack`)，未配置 MSR
- `create_user_shadow_stack` 返回 `ShadowStack::new(0, size)` — base=0 表示未实际分配

**方案**:

1. **分配用户态 Shadow Stack 内存**:
   - `create_user_shadow_stack` 改为通过 PMM 分配实际物理页 (当前有 TODO 注释)
   - 返回有效的 base 地址

2. **进程创建/切换时配置用户态 MSR**:
   - 在进程创建路径 (`UserProcManager::enter` 或 ELF 加载完成后) 写入:
     - `IA32_U_CET`: 用户态 CET 配置 (SH_STK_EN=1)
     - `IA32_PL3_SSP`: 用户态 Shadow Stack 指针 (ShadowStack.base + ShadowStack.size)
   - 在进程切换时 (`context_switch`) 保存/恢复这两个 MSR

3. **中断 Shadow Stack 表**:
   - 分配 N 个 Shadow Stack 用于 IST 1-7
   - 写入 `IA32_INTERRUPT_SSP_TABLE` MSR 指向该表
   - 在中断入口/出口切换 IST 指针

**涉及文件**:
- `framework/arch/shadow_stack.rs` — 核心实现
- `framework/proc/user_proc.rs` — 进程创建集成
- `framework/proc/api.rs` — 进程切换集成

**依赖**: PMM 分配 (已有), 进程切换路径 (已有)

**风险**: QEMU 可能不支持 CET, 需回退路径 (已有 `try_write_cr4` 模式)

---

### Group A: aarch64 MMU 用户态/设备映射 (4 项)

**目标**: 实现 aarch64 用户态页映射和设备/非缓存内存映射路径，消除 4 项 dead code。

**现状**:
- `vmm_aarch64.rs` 已有完整的 4 级页表管理 (L0-L3)
- `AP_EL1_RW` (EL1 读写) 和 `AP_BOTH_RW` (EL1+EL0 读写) 已定义
- `PT_AP_ALL_RW` 定义在 `mmu.rs` 但未使用
- `MAIR_DEVICE_nGnRnE` 和 `MAIR_NORMAL_NC` 定义在 `vmm_aarch64.rs` 但未使用
- `DESC_VALID` 定义但仅用于诊断

**方案**:

#### A1: `DESC_VALID` — 页表诊断路径

- 实现 `vmm_dump_page_table(vaddr: u64)` 函数, 遍历 L0→L3 打印每级描述符
- 使用 `DESC_VALID` 检查描述符有效位
- 集成到 `/proc/[pid]/page_tables` 或 debug 命令

#### A2+A3: `MAIR_DEVICE_nGnRnE` + `MAIR_NORMAL_NC` — 设备/非缓存映射

- 实现 `vmm_map_device(phys: u64, size: usize)` 函数, 使用 `MAIR_DEVICE_nGnRnE` 属性映射 MMIO 区域
- 实现 `vmm_map_noncacheable(phys: u64, size: usize)` 函数, 使用 `MAIR_NORMAL_NC` 属性映射 DMA 缓冲区
- 集成到设备驱动 MMIO 映射路径

#### A4: `PT_AP_ALL_RW` — 用户态页映射

- 实现 `vmm_map_user_page(table: &mut [u64; 512], vaddr: u64, paddr: u64)` 函数
- 使用 `PT_AP_ALL_RW` 设置 EL0 可读写权限
- 集成到 `vmm_create_user_page_table` 用户态映射路径

**涉及文件**:
- `framework/mm/vmm_aarch64.rs` — 核心实现
- `framework/arch/aarch64/mmu.rs` — mmu.rs 中的用户态映射

**依赖**: 无 (基础设施已就绪)

**风险**: 低 — 所有常量和基础设施已定义, 仅需编写使用路径

---

### Group G: GDT 动态分配重构 (1 项)

**目标**: 使用 `PerCpuGdt::new()` 重构 GDT 初始化路径，消除 dead code。

**现状**:
- `PerCpuGdt::new()` 已定义但未调用 (dead code)
- 当前 GDT 初始化通过 `per_cpu_gdt_mut` 直接构造结构体字段
- `PerCpuGdt` 有 `entries`/`ptr`/`tss`/`syscall`/`syscall_stack`/`ist0-3` 等字段

**方案**:

- 重构 per-CPU GDT 初始化: 将直接字段赋值替换为 `PerCpuGdt::new()` + 按需配置
- 保持 GDT 加载路径不变 (`lgdt`, `lidt` 等)
- 确保 TSS/IST 配置在 `new()` 之后正确设置

**涉及文件**:
- `framework/arch/x86_64/gdt.rs` — 核心重构

**依赖**: 无

**风险**: 低 — 仅初始化路径重构, 不影响运行时行为

---

## 三、实施顺序

| 阶段 | Group | 工作量 | 前置条件 |
|------|-------|--------|---------|
| Phase 1 | Group A (ARM MMU) | 2.5 天 | 无 — 最独立, 可立即开始 |
| Phase 2 | Group G (GDT) | 1-2 天 | 无 — 与 Phase 1 无关 |
| Phase 3 | Group S (CET) | 2-3 天 | 无 — 但复杂度最高, 建议最后做 |

选择理由:
- Group A 最独立 (4 项, aarch64 平台), 且基础设施完备
- Group G 最小 (1 项, x86_64), 可快速完成
- Group S 最复杂 (3 项, 涉及 MSR 写入 + 进程切换 + PMM), 建议最后集中攻关

---

## 四、验证标准

每个 Phase 完成后:

1. 双架构编译 0 warning 0 error
2. `audit_dead_code.py` 违规数递减 (8 → 7 → ... → 0)
3. host-tests 通过
4. 相关功能可用 (CET 检测 / aarch64 映射 / GDT 加载)

---

## 五、最终目标

所有 8 项消除后, `audit_dead_code.py` 违规数降至 **0** — 内核无预留死代码, 所有硬件常量均有使用路径。
