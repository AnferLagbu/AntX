use core::ffi::c_char;

use super::types::*;
use super::capability::*;
use super::trust_chain::*;
use super::token::*;
use super::context::*;
use super::permission::*;

static CHECKER: spin::Once<spin::Mutex<EnhancedPermissionChecker>> = spin::Once::new();

fn get_checker() -> &'static spin::Mutex<EnhancedPermissionChecker> {
    CHECKER.call_once(|| spin::Mutex::new(EnhancedPermissionChecker::new()))
}

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() { return ""; }
    unsafe {
        let len = (0..).find(|&i| *ptr.add(i) == 0).unwrap_or(0);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    }
}

#[no_mangle]
pub extern "C" fn pwid_enhanced_init() {
    let _checker = get_checker().lock();
}

#[no_mangle]
pub extern "C" fn pwid_check_permission_enhanced(
    pwid: u64,
    owner_pwid: u64,
    pwid_level: u8,
    pwid_flags: u8,
    access_type: u64,
    domain: u16,
    other_perms: u16,
) -> i32 {
    let checker = get_checker().lock();
    
    let level = PwidLevel::from_u8(pwid_level);
    let caps = match level {
        PwidLevel::Root => CapabilityMatrix::root_capabilities(),
        PwidLevel::Trustworthy => CapabilityMatrix::trustworthy_default(),
        PwidLevel::Untrustworthy => CapabilityMatrix::untrustworthy_default(),
    };
    
    let context = PermissionContext::from_current();
    
    match checker.check_permission(
        pwid,
        owner_pwid,
        level,
        pwid_flags,
        &caps,
        access_type as CapBits,
        domain as CapDomain,
        other_perms,
        &context,
        None,
        None,
    ) {
        PermissionResult::Allowed { .. } => 1,
        PermissionResult::Denied(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn pwid_create_elevation_token_internal(
    issuer: u64,
    holder: u64,
    domains: *const u16,
    caps: *const u64,
    count: u32,
    duration_secs: u64,
    max_uses: u32,
) -> i64 {
    if domains.is_null() || caps.is_null() || count == 0 || count > 8 {
        return -1;
    }

    unsafe {
        let domain_slice = core::slice::from_raw_parts(domains, count as usize);
        let cap_slice = core::slice::from_raw_parts(caps, count as usize);

        let mut checker = get_checker().lock();
        
        match checker.create_elevation_token(
            issuer,
            holder,
            domain_slice,
            cap_slice,
            duration_secs,
            max_uses,
        ) {
            Some(id) => id as i64,
            None => -1,
        }
    }
}

#[no_mangle]
pub extern "C" fn pwid_use_token_internal(token_id: u64) -> i32 {
    let mut checker = get_checker().lock();
    match checker.use_elevation_token(token_id) {
        Ok(()) => 0,
        Err(()) => -1,
    }
}

#[no_mangle]
pub extern "C" fn pwid_revoke_token_internal(token_id: u64, revoker: u64) -> i32 {
    let mut checker = get_checker().lock();
    if checker.revoke_token(token_id, revoker) { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn pwid_add_trust_internal(
    truster: u64,
    trusted: u64,
    trust_level: u8,
    domain: u16,
    cap_mask: u64,
    expires_at: u64,
) -> i32 {
    let mut checker = get_checker().lock();
    match checker.add_trust(
        truster,
        trusted,
        TrustLevel::from_u8(trust_level),
        domain as CapDomain,
        cap_mask,
        expires_at,
    ) {
        Ok(()) => 0,
        Err(()) => -1,
    }
}

#[no_mangle]
pub extern "C" fn pwid_remove_trust_internal(
    truster: u64,
    trusted: u64,
    domain: u16,
) -> i32 {
    let mut checker = get_checker().lock();
    if checker.remove_trust(truster, trusted, domain as CapDomain) { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn pwid_cleanup_internal() {
    let mut checker = get_checker().lock();
    checker.cleanup();
}
