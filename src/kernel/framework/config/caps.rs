//! 调试摘要与运行时能力查询 — framework 层 re-export + 运行时函数
//!
//! ## T6-9 迁移记录
//!
//! 类型定义 (ConfigSummary, KernelCapabilities, detect())
//! 已于 2026-06-16 迁移到 services::config::caps.
//! 本文件仅保留运行时查询函数 + re-export 保持调用方兼容.

use super::capacity::{MAX_CPUS, MAX_IRQS, MAX_PROCESSES, MAX_THREADS};
use super::kaslr::get_kaslr_offset;
use super::memory::PAGE_SIZE;

// re-export services 层类型
pub use crate::kernel::services::config::caps::{ConfigSummary, KernelCapabilities};

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
    crate::kernel::framework::arch::apic::is_initialized()
}

#[cfg(not(target_arch = "x86_64"))]
fn apic_initialized() -> bool {
    false
}

#[cfg(target_arch = "x86_64")]
fn ioapic_initialized() -> bool {
    crate::kernel::framework::arch::ioapic::is_initialized()
}

#[cfg(not(target_arch = "x86_64"))]
fn ioapic_initialized() -> bool {
    false
}
