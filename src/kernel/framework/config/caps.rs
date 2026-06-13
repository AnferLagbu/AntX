//! 调试摘要与运行时能力查询
//!
//! `ConfigSummary` 汇总当前内核容量与运行时检测的能力位, 用于:
//! - `init()` 中的 ASCII 表格打印
//! - procfs `/sys/config` 读取
//! - 调试器命令 (若启用)

use super::capacity::{MAX_CPUS, MAX_IRQS, MAX_PROCESSES, MAX_THREADS};
use super::kaslr::get_kaslr_offset;
use super::memory::PAGE_SIZE;

/// 配置摘要结构.
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
    /// 演进 9: 运行时 KASLR 偏移 (由 bootloader/entry 设置).
    pub kaslr_offset: u64,
    pub capabilities: KernelCapabilities,
}

/// 编译期 + 运行时能力标志.
#[derive(Debug, Clone, Copy)]
pub struct KernelCapabilities {
    /// 编译期启用了 SMP.
    pub smp: bool,
    /// Preempt-RT 内核.
    pub preempt: bool,
    /// 内核地址空间布局随机化.
    pub kaslr: bool,
    /// x86_64 KPTI 缓解措施.
    pub kpti: bool,
    /// AntX Barrier 子系统已编译入.
    pub barrier: bool,
}

impl KernelCapabilities {
    /// 从编译期 `cfg` 标志检测能力.
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

/// 获取配置摘要用于调试.
pub fn get_config_summary() -> ConfigSummary {
    ConfigSummary {
        max_cpus: MAX_CPUS,
        actual_cpus: crate::kernel::framework::smp::get_cpu_count(),
        max_irqs: MAX_IRQS,
        max_processes: MAX_PROCESSES,
        max_threads: MAX_THREADS,
        apic_enabled: apic_initialized(),
        ioapic_enabled: ioapic_initialized(),
        page_size: PAGE_SIZE,
        kaslr_offset: get_kaslr_offset(),
        capabilities: KernelCapabilities::detect(),
    }
}

/// 跨架构安全: 在 x86_64 上查 APIC 状态, 其他架构默认 false。
#[cfg(target_arch = "x86_64")]
fn apic_initialized() -> bool {
    crate::kernel::framework::arch::x86_64::apic::is_initialized()
}

#[cfg(not(target_arch = "x86_64"))]
fn apic_initialized() -> bool {
    false
}

#[cfg(target_arch = "x86_64")]
fn ioapic_initialized() -> bool {
    crate::kernel::framework::arch::x86_64::ioapic::is_initialized()
}

#[cfg(not(target_arch = "x86_64"))]
fn ioapic_initialized() -> bool {
    false
}
