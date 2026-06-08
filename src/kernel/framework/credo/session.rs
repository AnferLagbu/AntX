use super::identity;
use super::types::*;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

const MAX_ELEVATION_DEPTH: isize = 8;
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECS: u64 = 300;

const SUID_CTX: PwmContext = PwmContext {
    current_entry: core::ptr::null(),
    session_pwm: PwmId::ZERO,
    cached_uid: 0,
    cached_gid: 0,
    euid: 0,
    egid: 0,
    saved_euid: 0,
    saved_egid: 0,
    active_domain_id: DomainId::ZERO,
    elevation_granted_pwm: PwmId::ZERO,
};

pub struct SessionManager {
    pub current: UnsafeCell<PwmContext>,
    elevation_stack: UnsafeCell<[PwmContext; 8]>,
    elevation_depth: AtomicIsize,
    lock: AtomicBool,
}

impl SessionManager {
    pub const fn new() -> Self {
        Self {
            current: UnsafeCell::new(SUID_CTX),
            elevation_stack: UnsafeCell::new([SUID_CTX; 8]),
            elevation_depth: AtomicIsize::new(0),
            lock: AtomicBool::new(false),
        }
    }

    fn acquire(&self) {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }

    // ===== raw 子模块封装的 UnsafeCell 安全访问器 (Framekernel) =====
    // SAFETY rationale: 所有方法均通过 acquire/release 自旋锁保护,
    // 因此 UnsafeCell::get() 的别名违反风险被排除。

