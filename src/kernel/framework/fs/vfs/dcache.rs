//! dcache + icache — framework re-export 层
//!
//! 实现已迁移到 `services::fs::dcache`, 本文件仅 re-export 公共 API.
//! framework 内部代码 (如 ramfs) 通过本模块引用 dcache, 保持路径兼容.

pub use crate::kernel::services::fs::dcache::{
    DCacheResult, ICacheResult,
    dcache_lookup, dcache_insert, dcache_insert_negative,
    dcache_invalidate_parent, dcache_flush,
    icache_lookup, icache_insert, icache_invalidate, icache_flush,
    flush_all,
    dcache_hit_rate, icache_hit_rate,
    dcache_count, icache_count,
    reset_stats,
};
