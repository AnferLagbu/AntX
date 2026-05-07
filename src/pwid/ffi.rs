//! PWID FFI Interface Layer
//!
//! Provides extern "C" functions for C code to call into the Rust PWID implementation.
//! Maintains full API compatibility with original C implementation.

use super::types::*;
use super::manager;
use super::session;
use super::audit;
use super::trust_chain::{TrustChain, TrustEntry};
use super::token::{TokenManager, PwidToken, TokenType, MAX_TOKENS};
use core::sync::atomic::{AtomicBool, Ordering};

/// Global trust chain singleton
static TRUST_DONE: AtomicBool = AtomicBool::new(false);
static mut TRUST_CHAIN: Option<TrustChain> = None;

fn get_trust_chain() -> &'static mut TrustChain {
    if !TRUST_DONE.load(Ordering::Acquire) {
        unsafe {
            TRUST_CHAIN = Some(TrustChain::new());
        }
        TRUST_DONE.store(true, Ordering::Release);
    }
    unsafe { TRUST_CHAIN.as_mut().unwrap() }
}

/// Global token manager singleton
static mut TOKEN_MANAGER: Option<TokenManager> = None;
static TOKEN_INIT: AtomicBool = AtomicBool::new(false);

fn get_token_manager() -> &'static mut TokenManager {
    if !TOKEN_INIT.load(Ordering::Acquire) {
        unsafe {
            TOKEN_MANAGER = Some(TokenManager::new());
        }
        TOKEN_INIT.store(true, Ordering::Release);
    }
    unsafe { TOKEN_MANAGER.as_mut().unwrap() }
}

/// Genesis flag — set by kernel when --first boot parameter is present
static GENESIS_REQUESTED: AtomicBool = AtomicBool::new(false);

/// SYS_ADMIN capability constant (matching pwid.h)
const SYS_ADMIN_CAP: u64 = 0xFFFFFFFFFFFFFFFF;

/// USB Domain 5: USER_MGMT capability bits
const USER_MGMT_CAP_CREATE: u64 = 1 << 2;   // CAP_DOMAIN_USER_CREATE
const USER_MGMT_CAP_DELETE: u64 = 1 << 3;   // CAP_DOMAIN_USER_DELETE
const USER_MGMT_CAP_TOKEN: u64  = 1 << 5;   // CAP_DOMAIN_TOKEN_ISSUE
const USER_MGMT_CAP_TRUST: u64  = 1 << 6;   // CAP_DOMAIN_TRUST_ADD

/// Serial print macro (placeholder)
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

// ============================================================
// Initialization
// ============================================================

/// Initialize PWID system
#[no_mangle]
pub extern "C" fn pwid_init() {
    unsafe {
        manager::get_manager_mut().init();
    }
}

/// Try to load PWID database from disk
#[no_mangle]
pub extern "C" fn pwid_try_load() {
    // TODO: Implement storage loading
}

// ============================================================
// PWID Generation and Verification
// ============================================================

/// Generate a unique PWID from password, note, and level
#[no_mangle]
pub extern "C" fn pwid_generate(password: *const i8, note: *const i8, level: u8) -> u64 {
    if password.is_null() || note.is_null() {
        return 0;
    }
    
    let pwd = match unsafe { cstr_to_str(password) } {
        Ok(s) => s,
        Err(_) => return 0,
    };
    
    let note = match unsafe { cstr_to_str(note) } {
        Ok(s) => s,
        Err(_) => return 0,
    };
    
    manager::get_manager().generate(pwd, note, level)
}

/// Verify password for a given PWID
#[no_mangle]
pub extern "C" fn pwid_verify_password(pwid: u64, password: *const i8) -> i32 {
    if password.is_null() {
        return 0;
    }
    
    let password = match unsafe { cstr_to_str(password) } {
        Ok(s) => s,
        Err(_) => return 0,
    };
    
    if manager::get_manager().verify_password(pwid, password) {
        1
    } else {
        0
    }
}

// ============================================================
// User Management CRUD
// ============================================================

