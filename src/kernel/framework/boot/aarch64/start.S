/*
 * AArch64 启动入口 (汇编)
 *
 * QEMU virt (cortex-a72):
 *   入口 EL3 (0x40080000) → EL2 → EL1
 *
 * 职责:
 *   1. 检测当前异常级别
 *   2. EL3→EL2: 配置 SCR_EL3, 返回 EL2
 *   3. EL2→EL1: 配置 HCR_EL2, 返回 EL1
 *   4. EL1: 设置栈, 跳转 Rust entry()
 */

.section .text.boot
.global _start
_start:

// ============================================================
// 1. 确定当前异常级别
// ============================================================
    mrs  x0, CurrentEL
    lsr  x0, x0, #2        // EL = (CurrentEL >> 2) & 0x3
    and  x0, x0, #0x3

    cmp  x0, #3
    beq  el3_entry
    cmp  x0, #2
    beq  el2_entry
    cmp  x0, #1
    beq  el1_entry

    // 未知 EL: 进入死循环
    wfi
    b    .

// ============================================================
// 2. EL3 → EL2
// ============================================================
el3_entry:
    // 关闭 EL3 陷阱 (允许 EL2 访问所有系统寄存器)
    // SCR_EL3: NS=1 (非安全), HCE=1 (HVC 可用), RW=1 (EL2 为 AArch64)
    mov  x0, #(1 << 0)     // NS=1
    orr  x0, x0, #(1 << 8) // HCE=1
    orr  x0, x0, #(1 << 10) // RW=1 (lower EL is AArch64)
    msr  scr_el3, x0

    // 设置 SPSR_EL3: EL=EL2h, DAIF masked
    mov  x0, #0x3c9         // M[3:0]=1001 (EL2h), DAIF=1111 (all masked)
    msr  spsr_el3, x0

    // ELR_EL3 = el2_entry (返回地址)
    adr  x0, el2_entry
    msr  elr_el3, x0

    eret

// ============================================================
// 3. EL2 → EL1
// ============================================================
el2_entry:
    // HCR_EL2: RW=1 (EL1 为 AArch64), SWIO=1 (set/way I-cache 可用)
    mov  x0, #(1 << 31)     // RW=1
    orr  x0, x0, #(1 << 1)  // SWIO=1
    msr  hcr_el2, x0

    // CPTR_EL2: 清零 TFP (bit 10) — 否则即使 CPACR_EL1 允许, EL1 的 FP/SIMD 也会 trap 到 EL2
    mrs  x0, cptr_el2
    bic  x0, x0, #(1 << 10) // Clear TFP
    msr  cptr_el2, x0

    // 为 CPACR_EL1 和 CNTHCTL_EL2 设置最大访问权限 (EL1/EL0 可访问 FP/SIMD + Timer)
    mov  x0, #(0x3 << 20)   // FPEN = 0b11 (trap none)
    msr  cpacr_el1, x0

    // CNTHCTL_EL2: EL1PCTEN=1, EL1PCEN=1 (EL1 物理 Counter/Timer 可访问)
    mov  x0, #(1 << 0)      // EL1PCEN
    orr  x0, x0, #(1 << 1)  // EL1PCTEN
    msr  cnthctl_el2, x0

    // SCTLR_EL1: 清除 (将进行配置)
    msr  sctlr_el1, xzr

    // 设置 SPSR_EL2: EL=EL1h, DAIF masked
    mov  x0, #0x3c5         // M[3:0]=0101 (EL1h), DAIF=1111 (all masked)
    msr  spsr_el2, x0

    // ELR_EL2 = el1_entry
    adr  x0, el1_entry
    msr  elr_el2, x0

    eret

// ============================================================
// 4. EL1 入口 — 准备C运行环境
// ============================================================
el1_entry:
    // 4.1 设置栈指针 (boot stack)
    adrp x0, _boot_stack_top
    add  x0, x0, #:lo12:_boot_stack_top
    mov  sp, x0

    // 4.2 保存 x0 (可能包含 fdt 指针)
    // QEMU 启动时 x0 可能包含设备树地址，暂存
    adrp x1, _fdt_addr
    add  x1, x1, #:lo12:_fdt_addr
    str  x0, [x1]

    // 4.3 跳转 Rust entry()
    bl   entry

    // entry() should never return
    wfi
    b    .

// ============================================================
// 辅助符号
// ============================================================

// FDT 地址 (从 QEMU 传来)
.data
.global _fdt_addr
_fdt_addr:
    .quad 0

// Boot 栈
.bss
.align 16
_boot_stack_bottom:
    .space 262144            // 256KB boot stack
_boot_stack_top:
