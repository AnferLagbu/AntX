//! 架构特定实现 (framework TCB, 仅内部可见)
//!
//! ## 当前状态: ⏳ 占位
//!
//! 实际架构相关代码**仍在** [`kernel/arch/`](file:///home/anfer/Code/AntX/src/kernel/arch) 老位置:
//! - `kernel/arch/x86_64/` — gdt/apic/ioapic/tss/acpi/smp_init
//! - `kernel/arch/aarch64/` — mmu/gic/context/exception/psci/timer/uart
//!
//! ## 迁移路径
//!
//! 1. 将 `kernel/arch/x86_64/{gdt,idt,apic,ioapic,tss,acpi}.rs` 复制/迁移到
//!    `framework/arch/x86_64/` 目录
//! 2. 在 `framework/arch/x86_64/mod.rs` 集中 re-export, 通过 framework::arch::x86_64::gdt_init
//!    暴露给 services
//! 3. `kernel/arch/` 老位置标记为 `#[deprecated]`, 引导 services 通过 framework 调用
//!
//! ## 估算: 0.5 人月
//!
//! 评估日期: 2026-06-03
//! 风险: context_switch 汇编代码是性能热点, 重命名后需重测调度延迟
