//! dcache + icache — framework re-export 层
//!
//! 实现已迁移到 `services::fs::dcache`, 本文件仅 re-export 公共 API.
//! framework 内部代码 (如 ramfs) 通过本模块引用 dcache, 保持路径兼容.

pub use crate::kernel::services::fs::dcache::{
    DCacheResult, ICacheResult, dcache_count, dcache_flush, dcache_hit_rate, dcache_insert,
    dcache_insert_negative, dcache_invalidate_parent, dcache_lookup, flush_all, icache_count,
    icache_flush, icache_hit_rate, icache_insert, icache_invalidate, icache_lookup, reset_stats,
};
