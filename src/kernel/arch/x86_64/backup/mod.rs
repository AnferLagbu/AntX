//! x86-64 架构特定实现
//!
//! 包含 GDT, IDT, TSS, 分页等 x86-64 特有逻辑。

pub mod gdt;
pub mod tss;
// pub mod paging; // 未来扩展
