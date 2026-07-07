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
        let tsc = crate::arch!(timestamp());
        tsc.wrapping_mul(0x9e3779b97f4a7c15) >> 32
    };
    FIRST_TOKEN_ID.store(token_id, Ordering::Release);
    FIRST_TOKEN_USED.store(false, Ordering::Release);
    FIRST_TOKEN_CREATED.store(pwm_now(), Ordering::Release);
}

pub fn grant_from_first_token(
    target_pwm: u64,
    domain: CapDomain,
    caps: CapBits,
) -> Result<(), PwmError> {
    if FIRST_TOKEN_USED.load(Ordering::Acquire) {
        return Err(PwmError::TokenUsed);
    }

    let target = super::identity::find(target_pwm).ok_or(PwmError::NotFound)?;
    target.fetch_or_caps(domain, caps);

    FIRST_TOKEN_USED.store(true, Ordering::Release);

    super::audit::log(
        0,
        AuditAction::FirstTokenGrant,
        target_pwm,
        domain.as_u16() as u64,
        caps.as_u64(),
    );

    Ok(())
}

pub fn pwm_now() -> u64 {
    let tsc = crate::arch!(timestamp());
    let freq = raw::tsc_frequency();
    if freq > 0 {
        (tsc / freq) * 1_000_000
    } else {
        tsc
    }
}

// ============================================================================
// 特权子模块 (Framekernel raw): 集中 TSC 频率 FFI
// ============================================================================

pub(crate) mod raw {
    /// 安全获取 CPU TSC 频率 (C ABI 包装, 无内存不安全)
    pub fn tsc_frequency() -> u64 {
        unsafe extern "C" {
            fn cpu_get_tsc_frequency() -> u64;
        }
        // SAFETY: cpu_get_tsc_frequency 为纯函数 FFI, 无内存访问, 安全。
        unsafe { cpu_get_tsc_frequency() }
    }
}
