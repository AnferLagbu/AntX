#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯常量定义。
//! Slab 分配器配置常量 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/config/slab.rs, 2026-06-16 提取到 services.
//! 纯常量定义, 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export.

/// Default Slab cache size (4 KiB = one page).
pub const SLAB_DEFAULT_SIZE: usize = 4096;

/// Slab 对象最小尺寸 (字节).
pub const SLAB_MIN_OBJECT_SIZE: usize = 16;

/// Slab 对象最大尺寸 (字节).
pub const SLAB_MAX_OBJECT_SIZE: usize = 2048;

/// 通用 Slab 缓存数量.
pub const SLAB_GENERAL_CACHE_NUM: usize = 8;
