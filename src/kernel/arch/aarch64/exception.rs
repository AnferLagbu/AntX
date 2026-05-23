//! AArch64 异常向量表 (Exception Vector Table)
//!
//! ARMv8-A 异常级别 EL1 (内核) 异常向量表。
//! 每个异常类型有 4 个入口: 同异常级别使用 SP_EL0/SP_ELx, 不同异常级别使用 SP_EL0/SP_ELx。
//!
//! 向量表布局 (VBAR_EL1):
//!   +0x000: Synchronous  EL1t  (current EL with SP_EL0)
//!   +0x080: IRQ           EL1t
//!   +0x100: FIQ           EL1t
//!   +0x180: SError        EL1t
//!   +0x200: Synchronous  EL1h  (current EL with SP_ELx)
//!   +0x280: IRQ           EL1h
//!   +0x300: FIQ           EL1h
//!   +0x380: SError        EL1h
//!   +0x400: Synchronous  EL0 in AArch64
//!   +0x480: IRQ           EL0 in AArch64
//!   +0x500: FIQ           EL0 in AArch64
//!   +0x580: SError        EL0 in AArch64
//!   +0x600: Synchronous  EL0 in AArch32
//!   +0x680: IRQ           EL0 in AArch32
//!   +0x700: FIQ           EL0 in AArch32
//!   +0x780: SError        EL0 in AArch32

use core::arch::global_asm;

// ============================================================================
// 异常向量表 (global_asm)
// ============================================================================

