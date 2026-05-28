use super::types::*;
use super::identity;
use core::sync::atomic::{AtomicIsize, AtomicBool, Ordering};
use core::cell::UnsafeCell;

const MAX_ELEVATION_DEPTH: isize = 8;
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECS: u64 = 300;

pub struct SessionManager {
    pub current: UnsafeCell<PwmContext>,
    elevation_stack: UnsafeCell<[PwmContext; 8]>,
    elevation_depth: AtomicIsize,
    lock: AtomicBool,
}

impl SessionManager {
    pub const fn new() -> Self {
        const CTX: PwmContext = PwmContext {
            current_entry: core::ptr::null(),
            session_pwm: PwmId::ZERO,
            cached_uid: 0,
            cached_gid: 0,
        };
        Self {
            current: UnsafeCell::new(CTX),
            elevation_stack: UnsafeCell::new([CTX; 8]),
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

        self.acquire();
        unsafe {
            let ctx = &mut *self.current.get();
            ctx.current_entry = entry;
            ctx.session_pwm = PwmId(pwm);
            ctx.cached_uid = entry.get_uid();
            ctx.cached_gid = entry.get_gid();
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
}

use spin::Mutex;

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

pub fn is_logged_in() -> bool {
    GLOBAL_SESSION.lock().is_logged_in()
}

pub fn clear_lockout(pwm: u64) -> Result<(), PwmError> {
    GLOBAL_SESSION.lock().clear_lockout(pwm)
}
