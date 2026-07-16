# 硬件规范常量死代码消除计划

> 2026-07-14 最终轮次：8 项 `#[allow(dead_code)]` 均为硬件规范常量/函数预留。
> 通过实现对应平台特性来消除 `allow` 注解，使代码路径真正被使用。
>
> **前提**: 此前 dead code 消除工程已将违规数从 139 降至 8 (94.2%)。
> 本文档规划剩余 8 项的消除方案。

---

## 一、分类总览

> 2026-07-14 更新: 源码调研发现 3 项为冗余常量 (可直接删除), 无需功能激活. 总预估降至 3-5 天.

| # | 文件 | 常量/函数 | 平台 | 消除方式 | 预估工作量 |
|---|------|----------|------|---------|-----------|
| S1 | shadow_stack.rs:62 | `IA32_U_CET` (0x6A0) | x86_64 | 实现用户态 Shadow Stack MSR 配置 | 1-2 天 |
| S2 | shadow_stack.rs:67 | `IA32_PL3_SSP` (0x6A4) | x86_64 | 同上 (用户态 SSP 指针) | 与 S1 合并 |
| S3 | shadow_stack.rs:70 | `IA32_INTERRUPT_SSP_TABLE` (0x6A8) | x86_64 | 实现中断 Shadow Stack 表 | 1 天 |
| A1 | vmm_aarch64.rs:29 | `DESC_VALID` | aarch64 | **删除 (冗余)** — 已内嵌于 DESC_TYPE_* 的 bit 0 | 5 分钟 |
| A2 | vmm_aarch64.rs:36 | `MAIR_DEVICE_nGnRnE` | aarch64 | **删除 (冗余)** — 与 mmu.rs `PT_ATTR_DEVICE` 重复 | 5 分钟 |
| A3 | vmm_aarch64.rs:39 | `MAIR_NORMAL_NC` | aarch64 | **删除** — 无调用路径, 需时重新定义 | 5 分钟 |
| A4 | aarch64/mmu.rs:27 | `PT_AP_ALL_RW` | aarch64 | **删除或整合** — 与 vmm_aarch64.rs `AP_BOTH_RW` 重复 | 5 分钟 |
| G1 | gdt.rs:331 | `PerCpuGdt::new()` | x86_64 | **激活** — 静态初始化使用 `new()` | 10 分钟 |

**总预估**: Batch A (30 分钟) + Batch G (10 分钟) + Batch S (2-3 天) = 2.5-3.5 天

---

## 二、分组实施

### Batch A: 冗余常量删除 (3 项) — 立即可做

> 源码调研发现这 3 项是冗余定义, 可直接删除, 无需功能激活.

#### A1: `DESC_VALID` (vmm_aarch64.rs:30)

- **值**: `1 << 0`
- **分析**: ARM 页表描述符 valid bit 已内嵌于所有 `DESC_TYPE_*` 常量 — `DESC_TYPE_TABLE = 0b11`, `DESC_TYPE_BLOCK = 0b01`, `DESC_TYPE_PAGE = 0b11` 的 bit 0 均为 1. 当前代码通过 `| DESC_TYPE_TABLE` 构造描述符 (line 171), valid bit 自动设置. 无代码路径需要单独的 `DESC_VALID`.
- **消除**: 删除常量定义 + `#[allow(dead_code)]` 注解.

#### A2: `MAIR_DEVICE_nGnRnE` (vmm_aarch64.rs:37)

- **值**: `0`
- **分析**: mmu.rs 已有 `PT_ATTR_DEVICE = 0` (line 25, 注释: Device-nGnRnE memory, AttrIndx=0). 两者表达同一硬件语义. vmm_aarch64.rs 当前无设备内存映射路径, 若将来需要, 应在 mmu.rs 使用 `PT_ATTR_DEVICE`.
- **消除**: 删除常量定义 + `#[allow(dead_code)]` 注解.

#### A3: `MAIR_NORMAL_NC` (vmm_aarch64.rs:40)

- **值**: `2`
- **分析**: 非缓存 Normal 内存属性. 当前无路径使用, MAIR_EL1 初始化也未配置 AttrIndx=2. 若将来需要, 需先在 `init_mair_el1()` 中配置, 然后重新定义常量.
- **消除**: 删除常量定义 + `#[allow(dead_code)]` 注解.