global_asm!(
    r#"
// ============================================================
// AArch64 异常向量表 (VBAR_EL1, 每个入口 32 条指令 = 128 bytes)
// ARM 要求向量表按 2KB (2048 bytes) 对齐
// ============================================================
.section .vectors, "ax"
.balign 2048
.global exception_vector_table
exception_vector_table:

// -------- EL1t: current EL with SP_EL0 --------
.balign 128
curr_el_sp0_sync:
    b   unexpected_exception

.balign 128
curr_el_sp0_irq:
    b   unexpected_exception

.balign 128
curr_el_sp0_fiq:
    b   unexpected_exception

.balign 128
curr_el_sp0_serror:
    b   unexpected_exception

// -------- EL1h: current EL with SP_ELx (标准内核路径) --------
.balign 128
curr_el_spx_sync:
    // Save context
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    stp  x20, x21, [sp, #(8 * 20)]
    stp  x22, x23, [sp, #(8 * 22)]
    stp  x24, x25, [sp, #(8 * 24)]
    stp  x26, x27, [sp, #(8 * 26)]
    stp  x28, x29, [sp, #(8 * 28)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mov  x1, sp
    add  x1, x1, #(8 * 35)      // original SP_EL1
    str  x1, [sp, #(8 * 33)]

    // Call handler (x0 = frame pointer)
    mov  x0, sp
    bl   sync_exception_handler

    // Restore context
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

.balign 128
curr_el_spx_irq:
    // Save context
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    stp  x20, x21, [sp, #(8 * 20)]
    stp  x22, x23, [sp, #(8 * 22)]
    stp  x24, x25, [sp, #(8 * 24)]
    stp  x26, x27, [sp, #(8 * 26)]
    stp  x28, x29, [sp, #(8 * 28)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mov  x1, sp
    add  x1, x1, #(8 * 35)
    str  x1, [sp, #(8 * 33)]

    // Call IRQ handler
    mov  x0, sp
    bl   irq_handler

    // Restore context
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

.balign 128
curr_el_spx_fiq:
    b   unexpected_exception

.balign 128
curr_el_spx_serror:
    b   unexpected_exception

// -------- EL0 in AArch64 --------
.balign 128
lower_el_aarch64_sync:
    // SVC / 数据异常等从 EL0 同步进入
    // 保存 EL0 完整上下文到内核栈
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    stp  x20, x21, [sp, #(8 * 20)]
    stp  x22, x23, [sp, #(8 * 22)]
    stp  x24, x25, [sp, #(8 * 24)]
    stp  x26, x27, [sp, #(8 * 26)]
    stp  x28, x29, [sp, #(8 * 28)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mrs  x1, sp_el0
    str  x1, [sp, #(8 * 33)]

    // 检查 ESR_EL1.EC 判断异常类型
    mrs  x0, esr_el1
    lsr  x0, x0, #26        // EC = ESR[31:26]
    cmp  x0, #0x15          // SVC from AArch64
    beq  handle_svc

    // 其他 EL0 同步异常 → sync_exception_handler
    mov  x0, sp
    bl   sync_exception_handler

    // 恢复上下文并返回 EL0
    ldr  x1, [sp, #(8 * 33)]
    msr  sp_el0, x1
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

handle_svc:
    // SVC #0: x0 = 系统调用号, x1-x5 = 参数
    mov  x0, sp             // x0 = ExceptionFrame*
    bl   svc_handler

    // 返回值在 x0 中，恢复上下文后返回 EL0
    // 把返回值存入帧内 x0
    str  x0, [sp, #(8 * 0)]

    ldr  x1, [sp, #(8 * 33)]
    msr  sp_el0, x1
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    ldp  x20, x21, [sp, #(8 * 20)]
    ldp  x22, x23, [sp, #(8 * 22)]
    ldp  x24, x25, [sp, #(8 * 24)]
    ldp  x26, x27, [sp, #(8 * 26)]
    ldp  x28, x29, [sp, #(8 * 28)]
    add  sp, sp, #(8 * 35)
    eret

.balign 128
lower_el_aarch64_irq:
    // Save context from EL0
    sub  sp, sp, #(8 * 35)
    stp  x0, x1, [sp, #(8 * 0)]
    stp  x2, x3, [sp, #(8 * 2)]
    stp  x4, x5, [sp, #(8 * 4)]
    stp  x6, x7, [sp, #(8 * 6)]
    stp  x8, x9, [sp, #(8 * 8)]
    stp  x10, x11, [sp, #(8 * 10)]
    stp  x12, x13, [sp, #(8 * 12)]
    stp  x14, x15, [sp, #(8 * 14)]
    stp  x16, x17, [sp, #(8 * 16)]
    stp  x18, x19, [sp, #(8 * 18)]
    str  x30, [sp, #(8 * 30)]

    mrs  x0, elr_el1
    mrs  x1, spsr_el1
    stp  x0, x1, [sp, #(8 * 31)]
    mrs  x1, sp_el0
    str  x1, [sp, #(8 * 33)]

    // Handle IRQ
    mov  x0, sp
    bl   irq_handler_el0

    // Restore context for EL0
    ldr  x1, [sp, #(8 * 33)]
    msr  sp_el0, x1
    ldr  x30, [sp, #(8 * 30)]
    ldp  x0, x1, [sp, #(8 * 31)]
    msr  elr_el1, x0
    msr  spsr_el1, x1
    ldp  x0, x1, [sp, #(8 * 0)]
    ldp  x2, x3, [sp, #(8 * 2)]
    ldp  x4, x5, [sp, #(8 * 4)]
    ldp  x6, x7, [sp, #(8 * 6)]
    ldp  x8, x9, [sp, #(8 * 8)]
    ldp  x10, x11, [sp, #(8 * 10)]
    ldp  x12, x13, [sp, #(8 * 12)]
    ldp  x14, x15, [sp, #(8 * 14)]
    ldp  x16, x17, [sp, #(8 * 16)]
    ldp  x18, x19, [sp, #(8 * 18)]
    add  sp, sp, #(8 * 35)
    eret

.balign 128
lower_el_aarch64_fiq:
    b   unexpected_exception

.balign 128
lower_el_aarch64_serror:
    b   unexpected_exception

// -------- EL0 in AArch32 (未使用) --------
.balign 128
lower_el_aarch32_sync:
    b   unexpected_exception

.balign 128
lower_el_aarch32_irq:
    b   unexpected_exception

.balign 128
lower_el_aarch32_fiq:
    b   unexpected_exception

.balign 128
lower_el_aarch32_serror:
    b   unexpected_exception

// -------- 未预期异常处理 --------
unexpected_exception:
    mrs  x0, esr_el1
    mrs  x1, elr_el1
    mrs  x2, far_el1
    wfi
    b    unexpected_exception
"#
);

// ============================================================================
// 异常上下文 (保存/恢复)
// ============================================================================

/// 异常帧 (保存在 SPSR_EL1 + ELR_EL1)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExceptionFrame {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
    pub x4: u64,
    pub x5: u64,
    pub x6: u64,
    pub x7: u64,
    pub x8: u64,
    pub x9: u64,
    pub x10: u64,
    pub x11: u64,
    pub x12: u64,
    pub x13: u64,
    pub x14: u64,
    pub x15: u64,
    pub x16: u64,
    pub x17: u64,
    pub x18: u64,
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
    pub x29: u64, // FP
    pub x30: u64, // LR
    pub elr: u64,
    pub spsr: u64,
    pub sp: u64,
}

// ============================================================================
// 异常向量表导出 (供 boot 入口设置 VBAR_EL1)
// ============================================================================

extern "C" {
    /// 异常向量表起始地址 (定义在 asm)
    pub static exception_vector_table: u8;
}

// ============================================================================
// 异常处理函数
// ============================================================================

/// SVC 系统调用处理。
///
/// 从 EL0 通过 `svc #0` 进入。frame.x8 包含 svc 立即数 (通常为 0)。
/// 系统调用号在 frame.x0 中 (ARM EABI / Linux 惯例)。
/// 返回值为新的 x0。
#[no_mangle]
pub extern "C" fn svc_handler(frame: &mut ExceptionFrame) -> u64 {
    // 调用内核系统调用分发器
    // 使用 arch! 宏确保 x86_64 编译不受影响
    #[cfg(target_arch = "aarch64")]
    {
        // aarch64: syscall 模块未实现，返回 ENOSYS
        // TODO: 实现 aarch64 syscall 支持
        let _ = frame; // x86_64 路径使用 frame 参数, aarch64 暂不需要
        unsafe { crate::kernel::klog::serial_write_bytes(b"[SYSCALL] ENOSYS\n"); }
        0u64
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        let syscall_nr = frame.x8 as usize; // SVC number
        let arg0 = frame.x0 as usize;
        let arg1 = frame.x1 as usize;
        let arg2 = frame.x2 as usize;
        let arg3 = frame.x3 as usize;
        let _arg4 = frame.x4 as usize;
        let _ = (syscall_nr, arg0, arg1, arg2, arg3);
        0
    }
}

/// EL0 IRQ 处理器 (Phase 6 stub)
#[no_mangle]
pub extern "C" fn irq_handler_el0(_frame: &ExceptionFrame) {
    // GIC ACK + handle + EOI
    let intid = super::gic::acknowledge();
    if intid >= 1020 {
        // Spurious interrupt, no EOI needed
        return;
    }
    // Phase 6: 调用通用中断处理
    super::gic::end_of_interrupt(intid);
}

/// 默认同步异常处理 (EL1h)
#[no_mangle]
pub extern "C" fn sync_exception_handler(frame: &ExceptionFrame) {
    let esr: u64;
    let far: u64;
    unsafe {
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr);
        core::arch::asm!("mrs {}, far_el1", out(reg) far);
    }
    // Phase 5 stub: 记录寄存器后停机
    let _ = (esr, far, frame.elr);
    loop {
        unsafe { core::arch::asm!("wfi", options(nomem, nostack)); }
    }
}

/// 默认 IRQ 处理 (EL1h)
#[no_mangle]
pub extern "C" fn irq_handler(_frame: &ExceptionFrame) {
    // GIC ACK
    let intid = super::gic::acknowledge();
    if intid >= 1020 {
        return;
    }

    // Timer interrupt (PPI 30 = non-secure physical timer)
    if intid == 30 {
        crate::kernel::timer::on_timer_interrupt();
        extern "C" {
            fn scheduler_tick_mlfq();
        }
        unsafe { scheduler_tick_mlfq(); }
    }

    super::gic::end_of_interrupt(intid);
}

/// 默认 FIQ 处理 (EL1h)
#[no_mangle]
pub extern "C" fn fiq_handler(_frame: &ExceptionFrame) {}

/// 默认 SError 处理 (EL1h)
#[no_mangle]
pub extern "C" fn serror_handler(_frame: &ExceptionFrame) {
    loop {
        unsafe { core::arch::asm!("wfi", options(nomem, nostack)); }
    }
}

/// 初始化异常: 设置 VBAR_EL1 指向向量表, 清除 DAIF
pub unsafe fn init() {
    let vbar = &exception_vector_table as *const u8 as u64;
    core::arch::asm!("msr vbar_el1, {}", in(reg) vbar);

    // 清除 DAIF (Debug/SError/IRQ/FIQ 掩码), 使能中断
    core::arch::asm!("msr daifclr, #0xF");

    // ISB 确保写 VBAR 在取指前完成
    core::arch::asm!("isb");
}