use super::types::*;
use super::table;
use core::sync::atomic::{AtomicIsize, AtomicBool, Ordering};
use core::cell::UnsafeCell;

const MAX_ELEVATION_DEPTH: isize = 8;
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECS: u64 = 300;

pub struct SessionManager {
    pub current: UnsafeCell<PwidContext>,
    elevation_stack: UnsafeCell<[PwidContext; 8]>,
    elevation_depth: AtomicIsize,
    lock: AtomicBool,
}

impl SessionManager {
    pub const fn new() -> Self {
        const CTX: PwidContext = PwidContext {
            current_entry: core::ptr::null(),
            session_pwid: PwidId::ZERO,
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

    pub fn login(&self, note: &str, password: &str) -> Result<u64, PwidError> {
        let t = table::get_table();
        let entry = t.find_by_note(note).ok_or(PwidError::NotFound)?;

        if entry.has_flag(PwidFlags::DISABLED) {
            return Err(PwidError::Disabled);
        }

        let now = super::first_token::pwid_now();
        let lockout = entry.lockout_until.load(Ordering::Acquire);
        if lockout > 0 && now < lockout {
            return Err(PwidError::Disabled);
        }

        let pwid = entry.pwid.load(Ordering::Acquire);

        if !t.verify_password(pwid, password) {
            let attempts = entry.failed_attempts.fetch_add(1, Ordering::AcqRel) + 1;
            if attempts >= MAX_LOGIN_ATTEMPTS {
                entry.lockout_until.store(now + LOCKOUT_DURATION_SECS * 1_000_000, Ordering::Release);
                entry.add_flags(PwidFlags::LOCKED);
            }
            return Err(PwidError::PasswordIncorrect);
        }

        entry.failed_attempts.store(0, Ordering::Release);
        entry.remove_flags(PwidFlags::LOCKED);
        entry.last_login_time.store(now, Ordering::Release);

        self.acquire();
        unsafe {
            let ctx = &mut *self.current.get();
            ctx.current_entry = entry;
            ctx.session_pwid = PwidId(pwid);
        }
        self.release();

        super::audit::log(pwid, AuditAction::Login, pwid, 0, 0);

        Ok(pwid)
    }

    pub fn logout(&self) {
        self.acquire();
        unsafe {
            let ctx = &mut *self.current.get();
            let pwid = ctx.session_pwid.as_u64();
            ctx.current_entry = core::ptr::null();
            ctx.session_pwid = PwidId::ZERO;
            super::audit::log(pwid, AuditAction::Logout, pwid, 0, 0);
        }
        self.release();
    }

    pub fn get_current_pwid(&self) -> u64 {
        unsafe { (*self.current.get()).session_pwid.as_u64() }
    }

    pub fn get_current_entry(&self) -> *const PwidEntry {
        unsafe { (*self.current.get()).current_entry }
    }

    pub fn is_logged_in(&self) -> bool {
        self.get_current_pwid() != 0
    }

    pub fn clear_lockout(&self, pwid: u64) -> Result<(), PwidError> {
        let entry = table::find(pwid).ok_or(PwidError::NotFound)?;
        entry.lockout_until.store(0, Ordering::Release);
        entry.failed_attempts.store(0, Ordering::Release);
        entry.remove_flags(PwidFlags::LOCKED);
        Ok(())
    }
}

use spin::Mutex;

static GLOBAL_SESSION: Mutex<SessionManager> = Mutex::new(SessionManager::new());

pub fn login(note: &str, password: &str) -> Result<u64, PwidError> {
    GLOBAL_SESSION.lock().login(note, password)
}

pub fn logout() {
    GLOBAL_SESSION.lock().logout();
}

pub fn get_current_pwid() -> u64 {
    GLOBAL_SESSION.lock().get_current_pwid()
}

pub fn get_current_entry() -> *const PwidEntry {
    GLOBAL_SESSION.lock().get_current_entry()
}

pub fn is_logged_in() -> bool {
    GLOBAL_SESSION.lock().is_logged_in()
}

pub fn clear_lockout(pwid: u64) -> Result<(), PwidError> {
    GLOBAL_SESSION.lock().clear_lockout(pwid)
}
