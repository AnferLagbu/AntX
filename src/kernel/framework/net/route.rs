//! 路由表管理 (C5)
//!
//! 提供内核级路由条目的增删查功能, 与 smoltcp `Routes` 集成.
//!
//! ## 架构
//!
//! ```text
//! services/net/route.rs (safe 代理)
//!     │
//!     ▼
//! framework/net/route.rs (本文件, TCB)
//!     │
//!     ▼
//! smoltcp::iface::Routes (协议栈路由表)
//! ```
//!
//! ## 路由条目
//!
//! 每条路由包含:
//! - 目标 CIDR (如 192.168.1.0/24 或 0.0.0.0/0)
//! - 下一跳网关
//! - 掩码长度 (前缀)
//!
//! ## 与 smoltcp 的关系
//!
//! smoltcp `Routes` 使用 `heapless::Vec<Route, N>` 存储,
//! 本模块通过 `init::raw::stack_mut()` 访问 `iface.routes_mut()`
//! 来同步内核路由条目到协议栈.

use core::sync::atomic::Ordering;

use alloc::string::String;
use alloc::vec::Vec;

use crate::kernel::framework::net::types::NET_CONFIGURED;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 常量
// ============================================================================

/// 最大路由条目数 (与 smoltcp IFACE_MAX_ROUTE_COUNT 对齐)
pub const MAX_ROUTES: usize = 16;

// ============================================================================
// 路由条目 (内核表示)
// ============================================================================

/// 路由条目 — 内核级路由表示
///
/// 与 smoltcp `Route` 对应, 但使用 `alloc::Vec` 而非 `heapless::Vec`.
#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// 目标 CIDR 地址 (4 字节 IPv4)
    pub dest: [u8; 4],
    /// 前缀长度 (0-32)
    pub prefix_len: u8,
    /// 下一跳网关 (4 字节 IPv4)
    pub gateway: [u8; 4],
    /// 接口名 (可选, 当前未使用)
    pub iface: Option<String>,
}

impl RouteEntry {
    /// 构造默认路由 (0.0.0.0/0 via gateway)
    pub fn default_route(gateway: [u8; 4]) -> Self {
        Self {
            dest: [0, 0, 0, 0],
            prefix_len: 0,
            gateway,
            iface: None,
        }
    }

    /// 是否为默认路由
    pub fn is_default(&self) -> bool {
        self.prefix_len == 0
            && self.dest == [0, 0, 0, 0]
    }
}

/// 路由查询结果
#[derive(Debug, Clone)]
pub struct RouteQueryResult {
    /// 匹配的网关
    pub gateway: [u8; 4],
    /// 匹配的前缀长度
    pub prefix_len: u8,
}

// ============================================================================
// 内核路由表 (独立于 smoltcp, 用于 syscall 查询)
// ============================================================================

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

/// 内核路由表
///
/// 维护一份内核级路由条目副本, 同时同步到 smoltcp Routes.
/// smoltcp 负责实际数据包转发; 本表用于 syscall 查询和管理.
static KERNEL_ROUTE_TABLE: IrqSpinLock<Vec<RouteEntry>> = IrqSpinLock::new(Vec::new());

// ============================================================================
// 路由操作
// ============================================================================

/// 添加路由条目
///
/// 同时更新内核路由表和 smoltcp 路由表.
pub fn route_add(entry: RouteEntry) -> Result<(), Errno> {
    let mut table = KERNEL_ROUTE_TABLE.lock();

    // 容量检查
    if table.len() >= MAX_ROUTES {
        return Err(Errno::ENOMEM);
    }

    // 去重: 相同 CIDR + gateway 视为重复
    if table.iter().any(|r| {
        r.dest == entry.dest
            && r.prefix_len == entry.prefix_len
            && r.gateway == entry.gateway
    }) {
        return Err(Errno::EEXIST);
    }

    // 同步到 smoltcp 路由表
    if let Err(e) = sync_route_to_smoltcp(&entry) {
        return Err(e);
    }

    table.push(entry);
    Ok(())
}

/// 删除路由条目 (按 CIDR + gateway 匹配)
pub fn route_del(dest: [u8; 4], prefix_len: u8, gateway: [u8; 4]) -> Result<(), Errno> {
    let mut table = KERNEL_ROUTE_TABLE.lock();

    let idx = table.iter().position(|r| {
        r.dest == dest && r.prefix_len == prefix_len && r.gateway == gateway
    });

    match idx {
        Some(i) => {
            table.remove(i);
            // smoltcp 路由表重建 (简单策略: 清空后全量同步)
            rebuild_smoltcp_routes(&table);
            Ok(())
        }
        None => Err(Errno::ENOENT),
    }
}

