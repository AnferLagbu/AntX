//! x86-64 架构特定实现
//!
//! 包含 GDT, TSS, APIC, IOAPIC 等x86-64特有逻辑。
//!
//! ## Phase 2
//! - [x] `X8664` 结构体声明
//! - [x] `impl Arch for X8664` 完整实现 (所有 asm 封装)
//! - [ ] 调用方迁移 (Phase 3)

// ============================================================================
// 保留现有模块 (不动任何实现代码)
// ============================================================================

pub mod gdt;
pub mod tss;
pub mod apic;
pub mod ioapic;

// ============================================================================
// X8664 架构类型
// ============================================================================

/// x86_64 架构标记类型。
///
/// 零大小类型，通过 Arch trait 提供所有 x86_64 硬件操作。
pub struct X8664;

// ============================================================================
// impl Arch for X8664 — 完整 x86_64 硬件实现 (Phase 2)
// ============================================================================
//
// 每个方法用 #[inline(always)] 标记确保零开销。
// gdt_init() 等 x86 特有操作保留在 gdt.rs 内部，不入 trait。

use crate::kernel::arch::Arch;

impl Arch for X8664 {
    // ========================================================================
    // 中断控制
    // ========================================================================

    /// 禁用中断并返回 RFLAGS (含 IF 位)。
    ///
    /// x86_64: pushfq → pop rax → cli
    /// 返回值: 原始 RFLAGS 值，IF 位 (bit 9) 指示之前中断是否启用。
    #[inline(always)]
    fn interrupt_disable() -> usize {
        let flags: u64;
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

    /// 恢复中断状态。
    ///
    /// 仅当 flags 中 IF 位为 1 时才启用中断 (sti)。
    #[inline(always)]
    fn interrupt_restore(flags: usize) {
        if (flags as u64) & (1 << 9) != 0 {
            unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
        }
    }

    /// 启用中断 (sti)。
    #[inline(always)]
    fn interrupt_enable() {
        unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
    }

    /// 检查中断是否已启用。
    ///
    /// 读取 RFLAGS，检查 IF 位 (bit 9)。
    #[inline(always)]
    fn is_interrupt_enabled() -> bool {
        let flags: u64;
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

    // ========================================================================
    // CPU 控制
    // ========================================================================

    /// CPU 暂停等待中断 (hlt)。
    #[inline(always)]
    fn halt() {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }

    // ========================================================================
    // 内存管理单元 (MMU)
    // ========================================================================

    /// 刷新单个虚拟地址的 TLB (invlpg)。
    #[inline(always)]
    fn tlb_flush_page(vaddr: usize) {
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

    // ========================================================================
    // 上下文切换
    // ========================================================================

    /// 进程上下文切换。
    ///
    /// 调用汇编实现的 `process_switch_asm` (src/kernel/proc/switch.asm)。
    /// Phase 3 将汇编文件迁移至 arch/x86_64/context.asm 后不再依赖外部。
    #[inline(always)]
    fn context_switch(from: *mut u8, to: *const u8) {
        extern "C" {
            fn process_switch_asm(prev: *mut u8, next: *const u8);
        }
        unsafe { process_switch_asm(from, to); }
    }

    // ========================================================================
    // 用户态切换
    // ========================================================================

    /// 进入用户态执行 (iretq)。
    ///
    /// 构造 iretq 栈帧: SS → RSP → RFLAGS → CS → RIP。
    /// 参数 `arg` 传入 RDI (System V ABI 第一参数)。
    ///
    /// 此函数不会返回。
    #[inline(always)]
    fn enter_user(entry: usize, stack: usize, arg: usize) -> ! {
        // 用户态段选择子: RPL=3
        const USER_DS: u64 = 0x23; // GDT 用户数据段 (index 4 | RPL 3)
        const USER_CS: u64 = 0x1B; // GDT 用户代码段 (index 3 | RPL 3)
        const RFLAGS_IF: u64 = 0x202; // IF 位已置

        unsafe {
            core::arch::asm!(
                "cli",
                "mov ds, ax",
                "mov es, ax",
                "mov fs, ax",
                "mov gs, ax",
                "push rax",           // SS (USER_DS)
                "push rdx",           // RSP
                "push r8",            // RFLAGS
                "push r9",            // CS (USER_CS)
                "push r10",           // RIP (entry)
                "iretq",
                in("rax") USER_DS,
                in("rdx") stack,
                in("r8") RFLAGS_IF,
                in("r9") USER_CS,
                in("r10") entry,
                in("rdi") arg,        // 用户态入口参数
                options(noreturn)
            );
        }
    }

    /// 从内核态返回用户态 (iretq)。
    ///
    /// 假设栈上已有完整的 iretq 帧 (由中断入口构建)。
    /// 实际使用中通常由 context_switch 的 iretq 路径统一处理。
    #[inline(always)]
    fn return_to_user() {
        unsafe {
            core::arch::asm!(
                "iretq",
                options(noreturn)
            );
        }
    }

    // ========================================================================
    // CPU 信息
    // ========================================================================

    /// 获取当前 CPU ID (Local APIC ID)。
    #[inline(always)]
    fn cpu_id() -> u32 {
        // 优先使用 Local APIC (如果已初始化)
        use crate::kernel::arch::x86_64::apic;
        let id = apic::get_id();
        if id != 0 {
            return id;
        }
        // 回退: CPUID Leaf 1 → APIC ID in EBX[31:24]
        let (_, ebx, _, _) = crate::kernel::cpu::cpuid::cpuid(1, 0);
        (ebx >> 24) as u32
    }

    /// 获取高精度时间戳 (rdtsc)。
    #[inline(always)]
    fn timestamp() -> u64 {
        let lo: u32;
        let hi: u32;
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

    // ========================================================================
    // 内存屏障
    // ========================================================================

    /// 全内存屏障 (mfence)。
    #[inline(always)]
    fn fence() {
        unsafe {
            core::arch::asm!("mfence", options(nostack, preserves_flags));
        }
    }

    /// 写内存屏障 (sfence)。
    #[inline(always)]
    fn fence_w() {
        unsafe {
            core::arch::asm!("sfence", options(nostack, preserves_flags));
        }
    }

    // ========================================================================
    // 核间中断 (IPI)
    // ========================================================================

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

    // ========================================================================
    // 端口 I/O
    // ========================================================================

    /// 向 I/O 端口写入字节 (out dx, al)。
    #[inline(always)]
    fn outb(port: u16, value: u8) {
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags)
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
                options(nomem, nostack, preserves_flags)
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
                options(nomem, nostack, preserves_flags)
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
                options(nomem, nostack, preserves_flags)
            );
        }
        value
    }

    // ========================================================================
    // 系统控制
    // ========================================================================

    /// 关机 (键盘控制器 8042 + ACPI)。
    ///
    /// 优先尝试 ACPI shutdown，失败则通过键盘控制器触发 CPU reset。
    /// 此函数不会返回。
    fn shutdown() -> ! {
        // 尝试 8042 键盘控制器关机 (写入 0xFE)
        unsafe {
            core::arch::asm!(
                "mov al, 0xFE",
                "out 0x64, al",
                options(nomem, nostack)
            );
        }
        // 如果 8042 方法失败，尝试 triple fault
        unsafe {
            core::arch::asm!(
                "lidt [0]",
                "int 3",
                options(nomem, nostack)
            );
        }
        loop {
            core::hint::spin_loop();
        }
    }

    /// 重启 (键盘控制器 8042)。
    ///
    /// 向 8042 发送 0xFE 命令触发 CPU reset。
    /// 此函数不会返回。
    fn reboot() -> ! {
        // 等待 8042 输入缓冲区为空
        while Self::inb(0x64) & 2 != 0 {
            core::hint::spin_loop();
        }
        // 向 8042 发送 0xFE 命令触发 CPU reset
        Self::outb(0x64, 0xFE);
        // 如果 reset 失败，halt 等待
        loop {
            Self::halt();
        }
    }
}