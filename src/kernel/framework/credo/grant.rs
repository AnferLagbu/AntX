use super::types::*;
use core::sync::atomic::Ordering;

static GRANT_LOCK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn lock_grants() {
    while GRANT_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock_grants() {
    GRANT_LOCK.store(false, Ordering::Release);
}

// SAFETY: 仅在 GRANT_LOCK 保护下访问。裸指针访问避免 static mut 的多重 &mut 引用 UB。
static mut GRANT_RECORDS: [GrantRecord; MAX_GRANT_RECORDS] =
    [GrantRecord::EMPTY; MAX_GRANT_RECORDS];

pub fn add_record(record: GrantRecord) -> Result<(), PwmError> {
    lock_grants();
    let result = raw::records_mut().iter_mut()
        .find(|r| r.is_empty())
        .map(|slot| { *slot = record; Ok(()) })
        .unwrap_or(Err(PwmError::TableFull));
    unlock_grants();
    result
}

pub fn is_grantor(grantor_pwm: u64, grantee_pwm: u64, domain: CapDomain, caps: CapBits) -> bool {
    lock_grants();
    let found = raw::records().iter().any(|r| {
        r.grantor_pwm.0 == grantor_pwm
            && r.grantee_pwm.0 == grantee_pwm
            && r.domain == domain
            && (r.caps & caps) == caps
    });
    unlock_grants();
    found
}

pub fn clear_records(revoker_pwm: u64, target_pwm: u64, domain: CapDomain, caps: CapBits) {
    lock_grants();
    for record in raw::records_mut().iter_mut() {
        if record.grantor_pwm.0 == revoker_pwm
            && record.grantee_pwm.0 == target_pwm
            && record.domain == domain
        {
            record.caps = CapBits(record.caps.0 & !caps.0);
            if record.caps == CapBits::NONE {
                *record = GrantRecord::EMPTY;
            }
        }
    }
    unlock_grants();
}

// ============================================================================
// 特权子模块 (Framekernel raw): 集中 static mut GRANT_RECORDS 访问
// ============================================================================

pub(crate) mod raw {
    use super::*;

    /// 安全读视图 (调用方持有 GRANT_LOCK)
    pub fn records() -> &'static [GrantRecord; MAX_GRANT_RECORDS] {
        // SAFETY: 调用方契约持有 GRANT_LOCK, 保证唯一读访问。
        unsafe { &*core::ptr::addr_of!(GRANT_RECORDS) }
    }

    /// 安全写视图 (调用方持有 GRANT_LOCK)
    pub fn records_mut() -> &'static mut [GrantRecord; MAX_GRANT_RECORDS] {
        // SAFETY: 调用方契约持有 GRANT_LOCK, 保证唯一 &mut。
        unsafe { &mut *core::ptr::addr_of_mut!(GRANT_RECORDS) }
    }
}
