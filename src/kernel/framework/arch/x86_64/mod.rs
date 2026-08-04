//! x86-64 架构特定实现
//!
//! 包含 GDT, TSS, APIC, IOAPIC 等x86-64特有逻辑。
//!
//! ## 实现状态
//! - [x] `impl CoreArch for X8664` — 基础核心能力
//! - [x] `impl InterruptArch for X8664` — 中断 + IPI
//! - [x] `impl MmuArch for X8664` — MMU + 上下文 + 用户态
//! - [x] `impl SystemArch for X8664` — 端口IO + 电源管理
//! - [x] `impl Arch for X8664` — 超 trait (空)

// ============================================================================
// 保留现有模块 (不动任何实现代码)
// ============================================================================

pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod ioapic;
pub mod smp_init;
pub mod tss;

// ============================================================================
// X8664 架构类型
// ============================================================================

/// `x86_64` 架构标记类型。
///
/// 零大小类型，通过子 trait 提供所有 `x86_64` 硬件操作。
pub struct X8664;

// ============================================================================
// 子 trait 实现 — 拆分自原 Arch trait (Phase 8 refactor)
// 每个 trait 互不依赖，可独立单元测试。
// ============================================================================

use crate::kernel::framework::arch::{Arch, CoreArch, InterruptArch, MmuArch, SystemArch};

// ── CoreArch: 基础核心 ──────────────────────────────────────────────────

impl CoreArch for X8664 {
    /// 获取当前 CPU ID (Local APIC ID)。
    #[inline(always)]
    fn cpu_id() -> u32 {
        use crate::kernel::framework::arch::x86_64::apic;
        let id = apic::get_id();
        if id != 0 {
            return id;
        }
        let (_, ebx, _, _) = crate::kernel::framework::cpu::cpuid::cpuid(1, 0);
        ebx >> 24
    }

    /// 获取高精度时间戳 (rdtsc)。
    #[inline(always)]
    fn timestamp() -> u64 {
        let lo: u32;
        let hi: u32;
        // SAFETY: rdtsc 是序列化指令, 写 EAX/EDX; 声明 nostack/nomem/preserves_flags
        // 防止编译器在指令间重排或溢出状态.
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") lo,
                out("edx") hi,
                options(nostack, nomem, preserves_flags)
            );
        }
        (u64::from(hi) << 32) | u64::from(lo)
    }

    /// CPU 暂停等待中断 (hlt)。
    #[inline(always)]
    fn halt() {
        // SAFETY: hlt 暂停 CPU 至下一次中断; 不触及 Rust 可见的内存或寄存器.
        // nomem/nostack 标注正确.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }

    /// 全内存屏障 (mfence)。
    #[inline(always)]
    fn fence() {
        // SAFETY: mfence 排序所有 load/store; 未声明寄存器 clobber, preserves_flags 正确.
        unsafe {
            core::arch::asm!("mfence", options(nostack, preserves_flags));
        }
    }

    /// 写内存屏障 (sfence)。
    #[inline(always)]
    fn fence_w() {
        // SAFETY: sfence orders stores; no memory reads, no stack use.
        unsafe {
            core::arch::asm!("sfence", options(nostack, preserves_flags));
        }
    }

    /// 读内存屏障 (lfence)。
    #[inline(always)]
    fn fence_r() {
        // SAFETY: lfence orders loads; correct memory model barrier.
        unsafe {
            core::arch::asm!("lfence", options(nostack, preserves_flags));
        }
    }
}

// ── InterruptArch: 中断 + IPI ────────────────────────────────────────

