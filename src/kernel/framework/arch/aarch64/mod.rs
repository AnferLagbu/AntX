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
//! - [x] `impl InterruptArch for Aarch64` — DAIF + GICv3 SGI 中断
//! - [x] `impl MmuArch for Aarch64` — TTBR0/1 + 上下文切换 + eret
//! - [x] `impl SystemArch for Aarch64` — PSCI + port IO 桩
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

use crate::kernel::framework::arch::{Arch, CoreArch, InterruptArch, MmuArch, SystemArch};

// ── CoreArch: 基础核心 ──────────────────────────────────────────────────

impl CoreArch for Aarch64 {
    /// 获取当前 CPU ID (MPIDR_EL1 Aff0)。
    #[inline(always)]
    fn cpu_id() -> u32 {
        let mpidr: u64;
        // SAFETY: mrs mpidr_el1 是只读系统寄存器，无副作用。
        unsafe {
            asm!("mrs {}, mpidr_el1", out(reg) mpidr);
        }
        (mpidr & 0xFF) as u32
    }

    /// 获取高精度时间戳 (CNTPCT_EL0)。
    #[inline(always)]
    fn timestamp() -> u64 {
        let cnt: u64;
        // SAFETY: mrs cntpct_el0 是只读系统寄存器。
        unsafe {
            asm!("mrs {}, cntpct_el0", out(reg) cnt, options(nomem, nostack));
        }
        cnt
    }

    /// CPU 暂停等待中断 (wfi)。
    #[inline(always)]
    fn halt() {
        // SAFETY: wfi 是标准 CPU 暂停指令，无内存副作用。
        unsafe {
            asm!("wfi", options(nomem, nostack));
        }
    }

    /// 全内存屏障 (dsb sy)。
    #[inline(always)]
    fn fence() {
        // SAFETY: dmb sy 是 aarch64 全系统内存屏障。
        unsafe {
            asm!("dmb sy", options(nomem, nostack));
        }
    }

    /// 写内存屏障 (dmb st)。
    #[inline(always)]
    fn fence_w() {
        // SAFETY: dmb st 是 aarch64 写内存屏障。
        unsafe {
            asm!("dmb st", options(nomem, nostack));
        }
    }

    /// 读内存屏障 (dmb ld)。
    #[inline(always)]
    fn fence_r() {
        // SAFETY: dmb ld 是 aarch64 读内存屏障。
        unsafe {
            asm!("dmb ld", options(nomem, nostack));
        }
    }
}

// ── InterruptArch: 中断 + IPI ────────────────────────────────────────

impl InterruptArch for Aarch64 {
    /// 禁用 IRQ (DAIF bit 1) 并返回 DAIF。
    #[inline(always)]
    fn interrupt_disable() -> usize {
        let daif: u64;
        // SAFETY: mrs daif 读取 + msr daifset #2 禁用 IRQ；都使用立即数，
        // 无内存副作用。必须在关中断前返回原 DAIF。
        unsafe {
            asm!("mrs {}, daif", out(reg) daif);
            asm!("msr daifset, #2");
        }
        daif as usize
    }

    /// 恢复 DAIF。
    #[inline(always)]
    fn interrupt_restore(flags: usize) {
        // SAFETY: msr daif 恢复由 interrupt_disable 保存的标志位；
        // 立即数 #2 写入 DAIF 启用 IRQ。
        unsafe {
            asm!("msr daif, {}", in(reg) flags as u64);
        }
    }

    /// 启用 IRQ (msr daifclr)。
    #[inline(always)]
    fn interrupt_enable() {
        // SAFETY: msr daifclr #2 启用 IRQ (清除 I 屏蔽位)。
        unsafe {
            asm!("msr daifclr, #2");
        }
    }

    /// 检查 I (IRQ mask) bit。
    #[inline(always)]
    fn is_interrupt_enabled() -> bool {
        let daif: u64;
        // SAFETY: mrs daif 是只读系统寄存器读取。
        unsafe {
            asm!("mrs {}, daif", out(reg) daif);
        }
        (daif & (1 << 7)) == 0
    }

    /// GICv3 SGI 单播 (ICC_SGI1R_EL1)。
    fn send_ipi(target_cpu: u32, vector: u8) {
        let sgi: u64 = ((vector & 0xF) as u64) << 24 | (1u64 << (16 + (target_cpu & 0xF)));
        // SAFETY: msr icc_sgi1r_el1 触发 GICv3 SGI；
        // 目标 CPU 与 vector 已 mask 至合法范围。
        unsafe {
            asm!("msr icc_sgi1r_el1, {}", in(reg) sgi);
        }
    }

