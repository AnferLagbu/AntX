//! Session Management and Privilege Elevation
//!
//! Handles user sessions, login/logout, context switching,
//! and privilege elevation with token-based security.

use super::types::*;
use super::manager;
use super::sha256;
use core::sync::atomic::{AtomicU64, AtomicIsize, AtomicBool, Ordering};

/// Constant-time byte array comparison (prevents timing side-channel attacks)
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Serial print macro (placeholder)
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

/// Maximum elevation stack depth
const MAX_ELEVATION_DEPTH: usize = 8;

/// Maximum failed login attempts before lockout
pub const MAX_LOGIN_ATTEMPTS: u32 = 5;

/// Lockout duration in seconds after too many failures
pub const LOCKOUT_DURATION: u64 = 300; // 5 minutes

/// Session Manager - handles authentication and session state
pub struct SessionManager {
    /// Current active session context (pub for FFI access)
    pub current: core::cell::UnsafeCell<PwidContext>,
    
    /// Elevation stack for privilege escalation
    elevation_stack: core::cell::UnsafeCell<[PwidContext; MAX_ELEVATION_DEPTH]>,
    
    /// Current elevation depth
    elevation_depth: AtomicIsize,
    
    /// Current elevation token ID
    elevation_token_id: AtomicU64,
    
    /// Spinlock for thread safety
    lock: AtomicBool,
}

impl SessionManager {
    pub const fn new() -> Self {
        const DEFAULT_CONTEXT: PwidContext = PwidContext {
            current_entry: core::ptr::null(),
            session_pwid: 0,
        };
        
        Self {
            current: core::cell::UnsafeCell::new(DEFAULT_CONTEXT),
            elevation_stack: core::cell::UnsafeCell::new([DEFAULT_CONTEXT; MAX_ELEVATION_DEPTH]),
            elevation_depth: AtomicIsize::new(0),
            elevation_token_id: AtomicU64::new(0),
            lock: AtomicBool::new(false),
        }
    }

    /// Set the current session to a specific PWID
    pub fn set_context(&self, pwid: u64) {
        let mgr = manager::get_manager();
        
        if let Some(entry) = mgr.find(pwid) {
            if !entry.has_flag(PwidFlags::DISABLED) {
                unsafe {
                    let ctx = &mut *self.current.get();
                    ctx.current_entry = entry as *const PwidEntry;
                    ctx.session_pwid = pwid;
                }
            } else {
                self.clear_context();
            }
        } else {
            self.clear_context();
        }
    }

    /// Clear the current session context
    pub fn clear_context(&self) {
        unsafe {
            let ctx = &mut *self.current.get();
            ctx.current_entry = core::ptr::null();
            ctx.session_pwid = 0;
        }
    }

    /// Get current session's PWID
    pub fn get_current(&self) -> u64 {
        unsafe {
            let ctx = &*self.current.get();
            // v4: return 0 for no session (no guest PWID)
            ctx.session_pwid
        }
    }

    /// Get current entry pointer
    pub fn get_current_entry(&self) -> *const PwidEntry {
        unsafe { (*self.current.get()).current_entry }
    }

    /// Login with username and password
    pub fn login(&self, note: &str, password: &str) -> PwidError {
        let mgr = manager::get_manager();
        
        // Find user by note
        let entry = match mgr.find_by_note(note) {
            Some(e) => e,
            None => {
                serial_println!("[PWID] Login: user '{}' not found", note);
                return PwidError::NotFound;
            }
        };
        
        // Check if disabled
        if entry.has_flag(PwidFlags::DISABLED) {
            serial_println!("[PWID] Login: account '{}' disabled", note);
            return PwidError::Disabled;
        }
        
        // Check if locked
        if self.is_locked(entry.pwid.load(Ordering::Acquire)) {
            serial_println!("[PWID] Login: account '{}' locked", note);
            return PwidError::Disabled;
        }
        
        // Verify password with salted hash (constant-time comparison)
        let stored = &entry.password_hash;
        let mut salt = [0u8; PWID_SALT_LEN];
        salt.copy_from_slice(&stored[PWID_DIGEST_LEN..PWID_HASH_LEN]);
        let hash = super::manager::hash_with_salt(password, &salt);
        if !constant_time_eq(&hash, &stored[..PWID_DIGEST_LEN]) {
            serial_println!("[PWID] Login: incorrect password for '{}'", note);
            self.record_failed_login(entry.pwid.load(Ordering::Acquire));
            return PwidError::PasswordIncorrect;
        }
        
        // Success - set session
        let pwid = entry.pwid.load(Ordering::Acquire);
        
        // Clear failed attempts on successful login
        self.clear_failed_attempts(pwid);
        
        // Update last login time
        entry.last_login_time.store(get_current_time(), Ordering::Release);
        
        // Set as current session
        unsafe {
            let ctx = &mut *self.current.get();
            ctx.current_entry = entry as *const PwidEntry;
            ctx.session_pwid = pwid;
        }
        
        serial_println!("[PWID] Logged in as '{}'", note);
        
        PwidError::Ok
    }