    fn read_ctx<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&PwmContext) -> R,
    {
        self.acquire();
        // SAFETY: 持锁下访问 UnsafeCell, 排他性已保证。
        let r = unsafe { f(&*self.current.get()) };
        self.release();
        r
    }

    fn write_ctx<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut PwmContext) -> R,
    {
        self.acquire();
        // SAFETY: 持锁下访问 UnsafeCell, 排他性已保证。
        let r = unsafe { f(&mut *self.current.get()) };
        self.release();
        r
    }

    fn with_stack<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut [PwmContext; 8]) -> R,
    {
        self.acquire();
        // SAFETY: 持锁下访问 UnsafeCell, 排他性已保证。
        let r = unsafe { f(&mut *self.elevation_stack.get()) };
        self.release();
        r
    }

    pub fn login(&self, note: &str, password: &str) -> Result<u64, PwmError> {
        let t = identity::get_table();
        let entry = t.find_by_note(note).ok_or(PwmError::NotFound)?;

        if entry.has_flag(PwmFlags::DISABLED) {
            return Err(PwmError::Disabled);
        }

        let now = super::bootstrap::pwm_now();
        let lockout = entry.lockout_until.load(Ordering::Acquire);
        if lockout > 0 && now < lockout {
            return Err(PwmError::Disabled);
        }

        let pwm = entry.pwm.load(Ordering::Acquire);

        if !t.verify_password(pwm, password) {
            let attempts = entry.failed_attempts.fetch_add(1, Ordering::AcqRel) + 1;
            if attempts >= MAX_LOGIN_ATTEMPTS {
                entry
                    .lockout_until
                    .store(now + LOCKOUT_DURATION_SECS * 1_000_000, Ordering::Release);
                entry.add_flags(PwmFlags::LOCKED);
            }
            return Err(PwmError::PasswordIncorrect);
        }

        entry.failed_attempts.store(0, Ordering::Release);
        entry.remove_flags(PwmFlags::LOCKED);
        entry.last_login_time.store(now, Ordering::Release);

        let uid = entry.get_uid();
        let gid = entry.get_gid();

        self.write_ctx(|ctx| {
            ctx.current_entry = entry;
            ctx.session_pwm = PwmId(pwm);
            ctx.cached_uid = uid;
            ctx.cached_gid = gid;
            ctx.euid = uid;
            ctx.egid = gid;
            ctx.saved_euid = uid;
            ctx.saved_egid = gid;
            ctx.active_domain_id = DomainId::from_uid(uid);
            ctx.elevation_granted_pwm = PwmId::ZERO;
        });

        super::audit::log(pwm, AuditAction::Login, pwm, 0, 0);

        Ok(pwm)
    }

    pub fn logout(&self) {
        let pwm = self.write_ctx(|ctx| {
            let pwm = ctx.session_pwm.as_u64();
            ctx.current_entry = core::ptr::null();
            ctx.session_pwm = PwmId::ZERO;
            ctx.cached_uid = 0;
            ctx.cached_gid = 0;
            ctx.euid = 0;
            ctx.egid = 0;
            ctx.saved_euid = 0;
            ctx.saved_egid = 0;
            ctx.active_domain_id = DomainId::ZERO;
            ctx.elevation_granted_pwm = PwmId::ZERO;
            pwm
        });
        self.elevation_depth.store(0, Ordering::Release);
        super::audit::log(pwm, AuditAction::Logout, pwm, 0, 0);
    }

    pub fn get_current_pwm(&self) -> u64 {
        self.read_ctx(|ctx| ctx.session_pwm.as_u64())
    }

    pub fn get_current_entry(&self) -> *const PwmEntry {
        self.read_ctx(|ctx| ctx.current_entry)
    }

    pub fn get_current_uid(&self) -> u32 {
        self.read_ctx(|ctx| ctx.cached_uid)
    }

    pub fn get_current_gid(&self) -> u32 {
        self.read_ctx(|ctx| ctx.cached_gid)
    }

    pub fn get_euid(&self) -> u32 {
        self.read_ctx(|ctx| ctx.euid)
    }

    pub fn get_egid(&self) -> u32 {
        self.read_ctx(|ctx| ctx.egid)
    }

    pub fn get_saved_euid(&self) -> u32 {
        self.read_ctx(|ctx| ctx.saved_euid)
    }

    pub fn get_saved_egid(&self) -> u32 {
        self.read_ctx(|ctx| ctx.saved_egid)
    }

    pub fn get_domain_id(&self) -> DomainId {
        self.read_ctx(|ctx| ctx.active_domain_id)
    }

    pub fn is_logged_in(&self) -> bool {
        self.get_current_pwm() != 0
    }

    pub fn clear_lockout(&self, pwm: u64) -> Result<(), PwmError> {
        let entry = identity::find(pwm).ok_or(PwmError::NotFound)?;
        entry.lockout_until.store(0, Ordering::Release);
        entry.failed_attempts.store(0, Ordering::Release);
        entry.remove_flags(PwmFlags::LOCKED);
        Ok(())
    }

    pub fn elevate_for_suid(&self, target_pwm: u64) -> bool {
        let target_entry = match identity::find(target_pwm) {
            Some(e) => e,
            None => return false,
        };

        let target_uid = target_entry.get_uid();
        let target_gid = target_entry.get_gid();

        let depth = self.elevation_depth.load(Ordering::Acquire);
        if depth >= MAX_ELEVATION_DEPTH {
            return false;
        }

        let mut session_pwm = 0u64;
        self.write_ctx(|ctx| {
            session_pwm = ctx.session_pwm.as_u64();
            ctx.euid = target_uid;
            ctx.egid = target_gid;
            ctx.saved_euid = target_uid;
            ctx.saved_egid = target_gid;
            ctx.active_domain_id = DomainId::from_uid(target_uid);
            ctx.elevation_granted_pwm = PwmId(target_pwm);
        });

        // push current ctx snapshot onto stack
        let snapshot = self.read_ctx(|ctx| *ctx);
        self.with_stack(|stack| {
            stack[depth as usize] = snapshot;
        });

        self.elevation_depth.store(depth + 1, Ordering::Release);

        super::audit::log(
            session_pwm,
            AuditAction::Grant,
            target_pwm,
            0,
            1,
        );

        true
    }

    pub fn drop_elevation(&self) -> bool {
        let depth = self.elevation_depth.load(Ordering::Acquire);
        if depth == 0 {
            return false;
        }

        let saved = self.with_stack(|stack| stack[(depth - 1) as usize]);
        self.write_ctx(|ctx| {
            *ctx = saved;
        });
        self.elevation_depth.store(depth - 1, Ordering::Release);
        true
    }

    pub fn has_elevation_authority(&self, target_pwm: u64) -> bool {
        self.read_ctx(|ctx| ctx.elevation_granted_pwm == PwmId(target_pwm))
    }

    pub fn try_setuid(&self, target_uid: u32) -> bool {
        let table = identity::get_table();
        let target_entry = match table.find_by_uid(target_uid) {
            Some(e) => e,
            None => return false,
        };
        let target_pwm = target_entry.get_pwm().0;

        self.acquire();
        // SAFETY: `self` 由调用方保证为有效指针; 只读访问
        let current_pwm = unsafe { (*self.current.get()).session_pwm.as_u64() };
        self.release();

        if super::engine::check_privilege(target_pwm, current_pwm)
            || self.has_elevation_authority(target_pwm)
        {
            self.elevate_for_suid(target_pwm)
        } else {
            false
        }
    }

    pub fn try_setgid(&self, target_gid: u32) -> bool {
        let egid = self.get_egid();
        if target_gid == egid {
            return true;
        }

        let table = identity::get_table();
        let target_entry = match table.find_by_gid(target_gid) {
            Some(e) => e,
            None => return false,
        };
        let target_pwm = target_entry.get_pwm().0;

        let (current_pwm, cached_gid, saved_egid) = self.read_ctx(|ctx| {
            (ctx.session_pwm.as_u64(), ctx.cached_gid, ctx.saved_egid)
        });

        if super::engine::check_privilege(target_pwm, current_pwm) {
            self.write_ctx(|ctx| {
                ctx.egid = target_gid;
                ctx.saved_egid = target_gid;
            });
            return true;
        }

        if target_gid == cached_gid || target_gid == saved_egid {
            self.write_ctx(|ctx| {
                ctx.egid = target_gid;
            });
            return true;
        }

        false
    }

    pub fn try_seteuid(&self, target_euid: u32) -> bool {
        let euid = self.get_euid();
        if target_euid == euid {
            return true;
        }

        let table = identity::get_table();
        let target_entry = match table.find_by_uid(target_euid) {
            Some(e) => e,
            None => return false,
        };
        let target_pwm = target_entry.get_pwm().0;

        let (current_pwm, cached_uid, saved_euid) = self.read_ctx(|ctx| {
            (ctx.session_pwm.as_u64(), ctx.cached_uid, ctx.saved_euid)
        });

        if super::engine::check_privilege(target_pwm, current_pwm) {
            self.write_ctx(|ctx| {
                ctx.euid = target_euid;
                ctx.saved_euid = target_euid;
            });
            return true;
        }

        if target_euid == cached_uid || target_euid == saved_euid {
            self.write_ctx(|ctx| {
                ctx.euid = target_euid;
            });
            return true;
        }

        false
    }

    pub fn try_setegid(&self, target_egid: u32) -> bool {
        let egid = self.get_egid();
        if target_egid == egid {
            return true;
        }

        let table = identity::get_table();
        let target_entry = match table.find_by_gid(target_egid) {
            Some(e) => e,
            None => return false,
        };
        let target_pwm = target_entry.get_pwm().0;

        let (current_pwm, cached_gid, saved_egid) = self.read_ctx(|ctx| {
            (ctx.session_pwm.as_u64(), ctx.cached_gid, ctx.saved_egid)
        });

        if super::engine::check_privilege(target_pwm, current_pwm) {
            self.write_ctx(|ctx| {
                ctx.egid = target_egid;
                ctx.saved_egid = target_egid;
            });
            return true;
        }

        if target_egid == cached_gid || target_egid == saved_egid {
            self.write_ctx(|ctx| {
                ctx.egid = target_egid;
            });
            return true;
        }

        false
    }

    pub fn try_setreuid(&self, target_ruid: u32, target_euid: u32) -> bool {
        #[inline]
        fn has_uid_privilege(table: &identity::IdentityTable, uid: u32, current_pwm: u64) -> bool {
            if let Some(entry) = table.find_by_uid(uid) {
                super::engine::check_privilege(entry.get_pwm().0, current_pwm)
            } else {
                false
            }
        }

        let (current_pwm, old_cached_uid, old_euid, old_saved_euid) = self.read_ctx(|ctx| {
            (
                ctx.session_pwm.as_u64(),
                ctx.cached_uid,
                ctx.euid,
                ctx.saved_euid,
            )
        });

        let ruid_is_set = target_ruid != u32::MAX;
        let euid_is_set = target_euid != u32::MAX;

        let new_ruid = if ruid_is_set {
            target_ruid
        } else {
            old_cached_uid
        };
        let new_euid = if euid_is_set { target_euid } else { old_euid };

        if new_ruid == old_cached_uid && new_euid == old_euid {
            return true;
        }

        let table = identity::get_table();

        if ruid_is_set && new_ruid != old_cached_uid {
            let ok = has_uid_privilege(table, new_ruid, current_pwm) || new_ruid == old_euid;
            if !ok {
                return false;
            }
        }

        if euid_is_set && new_euid != old_euid {
            let ok = has_uid_privilege(table, new_euid, current_pwm)
                || new_euid == old_cached_uid
                || new_euid == old_saved_euid;
            if !ok {
                return false;
            }
        }

        let saved_euid_should_update = ruid_is_set || (euid_is_set && new_euid != old_cached_uid);
        self.write_ctx(|ctx| {
            ctx.cached_uid = new_ruid;
            ctx.euid = new_euid;
            if saved_euid_should_update {
                ctx.saved_euid = new_euid;
            }
        });

        true
    }

    pub fn try_setregid(&self, target_rgid: u32, target_egid: u32) -> bool {
        #[inline]
        fn has_gid_privilege(table: &identity::IdentityTable, gid: u32, current_pwm: u64) -> bool {
            if let Some(entry) = table.find_by_gid(gid) {
                super::engine::check_privilege(entry.get_pwm().0, current_pwm)
            } else {
                false
            }
        }

        let (current_pwm, old_cached_gid, old_egid, old_saved_egid) = self.read_ctx(|ctx| {
            (
                ctx.session_pwm.as_u64(),
                ctx.cached_gid,
                ctx.egid,
                ctx.saved_egid,
            )
        });

        let rgid_is_set = target_rgid != u32::MAX;
        let egid_is_set = target_egid != u32::MAX;

        let new_rgid = if rgid_is_set {
            target_rgid
        } else {
            old_cached_gid
        };
        let new_egid = if egid_is_set { target_egid } else { old_egid };

        if new_rgid == old_cached_gid && new_egid == old_egid {
            return true;
        }

        let table = identity::get_table();

        if rgid_is_set && new_rgid != old_cached_gid {
            let ok = has_gid_privilege(table, new_rgid, current_pwm) || new_rgid == old_egid;
            if !ok {
                return false;
            }
        }

        if egid_is_set && new_egid != old_egid {
            let ok = has_gid_privilege(table, new_egid, current_pwm)
                || new_egid == old_cached_gid
                || new_egid == old_saved_egid;
            if !ok {
                return false;
            }
        }

        let saved_egid_should_update = rgid_is_set || (egid_is_set && new_egid != old_cached_gid);
        self.write_ctx(|ctx| {
            ctx.cached_gid = new_rgid;
            ctx.egid = new_egid;
            if saved_egid_should_update {
                ctx.saved_egid = new_egid;
            }
        });

        true
    }
}

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock as Mutex;
// SAFETY: SessionManager uses spinlock (AtomicBool) for all mutations.
// UnsafeCell<PwmContext> is only accessed under the lock. elevation_stack
// is similarly guarded. All public methods acquire/release the lock.
unsafe impl Send for SessionManager {}
unsafe impl Sync for SessionManager {}

