//! AntX 内核 (纯 Rust 实现)
//!
//! ## 架构概览
//!
//! ```text
//! kernel/
//! ├── boot/          # 引导阶段 (Multiboot2, GRUB)
//! ├── arch/          # 架构相关 (GDT, IDT, TSS)
│   └── x86_64/      # x86-64 特定实现
//! ├── cpu/           # CPU 管理 (CPUID, MSR, TSC)
//! ├── smp/           # 多核支持 (AP 启动, IPI)
//! ├── interrupt/     # 中断子系统 (ISR, IRQ, IOAPIC)
//! ├── time/          # 时间管理 (PIT, HPET)
//! ├── memory/        # 内存管理 (PMM, VMM, Slab)
//! ├── logging/       # 日志系统 (klog, 格式化)
//! ├── fs/            # 文件系统 (VFS, mount)
//! └── meta/          # 元数据 (版本, 配置)
//! ```
//!
//! ## 设计理念
//!
//! - **功能复刻**: 理解 C 版本逻辑后用 Rust 惯用方式重写
//! - **类型安全**: 利用枚举、Option、Result 消除不安全代码
//! - **零成本抽象**: 关键路径性能与 C 版本相当
//! - **模块化**: 每个子系统独立可测试

pub mod boot;
pub mod arch;
pub mod cpu;
pub mod smp;
pub mod interrupt;
pub mod time;
pub mod memory;
pub mod logging;
pub mod fs;
pub mod meta;

// 重新导出常用类型 (方便其他模块使用)
pub use cpu::CpuInfo;
pub use logging::LogLevel;
