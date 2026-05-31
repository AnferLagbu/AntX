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
                pwm: PwmId::ZERO,
                action: AuditAction::Login,
                result: AuditResult::Success,
                target_pwm: PwmId::ZERO,
                details: 0,
            }; AUDIT_CAPACITY],
            count: AtomicUsize::new(0),
        }
    }

    pub fn log(&self, pwm: u64, action: AuditAction, target_pwm: u64, domain: u64, caps: u64) {
        let now = super::bootstrap::pwm_now();
        let idx = self.count.fetch_add(1, Ordering::AcqRel) % AUDIT_CAPACITY;
        let entry = &self.entries[idx];
        let ep = entry as *const AuditEntry as *mut AuditEntry;
        unsafe {
            (*ep).timestamp = now;
            (*ep).pwm = PwmId(pwm);
            (*ep).action = action;
            (*ep).result = AuditResult::Success;
            (*ep).target_pwm = PwmId(target_pwm);
            (*ep).details = (domain << 32) | (caps & 0xFFFFFFFF);
        }
    }

    pub fn dump(&self) {
        let count = self.count.load(Ordering::Acquire);
        let len = if count > AUDIT_CAPACITY {
            AUDIT_CAPACITY
        } else {
            count
        };
        for i in 0..len {
            let idx = if count > AUDIT_CAPACITY {
                (count - AUDIT_CAPACITY + i) % AUDIT_CAPACITY
            } else {
                i
            };
            let _e = &self.entries[idx];
            crate::serial_println!(
                "[AUDIT] t={} pwm={:#x} action={} target={:#x} details={:#x}",
                e.timestamp,
                e.pwm.as_u64(),
                e.action.as_u32(),
                e.target_pwm.as_u64(),
                e.details
            );
        }
    }

    pub fn get_entries(&self) -> &[AuditEntry; AUDIT_CAPACITY] {
        &self.entries
    }
}

static mut GLOBAL_AUDIT: AuditLog = AuditLog::new();

pub fn log(pwm: u64, action: AuditAction, target_pwm: u64, domain: u64, caps: u64) {
    unsafe {
        GLOBAL_AUDIT.log(pwm, action, target_pwm, domain, caps);
    }
}

pub fn dump() {
    unsafe {
        GLOBAL_AUDIT.dump();
    }
}