static GLOBAL_SESSION: Mutex<SessionManager> = Mutex::new(SessionManager::new());

pub fn login(note: &str, password: &str) -> Result<u64, PwmError> {
    GLOBAL_SESSION.lock().login(note, password)
}

pub fn logout() {
    GLOBAL_SESSION.lock().logout();
}

pub fn get_current_pwm() -> u64 {
    GLOBAL_SESSION.lock().get_current_pwm()
}

pub fn get_current_entry() -> *const PwmEntry {
    GLOBAL_SESSION.lock().get_current_entry()
}

pub fn get_current_uid() -> u32 {
    GLOBAL_SESSION.lock().get_current_uid()
}

pub fn get_current_gid() -> u32 {
    GLOBAL_SESSION.lock().get_current_gid()
}

pub fn get_euid() -> u32 {
    GLOBAL_SESSION.lock().get_euid()
}

pub fn get_egid() -> u32 {
    GLOBAL_SESSION.lock().get_egid()
}

pub fn get_saved_euid() -> u32 {
    GLOBAL_SESSION.lock().get_saved_euid()
}

pub fn get_saved_egid() -> u32 {
    GLOBAL_SESSION.lock().get_saved_egid()
}

pub fn get_current_domain_id() -> u64 {
    GLOBAL_SESSION.lock().get_domain_id().as_u64()
}