    /// Logout from current session
    pub fn logout(&self) {
        unsafe {
            let ctx = &*self.current.get();
            
            if !ctx.current_entry.is_null() {
                let entry = &*ctx.current_entry;
                serial_println!("[PWID] Logged out from '{}'", entry.get_note_str());
            }
        }
        
        self.clear_context();
    }

    /// Create a new user (requires active session with appropriate permissions)
    pub fn create_user(&self, password: &str, note: &str, level: u8) -> Result<u64, PwidError> {
        let mgr = manager::get_manager();
        
        // Get current session
        let current_pwid = self.get_current();
        let current_entry = mgr.find(current_pwid)
            .ok_or(PwidError::PermissionDenied)?;
        
        // Check if current account is disabled
        if current_entry.has_flag(PwidFlags::DISABLED) {
            return Err(PwidError::Disabled);
        }
        
        // v4: Check if creator holds USER_MGMT_CAP_CREATE
        let creator_caps = mgr.get_caps(current_pwid).unwrap_or([0; 16]);
        if (creator_caps[5] & 4) == 0 {  // USER_MGMT_CAP_CREATE = 1<<2
            return Err(PwidError::PermissionDenied);
        }
        
        // Check if note already exists
        if mgr.find_by_note(note).is_some() {
            return Err(PwidError::AlreadyExists);
        }
        
        // Create the user with creator's caps as ceiling
        let new_pwid = mgr.create(password, note, level, &creator_caps)?;
        
        serial_println!("[PWID] User '{}' created by '{}'", 
                       note, current_entry.get_note_str());
        
        Ok(new_pwid)
    }

    /// Elevate privileges to root (requires root password)
    pub fn elevate(&self, target_pwid: u64, password: &str, duration_secs: u64) -> PwidError {
        self.acquire_lock();
        let mgr = manager::get_manager();
        let current_pwid = self.get_current();
        
        // Must have an active session
        if current_pwid == 0 || self.get_current_entry().is_null() {
            self.release_lock();
            return PwidError::PermissionDenied;
        }
        
        // Check elevation depth limit
        if self.elevation_depth.load(Ordering::Acquire) >= MAX_ELEVATION_DEPTH as isize {
            self.release_lock();
            return PwidError::PermissionDenied;
        }
        
        // Find target (must be root)
        let target = match mgr.find(target_pwid) {
            Some(e) => e,
            None => { self.release_lock(); return PwidError::NotFound; },
        };
        
        if target.get_level() != PwidLevel::Root.as_u8() {
            self.release_lock();
            return PwidError::PermissionDenied;
        }
        
        // Verify target's password
        if !mgr.verify_password(target_pwid, password) {
            self.release_lock();
            return PwidError::PasswordIncorrect;
        }
        
        // Save current context to elevation stack (lock held)
        let depth = self.elevation_depth.fetch_add(1, Ordering::Relaxed) as usize;
        
        unsafe {
            let stack = &mut *self.elevation_stack.get();
            let current_ctx = &*self.current.get();
            stack[depth] = *current_ctx;
        }
        
        // Switch to root context
        unsafe {
            let ctx = &mut *self.current.get();
            ctx.current_entry = target as *const PwidEntry;
            ctx.session_pwid = target_pwid;
        }
        
        // TODO: Create elevation token here
        
        serial_println!("[PWID] Elevated to root for {} seconds", duration_secs);
        
        self.release_lock();
        PwidError::Ok
    }

    /// End privilege elevation and restore previous context
    pub fn end_elevation(&self) {
        self.acquire_lock();
        let depth = self.elevation_depth.load(Ordering::Acquire);
        
        if depth > 0 {
            let new_depth = depth - 1;
            self.elevation_depth.store(new_depth, Ordering::Release);
            
            // TODO: Revoke elevation token
            
            // Restore previous context
            if new_depth >= 0 {
                unsafe {
                    let stack = &*self.elevation_stack.get();
                    let prev = stack[new_depth as usize];
                    
                    let ctx = &mut *self.current.get();
                    *ctx = prev;
                }
                
                serial_println!("[PWID] Elevation ended (depth={})", new_depth);
            }
        }
        self.release_lock();
    }

