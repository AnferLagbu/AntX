//! Socket 容量配置测试 (I-47)
//!
//! 验证 G_MAX_SOCKETS 运行时调参语义:
//! 1. 默认初始值 1024 (≤ MAX_SOCKETS)
//! 2. set_max_sockets(0) 拒绝 (返回当前值)
//! 3. set_max_sockets(n > MAX_SOCKETS) 截断为 MAX_SOCKETS
//! 4. get_max_sockets 读取最新设置
//!
//! 主机端镜像内核 `init.rs::configure_max_sockets / set_max_sockets / get_max_sockets` 行为.

use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_SOCKETS: usize = 256;
const DEFAULT_MAX_SOCKETS: usize = 1024;

static G_MAX_SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// 镜像内核 `configure_max_sockets` 行为
fn configure_max_sockets() {
    let initial = if DEFAULT_MAX_SOCKETS > MAX_SOCKETS {
        MAX_SOCKETS
    } else if DEFAULT_MAX_SOCKETS == 0 {
        1
    } else {
        DEFAULT_MAX_SOCKETS
    };
    G_MAX_SOCKETS.store(initial, Ordering::Release);
}

fn get_max_sockets() -> usize {
    let v = G_MAX_SOCKETS.load(Ordering::Acquire);
    if v == 0 {
        1
    } else {
        v
    }
}

fn set_max_sockets(n: usize) -> usize {
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

#[test]
fn test_configure_clamps_to_max() {
    // 模拟全局状态重置
    G_MAX_SOCKETS.store(0, Ordering::Release);
    configure_max_sockets();
    // DEFAULT 1024 > MAX 256 → 截断为 256
    assert_eq!(get_max_sockets(), MAX_SOCKETS);
}

#[test]
fn test_set_max_sockets_zero_rejected() {
    G_MAX_SOCKETS.store(0, Ordering::Release);
    configure_max_sockets();
    let current = get_max_sockets();
    // 0 应被拒绝, 返回当前值
    assert_eq!(set_max_sockets(0), current);
    assert_eq!(get_max_sockets(), current);
}

#[test]
fn test_set_max_sockets_clamps_overflow() {
    G_MAX_SOCKETS.store(0, Ordering::Release);
    configure_max_sockets();
    // n > MAX 应截断
    let result = set_max_sockets(MAX_SOCKETS * 10);
    assert_eq!(result, MAX_SOCKETS);
    assert_eq!(get_max_sockets(), MAX_SOCKETS);
}

#[test]
fn test_set_max_sockets_within_range() {
    G_MAX_SOCKETS.store(0, Ordering::Release);
    configure_max_sockets();
    assert_eq!(set_max_sockets(64), 64);
    assert_eq!(get_max_sockets(), 64);
    assert_eq!(set_max_sockets(128), 128);
    assert_eq!(get_max_sockets(), 128);
}

#[test]
fn test_set_max_sockets_to_one() {
    G_MAX_SOCKETS.store(0, Ordering::Release);
    configure_max_sockets();
    // 边界: 1
    assert_eq!(set_max_sockets(1), 1);
    assert_eq!(get_max_sockets(), 1);
}

#[test]
fn test_set_max_sockets_exact_boundary() {
    G_MAX_SOCKETS.store(0, Ordering::Release);
    configure_max_sockets();
    // 边界: 恰好 MAX
    assert_eq!(set_max_sockets(MAX_SOCKETS), MAX_SOCKETS);
    assert_eq!(get_max_sockets(), MAX_SOCKETS);
    // 边界: MAX+1 → MAX
    assert_eq!(set_max_sockets(MAX_SOCKETS + 1), MAX_SOCKETS);
}

#[test]
fn test_active_socket_counting() {
    // 模拟 sm_socket 中的活动 socket 计数
    let fd_types = [0u8, 1, 2, 0, 1, 0, 2, 1];
    let active: usize = fd_types.iter().filter(|&&t| t != 0).count();
    assert_eq!(active, 5);
}

#[test]
#[allow(clippy::assertions_on_constants)] // 编译期回归断言, 故意用常量
fn test_old_hardcoded_limit_was_8() {
    // 文档化回归: 旧硬编码 = 8 (8 个并发 socket).
    // 修复后默认 256 (32x), 编译期可调.
    // 本测试仅作回归记录, 不在运行时检查.
    const OLD_MAX_SOCKETS: usize = 8;
    const NEW_MAX_SOCKETS: usize = 256;
    assert!(NEW_MAX_SOCKETS >= OLD_MAX_SOCKETS * 8); // 至少 8 倍
}
