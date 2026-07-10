//! QueenX host-tests 库
//!
//! ## 职责划分 (按 Rust 官方测试组织标准)
//!
//! 本 crate 承载两类内容:
//!
//! ### 1. 库代码 (供 bin/ 和 tests/ 引用)
//! - `hvfs`        — 模拟内核中的 HvFS 文件系统实现, 内部通过 `crate::kernel` 树
//!                    引用 hvfs_mock 重导出的虚拟内核 API
//! - `hvfs_mock`   — host 端 std 桩, 模拟 `kernel::sync::mutex::Mutex` 等
//!                    内核 API 树, 让 HvFS 代码在无 OS 环境下 host-runnable
//! - `framekernel_bench` — 微基准测试库, 供 `bin/framekernel_bench` 调用
//!
//! ### 2. 单元测试 (内联在各源文件 `#[cfg(test)] mod tests`)
//! - `buddy`       — 伙伴分配器
//! - `capability`  — 能力位矩阵
//! - `checksum`    — Fletcher2/4 / SHA-256 / EdonR 校验和
//! - `sha256`      — SHA-256 纯算法
//! - `dma_stream`  — DMA 状态机 + 校验
//!
//! ## 集成测试 (Cargo 自动发现)
//! `tests/` 目录下的每个 `.rs` 文件被 Cargo 视为独立测试二进制, 不在
//! `src/lib.rs` 中声明, 避免双重编译.
//!
//! ## 性能基准 (binary)
//! `src/bin/framekernel_bench.rs` 调用 `framekernel_bench::run_all()` 输出 JSON.
//! `tests/` 中的 `framekernel_bench.rs` (经 [[test]] 删除后已移除) 不再作为
//! 测试入口, 单元测试仅随 lib 编译时运行一次.

#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::module_inception)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::explicit_counter_loop)]

// ── 单元测试载体模块 (内联 #[cfg(test)] mod tests) ──
mod buddy;
mod capability;
mod checksum;
mod dma_stream;
mod sha256;

// ── 公共库代码 (供 tests/ 与 bin/ 引用) ──
pub mod framekernel_bench;
pub mod fsx;
pub mod hvfs;
pub mod hvfs_mock;

// 重导出 hvfs_mock::kernel, 让 hvfs 子模块内部的 `use crate::kernel::*` 引用
// 在 host 端通过 hvfs_mock 的虚拟内核树解析. 这是 HvFS host-runnable 的
// 关键桩机制, 不能删除.
pub use hvfs_mock::kernel;
