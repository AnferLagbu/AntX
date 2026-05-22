//! AArch64 架构实现
//!
//! 子模块:
//! - context:   上下文切换 (context_switch_asm)
//! - mmu:       MMU/页表管理 (identity mapping, TTBR0_EL1)
//! - exception: 异常向量表 + handler (VBAR_EL1)
//! - gic:       GICv3 中断控制器初始化
//! - psci:      PSCI 电源管理 (关机/重启)

pub mod context;
pub mod exception;
pub mod gic;
pub mod mmu;
pub mod psci;
pub mod timer;
pub mod uart;

use core::arch::asm;

/// AArch64 CPU 架构实现 (Aarch64 结构体)
pub struct Aarch64;

impl super::Arch for Aarch64 {
    // ---- 中断 ----
    #[inline(always)]
    fn interrupt_disable() -> usize {
        let daif: u64;
        unsafe {
            asm!("mrs {}, daif", out(reg) daif);
            asm!("msr daifset, #2"); // Disable IRQ (bit 1)
        }
        daif as usize
    }

    #[inline(always)]
    fn interrupt_restore(flags: usize) {
        unsafe {
            asm!("msr daif, {}", in(reg) flags as u64);
        }
    }

    #[inline(always)]
    fn interrupt_enable() {
        unsafe {
            asm!("msr daifclr, #2"); // Enable IRQ
        }
    }

    #[inline(always)]
    fn is_interrupt_enabled() -> bool {
        let daif: u64;
        unsafe { asm!("mrs {}, daif", out(reg) daif); }
        (daif & (1 << 7)) == 0 // bit 7 = I (IRQ mask)
    }

    // ---- CPU 控制 ----
    #[inline(always)]
    fn halt() {
        unsafe { asm!("wfi", options(nomem, nostack)); }
    }

    // ---- MMU ----
    #[inline(always)]
    fn tlb_flush_page(vaddr: usize) {
        unsafe {
            asm!("dsb ishst", options(nomem, nostack));
            asm!("tlbi vaae1, {}", in(reg) (vaddr as u64 >> 12));
            asm!("dsb ish", options(nomem, nostack));
            asm!("isb", options(nomem, nostack));
        }
    }

    #[inline(always)]
    fn tlb_flush_all() {
        unsafe {
            asm!("dsb ishst", options(nomem, nostack));
            asm!("tlbi vmalle1", options(nomem, nostack));
            asm!("dsb ish", options(nomem, nostack));
            asm!("isb", options(nomem, nostack));
        }
    }

    #[inline(always)]
    fn read_page_table_base() -> u64 {
        mmu::read_ttbr0()
    }

    #[inline(always)]
    fn write_page_table_base(paddr: u64) {
        unsafe {
            asm!("msr ttbr0_el1, {}", in(reg) paddr);
            asm!("isb", options(nomem, nostack));
        }
    }

    #[inline(always)]
    fn read_fault_address() -> usize {
        mmu::read_far() as usize
    }

    // ---- 上下文切换 ----
    /// AArch64 上下文切换。
    ///
    /// 保存/恢复 x19-x30, SP, TTBR0_EL1, SPSR_EL1, ELR_EL1。
    /// 通过 `eret` 跳转到目标上下文。
    fn context_switch(from: *mut u8, to: *const u8) {
        unsafe { context::switch(from, to); }
    }

    // ---- 用户态切换 ----
    /// 从 EL1 进入 EL0 执行用户程序。
    ///
    /// - 设置 SP_EL0 = user stack
    /// - 设置 ELR_EL1 = user entry
    /// - 设置 SPSR_EL1 = EL0t, DAIF clear (user mode)
    /// - x0 = arg (用户程序参数)
    /// - eret 跳转到 EL0
    fn enter_user(entry: usize, stack: usize, arg: usize) -> ! {
        // SPSR_EL1: M[3:0] = 0b0000 (EL0t), DAIF = 0 (no mask)
        let spsr: u64 = 0x0000; // EL0t, all interrupts unmasked

        unsafe {
            asm!(
                "msr sp_el0, {sp}",
                "msr elr_el1, {entry}",
                "msr spsr_el1, {spsr}",
                "mov x0, {arg}",
                "eret",
                sp = in(reg) stack as u64,
                entry = in(reg) entry as u64,
                spsr = in(reg) spsr,
                arg = in(reg) arg as u64,
                options(noreturn),
            );
        }
    }

