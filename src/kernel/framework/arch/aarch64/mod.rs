//! AArch64 架构 TCB 抽象 (framework 内部)
//!
//! ## 当前状态: ⏳ 占位
//!
//! 实际实现仍在 [`kernel/arch/aarch64/`](file:///home/anfer/Code/AntX/src/kernel/arch/aarch64):
//! - `mmu.rs` — MMU/页表
//! - `gic.rs` — GICv3 中断控制器
//! - `context.rs` — 上下文切换
//! - `exception.rs` — 异常处理
//! - `psci.rs` — 电源管理
//! - `timer.rs` / `uart.rs` — 定时器/串口
//!
//! ## 目标结构
//!
//! 迁移后, 本模块应:
//! 1. 集中所有 aarch64 硬件操作 (TTBR0/1 切换, GIC SGI, eret, etc.)
//! 2. 通过 `framework::arch::aarch64::Arch` trait 暴露给 framework 其他模块
//! 3. 外部模块 (services) 不直接 `use` 本模块, 走 framework 更高层 API
//!
//! 评估日期: 2026-06-03