impl InterruptArch for X8664 {
    /// 禁用中断并返回 RFLAGS (含 IF 位)。
    #[inline(always)]
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    fn interrupt_disable() -> usize {
        let flags: u64;
        // SAFETY: pushfq 压入 RFLAGS, pop 弹出到通用寄存器, 然后 cli 关中断.
        // nomem/nostack/preserves_flags 全部由该指令序列满足.
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                "cli",
                out(reg) flags,
                options(nomem, nostack, preserves_flags)
            );
        }
        flags as usize
    }

    /// 恢复中断状态，仅当 flags 中 IF 位为 1 时才启用。
    #[inline(always)]
    fn interrupt_restore(flags: usize) {
        if (flags as u64) & (1 << 9) != 0 {
            // SAFETY: sti 启用中断; nomem/nostack 成立, 对内存无可观察副作用.
            unsafe {
                core::arch::asm!("sti", options(nomem, nostack));
            }
        }
    }

    /// 启用中断 (sti)。
    #[inline(always)]
    fn interrupt_enable() {
        // SAFETY: sti enables interrupts; no memory access, no stack use.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }

    /// 检查 IF 位 (RFLAGS bit 9)。
    #[inline(always)]
    fn is_interrupt_enabled() -> bool {
        let flags: u64;
        // SAFETY: pushfq/pop 序列仅将 RFLAGS 读入寄存器; 对调用方 flags 保持不变.
        // 不触及内存或栈.
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                out(reg) flags,
                options(nomem, nostack, preserves_flags)
            );
        }
        (flags & (1 << 9)) != 0
    }

    /// 向目标 CPU 发送 IPI (通过 Local APIC)。
    #[inline(always)]
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    fn send_ipi(target_cpu: u32, vector: u8) {
        use crate::kernel::framework::arch::x86_64::apic;
        apic::send_ipi(target_cpu as u8, vector);
    }

    /// 广播 IPI 到所有 CPU (不含自身)。
    #[inline(always)]
    fn broadcast_ipi(vector: u8) {
        use crate::kernel::framework::arch::x86_64::apic;
        apic::broadcast_ipi(vector);
    }

    fn interrupt_early_init() {
        crate::kernel::framework::idt::idt_init();
    }

    fn interrupt_late_init() {
        // cpu_init 必须在 gdt_init 之前调用:
        // kpti_init 依赖 has_invpcid() → get_cpu_info() → cpu_init
        crate::kernel::framework::cpu::cpu_init();

        crate::kernel::framework::arch::x86_64::gdt::gdt_init();

        // 配置 SYSCALL/SYSRET 指令
        // 设置 EFER.SCE, STAR, LSTAR (高半部分地址), SFMASK
        #[cfg(target_arch = "x86_64")]
        {
            const IA32_EFER: u32 = 0xC0000080;
            const IA32_STAR: u32 = 0xC0000081;
            const IA32_LSTAR: u32 = 0xC0000082;
            const IA32_SFMASK: u32 = 0xC0000084;
            const EFER_SCE: u64 = 1 << 0;

            // SAFETY: MSR 写入在 boot 阶段单线程执行
            unsafe {
                let efer = crate::kernel::framework::cpu::msr::read_msr(IA32_EFER);
                crate::kernel::framework::cpu::msr::write_msr(IA32_EFER, efer | EFER_SCE);

                // STAR: [63:48] = SYSRET CS base (0x10), [47:32] = SYSCALL CS base (0x08)
                let star = (0x10u64 << 48) | (0x08u64 << 32);
                crate::kernel::framework::cpu::msr::write_msr(IA32_STAR, star);

                // LSTAR: syscall 入口点 (高半部分地址, KPTI 用户页表只映射高半区)
                // 注意: 函数指针返回的是 LMA (低地址), 需要转换为 VMA (高地址)
                // 链接脚本定义: _kernel_text_vma = 0xFFFF800001000000 + _kernel_text_lma
                // 因此偏移量 = 0xFFFF800001000000 (不是 KERNEL_BASE)
                // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
                unsafe extern "C" { fn syscall_entry(); }
                let entry_lma = syscall_entry as *const () as u64;
                let entry_hi = entry_lma + 0xFFFF800001000000u64;
                crate::klog_boot_info!(
                    "[SYSCALL] syscall_entry LMA={:#X}, LSTAR VMA={:#X}",
                    entry_lma, entry_hi
                );
                crate::kernel::framework::cpu::msr::write_msr(IA32_LSTAR, entry_hi);

                // SFMASK: 进入内核时清除 IF
                crate::kernel::framework::cpu::msr::write_msr(IA32_SFMASK, 1 << 9);
            }
        }

        crate::kernel::framework::idt::idt_init();
        crate::kernel::framework::arch::x86_64::apic::apic_init();
        crate::kernel::framework::smp::init();
        crate::kernel::framework::arch::x86_64::smp_init::init();
    }
}