pub fn is_logged_in() -> bool {
    GLOBAL_SESSION.lock().is_logged_in()
}

pub fn clear_lockout(pwm: u64) -> Result<(), PwmError> {
    GLOBAL_SESSION.lock().clear_lockout(pwm)
}

pub fn elevate_for_suid(target_pwm: u64) -> bool {
    GLOBAL_SESSION.lock().elevate_for_suid(target_pwm)
}

pub fn drop_elevation() -> bool {
    GLOBAL_SESSION.lock().drop_elevation()
}

pub fn has_elevation_authority(target_pwm: u64) -> bool {
    GLOBAL_SESSION.lock().has_elevation_authority(target_pwm)
}

pub fn try_setuid(target_uid: u32) -> bool {
    GLOBAL_SESSION.lock().try_setuid(target_uid)
}

pub fn try_setgid(target_gid: u32) -> bool {
    GLOBAL_SESSION.lock().try_setgid(target_gid)
}

pub fn try_seteuid(target_euid: u32) -> bool {
    GLOBAL_SESSION.lock().try_seteuid(target_euid)
}

pub fn try_setegid(target_egid: u32) -> bool {
    GLOBAL_SESSION.lock().try_setegid(target_egid)
}

pub fn try_setreuid(ruid: u32, euid: u32) -> bool {
    GLOBAL_SESSION.lock().try_setreuid(ruid, euid)
}

pub fn try_setregid(rgid: u32, egid: u32) -> bool {
    GLOBAL_SESSION.lock().try_setregid(rgid, egid)
}
