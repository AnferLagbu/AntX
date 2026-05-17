use super::types::*;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct FirstToken {
    pub token_id: u64,
    pub granted: AtomicBool,
    pub created_at: u64,
}

impl FirstToken {
    pub fn new(token_id: u64, created_at: u64) -> Self {
        Self {
            token_id,
            granted: AtomicBool::new(false),
            created_at,
        }
    }
}

static FIRST_TOKEN_USED: AtomicBool = AtomicBool::new(true);
static FIRST_TOKEN_ID: AtomicU64 = AtomicU64::new(0);
static FIRST_TOKEN_CREATED: AtomicU64 = AtomicU64::new(0);

pub fn generate_first_token() {
    let token_id = {
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        tsc.wrapping_mul(0x9e3779b97f4a7c15) >> 32
    };
    FIRST_TOKEN_ID.store(token_id, Ordering::Release);
    FIRST_TOKEN_USED.store(false, Ordering::Release);
    FIRST_TOKEN_CREATED.store(pwid_now(), Ordering::Release);
}

pub fn grant_from_first_token(
    target_pwid: u64,
    domain: CapDomain,
    caps: CapBits,
) -> Result<(), PwidError> {
    if FIRST_TOKEN_USED.load(Ordering::Acquire) {
        return Err(PwidError::TokenUsed);
    }

    let target = super::table::find(target_pwid).ok_or(PwidError::NotFound)?;
    target.fetch_or_caps(domain, caps);

    FIRST_TOKEN_USED.store(true, Ordering::Release);

    super::audit::log(0, AuditAction::FirstTokenGrant, target_pwid, domain.as_u16() as u64, caps.as_u64());

    Ok(())
}

pub fn pwid_now() -> u64 {
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    extern "C" {
        fn cpu_get_tsc_frequency() -> u64;
    }
    let freq = unsafe { cpu_get_tsc_frequency() };
    if freq > 0 {
        (tsc / freq) * 1_000_000
    } else {
        tsc
    }
}

#[no_mangle]
pub extern "C" fn pwid_first_token_generate() {
    generate_first_token();
}

#[no_mangle]
pub extern "C" fn pwid_first_token_grant(target_pwid: u64, domain: u16, caps: u64) -> i32 {
    match grant_from_first_token(target_pwid, CapDomain::from(domain), CapBits(caps)) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}