// ── MmuArch: MMU + 上下文 + 用户态 ───────────────────────────────────

impl MmuArch for X8664 {
    /// 刷新单个虚拟地址的 TLB (invlpg)。
    #[inline(always)]
    fn tlb_flush_page(vaddr: usize) {
        // SAFETY: invlpg takes the virtual address in a register and
        // invalidates the TLB entry; the address is a kernel VA.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) vaddr,
                options(nostack, preserves_flags)
            );
        }
    }

    /// 刷新全部 TLB (重载 CR3)。
    #[inline(always)]
    fn tlb_flush_all() {
        // SAFETY: 读再写 CR3 触发完整 TLB 刷新; 中间寄存器使用是 GPR 与 CR3 间的直接搬运.
        unsafe {
            core::arch::asm!(
                "mov rax, cr3",
                "mov cr3, rax",
                out("rax") _,
                options(nostack, preserves_flags)
            );
        }
    }

    /// 读取当前页表基地址 (mov rax, cr3)。
    #[inline(always)]
    fn read_page_table_base() -> u64 {
        let cr3: u64;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "mov {}, cr3",
                out(reg) cr3,
                options(nostack, preserves_flags)
            );
        }
        cr3
    }

    /// 切换页表 (mov to cr3)。
    #[inline(always)]
    fn write_page_table_base(paddr: u64) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "mov cr3, {}",
                in(reg) paddr,
                options(nostack, preserves_flags)
            );
        }
    }

    /// 读取页故障地址 (mov rax, cr2)。
    #[inline(always)]
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    fn read_fault_address() -> usize {
        let cr2: u64;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "mov {}, cr2",
                out(reg) cr2,
                options(nostack, preserves_flags)
            );
        }
        cr2 as usize
    }

    /// 进程上下文切换 (`process_switch_asm`)。
    #[inline(always)]
    fn context_switch(from: *mut u8, to: *const u8) {
        // SAFETY: C ABI 互操作，函数签名与外部代码约定一致
        unsafe extern "C" {
            fn process_switch_asm(prev: *mut u8, next: *const u8);
        }
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            process_switch_asm(from, to);
        }
    }

    /// 进入用户态 (iretq)。
    ///
    /// 通过 `global_asm!` 在汇编层面定义，确保在 `.kpti_trampoline` section。
    /// 切换到用户页表后，CPU 需要继续执行当前指令（构建 iretq 帧并执行 iretq），
    /// 如果不在 trampoline 区域，用户页表中该地址没有 USER 位，会导致 #PF。
    ///
    /// 调用约定 (System V AMD64 ABI):
    /// - rdi = entry (用户态 RIP)
    /// - rsi = stack (用户态 RSP)
    /// - rdx = arg (用户态参数，当前未使用)
    /// - rcx = `user_cr3` (用户页表物理地址)
    /// - r8 = kstack (内核栈高半区地址)
    #[inline(never)]
    fn enter_user(entry: usize, stack: usize, arg: usize, user_cr3: u64, kstack: u64) -> ! {
        // SAFETY: 调用方保证 entry/stack/user_cr3/kstack 有效。
        // 通过 FFI 调用汇编实现的 enter_user_asm。
        unsafe {
            unsafe extern "C" {
                fn enter_user_asm(entry: usize, stack: usize, arg: usize, user_cr3: u64, kstack: u64) -> !;
            }
            enter_user_asm(entry, stack, arg, user_cr3, kstack)
        }
    }

    /// 返回用户态 (iretq)。
    #[inline(always)]
    fn return_to_user() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!("iretq", options(noreturn));
        }
    }
}

