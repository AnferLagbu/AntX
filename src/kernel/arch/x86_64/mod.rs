//! x86-64 架构特定实现
//!
//! 包含 GDT, TSS, APIC, IOAPIC 等x86-64特有逻辑。
//!
//! ## Phase 1
//! - [x] `X8664` 结构体声明 (空壳)
//! - [x] `impl Arch for X8664` stub (编译占位, Phase 2 填肉)
//! - [ ] 完整实现 (Phase 2)

// ============================================================================
// 保留现有模块 (不动任何实现代码)
// ============================================================================

pub mod gdt;
pub mod tss;
pub mod apic;
pub mod ioapic;

// ============================================================================
// X8664 架构类型 (Phase 1: 空壳, Phase 2: 填肉)
// ============================================================================

/// x86_64 架构标记类型。
///
/// Phase 1: 空壳编译占位。
/// Phase 2: 替换为实际硬件操作实现。
pub struct X8664;

// ============================================================================
// impl Arch for X8664 — stub (Phase 1)
// ============================================================================
// 以下所有方法为 Phase 1 stub。
// Phase 2 时将替换为真实的 x86_64 硬件操作实现。

use crate::kernel::arch::Arch;

#[allow(unused_variables)]
impl Arch for X8664 {
    // --- 中断控制 (stub) ---
    fn interrupt_disable() -> usize { 0 }
    fn interrupt_restore(_flags: usize) {}
    fn interrupt_enable() {}
    fn is_interrupt_enabled() -> bool { false }

    // --- CPU 控制 (stub) ---
    fn halt() {}

    // --- MMU (stub) ---
    fn tlb_flush_page(_vaddr: usize) {}
    fn tlb_flush_all() {}
    fn read_page_table_base() -> u64 { 0 }
    fn write_page_table_base(_paddr: u64) {}
    fn read_fault_address() -> usize { 0 }

    // --- 上下文切换 (stub) ---
    fn context_switch(_from: *mut u8, _to: *const u8) {}

    // --- 用户态切换 (stub) ---
    fn enter_user(_entry: usize, _stack: usize, _arg: usize) -> ! {
        loop {} // Phase 2: sysret
    }
    fn return_to_user() {}

    // --- CPU 信息 (stub) ---
    fn cpu_id() -> u32 { 0 }
    fn timestamp() -> u64 { 0 }

    // --- 内存屏障 (stub) ---
    fn fence() {}
    fn fence_w() {}

    // --- IPI (stub) ---
    fn send_ipi(_target_cpu: u32, _vector: u8) {}
    fn broadcast_ipi(_vector: u8) {}

    // --- 端口 I/O (stub) ---
    fn outb(_port: u16, _value: u8) {}
    fn inb(_port: u16) -> u8 { 0 }
    fn outl(_port: u16, _value: u32) {}
    fn inl(_port: u16) -> u32 { 0 }

    // --- 系统控制 (stub) ---
    fn shutdown() -> ! { loop {} }
    fn reboot() -> ! { loop {} }
}