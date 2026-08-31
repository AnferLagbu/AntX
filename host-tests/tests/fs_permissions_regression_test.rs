//! fs: B06-02/03/07 权限与句柄修复回归测试
//!
//! 验收:
//!   - chown_syscall: UID/GID 未注册返回 EINVAL, 不再回退 root (B06-02)
//!   - open_by_handle_at_syscall: 无 CAP_SYS_ADMIN (SYSTEM 域 0x01) 返回 EPERM (B06-03)
//!   - poll_syscall: fd 上限用 VFS_MAX_FDS (32) 而非 256, 防越界索引 32 长数组 (B06-07)
//!
//! 说明: 三个 syscall 策略均依赖内核全局状态 (credo identity 表 / capability 矩阵 /
//! VFS_MANAGER fd_table), 无法在 host-tests 直接调用; 故镜像其纯判定逻辑
//! (与 td26_access_cap_test 同模式). 若内核实现变更, 本镜像需同步更新.
//!
//! 追踪: B06-02 / B06-03 / B06-07
//! SPDX-License-Identifier: Apache-2.0

// ============================================================================
// B06-02: chown_syscall UID/GID 判定镜像
// ============================================================================

/// 镜像 [services/fs/file_ops.rs::chown_syscall] 的 uid→pwm 判定 (B06-02 修复后):
///
/// 原实现 `tbl.find_by_uid(uid).map_or(0, ...)` 在 uid 未注册时回退 owner_pwm=0 (root),
/// 存在提权漏洞; 修复后未注册 uid/gid 返回 `EINVAL` (errno=22), 不再默认 root.
///
/// `uid_table`: 已注册身份的 (posix_uid, pwm) 列表; 返回 Err(errno) 表示拒绝.
fn chown_uid_to_pwm(uid_table: &[(u32, u64)], uid: u32) -> Result<u64, i32> {
    const EINVAL: i32 = 22;
    match uid_table.iter().find(|(u, _)| *u == uid) {
        Some((_, pwm)) => Ok(*pwm),
        None => Err(EINVAL),
    }
}

#[test]
fn chown_registered_uid_returns_pwm() {
    // 已注册 uid 正常映射到对应 pwm, 不回退 0
    let table = [(0u32, 100u64), (42, 200), (1000, 300)];
    assert_eq!(chown_uid_to_pwm(&table, 0), Ok(100));
    assert_eq!(chown_uid_to_pwm(&table, 42), Ok(200));
    assert_eq!(chown_uid_to_pwm(&table, 1000), Ok(300));
}

#[test]
fn chown_unregistered_uid_returns_einval_not_root() {
    // 未注册 uid 必须返回 EINVAL, 不得回退 0 (root) — B06-02 核心
    let table = [(0u32, 100u64), (42, 200)];
    assert_eq!(chown_uid_to_pwm(&table, 7), Err(22));
    assert_eq!(chown_uid_to_pwm(&table, 43), Err(22));
    // 空表时任意 uid 都拒绝
    assert_eq!(chown_uid_to_pwm(&[], 0), Err(22));
}

#[test]
fn chown_max_uid_sentinel_rejected() {
    // uid == u32::MAX (Linux "(uid_t)-1" 哨兵) 未注册 → EINVAL, 而非回退 root
    let table = [(0u32, 100u64)];
    assert_eq!(chown_uid_to_pwm(&table, u32::MAX), Err(22));
}

// ============================================================================
// B06-03: open_by_handle_at_syscall CAP 检查镜像
// ============================================================================

/// 镜像 [services/fs/file_handle.rs::open_by_handle_at_syscall] 的权限检查 (B06-03):
///
/// 采用 SYSTEM 域 (domain=0) + CAP_SYS_ADMIN (0x01), 与 mount/umount2 先例一致;
/// 无能力返回 `EPERM` (errno=1).
fn open_by_handle_cap_check(pwm: u64, has_sys_admin: impl Fn(u64) -> bool) -> Result<(), i32> {
    const EPERM: i32 = 1;
    if has_sys_admin(pwm) {
        Ok(())
    } else {
        Err(EPERM)
    }
}

#[test]
fn open_by_handle_without_cap_returns_eperm() {
    // 无 CAP_SYS_ADMIN 的进程必须被拒绝 (原实现"允许所有已认证进程"是缺陷)
    let no_cap = |_pwm: u64| false;
    assert_eq!(open_by_handle_cap_check(42, no_cap), Err(1));
}

#[test]
fn open_by_handle_with_cap_allowed() {
    // 拥有 CAP_SYS_ADMIN 的进程放行
    let with_cap = |_pwm: u64| true;
    assert_eq!(open_by_handle_cap_check(42, with_cap), Ok(()));
}

// ============================================================================
// B06-07: poll_syscall fd 上限判定镜像
// ============================================================================

/// 镜像 [services/fs/file_ops.rs::poll_syscall] 的 fd→POLLIN 判定 (B06-07 修复后):
///
/// 原实现用硬编码 `< 256` 做上限后直接索引 32 长 fd_table, fd∈[32,255] 越界 panic;
/// 修复后上限改用 `VFS_MAX_FDS` (32), fd≥32 一律视为"不就绪"且不越界.
const VFS_MAX_FDS: usize = 32;

fn poll_fd_ready(fd: i32, fd_used: &[bool; VFS_MAX_FDS]) -> bool {
    if (fd as usize) < VFS_MAX_FDS && fd_used[fd as usize] {
        true
    } else {
        false
    }
}

#[test]
fn poll_fd_within_capacity_and_used() {
    // fd < VFS_MAX_FDS 且 fd 表项 used → 就绪 (POLLIN)
    let used = [false; VFS_MAX_FDS];
    let mut used = used;
    used[3] = true;
    assert!(poll_fd_ready(3, &used));
}

#[test]
fn poll_fd_within_capacity_unused() {
    // fd < VFS_MAX_FDS 但未 used → 不就绪
    let used = [false; VFS_MAX_FDS];
    assert!(!poll_fd_ready(3, &used));
}

#[test]
fn poll_fd_exactly_at_capacity_not_ready() {
    // fd == VFS_MAX_FDS (32): 原 256 上限会越界索引, 修复后边界安全
    let used = [false; VFS_MAX_FDS];
    assert!(!poll_fd_ready(32, &used));
}

#[test]
fn poll_fd_beyond_capacity_not_ready() {
    // fd ∈ (32, 256): 原实现在此区间越界索引 32 长数组 → 修复后安全跳过 (B06-07 核心)
    let used = [false; VFS_MAX_FDS];
    assert!(!poll_fd_ready(50, &used));
    assert!(!poll_fd_ready(255, &used));
    // fd ≥ 256 也安全跳过
    assert!(!poll_fd_ready(256, &used));
    assert!(!poll_fd_ready(1000, &used));
}

#[test]
fn poll_negative_fd_not_ready() {
    // 负 fd (POSIX 无效描述符) 不就绪
    let used = [false; VFS_MAX_FDS];
    assert!(!poll_fd_ready(-1, &used));
}