/// Create a new PWID entry
#[no_mangle]
pub extern "C" fn pwid_create(password: *const i8, note: *const i8, level: u8) -> i32 {
    let (pwd, note) = match get_two_strings(password, note) {
        Some(pair) => pair,
        None => return PwidError::NotFound.as_i32(),
    };
    
    let empty_caps: [u64; 16] = [0; 16];
    match manager::get_manager().create(pwd, note, level, &empty_caps) {
        Ok(_) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Delete a PWID entry
#[no_mangle]
pub extern "C" fn pwid_delete(pwid: u64) -> i32 {
    match manager::get_manager().delete(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Disable a PWID entry
#[no_mangle]
pub extern "C" fn pwid_disable(pwid: u64) -> i32 {
    match manager::get_manager().disable(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Enable a disabled PWID entry
#[no_mangle]
pub extern "C" fn pwid_enable(pwid: u64) -> i32 {
    match manager::get_manager().enable(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Change password for a PWID entry
#[no_mangle]
pub extern "C" fn pwid_change_password(
    pwid: u64, 
    old_password: *const i8, 
    new_password: *const i8
) -> i32 {
    let (old_pwd, new_pwd) = match get_two_strings(old_password, new_password) {
        Some(pair) => pair,
        None => return PwidError::PasswordIncorrect.as_i32(),
    };
    
    match manager::get_manager().change_password(pwid, old_pwd, new_pwd) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Change note/description for a PWID entry
#[no_mangle]
pub extern "C" fn pwid_change_note(pwid: u64, new_note: *const i8) -> i32 {
    let note = match unsafe { cstr_to_str(new_note) } {
        Ok(s) => s,
        Err(_) => return PwidError::NotFound.as_i32(),
    };
    
    match manager::get_manager().change_note(pwid, note) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

// ============================================================
// Lookup Functions
// ============================================================

/// Find entry by PWID (returns pointer or NULL)
#[no_mangle]
pub extern "C" fn pwid_find(pwid: u64) -> *const PwidEntry {
    match manager::get_manager().find(pwid) {
        Some(entry) => entry as *const PwidEntry,
        None => core::ptr::null(),
    }
}

/// Find entry by note string (returns pointer or NULL)
#[no_mangle]
pub extern "C" fn pwid_find_by_note(note: *const i8) -> *const PwidEntry {
    if note.is_null() {
        return core::ptr::null();
    }
    
    let note_str = match unsafe { cstr_to_str(note) } {
        Ok(s) => s,
        Err(_) => return core::ptr::null(),
    };
    
    match manager::get_manager().find_by_note(note_str) {
        Some(entry) => entry as *const PwidEntry,
        None => core::ptr::null(),
    }
}

/// Get trust level for a PWID
#[no_mangle]
pub extern "C" fn pwid_get_level(pwid: u64) -> u8 {
    manager::get_manager()
        .get_level(pwid)
        .unwrap_or(0xFF)
}

/// Get FS capability bitmask for a PWID from its capability matrix (v4).
/// No longer derives caps from level — reads pwidentry.capability_mask[CAP_DOMAIN_FS].
#[no_mangle]
pub extern "C" fn pwid_get_fs_capability(pwid: u64) -> u64 {
    let mgr = manager::get_manager();
    match mgr.get_caps(pwid) {
        Some(caps) => caps[1],  // CAP_DOMAIN_FS = 1
        None => 0,
    }
}

/// Check if pwid has capability in a domain (v4).
/// domain: 0..15, required: bitmask
/// Returns 1 if the pwid's cap_mask[domain] superset-of required.
#[no_mangle]
pub extern "C" fn pwid_has_capability(pwid: u64, domain: u16, required: u64) -> i32 {
    let mgr = manager::get_manager();
    match mgr.get_caps(pwid) {
        Some(caps) => {
            let idx = (domain as usize) % 16;
            if (caps[idx] & required) == required { 1 } else { 0 }
        }
        None => 0,
    }
}

/// Get raw capability mask for a PWID in a specific domain (for C inline helper)
#[no_mangle]
pub extern "C" fn pwid_get_capability_raw(pwid: u64, domain: u16) -> u64 {
    let mgr = manager::get_manager();
    match mgr.get_caps(pwid) {
        Some(caps) => caps[(domain as usize) % 16],
        None => 0,
    }
}

/// Check if PWID has root privileges
#[no_mangle]
pub extern "C" fn pwid_is_root(pwid: u64) -> i32 {
    if manager::get_manager().is_root(pwid) { 1 } else { 0 }
}

/// Check if entry uses default password
#[no_mangle]
pub extern "C" fn pwid_has_default_password(pwid: u64) -> i32 {
    if manager::get_manager().has_default_password(pwid) { 1 } else { 0 }
}

/// Clear default password flag
#[no_mangle]
pub extern "C" fn pwid_clear_default_password_flag(pwid: u64) {
    manager::get_manager().clear_default_password_flag(pwid);
}

// ============================================================
// Permission Checking
// ============================================================

/// Check permission based on trust level
#[no_mangle]
pub extern "C" fn pwid_check_permission(pwid: u64, required_level: u8) -> i32 {
    if manager::get_manager().check_permission(pwid, required_level) { 1 } else { 0 }
}

/// Check if creator can create target level
#[no_mangle]
pub extern "C" fn pwid_can_create_level(creator_level: u8, target_level: u8) -> i32 {
    // Inline logic (same as PwidManager::can_create_level)
    let can_create = match PwidLevel::from_u8(creator_level) {
        Some(PwidLevel::Root) => true,
        Some(PwidLevel::Trusted) => target_level == PwidLevel::Untrustworthy.as_u8(),
        _ => false,
    };
    
    if can_create { 1 } else { 0 }
}

/// Check if modifier can modify target
#[no_mangle]
pub extern "C" fn pwid_can_modify(modifier_pwid: u64, target_pwid: u64) -> i32 {
    if manager::get_manager().can_modify(modifier_pwid, target_pwid) { 1 } else { 0 }
}

// ============================================================
// Root User Management
// ============================================================

/// Create derived root user (v4: create full-cap identity)
#[no_mangle]
pub extern "C" fn pwid_create_derived_root(password: *const i8, note: *const i8) -> i32 {
    let (pwd, note) = match get_two_strings(password, note) {
        Some(pair) => pair,
        None => return PwidError::NotFound.as_i32(),
    };
    
    match manager::get_manager().create_full_cap(pwd, note) {
        Ok(_) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Delete derived root user
#[no_mangle]
pub extern "C" fn pwid_delete_derived_root(pwid: u64) -> i32 {
    match manager::get_manager().delete_derived_root(pwid) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Create first identity (one-time operation via First Token)
#[no_mangle]
pub extern "C" fn pwid_create_first_identity(password: *const i8) -> i32 {
    let pwd = match unsafe { cstr_to_str(password) } {
        Ok(s) => s,
        Err(_) => return PwidError::PasswordIncorrect.as_i32(),
    };
    
    match manager::get_manager().create_first_identity(pwd) {
        Ok(_) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Check if any identity exists in the system
#[no_mangle]
pub extern "C" fn pwid_any_identity_exists() -> i32 {
    if manager::get_manager().any_identity_exists() { 1 } else { 0 }
}

// ============================================================
// Listing
// ============================================================

/// List all PWID entries
#[no_mangle]
pub extern "C" fn pwid_list_all() {
    manager::get_manager().list_all();
}

// ============================================================
// Session Management
// ============================================================

/// Set current session context
#[no_mangle]
pub extern "C" fn pwid_set_context(pwid: u64) {
    session::get_session().set_context(pwid)
}

/// Get current session's PWID
#[no_mangle]
pub extern "C" fn pwid_get_current() -> u64 {
    session::get_session().get_current()
}

/// Get current session's entry pointer
#[no_mangle]
pub extern "C" fn pwid_get_current_entry() -> *const PwidEntry {
    session::get_session().get_current_entry()
}

/// Login with username and password
#[no_mangle]
pub extern "C" fn pwid_login(note: *const i8, password: *const i8) -> i32 {
    let (note, password) = match get_two_strings(note, password) {
        Some(pair) => pair,
        None => return PwidError::NotFound.as_i32(),
    };
    
    session::get_session().login(note, password).as_i32()
}

/// Logout from current session
#[no_mangle]
pub extern "C" fn pwid_logout() {
    session::get_session().logout()
}

/// Create user (requires active session)
#[no_mangle]
pub extern "C" fn pwid_create_user(password: *const i8, note: *const i8, level: u8) -> i32 {
    let (pwd, note) = match get_two_strings(password, note) {
        Some(pair) => pair,
        None => return PwidError::PermissionDenied.as_i32(),
    };
    
    match session::get_session().create_user(pwd, note, level) {
        Ok(_) => 0,
        Err(e) => e.as_i32(),
    }
}

// ============================================================
// Privilege Elevation
// ============================================================

/// Elevate privileges to root
#[no_mangle]
pub extern "C" fn pwid_elevate(target_pwid: u64, password: *const i8, duration_secs: u64) -> i32 {
    let pwd = match unsafe { cstr_to_str(password) } {
        Ok(s) => s,
        Err(_) => return PwidError::PasswordIncorrect.as_i32(),
    };
    
    session::get_session().elevate(target_pwid, pwd, duration_secs).as_i32()
}

/// End privilege elevation
#[no_mangle]
pub extern "C" fn pwid_end_elevation() {
    session::get_session().end_elevation()
}

/// Check if currently elevated
#[no_mangle]
pub extern "C" fn pwid_is_elevated() -> i32 {
    if session::get_session().is_elevated() { 1 } else { 0 }
}

// ============================================================
// Security Features
// ============================================================

/// Check if PWID is expired
#[no_mangle]
pub extern "C" fn pwid_is_expired(pwid: u64) -> i32 {
    if session::get_session().is_expired(pwid) { 1 } else { 0 }
}

/// Check if PWID is locked out
#[no_mangle]
pub extern "C" fn pwid_is_locked(pwid: u64) -> i32 {
    if session::get_session().is_locked(pwid) { 1 } else { 0 }
}

/// Set expiry time
#[no_mangle]
pub extern "C" fn pwid_set_expiry(pwid: u64, expires_at: u64) {
    session::get_session().set_expiry(pwid, expires_at)
}

/// Extend expiry by days
#[no_mangle]
pub extern "C" fn pwid_extend_expiry(pwid: u64, days: u64) {
    session::get_session().extend_expiry(pwid, days)
}

/// Clear lockout status
#[no_mangle]
pub extern "C" fn pwid_clear_lockout(pwid: u64) {
    session::get_session().clear_lockout(pwid)
}

/// Record failed login attempt
#[no_mangle]
pub extern "C" fn pwid_record_failed_login(pwid: u64) {
    session::get_session().record_failed_login(pwid)
}

/// Clear failed login attempts
#[no_mangle]
pub extern "C" fn pwid_clear_failed_attempts(pwid: u64) {
    session::get_session().clear_failed_attempts(pwid)
}

/// Login with brute-force protection
#[no_mangle]
pub extern "C" fn pwid_login_with_bruteforce_protection(note: *const i8, password: *const i8) -> i32 {
    let (note, password) = match get_two_strings(note, password) {
        Some(pair) => pair,
        None => return PwidError::NotFound.as_i32(),
    };
    
    let mgr = manager::get_manager();
    let sess = session::get_session();
    
    let entry = match mgr.find_by_note(note) {
        Some(e) => e,
        None => return PwidError::NotFound.as_i32(),
    };
    
    let pwid = entry.pwid.load(core::sync::atomic::Ordering::Acquire);
    
    if entry.has_flag(PwidFlags::DISABLED) {
        return PwidError::Disabled.as_i32();
    }
    
    if sess.is_locked(pwid) {
        return PwidError::Disabled.as_i32();
    }
    
    if sess.is_expired(pwid) {
        return PwidError::Disabled.as_i32();
    }
    
    let hash = crate::pwid::sha256::sha256(password.as_bytes());
    if &entry.password_hash != &hash {
        sess.record_failed_login(pwid);
        audit::get_audit().log(pwid, 1, 1, 0, 0);
        return PwidError::PasswordIncorrect.as_i32();
    }
    
    sess.clear_failed_attempts(pwid);
    entry.last_login_time.store(get_current_time(), core::sync::atomic::Ordering::Release);
    
    unsafe {
        let ctx = &mut *sess.current.get();
        ctx.current_entry = entry as *const PwidEntry;
        ctx.session_pwid = pwid;
    }
    
    audit::get_audit().log(pwid, 1, 0, 0, 0);
    
    PwidError::Ok.as_i32()
}

// ============================================================
// Audit Log
// ============================================================

/// Record audit event
#[no_mangle]
pub extern "C" fn pwid_audit_log(pwid: u64, action: u32, result: u32, target_pwid: u64, details: u64) {
    audit::get_audit().log(pwid, action, result, target_pwid, details)
}

/// Dump audit log to serial
#[no_mangle]
pub extern "C" fn pwid_audit_dump() {
    audit::get_audit().dump()
}

// ============================================================
// Database Persistence
// ============================================================

/// Save PWID database to disk (delegates to storage layer)
#[no_mangle]
pub extern "C" fn pwid_save_to_disk() -> i32 {
    if manager::get_manager().is_modified() {
        manager::get_manager().clear_modified();
        // TODO: Implement HVFS file I/O via storage.rs
        0
    } else {
        0
    }
}

/// Load PWID database from disk (delegates to storage layer)
#[no_mangle]
pub extern "C" fn pwid_load_from_disk() -> i32 {
    0 // TODO: Implement HVFS file I/O via storage.rs
}

/// Check if database has been modified
#[no_mangle]
pub extern "C" fn pwid_is_modified() -> i32 {
    if manager::get_manager().is_modified() { 1 } else { 0 }
}

/// Mark database as modified
#[no_mangle]
pub extern "C" fn pwid_set_modified() {
    manager::get_manager().set_modified()
}

// ============================================================
// Periodic Maintenance
// ============================================================

/// Perform periodic cleanup tasks (expire tokens + trust entries)
#[no_mangle]
pub extern "C" fn pwid_periodic_cleanup() {
    get_trust_chain().clear_expired();
    get_token_manager().clear_expired();
}

// ============================================================
// Token System
// ============================================================

/// Create privilege elevation token
#[no_mangle]
pub extern "C" fn pwid_create_token(
    creator_pwid: u64,
    target_pwid: u64,
    permissions: u64,
    duration_secs: u64
) -> u64 {
    let now = get_time();
    let token = PwidToken::new(TokenType::Elevation, creator_pwid, target_pwid)
        .with_capabilities(&[1], &[permissions])
        .with_validity(0, now + duration_secs);
    match get_token_manager().create(token) {
        Some(id) => id,
        None => 0,
    }
}

/// Use a token for privilege elevation
#[no_mangle]
pub extern "C" fn pwid_use_token_internal(token_id: u64, user_pwid: u64) -> i32 {
    match get_token_manager().use_token(token_id) {
        Ok(()) => {
            let idx = match get_token_manager().find(token_id) {
                Some(i) => i,
                None => return -1,
            };
            let t = match get_token_manager().get(idx) {
                Some(t) => t,
                None => return -1,
            };
            if t.has_capability(1, 0xFFFFFFFFFFFFFFFF) {
                session::get_session().elevate(t.issuer_pwid, "", 0);
            }
            0
        }
        Err(()) => -1,
    }
}

/// Revoke a token
#[no_mangle]
pub extern "C" fn pwid_revoke_token_internal(token_id: u64, revoker_pwid: u64) -> i32 {
    if get_token_manager().revoke(token_id, revoker_pwid) { 0 } else { -1 }
}

/// Request genesis mode (for --first boot parameter)
#[no_mangle]
pub extern "C" fn pwid_request_genesis() {
    GENESIS_REQUESTED.store(true, Ordering::Release);
}

fn get_time() -> u64 {
    let tsc: u64;
    unsafe {
        core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack));
    }
    tsc
}

// ============================================================
// Trust Relations (Stubs - TODO: Full implementation)
// ============================================================

/// Add trust relationship between users
#[no_mangle]
pub extern "C" fn pwid_add_trust_relation(
    trustor_pwid: u64,
    trustee_pwid: u64,
    trust_level: u8
) -> i32 {
    let level = match trust_level {
        0 => TrustLevel::None,
        1 => TrustLevel::Basic,
        2 => TrustLevel::Operate,
        3 => TrustLevel::Delegate,
        4 => TrustLevel::Full,
        _ => return -1,
    };
    let entry = TrustEntry::new(trustor_pwid, trustee_pwid, level, 0, 0, 0, 0);
    match get_trust_chain().add(entry) {
        Ok(()) => 0,
        Err(()) => -1,
    }
}

/// Remove trust relationship
#[no_mangle]
pub extern "C" fn pwid_remove_trust_internal(
    trustor_pwid: u64,
    trustee_pwid: u64
) -> i32 {
    if get_trust_chain().remove(trustor_pwid, trustee_pwid, 0) { 0 } else { -1 }
}

/// Check if subject has a trust chain path to target for the given capability.
/// Returns 1 if trust exists, 0 otherwise.
/// max_depth limits delegation hops (8 = default kernel max).
#[no_mangle]
pub extern "C" fn pwid_check_trust(
    subject_pwid: u64,
    target_pwid: u64,
    domain: u16,
    required_caps: u64,
    max_depth: u8,
) -> i32 {
    let chain = get_trust_chain();
    if chain.check_chain(subject_pwid, target_pwid, domain, required_caps, max_depth).is_some() {
        1
    } else {
        0
    }
}

// ============================================================
// Enhanced Security Checks (Stubs)
// ============================================================

/// Enhanced permission check — orchestrates multi-layer security.
/// object_type: domain (0=system, 1=fs, 2=net, 3=proc, 4=device, 5=user_mgmt)
/// action: capability bitmask for the requested operation
/// Returns 1 if allowed, 0 if denied.
#[no_mangle]
pub extern "C" fn pwid_enhanced_check(
    subject_pwid: u64,
    object_type: u32,
    action: u32,
    _context: *const core::ffi::c_void
) -> i32 {
    let caps = action as u64;

    // Layer 0: Check if account is valid; unregistered → transitional allow
    let pwid_caps = pwid_get_fs_capability(subject_pwid);
    if pwid_caps == 0 {
        return 1;  // Transitional: unregistered/guest PWID — allow all
    }
    if (pwid_caps & caps) == caps {
        return 1;
    }

    // Layer 3: Trust chain check
    let chain = get_trust_chain();
    if chain.check_chain(subject_pwid, 0, object_type as u16, caps, 8).is_some() {
        return 1;
    }

    0
}

/// Periodic cleanup: expire trust entries and tokens
#[no_mangle]
pub extern "C" fn pwid_cleanup_internal() {
    get_trust_chain().clear_expired();
}

// ============================================================
// Helper Functions
// ============================================================

/// Convert C string to Rust string slice
unsafe fn cstr_to_str(ptr: *const i8) -> Result<&'static str, ()> {
    if ptr.is_null() {
        return Err(());
    }
    
    let cstr = core::ffi::CStr::from_ptr(ptr);
    cstr.to_str().map_err(|_| ())
}

/// Get two C strings as Rust strings (for functions with 2 string params)
fn get_two_strings(s1: *const i8, s2: *const i8) -> Option<(&'static str, &'static str)> {
    let str1 = unsafe { cstr_to_str(s1).ok()? };
    let str2 = unsafe { cstr_to_str(s2).ok()? };
    Some((str1, str2))
}

/// Get current time from TSC register
fn get_current_time() -> u64 {
    unsafe {
        let tsc: u64;
        core::arch::asm!(
            "rdtsc",
            out("eax") tsc,
            options(nostack, nomem)
        );
        tsc / 3_000_000_000u64
    }
}