/// 查询路由 (最长前缀匹配)
pub fn route_query(dest: [u8; 4]) -> Option<RouteQueryResult> {
    let table = KERNEL_ROUTE_TABLE.lock();

    table
        .iter()
        .filter(|r| cidr_contains(r, &dest))
        .max_by_key(|r| r.prefix_len)
        .map(|r| RouteQueryResult {
            gateway: r.gateway,
            prefix_len: r.prefix_len,
        })
}

/// 列出所有路由条目
pub fn route_list() -> Vec<RouteEntry> {
    KERNEL_ROUTE_TABLE.lock().clone()
}

// ============================================================================
// CIDR 匹配
// ============================================================================

/// 检查 dest 是否在 entry 的 CIDR 范围内
fn cidr_contains(entry: &RouteEntry, dest: &[u8; 4]) -> bool {
    if entry.prefix_len == 0 {
        return true; // 默认路由匹配所有
    }
    let mask = if entry.prefix_len >= 32 {
        0xFF_FF_FF_FFu32
    } else {
        !((1u32 << (32 - entry.prefix_len)) - 1)
    };
    let net_dest = u32::from_be_bytes(*dest) & mask;
    let net_entry = u32::from_be_bytes(entry.dest) & mask;
    net_dest == net_entry
}

// ============================================================================
// smoltcp 同步
// ============================================================================

/// 将单条路由同步到 smoltcp Routes
fn sync_route_to_smoltcp(entry: &RouteEntry) -> Result<(), Errno> {
    if !NET_CONFIGURED.load(Ordering::Acquire) {
        // 网络未配置, 仅记录到内核表, 不同步到 smoltcp
        return Ok(());
    }

    #[cfg(not(feature = "kernel_test"))]
    {
        use smoltcp::wire::{Ipv4Address, Ipv4Cidr};
        use crate::kernel::framework::net::init::raw;

        let stack = match raw::stack_mut() {
            Some(s) => s,
            None => return Ok(()), // 协议栈未初始化, 静默跳过
        };

        let gw = Ipv4Address::new(entry.gateway[0], entry.gateway[1], entry.gateway[2], entry.gateway[3]);
        let cidr = Ipv4Cidr::new(
            Ipv4Address::new(entry.dest[0], entry.dest[1], entry.dest[2], entry.dest[3]),
            entry.prefix_len,
        );

        stack.iface.routes_mut().update(|routes| {
            // 尝试添加; 满了则忽略 (内核表已记录, 协议栈表有限)
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
fn rebuild_smoltcp_routes(table: &[RouteEntry]) {
    #[cfg(not(feature = "kernel_test"))]
    {
        use smoltcp::wire::{Ipv4Address, Ipv4Cidr};
        use crate::kernel::framework::net::init::raw;

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

// ============================================================================
// Syscall 接口
// ============================================================================

/// sys_route_add — 添加路由条目
///
/// # 参数
/// - a0: dest[0..4] 作为 u32 (大端)
/// - a1: prefix_len
/// - a2: gateway[0..4] 作为 u32 (大端)
pub fn sys_route_add(dest_u32: u64, prefix_len: u64, gateway_u32: u64) -> i64 {
    let entry = RouteEntry {
        dest: (dest_u32 as u32).to_be_bytes(),
        prefix_len: if prefix_len > 32 { return -(Errno::EINVAL as i64); } else { prefix_len as u8 },
        gateway: (gateway_u32 as u32).to_be_bytes(),
        iface: None,
    };

    match route_add(entry) {
        Ok(()) => 0,
        Err(e) => -(e as i64),
    }
}

/// sys_route_del — 删除路由条目
///
/// # 参数
/// - a0: dest[0..4] 作为 u32 (大端)
/// - a1: prefix_len
/// - a2: gateway[0..4] 作为 u32 (大端)
pub fn sys_route_del(dest_u32: u64, prefix_len: u64, gateway_u32: u64) -> i64 {
    match route_del(
        (dest_u32 as u32).to_be_bytes(),
        if prefix_len > 32 { return -(Errno::EINVAL as i64); } else { prefix_len as u8 },
        (gateway_u32 as u32).to_be_bytes(),
    ) {
        Ok(()) => 0,
        Err(e) => -(e as i64),
    }
}

/// sys_route_query — 查询路由 (最长前缀匹配)
///
/// # 参数
/// - a0: dest[0..4] 作为 u32 (大端)
///
/// # 返回
/// - 成功: gateway 作为 u32 (大端) 正数
/// - 失败: 负数 errno
pub fn sys_route_query(dest_u32: u64) -> i64 {
    match route_query((dest_u32 as u32).to_be_bytes()) {
        Some(result) => u32::from_be_bytes(result.gateway) as i64,
        None => -(Errno::ENETUNREACH as i64),
    }
}
