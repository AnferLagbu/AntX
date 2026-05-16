use super::types::*;
use core::sync::atomic::Ordering;

static GRANT_LOCK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn lock_grants() {
    while GRANT_LOCK.compare_exchange_weak(
        false, true,
        Ordering::Acquire,
        Ordering::Relaxed,
    ).is_err() {
        core::hint::spin_loop();
    }
}

fn unlock_grants() {
    GRANT_LOCK.store(false, Ordering::Release);
}

static mut GRANT_RECORDS: [GrantRecord; MAX_GRANT_RECORDS] = [GrantRecord::EMPTY; MAX_GRANT_RECORDS];

pub fn add_record(record: GrantRecord) -> Result<(), PwidError> {
    lock_grants();
    let records = unsafe { &mut GRANT_RECORDS };
    for i in 0..MAX_GRANT_RECORDS {
        if records[i].is_empty() {
            records[i] = record;
            unlock_grants();
            return Ok(());
        }
    }
    unlock_grants();
    Err(PwidError::TableFull)
}

pub fn is_grantor(grantor_pwid: u64, grantee_pwid: u64, domain: CapDomain, caps: CapBits) -> bool {
    lock_grants();
    let records = unsafe { &GRANT_RECORDS };
    let mut found = false;
    for record in records.iter() {
        if record.grantor_pwid.0 == grantor_pwid
            && record.grantee_pwid.0 == grantee_pwid
            && record.domain == domain
            && (record.caps & caps) == caps
        {
            found = true;
            break;
        }
    }
    unlock_grants();
    found
}

pub fn clear_records(revoker_pwid: u64, target_pwid: u64, domain: CapDomain, caps: CapBits) {
    lock_grants();
    let records = unsafe { &mut GRANT_RECORDS };
    for record in records.iter_mut() {
        if record.grantor_pwid.0 == revoker_pwid
            && record.grantee_pwid.0 == target_pwid
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
