//! Slab 分配器配置常量
//!
//! `mm::slab` 通过 `pub use` 引用本模块, 避免重复定义。

/// Default Slab cache size (4 KiB = one page).
pub const SLAB_DEFAULT_SIZE: usize = 4096;

/// Minimum Slab object size (bytes).
pub const SLAB_MIN_OBJECT_SIZE: usize = 16;

/// Maximum Slab object size (bytes).
pub const SLAB_MAX_OBJECT_SIZE: usize = 2048;

/// Number of general-purpose Slab caches.
pub const SLAB_GENERAL_CACHE_NUM: usize = 8;
