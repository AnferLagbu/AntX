//! CPU 层面架构抽象封装
//!
//! Thin wrapper over `Arch` trait methods — CPU-level operations.
//! 所有对 `Arch` trait 的调用集中在此文件，方便未来多架构移植。
//!
//! ## Phase 1 状态
//! - [x] `cpu_id()` — 获取当前 CPU ID
//! - [x] `timestamp()` — 高精度时间戳
//!
//! ## 设计原则
//! - 零开销: 所有调用通过 `arch!()` 宏展开为静态分发
//! - 零依赖: 不引入额外的 trait 或抽象层
//! - 可替换: Phase 2/3 只需更新 Arch impl，此处无需改动

use crate::kernel::framework::arch::Arch;

/// 获取当前 CPU ID (APIC ID / MPIDR_EL1)。
#[inline(always)]
pub fn cpu_id() -> u32 {
    <crate::kernel::framework::arch::CurrentArch as Arch>::cpu_id()
}

/// 获取高精度时间戳 (rdtsc / CNTVCT_EL0)。
#[inline(always)]
pub fn timestamp() -> u64 {
    <crate::kernel::framework::arch::CurrentArch as Arch>::timestamp()
}

/// CPU 暂停直到中断 (hlt / wfi)。
#[inline(always)]
pub fn halt() {
    <crate::kernel::framework::arch::CurrentArch as Arch>::halt();
}

/// 发送核间中断到目标 CPU。
#[inline(always)]
pub fn send_ipi(target_cpu: u32, vector: u8) {
    <crate::kernel::framework::arch::CurrentArch as Arch>::send_ipi(target_cpu, vector);
}

/// 广播核间中断到所有 CPU。
#[inline(always)]
pub fn broadcast_ipi(vector: u8) {
    <crate::kernel::framework::arch::CurrentArch as Arch>::broadcast_ipi(vector);
}

/// 设置当前 CPU 的内核栈指针。
///
/// x86_64: 写入 TSS 的 RSP0 字段 (ring 0 栈)。
/// aarch64: 无操作 — SP_EL1 由上下文切换直接管理。
#[inline(always)]
pub fn set_kernel_stack(_stack: u64) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        crate::kernel::framework::arch::x86_64::tss::tss_set_kernel_stack(_stack);
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = _stack;
    }
}
