use super::types::*;
use core::sync::atomic::{AtomicUsize, Ordering};

const AUDIT_CAPACITY: usize = 256;

pub struct AuditLog {
    entries: [AuditEntry; AUDIT_CAPACITY],
    count: AtomicUsize,
}

impl AuditLog {
    pub const fn new() -> Self {
        Self {
            entries: [AuditEntry {
                timestamp: 0,
                pwid: PwidId::ZERO,
                action: AuditAction::Login,
                result: AuditResult::Success,
                target_pwid: PwidId::ZERO,
                details: 0,
            }; AUDIT_CAPACITY],
            count: AtomicUsize::new(0),
        }
    }

    pub fn log(&self, pwid: u64, action: AuditAction, target_pwid: u64, domain: u64, caps: u64) {
        let now = super::first_token::pwid_now();
        let idx = self.count.fetch_add(1, Ordering::AcqRel) % AUDIT_CAPACITY;
        let entry = &self.entries[idx];
        let ep = entry as *const AuditEntry as *mut AuditEntry;
        unsafe {
            (*ep).timestamp = now;
            (*ep).pwid = PwidId(pwid);
            (*ep).action = action;
            (*ep).result = AuditResult::Success;
            (*ep).target_pwid = PwidId(target_pwid);
            (*ep).details = (domain << 32) | (caps & 0xFFFFFFFF);
        }
    }

    pub fn dump(&self) {
        let count = self.count.load(Ordering::Acquire);
        let len = if count > AUDIT_CAPACITY { AUDIT_CAPACITY } else { count };
        for i in 0..len {
            let idx = if count > AUDIT_CAPACITY {
                (count - AUDIT_CAPACITY + i) % AUDIT_CAPACITY
            } else {
                i
            };
            let _e = &self.entries[idx];
            crate::serial_println!("[AUDIT] t={} pwid={:#x} action={} target={:#x} details={:#x}",
                e.timestamp, e.pwid.as_u64(), e.action.as_u32(), e.target_pwid.as_u64(), e.details);
        }
    }

    pub fn get_entries(&self) -> &[AuditEntry; AUDIT_CAPACITY] {
        &self.entries
    }
}

static mut GLOBAL_AUDIT: AuditLog = AuditLog::new();

pub fn log(pwid: u64, action: AuditAction, target_pwid: u64, domain: u64, caps: u64) {
    unsafe { GLOBAL_AUDIT.log(pwid, action, target_pwid, domain, caps); }
}

pub fn dump() {
    unsafe { GLOBAL_AUDIT.dump(); }
}
