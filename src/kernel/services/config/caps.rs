#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯类型定义与编译期检测。
//! 内核能力与配置摘要类型 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/config/caps.rs, 2026-06-16 提取到 services.
//! 纯类型定义与编译期检测, 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export + 运行时查询函数.

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
    /// QueenX Barrier 子系统已编译入.
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
