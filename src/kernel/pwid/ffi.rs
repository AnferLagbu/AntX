use super::types::*;
use super::table;
use super::session;
use super::engine;
use super::audit;
use super::storage;

macro_rules! klog_pwid {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_warn, $($arg)*)
    };
}

static INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn pwid_init() {
    if INITIALIZED.compare_exchange(
        false, true,
        core::sync::atomic::Ordering::AcqRel,
        core::sync::atomic::Ordering::Relaxed,
    ).is_err() {
        return;
    }
    let t = unsafe { table::get_table_mut() };
    t.init();
    klog_pwid!("PWID v5 initialized");
}

#[no_mangle]
pub extern "C" fn pwid_try_load() -> i32 {
    storage::load_database()
}

#[no_mangle]
pub extern "C" fn pwid_any_identity_exists() -> bool {
    table::get_table().any_identity_exists()
}

#[no_mangle]
pub extern "C" fn pwid_try_genesis(password: *const core::ffi::c_char) -> i64 {
    if password.is_null() { return PwidError::InvalidPassword.as_i32() as i64; }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) };
    let pwd_str = pwd.to_str().unwrap_or("");
    match table::get_table().bootstrap(pwd_str, "root") {
        Ok(pwid) => pwid as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwid_create(password: *const core::ffi::c_char, note: *const core::ffi::c_char, creator_pwid: u64) -> i64 {
    if password.is_null() || note.is_null() { return PwidError::InvalidPassword.as_i32() as i64; }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) }.to_str().unwrap_or("");
    let nte = unsafe { core::ffi::CStr::from_ptr(note) }.to_str().unwrap_or("");
    match table::get_table().create(pwd, nte, creator_pwid) {
        Ok(pwid) => pwid as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwid_delete(pwid: u64) -> i32 {
    match table::get_table().delete(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_disable(pwid: u64) -> i32 {
    match table::get_table().disable(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_enable(pwid: u64) -> i32 {
    match table::get_table().enable(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_verify_password(pwid: u64, password: *const core::ffi::c_char) -> bool {
    if password.is_null() { return false; }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) }.to_str().unwrap_or("");
    table::get_table().verify_password(pwid, pwd)
}

#[no_mangle]
pub extern "C" fn pwid_change_password(pwid: u64, old: *const core::ffi::c_char, new: *const core::ffi::c_char) -> i32 {
    if old.is_null() || new.is_null() { return PwidError::InvalidPassword.as_i32(); }
    let o = unsafe { core::ffi::CStr::from_ptr(old) }.to_str().unwrap_or("");
    let n = unsafe { core::ffi::CStr::from_ptr(new) }.to_str().unwrap_or("");
    match table::get_table().change_password(pwid, o, n) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_find(pwid: u64) -> bool {
    table::find(pwid).is_some()
}

#[no_mangle]
pub extern "C" fn pwid_find_entry(pwid: u64) -> *const PwidEntry {
    match table::find(pwid) {
        Some(e) => e as *const PwidEntry,
        None => core::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_has_cap_raw(pwid: u64, domain: u16, _cap_bit: u8) -> u64 {
    engine::get_caps(pwid, CapDomain(domain)).as_u64()
}

#[no_mangle]
pub extern "C" fn pwid_create_first_identity(password: *const core::ffi::c_char) -> i64 {
    if password.is_null() { return PwidError::InvalidPassword.as_i32() as i64; }
    let pwd = unsafe { core::ffi::CStr::from_ptr(password) }.to_str().unwrap_or("");
    match table::get_table().bootstrap(pwd, "root") {
        Ok(pwid) => pwid as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwid_get_fs_capability(pwid: u64) -> u64 {
    engine::get_caps(pwid, CapDomain::FS).as_u64()
}

#[no_mangle]
pub extern "C" fn pwid_has_capability(pwid: u64, domain: u16, required: u64) -> bool {
    engine::check(pwid, CapDomain(domain), CapBits(required))
}

#[no_mangle]
pub extern "C" fn pwid_get_capability_raw(pwid: u64, domain: u16) -> u64 {
    engine::get_caps(pwid, CapDomain(domain)).as_u64()
}

#[no_mangle]
pub extern "C" fn pwid_get_privilege_level(pwid: u64) -> u8 {
    engine::get_privilege_level(pwid)
}

#[no_mangle]
pub extern "C" fn pwid_get_creator(pwid: u64) -> u64 {
    engine::get_creator(pwid)
}

#[no_mangle]
pub extern "C" fn pwid_grant(grantor_pwid: u64, grantee_pwid: u64, domain: u16, caps: u64) -> i32 {
    match table::get_table().grant(grantor_pwid, grantee_pwid, CapDomain(domain), CapBits(caps)) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_revoke(revoker_pwid: u64, target_pwid: u64, domain: u16, caps: u64) -> i32 {
    match table::get_table().revoke(revoker_pwid, target_pwid, CapDomain(domain), CapBits(caps)) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_transfer_creator(current_creator: u64, target: u64, new_creator: u64) -> i32 {
    match table::get_table().transfer_creator(current_creator, target, new_creator) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_check_privilege(operator: u64, target: u64) -> bool {
    engine::check_privilege(operator, target)
}

#[no_mangle]
pub extern "C" fn pwid_login(note: *const core::ffi::c_char, password: *const core::ffi::c_char) -> i64 {
    if note.is_null() || password.is_null() { return PwidError::InvalidPassword.as_i32() as i64; }
    let n = unsafe { core::ffi::CStr::from_ptr(note) }.to_str().unwrap_or("");
    let p = unsafe { core::ffi::CStr::from_ptr(password) }.to_str().unwrap_or("");
    match session::login(n, p) {
        Ok(pwid) => pwid as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwid_logout() {
    session::logout();
}

#[no_mangle]
pub extern "C" fn pwid_get_current() -> u64 {
    session::get_current_pwid()
}

#[no_mangle]
pub extern "C" fn pwid_get_current_entry() -> *const PwidEntry {
    session::get_current_entry()
}

#[no_mangle]
pub extern "C" fn pwid_is_logged_in() -> bool {
    session::is_logged_in()
}

#[no_mangle]
pub extern "C" fn pwid_clear_lockout(pwid: u64) -> i32 {
    match session::clear_lockout(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn pwid_save_to_disk() -> i32 {
    storage::save_database()
}

#[no_mangle]
pub extern "C" fn pwid_load_from_disk() -> i32 {
    storage::load_database()
}

#[no_mangle]
pub extern "C" fn pwid_is_modified() -> bool {
    table::get_table().is_modified()
}

#[no_mangle]
pub extern "C" fn pwid_set_modified() {
    table::get_table().set_modified();
}

#[no_mangle]
pub extern "C" fn pwid_audit_log(pwid: u64, action: u32, target: u64, details: u64) {
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
    audit::log(pwid, act, target, 0, details);
}

#[no_mangle]
pub extern "C" fn pwid_audit_dump() {
    audit::dump();
}

#[no_mangle]
pub extern "C" fn pwid_recover_first(password: *const core::ffi::c_char, note: *const core::ffi::c_char) -> i64 {
    if password.is_null() || note.is_null() { return PwidError::InvalidPassword.as_i32() as i64; }
    let p = unsafe { core::ffi::CStr::from_ptr(password) }.to_str().unwrap_or("");
    let n = unsafe { core::ffi::CStr::from_ptr(note) }.to_str().unwrap_or("");
    match table::get_table().recover_with_first(p, n) {
        Ok(pwid) => pwid as i64,
        Err(e) => e.as_i32() as i64,
    }
}

#[no_mangle]
pub extern "C" fn pwid_list_all() {
    let t = table::get_table();
    let entries = t.list_all();
    for entry in entries {
        crate::klog_ffi!(klog_ffi_info, "[PWID] pwid={:016X} note={} level={}",
            entry.get_pwid().as_u64(),
            entry.get_note_str(),
            entry.privilege_level.load(core::sync::atomic::Ordering::Acquire));
    }
}

#[no_mangle]
pub extern "C" fn pwid_set_note(target_pwid: u64, note: *const core::ffi::c_char) -> i32 {
    if note.is_null() { return PwidError::InvalidPassword.as_i32(); }
    let n = unsafe { core::ffi::CStr::from_ptr(note) }.to_str().unwrap_or("");
    match table::get_table().set_note(target_pwid, n) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}
