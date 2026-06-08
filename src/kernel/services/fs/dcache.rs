//! dcache + icache 安全代理 — services 层
//!
//! 将 framework::fs::vfs::dcache 的 TCB 接口封装为 100% safe Rust API。
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use crate::kernel::services::fs::dcache;
//!
//! // 路径解析时先查 dcache
//! match dcache::lookup(parent_ino, "bin") {
//!     dcache::DCacheResult::Hit { ino, file_type } => { /* 使用缓存 */ }
//!     dcache::DCacheResult::Negative => { /* 确定不存在 */ }
//!     dcache::DCacheResult::Miss => { /* 回退到原始解析 */ }
//! }
//!
//! // 解析成功后插入缓存
//! dcache::insert(parent_ino, "bin", ino, file_type);
//!
//! // 文件创建/删除时失效
//! dcache::invalidate_parent(parent_ino);
//! ```

#![deny(unsafe_code)]

// Re-export 类型
pub use crate::kernel::framework::fs::vfs::dcache::{
    DCacheResult, ICacheResult,
};

/// dcache 查找
pub fn lookup(parent_ino: u32, name: &str) -> DCacheResult {
    crate::kernel::framework::fs::vfs::dcache::dcache_lookup(parent_ino, name)
}

/// dcache 插入 (正缓存)
pub fn insert(parent_ino: u32, name: &str, ino: u32, file_type: u8) {
    crate::kernel::framework::fs::vfs::dcache::dcache_insert(parent_ino, name, ino, file_type);
}

/// dcache 插入 (负缓存: 该路径不存在)
pub fn insert_negative(parent_ino: u32, name: &str) {
    crate::kernel::framework::fs::vfs::dcache::dcache_insert_negative(parent_ino, name);
}

/// dcache 失效: 指定父目录下所有条目
pub fn invalidate_parent(parent_ino: u32) {
    crate::kernel::framework::fs::vfs::dcache::dcache_invalidate_parent(parent_ino);
}

/// dcache 失效: 指定条目
pub fn invalidate_entry(parent_ino: u32, name: &str) {
    crate::kernel::framework::fs::vfs::dcache::dcache_invalidate_entry(parent_ino, name);
}

/// dcache 清空
pub fn flush() {
    crate::kernel::framework::fs::vfs::dcache::dcache_flush();
}

/// icache 查找
pub fn icache_lookup(ino: u32) -> Option<ICacheResult> {
    crate::kernel::framework::fs::vfs::dcache::icache_lookup(ino)
}

/// icache 插入/更新
pub fn icache_insert(ino: u32, file_type: u8, perm: u16, size: u32, mtime: u64) {
    crate::kernel::framework::fs::vfs::dcache::icache_insert(ino, file_type, perm, size, mtime);
}

/// icache 失效
pub fn icache_invalidate(ino: u32) {
    crate::kernel::framework::fs::vfs::dcache::icache_invalidate(ino);
}

/// icache 清空
pub fn icache_flush() {
    crate::kernel::framework::fs::vfs::dcache::icache_flush();
}

/// 同时清空 dcache + icache
pub fn flush_all() {
    crate::kernel::framework::fs::vfs::dcache::flush_all();
}

/// dcache 命中率 (hits, lookups)
pub fn dcache_hit_rate() -> (u64, u64) {
    crate::kernel::framework::fs::vfs::dcache::dcache_hit_rate()
}

/// icache 命中率 (hits, lookups)
pub fn icache_hit_rate() -> (u64, u64) {
    crate::kernel::framework::fs::vfs::dcache::icache_hit_rate()
}

/// dcache 条目数
pub fn dcache_count() -> usize {
    crate::kernel::framework::fs::vfs::dcache::dcache_count()
}

/// icache 条目数
pub fn icache_count() -> usize {
    crate::kernel::framework::fs::vfs::dcache::icache_count()
}

/// 重置统计计数器
pub fn reset_stats() {
    crate::kernel::framework::fs::vfs::dcache::reset_stats();
}
