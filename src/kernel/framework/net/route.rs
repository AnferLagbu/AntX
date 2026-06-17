//! 路由表管理 — framework 层 smoltcp 同步 + re-export
//!
//! ## T3-3 迁移记录
//!
//! 策略代码 (路由表 CRUD + CIDR 匹配 + syscall)
//! 已于 2026-06-16 迁移到 services::net::route.
//! 本文件仅保留 smoltcp 同步逻辑 (依赖 raw::stack_mut) + re-export.

use core::sync::atomic::Ordering;

use crate::kernel::framework::net::NET_CONFIGURED;
use crate::kernel::framework::errno::Errno;

// ============================================================================
// re-export services 层类型与函数
// ============================================================================

pub use crate::kernel::services::net::route::{
    RouteEntry, RouteQueryResult, MAX_ROUTES,
    route_add, route_del, route_query, route_list,
    sys_route_add, sys_route_del, sys_route_query,
};

// ============================================================================
// smoltcp 同步 (framework 机制, 依赖 raw::stack_mut)
// ============================================================================

/// 将单条路由同步到 smoltcp Routes
pub fn sync_route_to_smoltcp(entry: &RouteEntry) -> Result<(), Errno> {
    if !NET_CONFIGURED.load(Ordering::Acquire) {
        return Ok(());
    }

    #[cfg(not(feature = "kernel_test"))]
    {
        use smoltcp::wire::{Ipv4Address, Ipv4Cidr};
        use crate::kernel::framework::net::raw;

        let stack = match raw::stack_mut() {
            Some(s) => s,
            None => return Ok(()),
        };

        let gw = Ipv4Address::new(entry.gateway[0], entry.gateway[1], entry.gateway[2], entry.gateway[3]);
        let cidr = Ipv4Cidr::new(
            Ipv4Address::new(entry.dest[0], entry.dest[1], entry.dest[2], entry.dest[3]),
            entry.prefix_len,
        );

        stack.iface.routes_mut().update(|routes| {
            let route = smoltcp::iface::Route {
                cidr: cidr.into(),
                via_router: gw.into(),
                preferred_until: None,
                expires_at: None,
            };
            let _ = routes.push(route);
        });

        Ok(())
    }

    #[cfg(feature = "kernel_test")]
    {
        let _ = entry;
        Ok(())
    }
}

/// 从内核路由表全量重建 smoltcp Routes
pub fn rebuild_smoltcp_routes(table: &[RouteEntry]) {
    #[cfg(not(feature = "kernel_test"))]
    {
        use smoltcp::wire::{Ipv4Address, Ipv4Cidr};
        use crate::kernel::framework::net::raw;

        if !NET_CONFIGURED.load(Ordering::Acquire) {
            return;
        }

        let stack = match raw::stack_mut() {
            Some(s) => s,
            None => return,
        };

        stack.iface.routes_mut().update(|routes| {
            routes.clear();
            for entry in table {
                let gw = Ipv4Address::new(entry.gateway[0], entry.gateway[1], entry.gateway[2], entry.gateway[3]);
                let cidr = Ipv4Cidr::new(
                    Ipv4Address::new(entry.dest[0], entry.dest[1], entry.dest[2], entry.dest[3]),
                    entry.prefix_len,
                );
                let route = smoltcp::iface::Route {
                    cidr: cidr.into(),
                    via_router: gw.into(),
                    preferred_until: None,
                    expires_at: None,
                };
                let _ = routes.push(route);
            }
        });
    }

    #[cfg(feature = "kernel_test")]
    {
        let _ = table;
    }
}
