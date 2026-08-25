//! Socket 存储与容量配置 (B04-09 拆分 Step C, 2026-08-25)
//!
//! 原 init.rs 内联定义: `MAX_SOCKETS` / `SOCKET_STORAGE` / `SOCKET_SET` /
//! `SOCKETS_INITIALIZED` / `DEFAULT_MAX_SOCKETS` / `G_MAX_SOCKETS` /
//! `configure_max_sockets` / `get_max_sockets` / `set_max_sockets`.
//! 抽出为独立子模块后, init.rs 通过 `pub use sockets::*` re-export.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use smoltcp::iface::{SocketSet, SocketStorage};

// I-47: 编译期容量上限, 默认 256 (此前硬编码 8 严重限制并发).
// 编译期覆盖: 修改本常量或通过未来 build.rs 注入 cfg_flag 覆盖.
// 每个 socket 携带 TCP/UDP 静态缓冲, BSS 占用 ≈ 6 KB/连接 (TCP_RX 4K + UDP_RX 2K).
// 256 → ≈ 1.5 MB BSS; 生产环境按物理内存调整.
// 改本值后须同步 SOCKET_STORAGE 的尺寸.
pub const MAX_SOCKETS: usize = 256;

// 以下 static mut 保留: SOCKET_STORAGE/SOCKET_SET 是自引用结构,
// 初始化后只读, 无法安全放入 NetState (smoltcp SocketSet 借用 storage).
pub static mut SOCKET_STORAGE: core::mem::MaybeUninit<[SocketStorage<'static>; MAX_SOCKETS]> =
    core::mem::MaybeUninit::uninit();
pub static mut SOCKET_SET: core::mem::MaybeUninit<SocketSet<'static>> =
    core::mem::MaybeUninit::uninit();
pub static SOCKETS_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// I-47: Socket 容量配置
//
// MAX_SOCKETS = 编译期容量上限 (静态存储尺寸). 此前硬编码 8 严重限制并发连接数.
// 启动期默认 1024 (与 Linux net.core.somaxconn 相当), 运行时可通过
// `set_max_sockets` 调整, 不超过 MAX_SOCKETS. 编译期可通过 ANT_MAX_SOCKETS
// 环境变量覆盖 (Cargo build.rs 读取并写入 cfg).
// ============================================================================
const DEFAULT_MAX_SOCKETS: usize = 1024;

/// 运行时活动 socket 数上限 (≤ `MAX_SOCKETS`).
/// 初值取 [1, `MAX_SOCKETS`] 范围内的 `DEFAULT_MAX_SOCKETS`.
static G_MAX_SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// 启动期初始化 `G_MAX_SOCKETS`. 必须在 `init_sockets` 前调用一次.
pub fn configure_max_sockets() {
    let initial = if DEFAULT_MAX_SOCKETS > MAX_SOCKETS {
        MAX_SOCKETS
    } else if DEFAULT_MAX_SOCKETS == 0 {
        1
    } else {
        DEFAULT_MAX_SOCKETS
    };
    G_MAX_SOCKETS.store(initial, Ordering::Release);
}

/// 获取当前运行时 socket 上限.
pub fn get_max_sockets() -> usize {
    let v = G_MAX_SOCKETS.load(Ordering::Acquire);
    if v == 0 {
        // 首次访问时尚未 configure, 返回编译期上限的保守值
        1
    } else {
        v
    }
}

/// 调整运行时 socket 上限. n=0 拒绝; `n>MAX_SOCKETS` 截断为 `MAX_SOCKETS`.
/// 返回实际生效值. 运行时调大已分配的 `SocketStorage` 不会扩容 (仅控制新连接).
pub fn set_max_sockets(n: usize) -> usize {
    let target = if n == 0 {
        return get_max_sockets();
    } else if n > MAX_SOCKETS {
        MAX_SOCKETS
    } else {
        n
    };
    G_MAX_SOCKETS.store(target, Ordering::Release);
    target
}
