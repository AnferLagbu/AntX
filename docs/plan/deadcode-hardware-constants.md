# 硬件规范常量死代码消除方案

> 8 项 dead code 均为硬件规范常量。本方案逐一分析消除路径。

## 背景

- **来源**: deadcode-elimination-round2.md P0-P14 消除 129 项 + V1 占位清除 + vmm_switch_to_user 删除 + VGA crt cfg 修复, 共消除 131 项
- **剩余**: 8 项, 全为 `#[allow(dead_code)]` 标注的硬件规范常量或预留构造函数
- **目标**: 消除全部 8 项, 使 dead code audit 达到 0 violations (理想) 或仅剩无法消除的项
- **方案**: 按消除难度分三批: 可立即删除(冗余) / 可小规模激活 / 需功能集成

---

## Batch A: 冗余常量删除 (3 项)

> 这些常量在其他模块已有等价定义, 或其值已内嵌于类型常量中, 无需独立存在。

### A1. `DESC_VALID` (vmm_aarch64.rs:30)

- **值**: `1 << 0`
- **分析**: ARM 页表描述符的 valid bit (bit 0) 已内嵌于所有 `DESC_TYPE_*` 常量中 — `DESC_TYPE_TABLE = 0b11`, `DESC_TYPE_BLOCK = 0b01`, `DESC_TYPE_PAGE = 0b11` 的 bit 0 均为 1. 当前代码通过 `| DESC_TYPE_TABLE` 构造描述符 (line 171), valid bit 自动设置. 无任何代码路径需要单独的 `DESC_VALID`.
- **消除**: 删除 `DESC_VALID` 定义 + `#[allow(dead_code)]` 注解. 无需其他改动.
- **风险**: 低. 该常量从未被引用.
- **预估**: 1 行删除.

### A2. `MAIR_DEVICE_nGnRnE` (vmm_aarch64.rs:37)

- **值**: `0` (MAIR AttrIndx=0)
- **分析**: mmu.rs 中已有 `PT_ATTR_DEVICE = 0` (line 25, 注释: Device-nGnRnE memory, AttrIndx=0, MAIR[0]=0x44). 两者表达同一硬件语义. vmm_aarch64.rs 当前无设备内存映射路径 — 所有映射使用 `MAIR_NORMAL_WBWA` (AttrIndx=1). 若将来需要设备内存映射, 应在 mmu.rs 使用 `PT_ATTR_DEVICE`, 而非在 vmm_aarch64.rs 引入重复常量.
- **消除**: 删除 `MAIR_DEVICE_nGnRnE` 定义 + `#[allow(dead_code)]` 注解. 无需其他改动.
- **风险**: 低. 值为 0, 无代码引用.
- **预估**: 1 行删除.

### A3. `MAIR_NORMAL_NC` (vmm_aarch64.rs:40)

- **值**: `2` (MAIR AttrIndx=2)
- **分析**: 非缓存 Normal 内存属性. 当前 vmm_aarch64.rs 无任何路径使用此属性 — 所有内核映射用 WBWA (AttrIndx=1), 设备用 Device-nGnRnE (AttrIndx=0). 若将来需要非缓存映射 (DMA 一致性、设备共享内存), 需先在 `init_mair_el1()` 中配置 MAIR_EL1 的 AttrIndx=2 条目, 然后才能使用此常量. 当前 MAIR_EL1 初始化未配置 AttrIndx=2.
- **消除**: 删除 `MAIR_NORMAL_NC` 定义 + `#[allow(dead_code)]` 注解. 将来需要时在 `init_mair_el1()` 配置 AttrIndx=2 并重新定义.
- **风险**: 低. 无代码引用. 删除不丢失信息 (ARM 手册可查).
- **预估**: 1 行删除.

---

## Batch B: 跨模块重复常量整合 (1 项)

### B1. `PT_AP_ALL_RW` (aarch64/mmu.rs:28)

- **值**: `1 << 6` (AP[2:1] = 0b01, EL1+EL0 读写)
- **分析**: vmm_aarch64.rs 已有 `AP_BOTH_RW = 1 << 6` (line 45), 语义完全相同. 但 mmu.rs (`framework/arch/aarch64/`) 和 vmm_aarch64.rs (`framework/mm/`) 是不同模块, 不能直接引用对方的私有常量. mmu.rs 当前只映射内核 (EL1_RW), 用户页表映射尚未实装 — `PT_AP_ALL_RW` 是为用户页表预留.
- **消除方案 (选一)**:
  - **方案 1 (推荐)**: 在 `framework/mm/mod.rs` 或 `framework/arch/aarch64/mod.rs` 导出 `AP_BOTH_RW` 的 re-export, mmu.rs 通过 `crate::kernel::framework::mm::AP_BOTH_RW` 引用. 删除 mmu.rs 的 `PT_AP_ALL_RW`.
  - **方案 2**: 保留 `PT_AP_ALL_RW` 但在用户页表映射函数中使用它 (消除 dead code). 这要求实现 `map_user_page()` 类函数.
  - **方案 3**: 删除 `PT_AP_ALL_RW`, 将来实现用户页表时在函数内部定义.
- **风险**: 低. 当前无引用.
- **预估**: 方案 1 改动 2 文件 (mod.rs re-export + mmu.rs 删除). 方案 2 需实现用户页表映射.

---

## Batch C: 功能激活 (4 项)

> 这些常量/函数有明确的使用场景, 但对应功能路径尚未实装. 激活方案为添加最小化调用.

### C1. `IA32_U_CET` (shadow_stack.rs:63)

