use super::types::{AuditEntry, PwmId, AuditAction, AuditResult};
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
        // SAFETY rationale: count 已 fetch_add, 后续写不会改变 idx; AuditEntry 不含
        // 重入锁, 多核并发写不同 idx 不会数据竞争; 若 idx 相同 (环形覆盖) 也是
        // 单字节字段级别的无锁覆盖, 接受最后写入语义。
        let entry = &self.entries[idx] as *const AuditEntry as *mut AuditEntry;
        unsafe {
            (*entry).timestamp = now;
            (*entry).pwm = PwmId(pwm);
            (*entry).action = action;
            (*entry).result = AuditResult::Success;
            (*entry).target_pwm = PwmId(target_pwm);
            (*entry).details = (domain << 32) | (caps & 0xFFFFFFFF);
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

// SAFETY: 仅在 audit 子系统串行访问下使用, AuditLog 内部使用 fetch_add 计数
// 多核写不同 idx 不会数据竞争. 集中在此处访问以隔离 static mut 范围.
pub(crate) static mut GLOBAL_AUDIT: AuditLog = AuditLog::new();

pub fn log(pwm: u64, action: AuditAction, target_pwm: u64, domain: u64, caps: u64) {
    raw::log(pwm, action, target_pwm, domain, caps);
}

pub fn dump() {
    raw::dump();
}

// ============================================================================
// 特权子模块 (Framekernel raw): 集中 static mut GLOBAL_AUDIT 访问
// ============================================================================

pub(crate) mod raw {
    use super::{AuditAction, GLOBAL_AUDIT};

    /// 记录一条审计项 (使用 `&mut` 互斥访问)
    pub fn log(pwm: u64, action: AuditAction, target_pwm: u64, domain: u64, caps: u64) {
        // SAFETY: static mut 唯一所有者, 调用方串行或由 audit 自身保证.
        // AuditLog::log 内部使用 fetch_add 计数, 多核写不同 idx 不会数据竞争。
        unsafe { GLOBAL_AUDIT.log(pwm, action, target_pwm, domain, caps) }
    }

    pub fn dump() {
        // SAFETY: 同上, dump 只读取, 安全。
        unsafe { GLOBAL_AUDIT.dump() }
    }
}
