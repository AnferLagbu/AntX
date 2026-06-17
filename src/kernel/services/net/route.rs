#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 路由表管理 — services 层策略主体
//!
//! ## T3-3 迁移记录
//!
//! 原属 framework/net/route.rs, 2026-06-16 提取到 services.
//! 纯策略代码 (路由表 CRUD + CIDR 匹配 + syscall), 0 unsafe.
//! smoltcp 同步逻辑留在 framework (依赖 raw::stack_mut).

use alloc::string::String;
use alloc::vec::Vec;

use crate::kernel::framework::sync::IrqSpinLock;
use crate::kernel::framework::syscall::Errno;

// ============================================================================
// 常量
// ============================================================================

/// 最大路由条目数
pub const MAX_ROUTES: usize = 16;

// ============================================================================
// 路由条目
// ============================================================================

/// 路由条目 — 内核级路由表示
#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// 目标 CIDR 地址 (4 字节 IPv4)
    pub dest: [u8; 4],
    /// 前缀长度 (0-32)
    pub prefix_len: u8,
    /// 下一跳网关 (4 字节 IPv4)
    pub gateway: [u8; 4],
    /// 接口名 (可选)
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
        self.prefix_len == 0 && self.dest == [0, 0, 0, 0]
    }
}

/// 路由查询结果
#[derive(Debug, Clone)]
pub struct RouteQueryResult {
    pub gateway: [u8; 4],
    pub prefix_len: u8,
}

// ============================================================================
// 内核路由表
// ============================================================================

static KERNEL_ROUTE_TABLE: IrqSpinLock<Vec<RouteEntry>> = IrqSpinLock::new(Vec::new());

// ============================================================================
// 路由操作
// ============================================================================

/// 添加路由条目
pub fn route_add(entry: RouteEntry) -> Result<(), Errno> {
    let mut table = KERNEL_ROUTE_TABLE.lock();

    if table.len() >= MAX_ROUTES {
        return Err(Errno::ENOMEM);
    }

    if table.iter().any(|r| {
        r.dest == entry.dest
            && r.prefix_len == entry.prefix_len
            && r.gateway == entry.gateway
    }) {
        return Err(Errno::EEXIST);
    }

    // 同步到 smoltcp 路由表 (委托 framework)
    if let Err(e) = crate::kernel::framework::net::route::sync_route_to_smoltcp(&entry) {
        return Err(e);
    }

    table.push(entry);
    Ok(())
}

/// 删除路由条目
pub fn route_del(dest: [u8; 4], prefix_len: u8, gateway: [u8; 4]) -> Result<(), Errno> {
    let mut table = KERNEL_ROUTE_TABLE.lock();

    let idx = table.iter().position(|r| {
        r.dest == dest && r.prefix_len == prefix_len && r.gateway == gateway
    });

    match idx {
        Some(i) => {
            table.remove(i);
            crate::kernel::framework::net::route::rebuild_smoltcp_routes(&table);
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

fn cidr_contains(entry: &RouteEntry, dest: &[u8; 4]) -> bool {
    if entry.prefix_len == 0 {
        return true;
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
// Syscall 接口
// ============================================================================

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

pub fn sys_route_query(dest_u32: u64) -> i64 {
    match route_query((dest_u32 as u32).to_be_bytes()) {
        Some(result) => u32::from_be_bytes(result.gateway) as i64,
        None => -(Errno::ENETUNREACH as i64),
    }
}
