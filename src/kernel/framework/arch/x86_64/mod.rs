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

use crate::kernel::arch::{Arch, CoreArch, InterruptArch, MmuArch, SystemArch};

// ── CoreArch: 基础核心 ──────────────────────────────────────────────────

impl CoreArch for X8664 {
    /// 获取当前 CPU ID (Local APIC ID)。
    #[inline(always)]
    fn cpu_id() -> u32 {
        use crate::kernel::arch::x86_64::apic;
        let id = apic::get_id();
        if id != 0 {
            return id;
        }
        let (_, ebx, _, _) = crate::kernel::cpu::cpuid::cpuid(1, 0);
        ebx >> 24
    }

    /// 获取高精度时间戳 (rdtsc)。
    #[inline(always)]
    fn timestamp() -> u64 {
        let lo: u32;
        let hi: u32;
        // SAFETY: rdtsc is a serializing instruction that writes EAX/EDX; we
        // declare nostack/nomem/preserves_flags so the compiler does not
        // reorder or spill state across it.
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
        // SAFETY: hlt halts the CPU until the next interrupt; it touches no
        // memory or registers visible to Rust. nomem/nostack is correct.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }

    /// 全内存屏障 (mfence)。
    #[inline(always)]
    fn fence() {
        // SAFETY: mfence orders all loads/stores to memory; no register
        // clobbers declared, preserves_flags is correct.
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
        // SAFETY: pushes RFLAGS, pops into a general-purpose register, then
        // disables interrupts via cli. nomem/nostack/preserves_flags are all
        // satisfied by the instruction sequence.
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
            // SAFETY: sti enables interrupts; nomem/nostack holds, no
            // observable side-effect on memory.
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
        // SAFETY: pushfq/pop sequence only reads RFLAGS into a register;
        // flags remain preserved for caller. No memory or stack touched.
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
        use crate::kernel::arch::x86_64::apic;
        apic::send_ipi(target_cpu as u8, vector);
    }

    /// 广播 IPI 到所有 CPU (不含自身)。
    #[inline(always)]
    fn broadcast_ipi(vector: u8) {
        use crate::kernel::arch::x86_64::apic;
        apic::broadcast_ipi(vector);
    }

    fn interrupt_early_init() {
        crate::kernel::idt::idt_init();
    }

    fn interrupt_late_init() {
        crate::kernel::arch::x86_64::gdt::gdt_init();
        crate::kernel::idt::idt_init();
        crate::kernel::arch::x86_64::apic::apic_init();
        crate::kernel::smp::init();
        crate::kernel::arch::x86_64::smp_init::init();
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
        // SAFETY: reading and re-writing CR3 forces a full TLB flush; the
        // intermediate register use is a direct move between GPR and CR3.
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
        unsafe {
            process_switch_asm(from, to);
        }
    }

    /// 进入用户态 (iretq)。
    #[inline(always)]
    fn enter_user(entry: usize, stack: usize, arg: usize) -> ! {
        const USER_DS: u64 = 0x1B;
        const USER_CS: u64 = 0x23;
        const RFLAGS_IF: u64 = 0x202;

        unsafe {
            core::arch::asm!(
                "cli",
                "mov ds, ax",
                "mov es, ax",
                "mov fs, ax",
                "mov gs, ax",
                "push rax", "push rdx", "push r8", "push r9", "push r10",
                "iretq",
                in("rax") USER_DS,
                in("rdx") stack,
                in("r8") RFLAGS_IF,
                in("r9") USER_CS,
                in("r10") entry,
                in("rdi") arg,
                options(noreturn)
            );
        }
    }

    /// 返回用户态 (iretq)。
    #[inline(always)]
    fn return_to_user() {
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
        unsafe {
            core::arch::asm!("mov al, 0xFE", "out 0x64, al", options(nomem, nostack));
        }
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