    /// GICv3 SGI 广播 (IRM=1)。
    fn broadcast_ipi(vector: u8) {
        let sgi: u64 = (1u64 << 40) | ((vector & 0xF) as u64) << 24;
        // SAFETY: msr icc_sgi1r_el1 广播 SGI；IRM bit 40 设置为 1 触发广播。
        unsafe {
            asm!("msr icc_sgi1r_el1, {}", in(reg) sgi);
        }
    }

    fn interrupt_early_init() {
        // GICv3 + VBAR_EL1 已由 entry.rs / bootloader 配置
    }

    fn interrupt_late_init() {
        // GICv3 + 异常向量 + 定时器已由 entry.rs 配置
    }
}

// ── MmuArch: MMU + 上下文 + 用户态 ───────────────────────────────────

impl MmuArch for Aarch64 {
    /// TLBI VA 单页刷新。
    #[inline(always)]
    fn tlb_flush_page(vaddr: usize) {
        // SAFETY: 标准 TLB 单页失效序列；
        // vaddr 是有效内核虚拟地址 (>> 12 取页号)。
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
        // SAFETY: 标准 TLB 全失效序列 (VMALL E1)。
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
        // SAFETY: msr ttbr0_el1 写入新页表基址 + isb 同步流水线。
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
        // SAFETY: context::switch 是底层上下文切换函数；
        // from/to 由调度器提供保证指向有效 Process。
        unsafe {
            context::switch(from, to);
        }
    }

    /// 进入 EL0 (eret)。
    fn enter_user(entry: usize, stack: usize, arg: usize) -> ! {
        // KPTI: 首次进入 EL0 前, 切换 TTBR1_EL1 到 trampoline 页表.
        // 用户态运行时 TTBR1 指向最小化页表, 减少内核地址泄露面.
        // SAFETY: kpti_trampoline_ttbr1_or_kernel 返回有效物理地址;
        // 若 KPTI 未激活则返回 kernel_ttbr1 (无实际切换效果).
        unsafe {
            let tramp = crate::kernel::framework::mm::kpti::kpti_trampoline_ttbr1_or_kernel(
                crate::kernel::framework::mm::get_kernel_pml4(),
            );
            core::arch::asm!(
                "dsb ish",
                "msr ttbr1_el1, {0}",
                "isb",
                in(reg) tramp,
            );
        }

        let spsr: u64 = 0x0000;
        // SAFETY: 进入 EL0 标准序列 (msr sp_el0/elr_el1/spsr_el1 + eret)；
        // entry/stack/arg 由调用方提供合法用户态值；options(noreturn)。
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
        // KPTI: 返回 EL0 前, 切换 TTBR1_EL1 到 trampoline 页表.
        // SAFETY: 同 enter_user 中的 TTBR1 切换逻辑.
        unsafe {
            let tramp = crate::kernel::framework::mm::kpti::kpti_trampoline_ttbr1_or_kernel(
                crate::kernel::framework::mm::get_kernel_pml4(),
            );
            core::arch::asm!(
                "dsb ish",
                "msr ttbr1_el1, {0}",
                "isb",
                in(reg) tramp,
            );
        }
        // SAFETY: eret 是 aarch64 标准异常返回指令；
        // options(noreturn) 标识函数不会返回。
        unsafe {
            asm!("eret", options(noreturn));
        }
    }
}

// ── SystemArch: 端口 IO + 电源管理 ───────────────────────────────────

impl SystemArch for Aarch64 {
    fn outb(_port: u16, _value: u8) {}
    fn inb(_port: u16) -> u8 {
        0xFF
    }
    fn outl(_port: u16, _value: u32) {}
    fn inl(_port: u16) -> u32 {
        0xFFFF_FFFF
    }

    /// PSCI SYSTEM_OFF (SMC)。
    fn shutdown() -> ! {
        // SAFETY: smc #0 是 aarch64 安全监控调用；PSCI SYSTEM_OFF
        // 不会返回；loop + wfi 兜底防止固件未实现时的意外返回。
        unsafe {
            let func: u64 = 0x84000008;
            asm!("smc #0", in("x0") func, options(nostack));
        }
        loop {
            unsafe {
                asm!("wfi", options(nomem, nostack));
            }
        }
    }

    /// PSCI SYSTEM_RESET (SMC)。
    fn reboot() -> ! {
        // SAFETY: smc #0 + PSCI SYSTEM_RESET；不会返回。
        unsafe {
            let func: u64 = 0x84000009;
            asm!("smc #0", in("x0") func, options(nostack));
        }
        loop {
            unsafe {
                asm!("wfi", options(nomem, nostack));
            }
        }
    }
}

// ── Arch: 超 trait (空 body) ─────────────────────────────────────────

impl Arch for Aarch64 {}
