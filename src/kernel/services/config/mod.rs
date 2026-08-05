#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯常量与类型定义。
//! 内核配置常量 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/config/, 2026-06-16 提取到 services.
//! 纯常量与类型定义, 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export.

/// ConfigSummary 启动期编码 (原 framework/config/boot_image.rs)
pub mod boot_image;
/// 系统容量常量 (原 framework/config/capacity.rs)
pub mod capacity;
/// 内核能力与配置摘要类型 (原 framework/config/caps.rs)
pub mod caps;
/// 配置校验结果类型 (原 framework/config/error.rs)
pub mod error;
/// KASLR 配置常量与全局状态 (原 framework/config/kaslr.rs)
pub mod kaslr;
/// 内存布局常量 (原 framework/config/memory.rs)
pub mod memory;
/// /proc/sys/config 接口 (原 framework/config/procfs.rs)
pub mod procfs;
/// 调度器常量 (原 framework/config/sched.rs)
pub mod sched;
/// Slab 分配器配置常量 (原 framework/config/slab.rs)
pub mod slab;
/// LEGACY-6: 运行时 sysctl 框架
pub mod sysctl;
/// 启动自检 (原 framework/config/validate.rs)
pub mod validate;

pub use capacity::*;
pub use caps::*;
pub use error::ConfigError;
pub use kaslr::*;
pub use memory::*;
pub use sched::*;
pub use slab::*;
