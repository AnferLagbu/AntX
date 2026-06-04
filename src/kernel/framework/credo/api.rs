//! Credo 身份与权限框架 API 层
//!
//! 统一的身份管理 (PWM) / 能力矩阵 / 会话管理 / 审计入口,
//! 是 QueenX 安全子系统的对外契约面。
//!
//! ## 调用方契约
//! - `syscall::mod` —— `SYS_CREDO_LOGIN/LOGOUT/CREATE/DELETE/GRANT/REVOKE/CHECK_CAP` 等
//! - `fs::vfs` —— `vfs_open/write` 等文件操作前调用 `pwm_get_current()` 获取权限上下文
//! - `proc::api` —— 进程创建时分配 PWM, 销毁时回收
//! - `net::init` —— socket 操作前的权限校验
//! - `barrier::recovery` —— 会话状态纳入恢复域
//! - `console::gfx_console` —— 登录交互
//!
//! ## 内部接口
//! - `identity.rs` —— `IdentityTable`, PWM 生命周期管理
//! - `engine.rs` —— 能力检查引擎 (`check`/`get_caps`/`grant`/`revoke`)
//! - `session.rs` —— 会话管理器
//! - `storage.rs` —— 持久化 (sha256 + 序列化)
//! - `audit.rs` —— 审计日志
//! - `capability.rs` —— CapDomain / CapBits 能力矩阵
//!
//! ## 安全约束
//! - `pwm_init()` 必须单线程调用且只能调用一次 (AtomicBool 保护)
//! - 所有 `pwm_*` 函数内部使用 `identity::get_table()` 获取全局单例
//! - 密码传递走 `*const u8` C 风格字符串, 在入口处做 null 检查
//! - 能力检查在 engine 层用位运算, 无锁 (AtomicU64 矩阵)
//!
//! ## 性能特征
//! - 能力检查: O(1) 位运算, ≤ 5ns
//! - 身份查找: O(1) 哈希表
//! - 密码验证: SHA-256 计算, ~1μs
//!
//! ## 设计理念
//! - 无 Root 概念, 细粒度 CapDomain 矩阵
//! - 支持委托 (grant) 与撤销 (revoke)
//! - 完整审计追踪
//!
//! 所有公开函数使用 `#[no_mangle]` 以保证跨模块符号名稳定。

use super::audit;
use super::engine;
use super::identity;
use super::session;
use super::storage;
use super::types::*;
use crate::kernel::framework::lib::cstr::CStrExt;

macro_rules! klog_pwm {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_warn, $($arg)*)
    };
}

static INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[no_mangle]
pub fn pwm_init() {
    if INITIALIZED
        .compare_exchange(
            false,
            true,
            core::sync::atomic::Ordering::AcqRel,
            core::sync::atomic::Ordering::Relaxed,
        )
        .is_err()
    {
        return;
    }
    let t = identity::raw::get_table_mut();
    t.init();
    klog_pwm!("PWM v5 initialized");
}

#[no_mangle]
pub fn pwm_try_load() -> i32 {
    storage::load_database()
}

#[no_mangle]
pub fn pwm_any_identity_exists() -> bool {
    identity::get_table().any_identity_exists()
}