    /// 从 EL1 返回 EL0 (通过 eret, 使用当前 ELR/SPSR)。
    ///
    /// 典型场景: 系统调用返回后, 内核恢复用户态上下文并执行 eret。
    fn return_to_user() {
        unsafe {
            asm!("eret", options(noreturn));
        }
    }

    // ---- CPU 信息 ----
    #[inline(always)]
    fn cpu_id() -> u32 {
        let mpidr: u64;
        unsafe { asm!("mrs {}, mpidr_el1", out(reg) mpidr); }
        (mpidr & 0xFF) as u32
    }

    #[inline(always)]
    fn timestamp() -> u64 {
        let cnt: u64;
        unsafe { asm!("mrs {}, cntpct_el0", out(reg) cnt, options(nomem, nostack)); }
        cnt
    }

    // ---- 屏障 ----
    #[inline(always)]
    fn fence() {
        unsafe { asm!("dmb sy", options(nomem, nostack)); }
    }

    #[inline(always)]
    fn fence_w() {
        unsafe { asm!("dmb st", options(nomem, nostack)); }
    }

    // ---- IPI ----
    /// AArch64 GICv3 SGI (Software Generated Interrupt)。
    ///
    /// 通过 ICC_SGI1R_EL1 发送 SGI 到目标 CPU。
    fn send_ipi(target_cpu: u32, vector: u8) {
        // ICC_SGI1R_EL1:
        //   [23:16] TargetList = 1 << target_cpu
        //   [27:24] INTID = vector
        //   [46:44] IRM = 0 (route to specific)
        //   [40]    Aff3 = 0
        //   [39:32] Aff2 = 0
        //   [31:24] Aff1 = 0
        //   [15:0]  Aff0 = target_cpu
        let sgi: u64 = ((vector & 0xF) as u64) << 24          // INTID
                      | (1u64 << (16 + (target_cpu & 0xF)));   // TargetList
        unsafe {
            asm!("msr icc_sgi1r_el1, {}", in(reg) sgi);
        }
    }

    /// AArch64 GICv3 SGI 广播。
    fn broadcast_ipi(vector: u8) {
        // IRM = 1: 忽略 Affinity, 广播到所有 PE (不包括自己)
        let sgi: u64 = (1u64 << 40)                            // IRM
                      | ((vector & 0xF) as u64) << 24;         // INTID
        unsafe {
            asm!("msr icc_sgi1r_el1, {}", in(reg) sgi);
        }
    }

    // ---- 端口 IO (ARM 无 IO 端口空间) ----
    fn outb(_port: u16, _value: u8) {}
    fn inb(_port: u16) -> u8 { 0xFF }
    fn outl(_port: u16, _value: u32) {}
    fn inl(_port: u16) -> u32 { 0xFFFF_FFFF }

    // ---- 系统控制 ----
    fn shutdown() -> ! {
        unsafe {
            let func: u64 = 0x84000008; // PSCI SYSTEM_OFF
            asm!("smc #0", in("x0") func, options(nostack));
        }
        loop { unsafe { asm!("wfi", options(nomem, nostack)); } }
    }

    fn reboot() -> ! {
        unsafe {
            let func: u64 = 0x84000009; // PSCI SYSTEM_RESET
            asm!("smc #0", in("x0") func, options(nostack));
        }
        loop { unsafe { asm!("wfi", options(nomem, nostack)); } }
    }
}