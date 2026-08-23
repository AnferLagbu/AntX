# KPTI 完整化工程（原 B02-39，独立工程）

> 从 [audit-fix-02-framework-arch-asm.md](./audit-fix-02-framework-arch-asm.md) B02-39 擢升为独立工程（2026-08-21 用户决策）。
> 来源：审计附录 A F-04/F-08/F-10/F-16 + TOP 20 #3 + [code-audit-final-summary.md](./code-audit-final-summary.md)。
> 复核结论：两架构 KPTI 均为"半 KPTI"（页表名目隔离 + U/S 位权限，映射面未缩小），Meltdown 侧信道防御未实现。

## 工程计划 A: 现状与目标

### 背景

- **KPTI-01. 半 KPTI 证据（已复核）**
  - 描述：x86_64 `kpti.rs:333-338` `USER_PML4[256..512] = KERNEL_PML4[256..512]` 完整复制内核高半区；`kpti.rs:350-380` 整个 `.text` 映射进用户页表（PRESENT only）。aarch64 `kpti_aarch64.rs:158-167` `TRAMP_TTBR1` 复制完整 L0[256..511]；`arch/aarch64/mod.rs:272-310` `enter_user` 只切 TTBR0 未激活 trampoline。
  - 方案：本工程按 Phase 0-3 完整化两架构 KPTI 隔离。
  - 状态：[]

