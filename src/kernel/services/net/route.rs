#![deny(unsafe_code)]
//! 路由表安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::net::route` 的安全 API, 供用户态程序和 syscall 层调用.

// 重导出强类型
pub use crate::kernel::framework::net::route::{
    RouteEntry, RouteQueryResult, MAX_ROUTES,
};

use crate::kernel::framework::net::route::{route_add, route_del, route_query, route_list};
use crate::kernel::framework::syscall::types::Errno;

/// 添加路由条目 (安全封装)
pub fn add(entry: RouteEntry) -> Result<(), Errno> {
    route_add(entry)
}

/// 删除路由条目 (安全封装)
pub fn del(dest: [u8; 4], prefix_len: u8, gateway: [u8; 4]) -> Result<(), Errno> {
    route_del(dest, prefix_len, gateway)
}

/// 查询路由 (最长前缀匹配, 安全封装)
pub fn query(dest: [u8; 4]) -> Option<RouteQueryResult> {
    route_query(dest)
}

/// 列出所有路由条目 (安全封装)
pub fn list() -> alloc::vec::Vec<RouteEntry> {
    route_list()
}