- **值**: `0x6A0` (Intel CET User Configuration MSR)
- **分析**: `IA32_S_CET` (内核态) 已在 `enable_kernel_shadow_stack()` 中通过 `write_msr()` 激活 (line 264-267). `IA32_U_CET` 是用户态对等 MSR, 需在用户线程启用 Shadow Stack 时编程. 当前 `create_user_shadow_stack()` (line 320-328) 仅返回描述符, 未写入 MSR.
- **激活**: 在 `create_user_shadow_stack()` 中, 当 `shadow_stack_enabled` 为 true 时, 调用 `write_msr(IA32_U_CET, 0x1)` 启用用户态 Shadow Stack (SH_STK_EN bit).
- **前置条件**: 无. `create_user_shadow_stack()` 已存在, 只需在返回前添加 MSR 写入.
- **风险**: 中. MSR 写入影响用户态安全. 需确保 Shadow Stack 物理页已分配 (当前 TODO). 建议在物理页分配就绪后再激活.
- **预估**: ~10 行代码 (cfg-guarded MSR write block).

### C2. `IA32_PL3_SSP` (shadow_stack.rs:68)

- **值**: `0x6A4` (Intel CET Ring 3 Shadow Stack Pointer)
- **分析**: `IA32_PL0_SSP` (内核态) 已在 `alloc_kernel_shadow_stack()` 中通过 `write_msr()` 激活 (line 308-312). `IA32_PL3_SSP` 是用户态对等 MSR, 需在用户线程启动时编程. 同样在 `create_user_shadow_stack()` 中激活.
- **激活**: 在 `create_user_shadow_stack()` 中, 当 `shadow_stack_enabled` 为 true 时, 调用 `write_msr(IA32_PL3_SSP, ss.get_ssp())`.
- **前置条件**: 同 C1, 需 Shadow Stack 物理页分配就绪.
- **风险**: 中. 同 C1.
- **预估**: 与 C1 合并实现, 无额外代码.

### C3. `IA32_INTERRUPT_SSP_TABLE` (shadow_stack.rs:71)

- **值**: `0x6A8` (Intel CET Interrupt Shadow Stack Table)
- **分析**: 此 MSR 指向一个表, 每个 IDT entry 对应一个 Shadow Stack 指针. 中断发生时 CPU 自动从此表加载对应 IST 的 Shadow Stack 指针. 当前 IDT 设置 (x86_64/idt.rs) 未集成此功能.
- **激活**: 添加 `configure_interrupt_ssp_table(table_phys_addr: u64)` 函数, 在 `cet_init()` 或 IDT 初始化路径中调用. 需分配 SSP Table 物理页并填充每个 entry 的 Shadow Stack 指针.
- **前置条件**: (1) 需要中断 Shadow Stack 物理页分配, (2) 需要 IDT entry 与 SSP table entry 的对应关系.
- **风险**: 中高. 中断路径安全性敏感. 建议作为独立子任务.
- **预估**: ~30 行 (新函数 + IDT 初始化集成).

### C4. `PerCpuGdt::new()` (gdt.rs:332)

- **值**: `const fn new() -> Self` — Per-CPU GDT 构造函数
- **分析**: 当前 `PER_CPU_GDT` 静态数组 (line 351-352) 使用 `MaybeUninit::uninit()`, 然后在 `gdt_init()` 中通过 `per_cpu_gdt_mut()` 直接填充. `PerCpuGdt::new()` 是一个更清晰的初始化路径 — 先构造默认值, 再修改特定字段.
- **激活**: 将 `PER_CPU_GDT` 静态数组初始化改为 `[const { MaybeUninit::new(PerCpuGdt::new()) }; PER_CPU_MAX]`, 并移除 `#[allow(dead_code)]` + 注释. `gdt_init()` 中的 `per_cpu_gdt_mut()` 调用无需改动 — 它们在已初始化的结构上修改字段.
- **前置条件**: 无. `PerCpuGdt::new()` 已是 `const fn`, 所有内部类型支持 const 构造.
- **风险**: 低. `new()` 返回全零值, 与 `MaybeUninit::uninit()` + 后续填充的效果等价.
- **预估**: 2 行改动 (static 初始化 + 移除 allow).

---

## 实施计划

| 批次 | 项 | 消除方式 | 预估 | 依赖 |
|------|-----|----------|------|------|
| A | A1 DESC_VALID | 删除 | 1 行 | 无 |
| A | A2 MAIR_DEVICE_nGnRnE | 删除 | 1 行 | 无 |
| A | A3 MAIR_NORMAL_NC | 删除 | 1 行 | 无 |
| B | B1 PT_AP_ALL_RW | 跨模块整合或删除 | 2-5 行 | 无 |
| C | C1+C2 IA32_U_CET + PL3_SSP | activate in create_user_shadow_stack | ~10 行 | Shadow Stack 物理页分配 |
| C | C3 INTERRUPT_SSP_TABLE | 新函数 + IDT 集成 | ~30 行 | SSP Table 物理页 + IDT 集成 |
| C | C4 PerCpuGdt::new() | 静态初始化 | 2 行 | 无 |

**建议实施顺序**: A → B → C4 → C1+C2 → C3

- **A+B+C4**: 可立即实施, 无外部依赖, 消除 5 项
- **C1+C2**: 可在 Shadow Stack 物理页分配就绪后实施
- **C3**: 最复杂, 可作为独立子任务

## 验证

每批实施后:
1. `./ci/build.sh all` — 双架构 0 error 0 warning
2. `python3 scripts/audit_dead_code.py` — 违规数递减
3. host-tests 全部通过
4. 若改动 framework 层: `audit_safety_coverage.py` + `audit_services_boundary.py` 通过