- **KPTI-02. 目标架构**
  - 描述：用户态运行的页表（x86 `USER_PML4` / 每进程用户页表；aarch64 `TRAMP_TTBR1`）只含：用户空间 + 异常/中断/syscall 入口 trampoline 代码 + 入口路径必需内核数据页（含内核栈首页）——不含其余内核 `.text`/`.data`/`.bss` 映射。
  - 方案：x86 收敛到 `_kernel_text_start ~ _kpti_trampoline_end`（链接脚本 [x86_64.ld](../../src/kernel/framework/link/x86_64.ld#L46-L56) 已划出该区域）；aarch64 收敛到异常向量表所在 L1 条目。
  - 状态：[]

### 前置调研（Phase 0）

- **KPTI-03. 入口路径内存依赖清单（Phase 0）**
  - 描述：枚举 x86_64（isr/irq/syscall/int 0x80）与 aarch64（EL0 sync/irq/svc）入口在 CR3/TTBR 切换前访问的全部代码页/数据页/栈页。**含 aarch64 用户态首个 SVC 陷入路径卡死定位**（来源：分册 2 B02-25 调研根因问题 1——init `_start` 第一动作 `print_char` = `fs_write` = `svc #0` 未返回，卡点在 handle_el0_sync → KERNEL_TTBR1 切换 → svc_handler 路径）。
  - 方案：逐入口静态分析汇编（isr.asm / mod.rs enter_user_asm / exception.rs global_asm），输出"入口 → 依赖内存"矩阵。已知起点：x86 依赖 USER_CR3_SAVE（.bss）+ SyscallPerCpu（per-CPU）+ GDT/IDT/TSS（用户态中断 CPU 硬件访问）+ TSS.RSP0/IST 内核栈页（CPU 在用户 CR3 下推帧）；aarch64 依赖异常向量表页 + KERNEL_TTBR1 数据页 + SP_EL1 内核栈页。
  - 状态：[]

### aarch64 完整化（Phase 1，近期）

- **KPTI-04. TRAMP_TTBR1 最小化**
  - 描述：`kpti_aarch64.rs:158-167` 当前复制完整 L0[256..511]，需改为仅映射异常向量表所在 L1 条目 + 入口代码页 + KERNEL_TTBR1 数据页 + SP_EL1 内核栈页。
  - 方案：基于 KPTI-03 依赖清单，`kpti_init` 重建 trampoline 页表；异常向量表位于 [exception.rs](../../src/kernel/framework/arch/aarch64/exception.rs#L46-L49) `.vectors` section。
  - 状态：[]

- **KPTI-05. enter_user 激活 trampoline**
  - 描述：[arch/aarch64/mod.rs:272-310](../../src/kernel/framework/arch/aarch64/mod.rs#L272-L310) `enter_user` 只切 TTBR0，TTBR1 保持完整内核页表，首次进入 EL0 无隔离。
  - 方案：eret 前调用 `kpti_exit_to_user()` 切换 TRAMP_TTBR1；`return_to_user` 确认一致。
  - 状态：[]

- **KPTI-06. aarch64 验证**
  - 描述：QEMU aarch64 启动 + EL0 往返 + 隔离断言。
  - 方案：`./scripts/qemu_boot_test.sh aarch64`；host-tests 断言 trampoline 页表内容（不含内核 `.text`/`.data`）；EL0 访问高半区应触发异常而非可读。
  - 状态：[]

### x86_64 完整化（Phase 2，中长期）

- **KPTI-07. .text 映射收窄到 trampoline 区域**
  - 描述：[kpti.rs:482-551](../../src/kernel/framework/mm/kpti.rs#L482-L551) `map_text_region_in_user_pml4` 当前映射 `_kernel_text_start ~ _kernel_text_end`（整个内核代码），应收窄到 `_kernel_text_start ~ _kpti_trampoline_end`（含 .kpti_trampoline + isr.o 全部入口代码，链接脚本已保证入口代码位于该区域）。
  - 方案：`kpti_init` 与 `create_user_page_table`（vmm_x86_64.rs:636）同步收窄；映射后断言其余内核代码页在用户页表中不存在。
  - 状态：[]

- **KPTI-08. USER_PML4 高半区复制移除**
  - 描述：`kpti.rs:333-338` 不再复制 `KERNEL_PML4[256..512]`，改为按 KPTI-03 依赖清单显式映射必需数据页（USER_CR3_SAVE、SyscallPerCpu、GDT/IDT/TSS、TSS.RSP0/IST 内核栈页）。
  - 方案：新增"必需内核页清单"集中管理（链接脚本符号 + 运行时枚举）；`kpti_sync_pml4_entry` 语义调整（高半区新增映射不再自动同步，改显式登记）。
  - 状态：[]

- **KPTI-09. x86_64 验证**
  - 描述：QEMU x86_64 Ring 3 + syscall/中断往返 + 隔离断言。
  - 方案：`./scripts/qemu_boot_test.sh x86_64`（含 Ring 3 到达，顺带闭合分册 2 B02-25）；host-tests 断言进程用户页表不含内核 `.text`/`.data` 映射。
  - 状态：[]
  - **前置依赖**：x86_64 到达 Ring 3 需 [x86-init-probe-project.md](./x86-init-probe-project.md)（X86IP-06）先解决 init_all 启动阻塞；Phase 1（aarch64）与 Phase 2 代码实施不受影响，仅 x86_64 侧 QEMU 验证被阻塞。

### 每进程一致性与强化验证（Phase 3）

- **KPTI-10. 每进程页表与共享模板统一**
  - 描述：[vmm_x86_64.rs:623-659](../../src/kernel/framework/mm/vmm_x86_64.rs#L623-L659) `create_user_page_table` 与 `kpti_init` 的映射逻辑保持同步（Phase 2 收窄后两者都只映射 trampoline + 必需数据页）。
  - 方案：抽公共函数；host-tests 对任意进程页表断言隔离属性。
  - 状态：[]

- **KPTI-11. 页表内容断言 host-tests**
  - 描述：当前无任何测试验证用户页表"不含内核映射"（分册 2 审查已指出 B02-39 仅表层检查）。
  - 方案：新增 host-tests 遍历用户页表（每进程 + 共享模板），断言高半区仅含 trampoline 区域与白名单数据页。
  - 状态：[]

- **KPTI-12. 完整回归 + 文档同步**
  - 描述：双架构 QEMU 完整回归（Ring 3 到达 + 用户态陷入/返回）+ docs 同步。
  - 方案：§2.3 门槛 + 专项 QEMU；本工程文档与分册 2 B02-39 状态联动更新。
  - 状态：[]

### 决策记录

- **DECISION-056**
  - 描述：工程分阶段：aarch64 先行（Phase 1，工程量小、机制可验证），x86_64 后行（Phase 2，依赖枚举复杂）。来源：2026-08-21 用户决策。
  - 状态：[X]

- **DECISION-057**
  - 描述：x86_64 采用"渐进收敛"而非一次性严格最小化：先收窄 `.text` 到 trampoline 区域（低风险，trampoline 区域已含全部入口代码），再按依赖清单逐项最小化数据页。降低漏映射 Triple Fault 风险。
  - 状态：[X]

### 验证标准

- §2.3 5 条门槛全过（双架构 cargo build / clippy / make / host-tests / QEMU）
- 专项：QEMU 双架构 + Ring 3 往返（补分册 2 B02-25）；页表内容 host-tests（KPTI-11）
- 隔离断言：用户态访问内核高半区（x86 高半区 VMA、aarch64 TTBR1 空间）触发异常而非可读

### 风险与回退

- **漏映射 Triple Fault**：x86 最小化漏掉入口依赖页 → 用户态首个中断即 Triple Fault。缓解：Phase 0 依赖清单 + Phase 2 渐进收敛 + QEMU 每步验证。
- **G 位/TLB 残留**（审计 F-16）：依赖无 G 位 + CR3 切换刷新；若其他路径设 G 位需一并清理（审计已识别 boot 路径）。
- **性能**：trampoline 页表缩小可能增加页表分配/切换成本；PCID（已启用）缓解。
- **回退**：`KernelCapabilities::kpti` 编译期开关可整关（kpti.rs:12 设计），任何阶段可回退到现状。
