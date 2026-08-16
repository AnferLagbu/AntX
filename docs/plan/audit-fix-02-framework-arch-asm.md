# 审计修复分册 02：framework 架构与汇编

> 修复 framework/arch（x86_64 + aarch64）、boot 汇编、链接脚本与用户态入口的缺陷，含附录 A 汇编/链接脚本 16 项 + P0-16 + TOP 20 架构相关项。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 附录 A + 第 3.4 节 + 第 7 章。

## 工程计划 A: 汇编与链接脚本修复

### 背景

- **汇编/链接脚本为独立深审域**
  - 描述：附录 A 对 boot 汇编、arch 汇编、x86_64.ld / link.x 逐行审计，产出 16 项缺陷（2 项 C0 + 7 项 H + 5 项 M + 2 项 L）与 5 项关注项。
  - 方案：按 C0/H/M/L 顺序修复，每项改动后跑 QEMU 双架构启动测试。
  - 状态：[]

- **架构安全不变式关联**
  - 描述：I1（CPU 状态保护）/I3（用户态 CPU 状态经 framework）直接依赖本分册修复；aarch64 KPTI 不完整属 Meltdown 级风险（TOP 20 #3）。
  - 方案：KPTI 相关项（F-04/F-08/F-10）单开 PR 处理。
  - 状态：[]

### 待办

- **F-01 trampoline.asm SINFO 布局一致性（C0）**
  - 描述：`trampoline.asm` 的 SINFO 字段布局与 Rust 端 `ApStartupInfo` 字节序不一致，trampoline magic 偏移脆弱。
  - 方案：对齐两端的字段序/字节序；加编译期布局断言或汇编侧注释校验。
  - 状态：[]

- **F-02 isr.asm USER_CR3_SAVE 段归属（C0）**
  - 描述：`USER_CR3_SAVE` 定义在 `.bss` 但段切换在 `.text` 中段且声明 `extern`，布局假设是 LMA 直接地址。
  - 方案：核对链接脚本中 `.bss` 的 LMA/VMA；必要时改为显式段声明。
  - 状态：[]

- **F-03 x86_64.ld _kernel_size 计算基（H）**
  - 描述：`x86_64.ld` 的 `_kernel_size` 基于 VMA 计算，但应基于 LMA（与 aarch64 不一致）。
  - 方案：改为 LMA 计算；补双架构一致性测试。
  - 状态：[]

- **F-04 用户态 link.x KPTI 布局（H）**
  - 描述：用户态链接脚本无 KPTI 兼容布局，用户进程入口无 `__entry` 符号对齐保证。
  - 方案：在 `.text` 起始加 `_user_start = .;`、`.bss` 结束处 `_user_end = .;`；保证入口对齐。
  - 状态：[]

- **F-05 isr.asm 入口寄存器破坏 + swapgs 时序（H）**
  - 描述：入口寄存器破坏与 swapgs 时序存在双重诊断痕迹（已显式标注但未清理）。
  - 方案：清理诊断 push/pop 序列；用 `#[cfg(feature = "debug_isr")]` 隔离（见 P0-16）。
  - 状态：[]

- **F-06 aarch64 start.S EL 阶段配置（H）**
  - 描述：EL3→EL2→EL1 转换未配置 MAIR_EL1 / TCR_EL1 的 EL2 阶段，`eret` 后 EL1 处于未知状态。
  - 方案：在降级为 EL1 前于 EL2 配置 MAIR_EL1/TCR_EL1 影子寄存器。
  - 状态：[]

- **F-07 aarch64 context.rs eret 前 isb（H）**
  - 描述：上下文切换 `eret` 前未 `isb` 同步 SPSR/ELR 写入。
  - 方案：`eret` 前加 `isb`（或 `dsb + isb`）。
  - 状态：[]

- **F-08 aarch64 exception.rs KPTI 完整化（H）**
  - 描述：EL0 IRQ/SVC handler 缺 TTBR0 切换，KPTI 不完整（TOP 20 #3，Meltdown 可攻击）。
  - 方案：单开 PR：handler 入口切 KERNEL_TTBR1，出口恢复用户 TTBR0 + ASID。
  - 状态：[]

- **F-09 enter_user_asm 段寄存器与 swapgs 顺序（H）**
  - 描述：`arch/x86_64/mod.rs` `enter_user_asm` 段寄存器加载与 swapgs 顺序逻辑依赖注释不充分。
  - 方案：重写顺序说明注释 + 补充屏障；与 F-16 一并处理。
  - 状态：[]

