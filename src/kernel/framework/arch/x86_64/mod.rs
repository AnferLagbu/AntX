//! x86_64 架构 TCB 抽象 (framework 内部)
//!
//! ## 当前状态: ⏳ 占位
//!
//! 实际实现仍在 [`kernel/arch/x86_64/`](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64):
//! - `gdt.rs` — GDT/TSS
//! - `apic.rs` / `ioapic.rs` — 中断控制器
//! - `acpi.rs` — 电源管理
//! - `smp_init.rs` / `trampoline.asm` — SMP 启动
//!
//! ## 目标结构
//!
//! 迁移后, 本模块应:
//! 1. 集中所有 x86_64 硬件操作 (GDT 写, IDT 加载, APIC 寄存器, etc.)
//! 2. 通过 `framework::arch::x86_64::Arch` trait 暴露给 framework 其他模块
//! 3. 外部模块 (services) 不直接 `use` 本模块, 走 framework 更高层 API
//!
//! 评估日期: 2026-06-03
