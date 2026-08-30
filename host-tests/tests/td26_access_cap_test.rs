//! TD-26: access_syscall 能力制校验回归 (B06-04 / DECISION-077 方案 A)
//!
//! 镜像内核 [src/kernel/services/fs/access.rs] 的 mode→FS_CAP 映射逻辑,
//! 验证:
//!   1. `F_OK` (mode=0) 不要求任何能力, 仅做存在性检查
//!   2. `R_OK`/`W_OK`/`X_OK` 正确映射到 `FS_CAP_READ/WRITE/EXECUTE`
//!   3. 组合 mode (位或) 映射为组合能力位
//!   4. 能力充足 → 放行; 能力不足 → EACCES
//!   5. mode 越界 → EINVAL (与内核 `0..=0o7` 校验一致)
//!
//! 说明: access_syscall 依赖内核全局 PWM 状态 + vfs_stat_safe 存在性检查,
//! 无法在 host-tests 直接调用; 故镜像其纯权限判定逻辑 (DECISION-077 方案 A),
//! 与 td21_early_vfs_eacces_test 同模式. 若内核实现变更, 本镜像需同步更新.

const EACCES: i32 = -13; // POSIX EACCES
const EINVAL: i32 = -22; // POSIX EINVAL

/// 镜像 [services/fs/access.rs] 的 R_OK/W_OK/X_OK/F_OK 常量
const F_OK: i32 = 0;
const R_OK: i32 = 4;
const W_OK: i32 = 2;
const X_OK: i32 = 1;

/// 镜像 [services/credo/capability.rs] 的 FS 能力位 (framework re-export 壳同义)
const FS_CAP_READ: u64 = 1 << 0;
const FS_CAP_WRITE: u64 = 1 << 1;
const FS_CAP_EXECUTE: u64 = 1 << 2;

/// 镜像 [framework/credo/api.rs::pwm_has_capability] 简化版:
/// pwm == u64::MAX 表示全权; pwm == 0 无能力; 其余按传入的能力掩码表判定.
fn pwm_has_capability(pwm: u64, domain: u16, required: u64) -> bool {
    if pwm == 0 {
        return false;
    }
    // 仅 FS 域 (domain=1) 参与判定; 非 FS 域在 access 中不使用.
    if domain != 1 {
        return false;
    }
    if pwm == u64::MAX {
        return true;
    }
    // 低 3 位 = FS_CAP_READ|WRITE|EXECUTE (与内核 VIABLE_FLOOR 语义一致:
    // 非全权进程仅拥有 FS_CAP_READ|FS_CAP_EXECUTE, 无 WRITE)
    let caps = pwm & 0b111;
    (caps & required) == required
}

/// 镜像 [services/fs/access.rs::access_syscall] 的权限判定逻辑 (DECISION-077 方案 A):
/// 返回 None = 通过, Some(errno) = 拒绝.
fn access_check(pwm: u64, mode: i32, path_exists: bool) -> Option<i32> {
    // 越界校验 (与内核 `0..=0o7` 一致)
    if !(0..=0o7).contains(&mode) {
        return Some(EINVAL);
    }
    // 能力制校验: F_OK 不要求能力
    let mut required_caps: u64 = 0;
    if mode & R_OK != 0 {
        required_caps |= FS_CAP_READ;
    }
    if mode & W_OK != 0 {
        required_caps |= FS_CAP_WRITE;
    }
    if mode & X_OK != 0 {
        required_caps |= FS_CAP_EXECUTE;
    }
    if required_caps != 0 && !pwm_has_capability(pwm, 1, required_caps) {
        return Some(EACCES);
    }
    // 存在性检查
    if !path_exists {
        return Some(EACCES);
    }
    None
}

#[test]
fn f_ok_requires_no_capability() {
    // F_OK (mode=0): 无能力进程也应通过 (仅存在性检查)
    assert_eq!(access_check(0, F_OK, true), None);
    // 路径不存在时仍拒绝
    assert_eq!(access_check(0, F_OK, false), Some(EACCES));
}

#[test]
fn r_ok_maps_to_fs_cap_read() {
    // R_OK → FS_CAP_READ (bit0)
    // 无 WRITE 能力的进程 (pwm=0b101 = READ|EXECUTE) 应通过 R_OK
    assert_eq!(access_check(0b101, R_OK, true), None);
    // 完全无能力 (pwm=0) 拒绝
    assert_eq!(access_check(0, R_OK, true), Some(EACCES));
    // 只有 EXECUTE 无 READ 的进程拒绝
    assert_eq!(access_check(0b100, R_OK, true), Some(EACCES));
}

#[test]
fn w_ok_maps_to_fs_cap_write() {
    // W_OK → FS_CAP_WRITE (bit1)
    // 内核 VIABLE_FLOOR 默认进程无 WRITE, 故非全权必拒绝
    assert_eq!(access_check(0b101, W_OK, true), Some(EACCES));
    // 拥有 WRITE 能力的进程通过
    assert_eq!(access_check(0b111, W_OK, true), None);
    // 全权进程通过
    assert_eq!(access_check(u64::MAX, W_OK, true), None);
}

#[test]
fn x_ok_maps_to_fs_cap_execute() {
    // X_OK → FS_CAP_EXECUTE (bit2)
    assert_eq!(access_check(0b101, X_OK, true), None);
    assert_eq!(access_check(0b001, X_OK, true), Some(EACCES));
    assert_eq!(access_check(u64::MAX, X_OK, true), None);
}

#[test]
fn combined_mode_maps_to_combined_caps() {
    // R_OK | W_OK → FS_CAP_READ | FS_CAP_WRITE
    assert_eq!(access_check(0b111, R_OK | W_OK, true), None);
    assert_eq!(access_check(0b101, R_OK | W_OK, true), Some(EACCES));
    // R_OK | X_OK → FS_CAP_READ | FS_CAP_EXECUTE
    assert_eq!(access_check(0b101, R_OK | X_OK, true), None);
    // 全权进程任意组合通过
    assert_eq!(access_check(u64::MAX, R_OK | W_OK | X_OK, true), None);
}

#[test]
fn invalid_mode_rejected() {
    // mode 越界 (>= 0o10) → EINVAL, 与内核 `0..=0o7` 校验一致
    assert_eq!(access_check(u64::MAX, 0o10, true), Some(EINVAL));
    assert_eq!(access_check(u64::MAX, 0o77, true), Some(EINVAL));
    assert_eq!(access_check(u64::MAX, -1, true), Some(EINVAL));
}

#[test]
fn capability_check_happens_before_existence() {
    // 能力不足时, 即使路径存在也拒绝 (能力检查先于存在性检查)
    assert_eq!(access_check(0b001, W_OK, true), Some(EACCES));
    // 能力不足且路径不存在 → 仍 EACCES
    assert_eq!(access_check(0b001, W_OK, false), Some(EACCES));
}

#[test]
fn viable_floor_process_can_read_execute() {
    // 内核 VIABLE_FLOOR (FS_CAP_READ|EXECUTE) 进程:
    // R_OK 与 X_OK 通过, W_OK 拒绝
    let viable_floor_pwm = FS_CAP_READ | FS_CAP_EXECUTE;
    assert_eq!(access_check(viable_floor_pwm, R_OK, true), None);
    assert_eq!(access_check(viable_floor_pwm, X_OK, true), None);
    assert_eq!(access_check(viable_floor_pwm, W_OK, true), Some(EACCES));
}
