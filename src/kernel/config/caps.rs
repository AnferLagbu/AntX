//! 调试摘要与运行时能力查询
//!
//! `ConfigSummary` 汇总当前内核容量与运行时检测的能力位, 用于:
//! - `init()` 中的 ASCII 表格打印
//! - procfs `/sys/config` 读取
//! - 调试器命令 (若启用)

use super::capacity::{MAX_CPUS, MAX_IRQS, MAX_PROCESSES, MAX_THREADS};
use super::memory::PAGE_SIZE;

/// Configuration summary structure.
#[derive(Debug, Clone, Copy)]
pub struct ConfigSummary {
    pub max_cpus: usize,
    pub actual_cpus: u32,
    pub max_irqs: usize,
    pub max_processes: usize,
    pub max_threads: usize,
    pub apic_enabled: bool,
    pub ioapic_enabled: bool,
    pub page_size: u64,
    pub capabilities: KernelCapabilities,
}

/// Compile-time + runtime capability flags.
#[derive(Debug, Clone, Copy)]
pub struct KernelCapabilities {
    /// SMP enabled at compile time.
    pub smp: bool,
    /// Preempt-RT kernel.
    pub preempt: bool,
    /// Kernel address-space layout randomization.
    pub kaslr: bool,
    /// x86_64 KPTI mitigation.
    pub kpti: bool,
    /// AntX Barrier subsystem compiled in.
    pub barrier: bool,
}

impl KernelCapabilities {
    /// Detect capabilities from compile-time `cfg` flags.
    pub const fn detect() -> Self {
        Self {
            smp: cfg!(feature = "smp"),
            preempt: cfg!(feature = "preempt"),
            kaslr: cfg!(feature = "kaslr"),
            kpti: cfg!(target_arch = "x86_64"),
            barrier: cfg!(feature = "barrier"),
        }
    }
}

/// Get configuration summary for debugging.
pub fn get_config_summary() -> ConfigSummary {
    ConfigSummary {
        max_cpus: MAX_CPUS,
        actual_cpus: crate::kernel::smp::get_cpu_count(),
        max_irqs: MAX_IRQS,
        max_processes: MAX_PROCESSES,
        max_threads: MAX_THREADS,
        apic_enabled: cfg!(target_arch = "x86_64")
            && crate::kernel::arch::x86_64::apic::is_initialized(),
        ioapic_enabled: cfg!(target_arch = "x86_64")
            && crate::kernel::arch::x86_64::ioapic::is_initialized(),
        page_size: PAGE_SIZE,
        capabilities: KernelCapabilities::detect(),
    }
}
