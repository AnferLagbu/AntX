//! AArch64 架构特定实现 (Phase 1 stub)
//!
//! 当前为编译占位实现。所有方法均为 `unimplemented!()` 或返回默认值。
//! Phase 3+ 时将替换为真实的 ARM64 硬件操作实现。
//!
//! ARM64 与 x86_64 的关键差异:
//! - 中断: 使用 DAIF 寄存器而非 RFLAGS.IF
//! - MMU: TTBR0_EL1/TTBR1_EL1 而非 CR3
//! - TLB: tlbi vaae1/tlbi vmalle1 而非 invlpg/write CR3
//! - 端口 I/O: 不存在，使用 MMIO
//! - 时间戳: CNTVCT_EL0 而非 RDTSC
//! - 系统控制: PSCI 而非 ACPI
//!
//! ## Phase 1 状态
//! - [x] `Aarch64` 结构体声明
//! - [x] `impl Arch for Aarch64` stub
//! - [ ] 完整实现 (Phase 3+)

use crate::kernel::arch::Arch;

/// AArch64 架构标记类型。
///
/// Phase 1: stub 编译占位。
/// Phase 3+: 替换为真实 ARM64 硬件操作实现。
pub struct Aarch64;

// ============================================================================
// impl Arch for Aarch64 — stub (Phase 1, 编译占位)
// ============================================================================
// 所有方法为 stub，Phase 3+ 替换为真实 ARM64 实现。

#[allow(unused_variables)]
impl Arch for Aarch64 {
    // --- 中断控制 (stub: DAIF) ---
    fn interrupt_disable() -> usize { 0 }
    fn interrupt_restore(_flags: usize) {}
    fn interrupt_enable() {}
    fn is_interrupt_enabled() -> bool { false }

    // --- CPU 控制 (stub: WFI) ---
    fn halt() {}

    // --- MMU (stub: TTBR0_EL1, FAR_EL1) ---
    fn tlb_flush_page(_vaddr: usize) {}
    fn tlb_flush_all() {}
    fn read_page_table_base() -> u64 { 0 }
    fn write_page_table_base(_paddr: u64) {}
    fn read_fault_address() -> usize { 0 }

    // --- 上下文切换 (stub) ---
    fn context_switch(_from: *mut u8, _to: *const u8) {}

    // --- 用户态切换 (stub: ERET) ---
    fn enter_user(_entry: usize, _stack: usize, _arg: usize) -> ! { loop {} }
    fn return_to_user() {}

    // --- CPU 信息 (stub: MPIDR_EL1, CNTVCT_EL0) ---
    fn cpu_id() -> u32 { 0 }
    fn timestamp() -> u64 { 0 }

    // --- 内存屏障 (stub: DSB/DMB) ---
    fn fence() {}
    fn fence_w() {}

    // --- IPI (stub: GIC) ---
    fn send_ipi(_target_cpu: u32, _vector: u8) {}
    fn broadcast_ipi(_vector: u8) {}

    // --- 端口 I/O (ARM64 无端口 I/O，使用 MMIO) ---
    fn outb(_port: u16, _value: u8) {}
    fn inb(_port: u16) -> u8 { 0 }
    fn outl(_port: u16, _value: u32) {}
    fn inl(_port: u16) -> u32 { 0 }

    // --- 系统控制 (stub: PSCI) ---
    fn shutdown() -> ! { loop {} }
    fn reboot() -> ! { loop {} }
}