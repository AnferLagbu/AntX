//! AArch64 上下文切换
//!
//! AAPCS64 callee-saved 寄存器: x19-x30, SP
//! 系统寄存器: TTBR0_EL1, SPSR_EL1, ELR_EL1
//!
//! 上下文布局 (复用 ProcessContext 偏移, 17×8=136 bytes):
//!   +0x00: x19       +0x40: x24       +0x80: x29 (FP)
//!   +0x08: x20       +0x48: x25       +0x88: lr  (x30)
//!   +0x10: x21       +0x50: x26       +0x90: sp
//!   +0x18: x22       +0x58: x27       +0x98: ttbr0_el1
//!   +0x20: x23       +0x60: x28       +0xA0: spsr_el1
//!   +0x28: rbx(未用/0)               +0xA8: elr_el1
//!   +0x30: rbp(未用/0)               +0xB0: ss(未用/0)
//!   +0x38: rax(未用/0)

use core::arch::global_asm;

// ============================================================================
// 上下文切换汇编
// ============================================================================

global_asm!(
    r#"
.section .text.context_switch, "ax"
.global context_switch_asm

// 汇编函数签名: void context_switch_asm(u64* prev, const u64* next);
// x0 = prev (save), x1 = next (restore)
context_switch_asm:
    // 禁用中断 (DAIF set)
    msr  daifset, #0xF

    // === Save current context to [x0] ===

    // 被调用方保存的 GP 寄存器
    str  x19, [x0, #0x00]
    str  x20, [x0, #0x08]
    str  x21, [x0, #0x10]
    str  x22, [x0, #0x18]
    str  x23, [x0, #0x20]
    str  x24, [x0, #0x28]   // Field: rbx
    str  x25, [x0, #0x30]   // Field: rbp
    str  x26, [x0, #0x38]   // Field: rax
    str  x27, [x0, #0x40]   // Field: rip → x24
    str  x28, [x0, #0x48]   // Field: rsp → x25

    // Actually let me remap to use ProcessContext offsets:
    // ProcessContext: r15(0) r14(8) r13(16) r12(24) rbx(32) rbp(40) rax(48)
    //                rip(56) rsp(64) rflags(72) cr3(80) cs(88) ds(96)
    //                es(104) fs(112) gs(120) ss(128)

    // Map aarch64 → x86_64 fields:
    // x19→r15(0)  x20→r14(8)  x21→r13(16)  x22→r12(24)
    // x23→rbx(32)  x24→rbp(40)  x25→rax(48)
    // 寄存器对应: x26→rip(56)  x27→rsp(64)  x28→rflags(72)
    // x29→cr3(80)  x30→cs(88)  sp→ds(96)
    // 字段映射: ttbr0→es(104)  spsr→fs(112)  elr→gs(120)  0→ss(128)

    str  x19, [x0, #0]
    str  x20, [x0, #8]
    str  x21, [x0, #16]
    str  x22, [x0, #24]
    str  x23, [x0, #32]
    str  x24, [x0, #40]
    str  x25, [x0, #48]
    str  x26, [x0, #56]
    str  x27, [x0, #64]
    str  x28, [x0, #72]
    str  x29, [x0, #80]
    str  x30, [x0, #88]
    // 保存 SP (函数调用前的当前栈指针)
    mov  x2, sp
    str  x2, [x0, #96]

    // 读系统寄存器
    mrs  x2, ttbr0_el1
    str  x2, [x0, #104]
    mrs  x2, spsr_el1
    str  x2, [x0, #112]
    mrs  x2, elr_el1
    str  x2, [x0, #120]
    // ss field (128): write 0
    str  xzr, [x0, #128]

    // === 从 [x1] 恢复下一个上下文 ===
    ldr  x19, [x1, #0]
    ldr  x20, [x1, #8]
    ldr  x21, [x1, #16]
    ldr  x22, [x1, #24]
    ldr  x23, [x1, #32]
    ldr  x24, [x1, #40]
    ldr  x25, [x1, #48]
    ldr  x26, [x1, #56]
    ldr  x27, [x1, #64]
    ldr  x28, [x1, #72]
    ldr  x29, [x1, #80]
    ldr  x30, [x1, #88]
    // SP
    ldr  x2, [x1, #96]
    mov  sp, x2

    // 恢复系统寄存器
    ldr  x2, [x1, #104]
    msr  ttbr0_el1, x2
    isb

    ldr  x2, [x1, #112]
    msr  spsr_el1, x2
    ldr  x2, [x1, #120]
    msr  elr_el1, x2

    // 恢复中断状态 (从 SPSR_EL1)
    ldr  x2, [x1, #112]   // SPSR_EL1
    // 提取 DAIF bits 并恢复
    and  x3, x2, #0x3C0   // DAIF mask
    msr  daif, x3

    // eret (跳转到 ELR_EL1 或 EL0 取决于 SPSR)
    eret
"#
);

// ============================================================================
// AArch64 上下文结构 (用于编译时验证布局)
// ============================================================================

/// AArch64 上下文布局 (对应 ProcessContext 偏移)
#[repr(C)]
pub struct Aarch64Context {
    pub x19: u64,   // offset 0 → r15
    pub x20: u64,   // offset 8 → r14
    pub x21: u64,   // offset 16 → r13
    pub x22: u64,   // offset 24 → r12
    pub x23: u64,   // offset 32 → rbx
    pub x24: u64,   // offset 40 → rbp
    pub x25: u64,   // offset 48 → rax
    pub x26: u64,   // offset 56 → rip
    pub x27: u64,   // offset 64 → rsp
    pub x28: u64,   // offset 72 → rflags
    pub x29: u64,   // offset 80 → cr3  (FP)
    pub lr: u64,    // offset 88 → cs   (x30)
    pub sp: u64,    // offset 96 → ds
    pub ttbr0: u64, // offset 104 → es
    pub spsr: u64,  // offset 112 → fs
    pub elr: u64,   // offset 120 → gs
    pub _pad: u64,  // offset 128 → ss
}

unsafe extern "C" {
    pub fn context_switch_asm(prev: *const u64, next: *const u64);
}

/// 执行上下文切换。from/to 为原始指针 (实际指向 ProcessContext)。
///
/// # Safety
/// 调用者必须确保两个上下文指针有效且已初始化。
#[inline(always)]
pub unsafe fn switch(from: *mut u8, to: *const u8) { unsafe {
    context_switch_asm(from as *const u64, to as *const u64);
}}
