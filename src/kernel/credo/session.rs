use super::types::*;
use super::identity;
use core::sync::atomic::{AtomicIsize, AtomicBool, Ordering};
use core::cell::UnsafeCell;

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
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    fn release(&self) {
        self.lock.store(false, Ordering::Release);
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
                entry.lockout_until.store(now + LOCKOUT_DURATION_SECS * 1_000_000, Ordering::Release);
                entry.add_flags(PwmFlags::LOCKED);
            }
            return Err(PwmError::PasswordIncorrect);
        }

        entry.failed_attempts.store(0, Ordering::Release);
        entry.remove_flags(PwmFlags::LOCKED);
        entry.last_login_time.store(now, Ordering::Release);

        let uid = entry.get_uid();
        let gid = entry.get_gid();

        self.acquire();
        unsafe {
            let ctx = &mut *self.current.get();
            ctx.current_entry = entry;
            ctx.session_pwm = PwmId(pwm);
            ctx.cached_uid = uid;
            ctx.cached_gid = gid;
            ctx.euid = uid;
            ctx.egid = gid;
            ctx.elevation_granted_pwm = PwmId::ZERO;
        }
        self.release();

        super::audit::log(pwm, AuditAction::Login, pwm, 0, 0);

        Ok(pwm)
    }

    pub fn logout(&self) {
        self.acquire();
        unsafe {
            let ctx = &mut *self.current.get();
            let pwm = ctx.session_pwm.as_u64();
            ctx.current_entry = core::ptr::null();
            ctx.session_pwm = PwmId::ZERO;
            ctx.cached_uid = 0;
            ctx.cached_gid = 0;
            ctx.euid = 0;
            ctx.egid = 0;
            ctx.elevation_granted_pwm = PwmId::ZERO;
            self.elevation_depth.store(0, Ordering::Release);
            super::audit::log(pwm, AuditAction::Logout, pwm, 0, 0);
        }
        self.release();
    }

    pub fn get_current_pwm(&self) -> u64 {
        unsafe { (*self.current.get()).session_pwm.as_u64() }
    }

    pub fn get_current_entry(&self) -> *const PwmEntry {
        unsafe { (*self.current.get()).current_entry }
    }

    pub fn get_current_uid(&self) -> u32 {
        unsafe { (*self.current.get()).cached_uid }
    }

    pub fn get_current_gid(&self) -> u32 {
        unsafe { (*self.current.get()).cached_gid }
    }

    pub fn get_euid(&self) -> u32 {
        unsafe { (*self.current.get()).euid }
    }

    pub fn get_egid(&self) -> u32 {
        unsafe { (*self.current.get()).egid }
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

        self.acquire();
        let depth = self.elevation_depth.load(Ordering::Acquire);
        if depth >= MAX_ELEVATION_DEPTH {
            self.release();
            return false;
        }

        unsafe {
            let stack = &mut *self.elevation_stack.get();
            let ctx = &*self.current.get();
            stack[depth as usize] = *ctx;

            let elevated = &mut *self.current.get();
            elevated.euid = target_uid;
            elevated.egid = target_gid;
            elevated.elevation_granted_pwm = PwmId(target_pwm);
        }
        self.elevation_depth.store(depth + 1, Ordering::Release);
        self.release();

        super::audit::log(
            unsafe { (*self.current.get()).session_pwm.as_u64() },
            AuditAction::Grant,
            target_pwm,
            0,
            1,
        );

        true
    }

    pub fn drop_elevation(&self) -> bool {
        self.acquire();
        let depth = self.elevation_depth.load(Ordering::Acquire);
        if depth == 0 {
            self.release();
            return false;
        }

        unsafe {
            let stack = &mut *self.elevation_stack.get();
            let saved = stack[(depth - 1) as usize];
            let ctx = &mut *self.current.get();
            *ctx = saved;
        }
        self.elevation_depth.store(depth - 1, Ordering::Release);
        self.release();
        true
    }

    pub fn has_elevation_authority(&self, target_pwm: u64) -> bool {
        unsafe {
            (*self.current.get()).elevation_granted_pwm == PwmId(target_pwm)
        }
    }

    pub fn try_setuid(&self, target_uid: u32) -> bool {
        let table = identity::get_table();
        let target_entry = match table.find_by_uid(target_uid) {
            Some(e) => e,
            None => return false,
        };
        let target_pwm = target_entry.get_pwm().0;

        self.acquire();
        let current_pwm = unsafe { (*self.current.get()).session_pwm.as_u64() };
        self.release();

        if super::engine::check_privilege(target_pwm, current_pwm) {
            self.elevate_for_suid(target_pwm)
        } else if self.has_elevation_authority(target_pwm) {
            self.elevate_for_suid(target_pwm)
        } else {
            false
        }
    }

    pub fn try_setgid(&self, target_gid: u32) -> bool {
        let table = identity::get_table();
        let target_entry = match table.find_by_gid(target_gid) {
            Some(e) => e,
            None => return false,
        };
        let target_pwm = target_entry.get_pwm().0;

        self.acquire();
        let current_pwm = unsafe { (*self.current.get()).session_pwm.as_u64() };
        self.release();

        if super::engine::check_privilege(target_pwm, current_pwm) {
            self.acquire();
            unsafe {
                let ctx = &mut *self.current.get();
                ctx.egid = target_gid;
            }
            self.release();
            true
        } else {
            false
        }
    }
}

use spin::Mutex;

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