#[no_mangle]
pub fn pwm_try_genesis(password: *const u8) -> i64 {
    let pwd = password.as_kstr();
    match identity::raw::get_table_mut().bootstrap(pwd, "root") {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub fn pwm_create(
    password: *const u8,
    note: *const u8,
    creator_pwm: u64,
) -> i64 {
    let pwd = password.as_kstr();
    let nte = note.as_kstr();
    match identity::raw::get_table_mut().create(pwd, nte, creator_pwm) {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub fn pwm_delete(pwm: u64) -> i32 {
    match identity::get_table().delete(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_disable(pwm: u64) -> i32 {
    match identity::get_table().disable(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_enable(pwm: u64) -> i32 {
    match identity::get_table().enable(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_verify_password(pwm: u64, password: *const u8) -> bool {
    if password.is_null() {
        return false;
    }
    let pwd = password.as_kstr();
    identity::get_table().verify_password(pwm, pwd)
}

#[no_mangle]
pub fn pwm_change_password(
    pwm: u64,
    old: *const u8,
    new: *const u8,
) -> i32 {
    let o = old.as_kstr();
    let n = new.as_kstr();
    match identity::raw::get_table_mut().change_password(pwm, o, n) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_find(pwm: u64) -> bool {
    identity::find(pwm).is_some()
}

#[no_mangle]
pub fn pwm_find_entry(pwm: u64) -> *const PwmEntry {
    match identity::find(pwm) {
        Some(e) => e as *const PwmEntry,
        None => core::ptr::null(),
    }
}

#[no_mangle]
pub fn pwm_has_cap_raw(pwm: u64, domain: u16, _cap_bit: u8) -> u64 {
    engine::get_caps(pwm, CapDomain(domain)).as_u64()
}

#[no_mangle]
pub fn pwm_create_first_identity(password: *const u8) -> i64 {
    let pwd = password.as_kstr();
    match identity::raw::get_table_mut().bootstrap(pwd, "root") {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub fn pwm_get_fs_capability(pwm: u64) -> u64 {
    engine::get_caps(pwm, CapDomain::FS).as_u64()
}

#[no_mangle]
pub fn pwm_has_capability(pwm: u64, domain: u16, required: u64) -> bool {
    engine::check(pwm, CapDomain(domain), CapBits(required))
}

#[no_mangle]
pub fn pwm_get_capability_raw(pwm: u64, domain: u16) -> u64 {
    engine::get_caps(pwm, CapDomain(domain)).as_u64()
}

#[no_mangle]
pub fn pwm_get_privilege_level(pwm: u64) -> u8 {
    engine::get_privilege_level(pwm)
}

#[no_mangle]
pub fn pwm_get_creator(pwm: u64) -> u64 {
    engine::get_creator(pwm)
}

#[no_mangle]
pub fn pwm_grant(grantor_pwm: u64, grantee_pwm: u64, domain: u16, caps: u64) -> i32 {
    match identity::get_table().grant(grantor_pwm, grantee_pwm, CapDomain(domain), CapBits(caps)) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_revoke(revoker_pwm: u64, target_pwm: u64, domain: u16, caps: u64) -> i32 {
    match identity::get_table().revoke(revoker_pwm, target_pwm, CapDomain(domain), CapBits(caps)) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_transfer_creator(current_creator: u64, target: u64, new_creator: u64) -> i32 {
    match identity::get_table().transfer_creator(current_creator, target, new_creator) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_check_privilege(operator: u64, target: u64) -> bool {
    engine::check_privilege(operator, target)
}

#[no_mangle]
pub fn pwm_login(
    note: *const u8,
    password: *const u8,
) -> i64 {
    let n = note.as_kstr();
    let p = password.as_kstr();
    match session::login(n, p) {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub fn pwm_logout() {
    session::logout();
}

#[no_mangle]
pub fn pwm_get_current() -> u64 {
    session::get_current_pwm()
}

#[no_mangle]
pub fn pwm_get_current_entry() -> *const PwmEntry {
    session::get_current_entry()
}

#[no_mangle]
pub fn pwm_is_logged_in() -> bool {
    session::is_logged_in()
}

#[no_mangle]
pub fn pwm_get_current_uid() -> u32 {
    session::get_current_uid()
}

#[no_mangle]
pub fn pwm_get_current_gid() -> u32 {
    session::get_current_gid()
}

#[no_mangle]
pub fn pwm_get_euid() -> u32 {
    session::get_euid()
}

#[no_mangle]
pub fn pwm_get_egid() -> u32 {
    session::get_egid()
}

#[no_mangle]
pub fn pwm_elevate_for_suid(target_pwm: u64) -> bool {
    session::elevate_for_suid(target_pwm)
}

#[no_mangle]
pub fn pwm_drop_elevation() -> bool {
    session::drop_elevation()
}

#[no_mangle]
pub fn pwm_has_elevation_authority(target_pwm: u64) -> bool {
    session::has_elevation_authority(target_pwm)
}

#[no_mangle]
pub fn pwm_try_setuid(target_uid: u32) -> bool {
    session::try_setuid(target_uid)
}

#[no_mangle]
pub fn pwm_get_uid(pwm: u64) -> u32 {
    match identity::find(pwm) {
        Some(e) => e.get_uid(),
        None => 0xFFFFFFFF,
    }
}

#[no_mangle]
pub fn pwm_get_gid(pwm: u64) -> u32 {
    match identity::find(pwm) {
        Some(e) => e.get_gid(),
        None => 0xFFFFFFFF,
    }
}

#[no_mangle]
pub fn pwm_clear_lockout(pwm: u64) -> i32 {
    match session::clear_lockout(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn pwm_save_to_disk() -> i32 {
    storage::save_database()
}

#[no_mangle]
pub fn pwm_load_from_disk() -> i32 {
    storage::load_database()
}

#[no_mangle]
pub fn pwm_is_modified() -> bool {
    identity::get_table().is_modified()
}

#[no_mangle]
pub fn pwm_set_modified() {
    identity::get_table().set_modified();
}

#[no_mangle]
pub fn pwm_audit_log(pwm: u64, action: u32, target: u64, details: u64) {
    let act = match action {
        1 => AuditAction::Login,
        2 => AuditAction::Logout,
        3 => AuditAction::Create,
        4 => AuditAction::Delete,
        5 => AuditAction::Modify,
        8 => AuditAction::PasswordChange,
        10 => AuditAction::Grant,
        11 => AuditAction::Revoke,
        12 => AuditAction::TransferCreator,
        13 => AuditAction::FirstTokenGrant,
        _ => AuditAction::Modify,
    };
    audit::log(pwm, act, target, 0, details);
}

#[no_mangle]
pub fn pwm_audit_dump() {
    audit::dump();
}

#[no_mangle]
pub fn pwm_recover_first(
    password: *const u8,
    note: *const u8,
) -> i64 {
    let p = password.as_kstr();
    let n = note.as_kstr();
    match identity::get_table().recover_with_first(p, n) {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}
