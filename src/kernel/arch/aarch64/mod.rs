//! AArch64 架构实现
//!
//! 子模块:
//! - mmu:       MMU/页表管理 (identity mapping, TTBR0_EL1)
//! - exception: 异常向量表 + handler (VBAR_EL1)
//! - gic:       GICv3 中断控制器初始化
//! - psci:      PSCI 电源管理 (关机/重启)

pub mod exception;
pub mod gic;
pub mod mmu;
pub mod psci;

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
    fn context_switch(_from: *mut u8, _to: *const u8) {
        // Phase 5 stub: TTBR0 + SP + ELR + 通用寄存器切换
        unimplemented!("context_switch for AArch64 (Phase 6)")
    }

    // ---- 用户态切换 ----
    fn enter_user(_entry: usize, _stack: usize, _arg: usize) -> ! {
        unimplemented!("enter_user for AArch64 (Phase 6)")
    }

    fn return_to_user() {
        // Phase 5 stub
        unimplemented!("return_to_user for AArch64 (Phase 6)")
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
    fn send_ipi(_target_cpu: u32, _vector: u8) {
        // Phase 5 stub: GIC SGI (Phase 6)
    }

    fn broadcast_ipi(_vector: u8) {
        // Phase 5 stub: GIC SGI broadcast (Phase 6)
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