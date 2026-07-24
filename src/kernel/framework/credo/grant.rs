use super::types::*;
use crate::kernel::framework::sync::IrqSpinLock;

/// Grant 记录表 — 替代 static mut, 由 IrqSpinLock 保护并发访问
static GRANT_RECORDS: IrqSpinLock<[GrantRecord; MAX_GRANT_RECORDS]> =
    IrqSpinLock::new([GrantRecord::EMPTY; MAX_GRANT_RECORDS]);

pub fn add_record(record: GrantRecord) -> Result<(), PwmError> {
    let mut guard = GRANT_RECORDS.lock();
    guard.iter_mut()
        .find(|r| r.is_empty())
        .map(|slot| { *slot = record; Ok(()) })
        .unwrap_or(Err(PwmError::TableFull))
}

pub fn is_grantor(grantor_pwm: u64, grantee_pwm: u64, domain: CapDomain, caps: CapBits) -> bool {
    let guard = GRANT_RECORDS.lock();
    guard.iter().any(|r| {
        r.grantor_pwm.0 == grantor_pwm
            && r.grantee_pwm.0 == grantee_pwm
            && r.domain == domain
            && (r.caps & caps) == caps
    })
}

pub fn clear_records(revoker_pwm: u64, target_pwm: u64, domain: CapDomain, caps: CapBits) {
    let mut guard = GRANT_RECORDS.lock();
    for record in guard.iter_mut() {
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
}