// ============================================================================
// enter_user 汇编实现 (KPTI trampoline)
// ============================================================================

// 进入用户态的汇编实现。
//
// 必须放在 `.kpti_trampoline` section，因为切换到用户页表后，
// CPU 需要继续执行当前指令（构建 iretq 帧并执行 iretq）。
// 如果不在 trampoline 区域，用户页表中该地址没有 USER 位，会导致 #PF。
//
// 调用约定 (System V AMD64 ABI):
// - rdi = entry (用户态 RIP)
// - rsi = stack (用户态 RSP)
// - rdx = arg (用户态参数，当前未使用)
// - rcx = user_cr3 (用户页表物理地址)
// - r8 = kstack (内核栈高半区地址)
//
// 执行流程:
// 1. 诊断输出 (CPL=0, 内核栈)
// 2. 切换到用户栈 (CPL=0, 仍可访问高半区)
// 3. 在用户栈构建 iretq 帧
// 4. 切换 CR3 到用户页表 (CPL=0, 高半区 trampoline 可执行)
// 5. 加载用户段寄存器 (CPL→3)
// 6. iretq (从用户栈读取帧)
core::arch::global_asm!(r#"
    .section .kpti_trampoline
    .global enter_user_asm
    .type enter_user_asm, @function
enter_user_asm:
    // 参数: rdi=entry, rsi=stack, rdx=arg, rcx=user_cr3, r8=kstack
    cli

    // ── 诊断: enter_user_asm 入口处 GS_BASE ──
    // 在任何操作之前读取 IA32_GS_BASE, 验证从 Rust 到汇编的 GS 状态
    push rax
    push rdx
    push rcx
    mov dx, 0x3F8
    mov al, 0x50                   // 'P' - enter_user_asm 入口 GS_BASE
    out dx, al
    mov ecx, 0xC0000101            // IA32_GS_BASE
    rdmsr                           // EDX:EAX = IA32_GS_BASE
    mov r14, rax
    mov r15, 16
99: rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb 98f
    add al, 0x27
98: add al, 0x30
    mov dx, 0x3F8
    out dx, al
    dec r15
    jnz 99b
    pop rcx
    pop rdx
    pop rax
    // ── 诊断结束 ──

    // 诊断点 1: 进入 enter_user_asm
    push rax
    mov dx, 0x3F8
    mov al, 0x41                    // 'A' - 标记进入 enter_user_asm
    out dx, al
    pop rax

    // 保存 user_cr3 到 rax (在清除寄存器前)
    mov rax, rcx

    // 保存 entry 到 r12 (在清除寄存器前)
    mov r12, rdi

    // 诊断点 2: 准备切换到用户栈
    push rax
    mov dx, 0x3F8
    mov al, 0x42                    // 'B' - 准备切换 RSP 到用户栈
    out dx, al
    pop rax

    // 清除寄存器 (防止泄露内核信息到用户态)
    // 注意：rax 保存 user_cr3，稍后用于切换 CR3
    // 注意：rsi 保存用户栈地址，稍后用于切换 RSP
    mov r8, rsi                     // 暂存用户栈到 r8
    xor ecx, ecx        // 清 rcx
    xor esi, esi        // 清 rsi
    xor edi, edi        // 清 rdi
    xor ebp, ebp        // 清 rbp
    xor r9d, r9d        // 清 r9
    xor r10d, r10d      // 清 r10
    xor r11d, r11d      // 清 r11

    // 切换到用户栈 (CPL=0, 仍可访问高半区)
    mov rsp, r8

    // 诊断点 3: 已切换到用户栈, 构建 iretq 帧
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C' - 已切换 RSP, 构建 iretq 帧
    out dx, al
    pop rax

    // 在用户栈构建 iretq 帧
    // ⚠ 关键修复 (TRACK-INIT-RING3):
    // iretq 帧必须在用户栈上, 而非内核栈.
    // 原因: 切换 CR3 到 USER_PML4 后, 内核栈页面没有 USER 位,
    // iretq 尝试从内核栈读取帧数据会触发 #PF.
    push 0x1B           // SS (用户数据段)
    
    // 诊断点 C1: SS 已 push
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C1' - SS pushed
    out dx, al
    pop rax
    
    push r8             // RSP (用户栈, 当前 RSP 值)
    
    // 诊断点 C2: RSP 已 push
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C2' - RSP pushed
    out dx, al
    pop rax
    
    push 0x202          // RFLAGS (IF 位)
    
    // 诊断点 C3: RFLAGS 已 push
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C3' - RFLAGS pushed
    out dx, al
    pop rax
    
    push 0x23           // CS (用户代码段)
    
    // 诊断点 C4: CS 已 push
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C4' - CS pushed
    out dx, al
    pop rax
    
    push r12            // RIP (用户入口, 使用保存的 r12)
    
    // 诊断点 C5: RIP 已 push, iretq 帧构建完成
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C5' - RIP pushed, frame complete
    out dx, al
    pop rax

    // ═══ 关键修复 (TRACK-INIT-RING3): 更新 SyscallPerCpu.user_pml4 ═══
    // 中断/异常返回路径使用 [gs:USER_PML4_OFF] 切换回用户页表.
    // 若不更新, 仍为 KPTI 初始化时的共享页表, 非当前进程的专用页表,
    // 导致用户代码/栈页不可访问 → #PF → Triple Fault.
    // 此时 IA32_GS_BASE = per_cpu_addr, [gs:USER_PML4_OFF] 可安全写入.
    // rax = user_cr3 (当前进程的用户页表物理地址)
    mov gs:[0x10], rax                  // USER_PML4_OFF = 16, 写入 user_pml4

    // ═══ swapgs: 必须在加载 GS 段寄存器之前执行! ═══
    // 根因: mov gs, cx 会从 GDT 描述符加载隐藏基址到 IA32_GS_BASE.
    // 用户数据段描述符 base=0, 导致 IA32_GS_BASE 被清零.
    // 若 swapgs 在 mov gs 之后, 两个 MSR 都为 0, syscall [gs:0] → #PF → Triple Fault.
    // 正确顺序: swapgs (IA32_GS_BASE=0, IA32_KERNEL_GS_BASE=per_cpu_addr)
    //           → mov gs, cx (IA32_GS_BASE 保持 0, IA32_KERNEL_GS_BASE 不受影响)
    swapgs

    // ═══ 自检式调试: 验证 swapgs 后 IA32_KERNEL_GS_BASE 非零 ═══
    // swapgs 后: IA32_GS_BASE=0, IA32_KERNEL_GS_BASE=per_cpu_addr
    // 若 IA32_KERNEL_GS_BASE=0 → swapgs 前两个 MSR 都为 0 → BUG
    // 输出: 'Q' + 16 hex digits + ('!' 若 BUG)
    push rax
    push rdx
    push rcx
    mov dx, 0x3F8
    mov al, 0x51                   // 'Q' - swapgs 后自检
    out dx, al
    mov ecx, 0xC0000102            // IA32_KERNEL_GS_BASE
    rdmsr                          // EDX:EAX = IA32_KERNEL_GS_BASE
    shl rdx, 32
    or rdx, rax                    // RDX = 完整 64 位值
    mov r14, rdx
    mov r15, 16
97: rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb 96f
    add al, 0x27
96: add al, 0x30
    mov dx, 0x3F8
    out dx, al
    dec r15
    jnz 97b
    // 自检: IA32_KERNEL_GS_BASE == 0 → 输出 '!' BUG 标记
    test r14, r14
    jnz 95f
    mov dx, 0x3F8
    mov al, 0x21                   // '!' - BUG: swapgs 后 KERNEL_GS_BASE=0!
    out dx, al
95: pop rcx
    pop rdx
    pop rax
    // ═══ 自检式调试结束 ═══

    // 加载用户态段寄存器 (必须在 mov cr3 之前!).
    // 原因: mov ds/es/fs/gs 需要读取 GDT, GDT 在高半区.
    // 切换 CR3 到用户页表后, 高半区未映射, 无法访问 GDT → #PF.
    // 此时 CPL=0, 内核页表仍有效, GDT 可访问.
    // 注意: mov gs, cx 会将 GDT 描述符的 base(=0) 写入 IA32_GS_BASE,
    // 但 swapgs 已在上方执行, IA32_GS_BASE 已为 0, 不受影响.
    // IA32_KERNEL_GS_BASE 不受 mov gs 指令影响, 保持 per_cpu_addr.
    mov cx, 0x1B
    mov ds, cx
    
    // 诊断点 C6: DS 已加载
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C6' - DS loaded
    out dx, al
    pop rax
    
    mov es, cx
    
    // 诊断点 C7: ES 已加载
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C7' - ES loaded
    out dx, al
    pop rax
    
    mov fs, cx

    // 诊断点 C8: FS 已加载
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C8' - FS loaded
    out dx, al
    pop rax

    mov gs, cx
    
    // 诊断点 C9: GS 已加载, 段寄存器全部加载完成
    push rax
    mov dx, 0x3F8
    mov al, 0x43                    // 'C9' - GS loaded, all segments ready
    out dx, al
    pop rax

    // ═══ 自检式调试: 验证 mov gs 后 IA32_KERNEL_GS_BASE 仍非零 ═══
    // mov gs, cx 可能将 GDT 描述符 base(=0) 写入 IA32_GS_BASE,
    // 但 IA32_KERNEL_GS_BASE 不受影响. 若为 0 → BUG
    // 输出: 'R' + 16 hex digits + ('!' 若 BUG)
    push rax
    push rdx
    push rcx
    mov dx, 0x3F8
    mov al, 0x52                   // 'R' - mov gs 后自检
    out dx, al
    mov ecx, 0xC0000102            // IA32_KERNEL_GS_BASE
    rdmsr                          // EDX:EAX = IA32_KERNEL_GS_BASE
    shl rdx, 32
    or rdx, rax                    // RDX = 完整 64 位值
    mov r14, rdx
    mov r15, 16
94: rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb 93f
    add al, 0x27
93: add al, 0x30
    mov dx, 0x3F8
    out dx, al
    dec r15
    jnz 94b
    // 自检: IA32_KERNEL_GS_BASE == 0 → 输出 '!' BUG 标记
    test r14, r14
    jnz 92f
    mov dx, 0x3F8
    mov al, 0x21                   // '!' - BUG: mov gs 后 KERNEL_GS_BASE=0!
    out dx, al
92: pop rcx
    pop rdx
    pop rax
    // ═══ 自检式调试结束 ═══

    // ⚠ 关键修复 (TRACK-INIT-RING3):
    // 直接 fall-through 到 trampoline 后续代码.
    // 原因: 代码顺序执行, 无需显式跳转.
    // 高半区 VMA 已在用户页表中映射 (map_text_region_in_user_pml4),
    // 切换 CR3 后 CPU 仍可继续执行.
    
    // 诊断点 D: 即将切换 CR3
    push rax
    mov dx, 0x3F8
    mov al, 0x44                    // 'D' - about to switch CR3
    out dx, al
    pop rax

    // ⚠ 关键修复 (TRACK-INIT-RING3):
    // 切换 CR3 到用户页表.
    // rax 保存 user_cr3.
    // 高半区 VMA 已在用户页表中映射, 切换后 CPU 可继续执行.
    mov cr3, rax
    
    // 诊断点 F: CR3 已切换, 即将执行 iretq
    // 注意: 此时已在用户页表, 但低半区有映射, 可以继续执行
    push rax
    mov dx, 0x3F8
    mov al, 0x46                    // 'F' - CR3 switched, about to iretq
    out dx, al
    pop rax

    // ═══ 自检式调试: 输出 iretq 帧关键值 (hex) ═══
    // 输出 'G' 标记
    mov r14, rax
    mov rax, 0x47
    mov dx, 0x3F8
    out dx, al
    mov rax, r14
    // 输出 RIP (r12 = 用户入口地址), 16 个 hex 数字
    mov r14, r12
    mov r15, 16
99: rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb 98f
    add al, 0x27
98: add al, 0x30
    mov dx, 0x3F8
    out dx, al
    dec r15
    jnz 99b
    // 输出 'H' 标记
    mov r14, rax
    mov rax, 0x48
    mov dx, 0x3F8
    out dx, al
    mov rax, r14
    // 输出 RSP (当前栈指针 = iretq 帧地址)
    mov r14, rsp
    mov r15, 16
99: rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb 98f
    add al, 0x27
98: add al, 0x30
    mov dx, 0x3F8
    out dx, al
    dec r15
    jnz 99b
    // 输出 'I' 标记
    mov r14, rax
    mov rax, 0x49
    mov dx, 0x3F8
    out dx, al
    mov rax, r14
    // 输出 CR3 (rax = user_cr3)
    mov r14, rax
    mov r15, 16
99: rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb 98f
    add al, 0x27
98: add al, 0x30
    mov dx, 0x3F8
    out dx, al
    dec r15
    jnz 99b
    // 恢复 rax = user_cr3 (r14 在最后一次 hex 输出后 = 0)
    mov rax, r14
    // ═══ 自检式调试结束 ═══

    // 清除 rax (防止泄露)
    xor eax, eax

    // iretq 返回用户态
    // iretq 从用户栈恢复: RIP, CS, RFLAGS, RSP, SS
    // swapgs 已在段寄存器加载前执行, IA32_KERNEL_GS_BASE = per_cpu_addr
    iretq
    .size enter_user_asm, . - enter_user_asm
"#);

// ── SystemArch: 端口 IO + 电源管理 ───────────────────────────────────

impl SystemArch for X8664 {
    /// 向 I/O 端口写入字节 (out dx, al)。
    #[inline(always)]
    fn outb(port: u16, value: u8) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nostack, preserves_flags)
            );
        }
    }

    /// 从 I/O 端口读取字节 (in al, dx)。
    #[inline(always)]
    fn inb(port: u16) -> u8 {
        let value: u8;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "in al, dx",
                out("al") value,
                in("dx") port,
                options(nostack, preserves_flags)
            );
        }
        value
    }

    /// 向 I/O 端口写入双字 (out dx, eax)。
    #[inline(always)]
    fn outl(port: u16, value: u32) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "out dx, eax",
                in("dx") port,
                in("eax") value,
                options(nostack, preserves_flags)
            );
        }
    }

    /// 从 I/O 端口读取双字 (in eax, dx)。
    #[inline(always)]
    fn inl(port: u16) -> u32 {
        let value: u32;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "in eax, dx",
                out("eax") value,
                in("dx") port,
                options(nostack, preserves_flags)
            );
        }
        value
    }

    /// 关机 (8042 + triple fault 回退)。
    fn shutdown() -> ! {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!("mov al, 0xFE", "out 0x64, al", options(nomem, nostack));
        }
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!("lidt [0]", "int 3", options(nomem, nostack));
        }
        loop {
            core::hint::spin_loop();
        }
    }

    /// 重启 (键盘控制器 8042 → CPU reset)。
    fn reboot() -> ! {
        while <Self as SystemArch>::inb(0x64) & 2 != 0 {
            core::hint::spin_loop();
        }
        <Self as SystemArch>::outb(0x64, 0xFE);
        loop {
            <Self as CoreArch>::halt();
        }
    }
}

// ── Arch: 超 trait (空 body) ─────────────────────────────────────────

impl Arch for X8664 {}
