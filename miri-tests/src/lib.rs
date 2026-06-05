//! Miri 验证 TCB 纯 Rust 算法库
//!
//! 本 crate 与 `queenx` 内核解耦, 专门用于在 Miri 解释器中扫描
//! TCB (Trusted Computing Base) 中的纯 Rust 算法, 检测以下 UB:
//!
//! | UB 类型 | Miri 检测 |
//! |---------|-----------|
//! | 越界访问 | `out-of-bounds` |
//! | use-after-free | `memory leaked` / `dangling reference` |
//! | 数据竞争 (RacyCell) | `data race` |
//! | 整数溢出 | `overflow` |
//! | 未初始化内存 | `read from uninitialized` |
//!
//! ## 覆盖范围
//!
//! - `racy_cell`: 跨线程无锁访问原语
//! - `frame`: 物理页分配 / 对齐计算
//! - `gf256`: GF(2^8) RAIDZ 算术
//! - `boot_image`: 引导映像编码 / 校验
//! - `validators`: 配置 / 内存布局校验
//!
//! ## 不在 Miri 覆盖范围
//!
//! - C ABI FFI (extern "C" 调用)
//! - 内联汇编 (asm!)
//! - 内核态上下文 (中断、锁、调度)
//! - 物理 MMIO
//!
//! 这些需依赖 3.3/3.4 别名检测与硬件测试。

// 不强制 unsafe_op_in_unsafe_fn, 让 unsafe fn 内部可省略 unsafe 块
#![warn(missing_debug_implementations)]

pub mod racy_cell;
pub mod frame;
pub mod gf256;
pub mod boot_image;
pub mod validators;
pub mod alias_registry;
pub mod dma;
pub mod arch_consistency;
pub mod credo_policy;
pub mod credo_grants;
pub mod credo_sessions;
pub mod credo_audit;
pub mod barrier_attribution;
pub mod user_proc_sync;