- **F-10 proc/switch.asm KPTI 兼容（M）**
  - 描述：`process_switch_asm` 缺 KPTI 兼容处理（CR3 切换不在 KPTI trampoline 区）。
  - 方案：进程切换 CR3 走 KPTI trampoline 路径。
  - 状态：[]

- **F-11 aarch64 mmu.rs SCTLR_EL1 完整化（M）**
  - 描述：`enable_mmu` 启用 C/I cache，但 `init()` 中 SCTLR_EL1 处理不完整。
  - 方案：补全 SCTLR_EL1 各字段初始化顺序。
  - 状态：[]

- **F-12 smp_init.rs start_ap 锁与 cli 顺序（M）**
  - 描述：`start_ap` 无 `lock` 注解，`cli` 顺序与 `AP_STARTUP_LOCK` 顺序冲突。
  - 方案：为 `start_ap` 加 `#[lock]` 注解或文档说明；统一 cli/加锁顺序。
  - 状态：[]

- **F-13 GDT_SYSRET 选择子布局同步（M）**
  - 描述：`GDT_SYSRET` 选择子布局 `0x18|3`/`0x20|3`，但汇编 `enter_user_asm` push `0x1B/0x23`，未与 GDT 同步。
  - 方案：统一选择子定义来源（汇编侧用宏引用 GDT 偏移）。
  - 状态：[]

- **F-14 aarch64 interrupt_restore 恢复 D/A/F 位（M）**
  - 描述：`interrupt_restore` 不恢复 D/A/F 位，与 x86_64 对称性问题。
  - 方案：补 SPSR D/A/F 位恢复逻辑。
  - 状态：[]

- **F-15 stage1.asm Multiboot2 校验和（L）**
  - 描述：`boot/stage1.asm` Multiboot2 信息手工组装无校验和验证。
  - 方案：加组装后校验和断言；与 P0-18（stage1.bin 全 0）联动核实产物。
  - 状态：[]

- **F-16 enter_user_asm wbinvd/屏障 + TLB flush（H）**
  - 描述：`enter_user_asm` 缺 `swapgs` 与 `iretq` 之间的 `wbinvd`/屏障，CR3 切换未 flush TLB。
  - 方案：按实际 KPTI 需求补 `mov cr3` 后屏障与 TLB flush 策略。
  - 状态：[]

- **P0-16 isr.asm 诊断代码隔离**
  - 描述：[isr.asm:50-198](file:///home/anfer/Code/QueenX/src/kernel/framework/boot/isr.asm) 每个 IRQ stub 插入 `mov dx, 0x3F8; mov al, 0x5A; out dx, al` 诊断序列，污染中断入口；`isr_common` 约 130 行诊断 push/pop 破坏栈布局。
  - 方案：诊断代码用 `#[cfg(feature = "debug_isr")]`（汇编侧用 `%ifdef`）隔离，生产构建不包含。
  - 状态：[]

- **TOP 20 #17 SMEP/SMAP 启用**
  - 描述：全局 CR4 写入仅 PAE/OSFXSR/PCIDE/CET，未设置 SMEP（bit 20）/SMAP（bit 21），用户态可执行内核代码。
  - 方案：启动路径 CR4 写 SMEP/SMAP；检查所有内核→用户指针访问路径满足 SMAP 要求（`stac/clac` 或 `__user` 语义）。
  - 状态：[]

- **O-01~O-05 附加关注项**
  - 描述：O-01 `'!'`(0x21) 与 IRQ vector 混淆；O-02 KPTI trampoline 间距未校验；O-03 aarch64 TTBR1_EL1 未处理；O-04 `mov ax, 0x23` 硬编码；O-05 aarch64 psci.rs 缺失读取。
  - 方案：O-01/O-04 随 F-05/F-13 一并处理；O-02 链接脚本加间距断言；O-03 随 F-08；O-05 核实链接脚本符号后补。
  - 状态：[]

### 验证门槛

- **双架构 QEMU 启动**
  - 描述：汇编/链接脚本改动必须跑 QEMU 真实启动（改动 boot 相关）。
  - 方案：`./scripts/qemu_boot_test.sh all`；先修复 qemu_boot_test.sh `FAIL_OK` 默认值（分册 01）。
  - 状态：[]

- **KPTI 回归**
  - 描述：KPTI 相关改动验证用户态陷入/返回路径与 syscall 入口页表切换。
  - 方案：跑 `host-tests` 中 usermode 相关用例 + QEMU Ring 3 到达日志。
  - 状态：[]

### 决策记录

- **DECISION-048**
  - 描述：KPTI 完整化（F-04/F-08/F-10 + TOP 20 #3）单开 PR，与其余汇编项隔离。
  - 方案：风险最高的改动独立审查、独立回滚面。
  - 状态：[]