---

### Batch B: 跨模块重复常量整合 (1 项) — 立即可做

#### B1: `PT_AP_ALL_RW` (aarch64/mmu.rs:28)

- **值**: `1 << 6`
- **分析**: vmm_aarch64.rs 已有 `AP_BOTH_RW = 1 << 6` (line 45), 语义完全相同. 但两模块不同, 不能直接引用. mmu.rs 当前只映射内核 (EL1_RW), 用户页表映射尚未实装.
- **消除 (选一)**:
  - **方案 1 (推荐)**: 删除 `PT_AP_ALL_RW`, 将来实现用户页表时在函数内部定义或从 vmm_aarch64 re-export.
  - **方案 2**: 在 mod.rs re-export `AP_BOTH_RW`, mmu.rs 通过 crate path 引用.

---

### Batch C: 功能激活 (4 项)

#### Group S: Intel CET 用户态 Shadow Stack (3 项) — 2-3 天

**目标**: 启用用户态 Shadow Stack.

**现状**:
- 内核态 Shadow Stack 已实现: `enable_kernel_shadow_stack()` 写 `IA32_S_CET` + `IA32_PL0_SSP`
- 用户态 Shadow Stack 仅创建描述符 (`create_user_shadow_stack`), 未配置 MSR

**方案**:
1. `create_user_shadow_stack` 改为通过 PMM 分配物理页 (当前 TODO), 写入 `IA32_U_CET` + `IA32_PL3_SSP`
2. 进程切换时保存/恢复这两个 MSR
3. `IA32_INTERRUPT_SSP_TABLE`: 新函数 + IDT 初始化集成

**依赖**: PMM 分配 (已有), 进程切换路径 (已有)
**风险**: QEMU 可能不支持 CET, 需回退路径 (已有 `try_write_cr4` 模式)

---

#### Group G: GDT `PerCpuGdt::new()` 激活 (1 项) — 10 分钟

**目标**: 使用 `PerCpuGdt::new()` 替代 `MaybeUninit::uninit()` 静态初始化.

**现状**:
- `PerCpuGdt::new()` 已是 `const fn`, 所有内部类型支持 const 构造
- 当前 `PER_CPU_GDT` 使用 `MaybeUninit::uninit()`, 后续由 `per_cpu_gdt_mut()` 填充

**方案**: 将静态数组改为 `[const { MaybeUninit::new(PerCpuGdt::new()) }; PER_CPU_MAX]`, 移除 `#[allow(dead_code)]`.

**风险**: 低 — `new()` 返回全零值, 与 `MaybeUninit::uninit()` + 后续填充等价.

---

## 三、实施顺序

| 阶段 | Group | 工作量 | 前置条件 |
|------|-------|--------|---------|
| Phase 1 | Batch A + B (删除冗余) | 30 分钟 | 无 — 立即可做 |
| Phase 2 | Group G (GDT) | 10 分钟 | 无 |
| Phase 3 | Group S (CET) | 2-3 天 | Shadow Stack 物理页分配 |

选择理由:
- Phase 1 最简单 (删除冗余), 立即可做
- Phase 2 最小 (1 项), 快速完成
- Phase 3 最复杂 (MSR + 进程切换 + PMM), 建议最后攻关

---

## 四、验证标准

每个 Phase 完成后:

1. 双架构编译 0 warning 0 error
2. `audit_dead_code.py` 违规数递减 (8 → 7 → ... → 0)
3. host-tests 通过
4. 相关功能可用 (CET 检测 / aarch64 映射 / GDT 加载)

---

## 五、最终目标

Phase 1-2 完成后, `audit_dead_code.py` 违规数降至 **4** (仅剩 CET 3 项 + GDT 1 项已激活).
Phase 3 完成后, 违规数降至 **0** — 内核无预留死代码, 所有硬件常量均有使用路径.

**当前进度**:
- Phase 1 (Batch A+B): [ ] 待实施 — 删除 4 项冗余常量
- Phase 2 (Group G): [ ] 待实施 — 激活 PerCpuGdt::new()
- Phase 3 (Group S): [ ] 待实施 — 用户态 Shadow Stack MSR 配置
