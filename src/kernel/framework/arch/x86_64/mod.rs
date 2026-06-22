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

/// x86_64 架构标记类型。
///
/// 零大小类型，通过子 trait 提供所有 x86_64 硬件操作。
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
        ((hi as u64) << 32) | (lo as u64)
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
                extern "C" { fn syscall_entry(); }
                let entry_hi = syscall_entry as *const () as u64
                    + crate::kernel::framework::mm::KERNEL_BASE as u64;
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

    /// 进程上下文切换 (process_switch_asm)。
    #[inline(always)]
    fn context_switch(from: *mut u8, to: *const u8) {
        extern "C" {
            fn process_switch_asm(prev: *mut u8, next: *const u8);
        }
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            process_switch_asm(from, to);
        }
    }

    /// 进入用户态 (iretq)。
    #[inline(always)]
    fn enter_user(entry: usize, stack: usize, arg: usize, user_cr3: u64, kstack: u64) -> ! {
        const USER_DS: u64 = 0x1B;
        const USER_CS: u64 = 0x23;
        const RFLAGS_IF: u64 = 0x202;
        // KERNEL_BASE 高半部分偏移, 用于将低半部分地址转换为高半部分地址.
        const KBASE_HI: u64 = 0xFFFF800000000000u64;

        // SAFETY: 调用方保证 entry/stack/user_cr3/kstack 有效.
        // 策略:
        // 1. 先切换到进程内核栈 (kstack, 高半部分地址)
        // 2. 跳转到高半部分地址执行
        // 3. 在高半部分切换 CR3 到用户页表
        // 4. 加载段寄存器 + iretq
        // 这样 CR3 切换后, 栈和指令都在高半部分, 用户页表有高半部分映射.
        //
        // 仅使用 callee-saved 寄存器 (r12-r15) 传递关键参数,
        // 避免编译器寄存器分配冲突导致参数错位 (之前 in("r11") user_cr3
        // 被编译器重映射为 rdx 导致 CR3 写入错误值).
        // 常量通过 const 操作数传递, 由汇编器直接编码为立即数.
        // SAFETY: 见上方完整注释 — 调用方保证 entry/stack/user_cr3/kstack 有效.
        unsafe {
            core::arch::asm!(
                "cli",
                // 切换到进程内核栈 (高半部分地址)
                "mov rsp, r14",
                // 计算高半部分地址并跳转
                "lea rax, [rip + 2f]",
                "mov rcx, {kbase_hi}",
                "add rax, rcx",
                "jmp rax",
                "2:",
                // --- 以下在高半部分执行 ---
                "mov cr3, r15",                // 切换到用户页表
                "mov ecx, {user_ds}",
                "mov ds, cx",
                "mov es, cx",
                "mov fs, cx",
                "mov gs, cx",
                // ⚠ 教训 (TRACK-INIT-RING3): 此处不可 swapgs!
                //
                // 本内核的 GS 约定与 Linux 不同:
                //   IA32_GS_BASE      = 0 (从不显式设置)
                //   IA32_KERNEL_GS_BASE = per_cpu_addr (gdt_init 设置)
                //
                // 内核态常态: GS_BASE=0, KERNEL_GS_BASE=per_cpu_addr
                // 访问 per-CPU: swapgs → gs:offset → swapgs
                //
                // 若在此 swapgs, 会将 per_cpu_addr 交换到 GS_BASE,
                // 导致 syscall_entry 的 swapgs 将其换回 KERNEL_GS_BASE,
                // 使 GS_BASE=0, [gs:0x0] 访问地址 0 → page fault.
                // 此前曾在此处错误地插入了 swapgs, 导致 iretq 后
                // 第一个 syscall 即触发 page fault.
                "push {user_ds}",              // SS
                "push r13",                    // RSP (stack)
                "push {rflags_if}",            // RFLAGS
                "push {user_cs}",              // CS
                "push r12",                    // RIP (entry)
                "iretq",
                in("r12") entry,
                in("r13") stack,
                in("r14") kstack,
                in("r15") user_cr3,
                in("rdi") arg,
                kbase_hi = const KBASE_HI,
                user_ds = const USER_DS,
                user_cs = const USER_CS,
                rflags_if = const RFLAGS_IF,
                options(noreturn)
            );
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