    /// Check if currently elevated
    pub fn is_elevated(&self) -> bool {
        self.elevation_depth.load(Ordering::Acquire) > 0
    }

    /// Check if a PWID is expired (v4: no special protection)
    pub fn is_expired(&self, pwid: u64) -> bool {
        let mgr = manager::get_manager();
        
        match mgr.find(pwid) {
            Some(entry) => {
                let expires_at = entry.expires_at.load(Ordering::Acquire);
                
                // Zero means never expires
                if expires_at == 0 {
                    return false;
                }
                
                let now = get_current_time();
                now >= expires_at
            }
            None => true,
        }
    }

    /// Check if a PWID is locked out
    pub fn is_locked(&self, pwid: u64) -> bool {
        let mgr = manager::get_manager();
        
        match mgr.find(pwid) {
            Some(entry) => {
                if !entry.has_flag(PwidFlags::LOCKED) {
                    return false;
                }
                
                let lockout_until = entry.lockout_until.load(Ordering::Acquire);
                
                if lockout_until == 0 {
                    return false;
                }
                
                let now = get_current_time();
                
                // If lockout has expired, unlock automatically
                if now >= lockout_until {
                    entry.remove_flags(PwidFlags::LOCKED);
                    entry.lockout_until.store(0, Ordering::Release);
                    entry.failed_attempts.store(0, Ordering::Release);
                    return false;
                }
                
                true
            }
            None => true,
        }
    }

    /// Record a failed login attempt
    pub fn record_failed_login(&self, pwid: u64) {
        let mgr = manager::get_manager();
        
        if let Some(entry) = mgr.find(pwid) {
            let attempts = entry.failed_attempts.fetch_add(1, Ordering::Relaxed) + 1;
            
            if attempts >= MAX_LOGIN_ATTEMPTS {
                entry.add_flags(PwidFlags::LOCKED);
                entry.lockout_until.store(
                    get_current_time() + LOCKOUT_DURATION,
                    Ordering::Release
                );
                
                serial_println!("[PWID] Account locked due to too many failed attempts");
            }
        }
    }

    /// Clear failed login attempts
    pub fn clear_failed_attempts(&self, pwid: u64) {
        let mgr = manager::get_manager();
        
        if let Some(entry) = mgr.find(pwid) {
            entry.failed_attempts.store(0, Ordering::Release);
        }
    }

    /// Set expiry time for a PWID
    pub fn set_expiry(&self, pwid: u64, expires_at: u64) {
        let mgr = manager::get_manager();
        
        if let Some(entry) = mgr.find(pwid) {
            entry.expires_at.store(expires_at, Ordering::Release);
            entry.remove_flags(PwidFlags::EXPIRED);
            mgr.set_modified();
        }
    }

    /// Extend expiry by specified number of days
    pub fn extend_expiry(&self, pwid: u64, days: u64) {
        let mgr = manager::get_manager();
        
        if let Some(entry) = mgr.find(pwid) {
            let now = get_current_time();
            let extension = days * 86400; // seconds per day
            
            let current_expiry = entry.expires_at.load(Ordering::Acquire);
            let new_expiry = if current_expiry > now {
                current_expiry + extension
            } else {
                now + extension
            };
            
            entry.expires_at.store(new_expiry, Ordering::Release);
            entry.remove_flags(PwidFlags::EXPIRED);
            mgr.set_modified();
        }
    }

    /// Clear lockout status
    pub fn clear_lockout(&self, pwid: u64) {
        let mgr = manager::get_manager();
        
        if let Some(entry) = mgr.find(pwid) {
            entry.remove_flags(PwidFlags::LOCKED);
            entry.lockout_until.store(0, Ordering::Release);
            entry.failed_attempts.store(0, Ordering::Release);
        }
    }

    // ==================== Private Methods ====================

    #[inline(always)]
    fn acquire_lock(&self) {
        while self.lock.compare_exchange_weak(
            false, true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn release_lock(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

/// Get current time from TSC register
fn get_current_time() -> u64 {
    unsafe {
        let tsc: u64;
        core::arch::asm!(
            "rdtsc",
            out("eax") tsc,
            options(nostack, nomem)
        );
        tsc / 3_000_000_000u64
    }
}

// Global instance
static mut GLOBAL_SESSION: SessionManager = SessionManager::new();

/// Get reference to global session manager
pub fn get_session() -> &'static SessionManager {
    unsafe { &GLOBAL_SESSION }
}
