use super::audit;
use super::engine;
use super::identity;
use super::session;
use super::storage;
use super::types::*;

macro_rules! klog_pwm {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_warn, $($arg)*)
    };
}

static INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn pwm_init() {
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
    let t = unsafe { identity::get_table_mut() };
    t.init();
    klog_pwm!("PWM v5 initialized");
}

#[no_mangle]
pub extern "C" fn pwm_try_load() -> i32 {
    storage::load_database()
}

#[no_mangle]
pub extern "C" fn pwm_any_identity_exists() -> bool {
    identity::get_table().any_identity_exists()
}

#[no_mangle]
pub extern "C" fn pwm_try_genesis(password: *const core::ffi::c_char) -> i64 {
    if password.is_null() {
        return PwmError::InvalidPassword.as_i32() as i64;
    }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) };
    let pwd_str = pwd.to_str().unwrap_or("");
    match identity::get_table().bootstrap(pwd_str, "root") {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwm_create(
    password: *const core::ffi::c_char,
    note: *const core::ffi::c_char,
    creator_pwm: u64,
) -> i64 {
    if password.is_null() || note.is_null() {
        return PwmError::InvalidPassword.as_i32() as i64;
    }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) }
        .to_str()
        .unwrap_or("");
    let nte = unsafe { core::ffi::CStr::from_ptr(note) }
        .to_str()
        .unwrap_or("");
    match identity::get_table().create(pwd, nte, creator_pwm) {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwm_delete(pwm: u64) -> i32 {
    match identity::get_table().delete(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_disable(pwm: u64) -> i32 {
    match identity::get_table().disable(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_enable(pwm: u64) -> i32 {
    match identity::get_table().enable(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_verify_password(pwm: u64, password: *const core::ffi::c_char) -> bool {
    if password.is_null() {
        return false;
    }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) }
        .to_str()
        .unwrap_or("");
    identity::get_table().verify_password(pwm, pwd)
}

#[no_mangle]
pub extern "C" fn pwm_change_password(
    pwm: u64,
    old: *const core::ffi::c_char,
    new: *const core::ffi::c_char,
) -> i32 {
    if old.is_null() || new.is_null() {
        return PwmError::InvalidPassword.as_i32();
    }
    let o = unsafe { core::ffi::CStr::from_ptr(old) }
        .to_str()
        .unwrap_or("");
    let n = unsafe { core::ffi::CStr::from_ptr(new) }
        .to_str()
        .unwrap_or("");
    match identity::get_table().change_password(pwm, o, n) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_find(pwm: u64) -> bool {
    identity::find(pwm).is_some()
}

#[no_mangle]
pub extern "C" fn pwm_find_entry(pwm: u64) -> *const PwmEntry {
    match identity::find(pwm) {
        Some(e) => e as *const PwmEntry,
        None => core::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_has_cap_raw(pwm: u64, domain: u16, _cap_bit: u8) -> u64 {
    engine::get_caps(pwm, CapDomain(domain)).as_u64()
}

#[no_mangle]
pub extern "C" fn pwm_create_first_identity(password: *const core::ffi::c_char) -> i64 {
    if password.is_null() {
        return PwmError::InvalidPassword.as_i32() as i64;
    }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) }
        .to_str()
        .unwrap_or("");
    match identity::get_table().bootstrap(pwd, "root") {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwm_get_fs_capability(pwm: u64) -> u64 {
    engine::get_caps(pwm, CapDomain::FS).as_u64()
}

#[no_mangle]
pub extern "C" fn pwm_has_capability(pwm: u64, domain: u16, required: u64) -> bool {
    engine::check(pwm, CapDomain(domain), CapBits(required))
}

#[no_mangle]
pub extern "C" fn pwm_get_capability_raw(pwm: u64, domain: u16) -> u64 {
    engine::get_caps(pwm, CapDomain(domain)).as_u64()
}

#[no_mangle]
pub extern "C" fn pwm_get_privilege_level(pwm: u64) -> u8 {
    engine::get_privilege_level(pwm)
}

#[no_mangle]
pub extern "C" fn pwm_get_creator(pwm: u64) -> u64 {
    engine::get_creator(pwm)
}

#[no_mangle]
pub extern "C" fn pwm_grant(grantor_pwm: u64, grantee_pwm: u64, domain: u16, caps: u64) -> i32 {
    match identity::get_table().grant(grantor_pwm, grantee_pwm, CapDomain(domain), CapBits(caps)) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_revoke(revoker_pwm: u64, target_pwm: u64, domain: u16, caps: u64) -> i32 {
    match identity::get_table().revoke(revoker_pwm, target_pwm, CapDomain(domain), CapBits(caps)) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_transfer_creator(current_creator: u64, target: u64, new_creator: u64) -> i32 {
    match identity::get_table().transfer_creator(current_creator, target, new_creator) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_check_privilege(operator: u64, target: u64) -> bool {
    engine::check_privilege(operator, target)
}

#[no_mangle]
pub extern "C" fn pwm_login(
    note: *const core::ffi::c_char,
    password: *const core::ffi::c_char,
) -> i64 {
    if note.is_null() || password.is_null() {
        return PwmError::InvalidPassword.as_i32() as i64;
    }
    let n = unsafe { core::ffi::CStr::from_ptr(note) }
        .to_str()
        .unwrap_or("");
    let p = unsafe { core::ffi::CStr::from_ptr(password) }
        .to_str()
        .unwrap_or("");
    match session::login(n, p) {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwm_logout() {
    session::logout();
}

#[no_mangle]
pub extern "C" fn pwm_get_current() -> u64 {
    session::get_current_pwm()
}

#[no_mangle]
pub extern "C" fn pwm_get_current_entry() -> *const PwmEntry {
    session::get_current_entry()
}

#[no_mangle]
pub extern "C" fn pwm_is_logged_in() -> bool {
    session::is_logged_in()
}

#[no_mangle]
pub extern "C" fn pwm_get_current_uid() -> u32 {
    session::get_current_uid()
}

#[no_mangle]
pub extern "C" fn pwm_get_current_gid() -> u32 {
    session::get_current_gid()
}

#[no_mangle]
pub extern "C" fn pwm_get_euid() -> u32 {
    session::get_euid()
}

#[no_mangle]
pub extern "C" fn pwm_get_egid() -> u32 {
    session::get_egid()
}

#[no_mangle]
pub extern "C" fn pwm_elevate_for_suid(target_pwm: u64) -> bool {
    session::elevate_for_suid(target_pwm)
}

#[no_mangle]
pub extern "C" fn pwm_drop_elevation() -> bool {
    session::drop_elevation()
}

#[no_mangle]
pub extern "C" fn pwm_has_elevation_authority(target_pwm: u64) -> bool {
    session::has_elevation_authority(target_pwm)
}

#[no_mangle]
pub extern "C" fn pwm_try_setuid(target_uid: u32) -> bool {
    session::try_setuid(target_uid)
}

#[no_mangle]
pub extern "C" fn pwm_get_uid(pwm: u64) -> u32 {
    match identity::find(pwm) {
        Some(e) => e.get_uid(),
        None => 0xFFFFFFFF,
    }
}

#[no_mangle]
pub extern "C" fn pwm_get_gid(pwm: u64) -> u32 {
    match identity::find(pwm) {
        Some(e) => e.get_gid(),
        None => 0xFFFFFFFF,
    }
}

#[no_mangle]
pub extern "C" fn pwm_clear_lockout(pwm: u64) -> i32 {
    match session::clear_lockout(pwm) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwm_save_to_disk() -> i32 {
    storage::save_database()
}

#[no_mangle]
pub extern "C" fn pwm_load_from_disk() -> i32 {
    storage::load_database()
}

#[no_mangle]
pub extern "C" fn pwm_is_modified() -> bool {
    identity::get_table().is_modified()
}

#[no_mangle]
pub extern "C" fn pwm_set_modified() {
    identity::get_table().set_modified();
}

#[no_mangle]
pub extern "C" fn pwm_audit_log(pwm: u64, action: u32, target: u64, details: u64) {
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
pub extern "C" fn pwm_audit_dump() {
    audit::dump();
}

#[no_mangle]
pub extern "C" fn pwm_recover_first(
    password: *const core::ffi::c_char,
    note: *const core::ffi::c_char,
) -> i64 {
    if password.is_null() || note.is_null() {
        return PwmError::InvalidPassword.as_i32() as i64;
    }
    let p = unsafe { core::ffi::CStr::from_ptr(password) }
        .to_str()
        .unwrap_or("");
    let n = unsafe { core::ffi::CStr::from_ptr(note) }
        .to_str()
        .unwrap_or("");
    match identity::get_table().recover_with_first(p, n) {
        Ok(pwm) => pwm as i64,
        Err(e) => e.as_i32() as i64,
    }
}
