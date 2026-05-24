//! AArch64 架构实现
//!
//! 子模块:
//! - context:   上下文切换 (context_switch_asm)
//! - mmu:       MMU/页表管理 (identity mapping, TTBR0_EL1)
//! - exception: 异常向量表 + handler (VBAR_EL1)
//! - gic:       GICv3 中断控制器初始化
//! - psci:      PSCI 电源管理 (关机/重启)
//! - timer:     ARM Generic Timer
//! - uart:      PL011 UART 驱动
//!
//! ## 实现状态
//! - [x] `impl CoreArch for Aarch64` — 基础核心能力
//! - [x] `impl InterruptArch for Aarch64` — DAIF + GICv3 SGI
//! - [x] `impl MmuArch for Aarch64` — TTBR0/1 + context switch + eret
//! - [x] `impl SystemArch for Aarch64` — PSCI + port IO stubs
//! - [x] `impl Arch for Aarch64` — 超 trait (空)
//! - [x] `barrier` — 栏栈恢复 (SGI 7 替代 int 0x82)

pub mod barrier;
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

use crate::kernel::arch::{CoreArch, InterruptArch, MmuArch, SystemArch, Arch};

// ── CoreArch: 基础核心 ──────────────────────────────────────────────────

impl CoreArch for Aarch64 {
    /// 获取当前 CPU ID (MPIDR_EL1 Aff0)。
    #[inline(always)]
    fn cpu_id() -> u32 {
        let mpidr: u64;
        unsafe { asm!("mrs {}, mpidr_el1", out(reg) mpidr); }
        (mpidr & 0xFF) as u32
    }

    /// 获取高精度时间戳 (CNTPCT_EL0)。
    #[inline(always)]
    fn timestamp() -> u64 {
        let cnt: u64;
        unsafe { asm!("mrs {}, cntpct_el0", out(reg) cnt, options(nomem, nostack)); }
        cnt
    }

    /// CPU 暂停等待中断 (wfi)。
    #[inline(always)]
    fn halt() {
        unsafe { asm!("wfi", options(nomem, nostack)); }
    }

    /// 全内存屏障 (dsb sy)。
    #[inline(always)]
    fn fence() {
        unsafe { asm!("dmb sy", options(nomem, nostack)); }
    }

    /// 写内存屏障 (dmb st)。
    #[inline(always)]
    fn fence_w() {
        unsafe { asm!("dmb st", options(nomem, nostack)); }
    }

    /// 读内存屏障 (dmb ld)。
    #[inline(always)]
    fn fence_r() {
        unsafe { asm!("dmb ld", options(nomem, nostack)); }
    }
}

// ── InterruptArch: 中断 + IPI ────────────────────────────────────────

impl InterruptArch for Aarch64 {
    /// 禁用 IRQ (DAIF bit 1) 并返回 DAIF。
    #[inline(always)]
    fn interrupt_disable() -> usize {
        let daif: u64;
        unsafe {
            asm!("mrs {}, daif", out(reg) daif);
            asm!("msr daifset, #2");
        }
        daif as usize
    }

    /// 恢复 DAIF。
    #[inline(always)]
    fn interrupt_restore(flags: usize) {
        unsafe {
            asm!("msr daif, {}", in(reg) flags as u64);
        }
    }

    /// 启用 IRQ (msr daifclr)。
    #[inline(always)]
    fn interrupt_enable() {
        unsafe {
            asm!("msr daifclr, #2");
        }
    }

    /// 检查 I (IRQ mask) bit。
    #[inline(always)]
    fn is_interrupt_enabled() -> bool {
        let daif: u64;
        unsafe { asm!("mrs {}, daif", out(reg) daif); }
        (daif & (1 << 7)) == 0
    }

    /// GICv3 SGI 单播 (ICC_SGI1R_EL1)。
    fn send_ipi(target_cpu: u32, vector: u8) {
        let sgi: u64 = ((vector & 0xF) as u64) << 24
                      | (1u64 << (16 + (target_cpu & 0xF)));
        unsafe {
            asm!("msr icc_sgi1r_el1, {}", in(reg) sgi);
        }
    }

    /// GICv3 SGI 广播 (IRM=1)。
    fn broadcast_ipi(vector: u8) {
        let sgi: u64 = (1u64 << 40)
                      | ((vector & 0xF) as u64) << 24;
        unsafe {
            asm!("msr icc_sgi1r_el1, {}", in(reg) sgi);
        }
    }
}

// ── MmuArch: MMU + 上下文 + 用户态 ───────────────────────────────────

impl MmuArch for Aarch64 {
    /// TLBI VA 单页刷新。
    #[inline(always)]
    fn tlb_flush_page(vaddr: usize) {
        unsafe {
            asm!("dsb ishst", options(nomem, nostack));
            asm!("tlbi vaae1, {}", in(reg) (vaddr as u64 >> 12));
            asm!("dsb ish", options(nomem, nostack));
            asm!("isb", options(nomem, nostack));
        }
    }

    /// TLBI VMALL 全刷新。
    #[inline(always)]
    fn tlb_flush_all() {
        unsafe {
            asm!("dsb ishst", options(nomem, nostack));
            asm!("tlbi vmalle1", options(nomem, nostack));
            asm!("dsb ish", options(nomem, nostack));
            asm!("isb", options(nomem, nostack));
        }
    }

    /// 读取 TTBR0_EL1。
    #[inline(always)]
    fn read_page_table_base() -> u64 {
        mmu::read_ttbr0()
    }

    /// 写入 TTBR0_EL1 + ISB。
    #[inline(always)]
    fn write_page_table_base(paddr: u64) {
        unsafe {
            asm!("msr ttbr0_el1, {}", in(reg) paddr);
            asm!("isb", options(nomem, nostack));
        }
    }

    /// 读取 FAR_EL1。
    #[inline(always)]
    fn read_fault_address() -> usize {
        mmu::read_far() as usize
    }

    /// AArch64 上下文切换 (x19-x30 + SP + TTBR0 + SPSR + ELR)。
    fn context_switch(from: *mut u8, to: *const u8) {
        unsafe { context::switch(from, to); }
    }

    /// 进入 EL0 (eret)。
    fn enter_user(entry: usize, stack: usize, arg: usize) -> ! {
        let spsr: u64 = 0x0000;
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

    /// 返回 EL0 (eret)。
    fn return_to_user() {
        unsafe {
            asm!("eret", options(noreturn));
        }
    }
}

// ── SystemArch: 端口 IO + 电源管理 ───────────────────────────────────

impl SystemArch for Aarch64 {
    fn outb(_port: u16, _value: u8) {}
    fn inb(_port: u16) -> u8 { 0xFF }
    fn outl(_port: u16, _value: u32) {}
    fn inl(_port: u16) -> u32 { 0xFFFF_FFFF }

    /// PSCI SYSTEM_OFF (SMC)。
    fn shutdown() -> ! {
        unsafe {
            let func: u64 = 0x84000008;
            asm!("smc #0", in("x0") func, options(nostack));
        }
        loop { unsafe { asm!("wfi", options(nomem, nostack)); } }
    }

    /// PSCI SYSTEM_RESET (SMC)。
    fn reboot() -> ! {
        unsafe {
            let func: u64 = 0x84000009;
            asm!("smc #0", in("x0") func, options(nostack));
        }
        loop { unsafe { asm!("wfi", options(nomem, nostack)); } }
    }
}

// ── Arch: 超 trait (空 body) ─────────────────────────────────────────

impl Arch for Aarch64 {}