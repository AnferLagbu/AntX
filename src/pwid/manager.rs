//! PWID Manager - Core Identity Management
//!
//! Manages PWID entries, provides CRUD operations, permission checking,
//! and user lifecycle management.

use super::types::*;
use super::sha256;
use super::audit;
use super::storage;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, AtomicU8, AtomicU16, AtomicU32, Ordering};

/// Serial print macro (placeholder)
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

/// Constant-time byte array comparison (prevents timing side-channel attacks)
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Hash password with salt: sha256(salt || password)
pub(crate) fn hash_with_salt(password: &str, salt: &[u8; PWID_SALT_LEN]) -> [u8; 32] {
    let mut input = [0u8; 256];
    let mut pos = 0usize;
    for byte in salt.iter() {
        input[pos] = *byte;
        pos += 1;
    }
    for byte in password.bytes().take(255 - pos) {
        input[pos] = byte;
        pos += 1;
    }
    let hash = sha256::sha256(&input[..pos]);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash[..32.min(hash.len())]);
    result
}

/// Global PWID manager instance
pub struct PwidManager {
    /// Table of all PWID entries
    pub entries: [PwidEntry; MAX_PWID_ENTRIES],
    
    /// Number of active entries
    pub count: AtomicUsize,
    
    /// Flag indicating if any identity exists in the system
    pub any_identity_exists: AtomicBool,
    
    /// Flag indicating if database has been modified
    modified: AtomicBool,
    
    /// Spinlock for thread safety
    lock: AtomicBool,
}

impl PwidManager {
    pub const fn new() -> Self {
        // Create array of default entries using const fn
        const DEFAULT_ENTRY: PwidEntry = PwidEntry {
            pwid: AtomicU64::new(0),
            level: AtomicU8::new(0),
            flags: AtomicU16::new(0),
            capability_mask: [0; 16],
            note: [0u8; PWID_NOTE_LEN],
            password_hash: [0u8; PWID_HASH_LEN],
            created_time: AtomicU64::new(0),
            expires_at: AtomicU64::new(0),
            lockout_until: AtomicU64::new(0),
            failed_attempts: AtomicU32::new(0),
            last_login_time: AtomicU64::new(0),
        };
        
        Self {
            entries: [DEFAULT_ENTRY; MAX_PWID_ENTRIES],
            count: AtomicUsize::new(0),
            any_identity_exists: AtomicBool::new(false),
            modified: AtomicBool::new(false),
            lock: AtomicBool::new(false),
        }
    }

    /// Initialize the PWID system
    pub fn init(&mut self) {
        // Clear all entries
        for i in 0..MAX_PWID_ENTRIES {
            self.entries[i] = PwidEntry::new();
        }
        
        self.count.store(0, Ordering::Release);
        self.any_identity_exists.store(false, Ordering::Release);
        self.modified.store(false, Ordering::Release);
        
        serial_println!("[PWID] Manager initialized");
    }

    /// Generate a unique PWID from password and note
    pub fn generate(&self, password: &str, note: &str, level: u8) -> u64 {
        let mut input = [0u8; 256];
        let mut pos = 0usize;
        
        // Copy password
        for byte in password.bytes().take(128) {
            input[pos] = byte;
            pos += 1;
        }
        
        // Add separator
        if pos < 255 {
            input[pos] = b':';
            pos += 1;
        }
        
        // Copy note
        for byte in note.bytes().take(255 - pos) {
            input[pos] = byte;
            pos += 1;
        }
        
        // Hash the combined input
        let hash = sha256::sha256(&input[..pos]);
        
        // Construct PWID from hash and level
        let mut pwid: u64 = (level as u64) << 60;
        
        for i in 0..7 {
            pwid |= (hash[i] as u64) << (i * 8);
        }
        
        // Use only lower 4 bits of last byte to avoid overflow
        pwid |= ((hash[7] & 0x0F) as u64) << 56;
        
        pwid
    }

    /// Verify a password against stored hash (constant-time, salted)
    pub fn verify_password(&self, pwid: u64, password: &str) -> bool {
        match self.find(pwid) {
            Some(entry) => {
                let stored = &entry.password_hash;
                let mut salt = [0u8; PWID_SALT_LEN];
                salt.copy_from_slice(&stored[PWID_DIGEST_LEN..PWID_HASH_LEN]);
                let hash = hash_with_salt(password, &salt);
                constant_time_eq(&hash, &stored[..PWID_DIGEST_LEN])
            }
            None => false,
        }
    }

    /// Create a new PWID entry
    pub fn create(&self, password: &str, note: &str, level: u8, caps: &[u64; 16]) -> Result<u64, PwidError> {
        self.acquire_lock();
        let result = self.create_internal(password, note, level, caps);
        self.release_lock();
        result
    }

    /// Internal create implementation (must hold lock)
    fn create_internal(&self, password: &str, note: &str, level: u8, caps: &[u64; 16]) -> Result<u64, PwidError> {
        // Check table capacity
        if self.count.load(Ordering::Acquire) >= MAX_PWID_ENTRIES {
            serial_println!("[PWID] Error: table full");
            return Err(PwidError::TableFull);
        }
        
        // Validate trust level
        if level > PwidLevel::Untrustworthy.as_u8() {
            serial_println!("[PWID] Error: invalid level {}", level);
            return Err(PwidError::InvalidLevel);
        }
        
        // Find free slot
        let slot = self.find_free_slot()
            .ok_or_else(|| {
                serial_println!("[PWID] Error: no free slot found");
                PwidError::TableFull
            })?;
        
        // Generate unique identifier
        let new_pwid = self.generate(password, note, level);
        
        // Initialize entry
        unsafe {
            let entry_ptr = self.entries.as_ptr() as *mut PwidEntry;
            let entry = &mut *entry_ptr.add(slot);
            
            entry.pwid.store(new_pwid, Ordering::Release);
            entry.level.store(level, Ordering::Release);
            entry.flags.store(0, Ordering::Release);
            entry.capability_mask.copy_from_slice(caps);
            entry.set_note(note);
            
            // Generate random salt from TSC + loop iteration entropy
            let mut salt = [0u8; PWID_SALT_LEN];
            for i in 0..PWID_SALT_LEN {
                let tsc: u64;
                unsafe { core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack)); }
                salt[i] = (tsc.wrapping_add(i as u64).wrapping_mul(0x9E3779B97F4A7C15) >> 32) as u8;
            }
            
            // Hash password with salt
            let hash = hash_with_salt(password, &salt);
            entry.password_hash[..PWID_DIGEST_LEN].copy_from_slice(&hash);
            entry.password_hash[PWID_DIGEST_LEN..PWID_HASH_LEN].copy_from_slice(&salt);
            
            // Set creation time
            entry.created_time.store(get_current_time(), Ordering::Release);
        }
        
        // Update count
        self.count.fetch_add(1, Ordering::Relaxed);
        self.set_modified();
        audit::get_audit().log(new_pwid, 2, 1, 0, 0);  // action=2: create, result=1: success
        
        serial_println!("[PWID] Created: 0x{:016X} note='{}' level={}", 
                       new_pwid, note, level);
        
        Ok(new_pwid)
    }

    /// Delete a PWID entry
    pub fn delete(&self, pwid: u64) -> Result<(), PwidError> {
        self.acquire_lock();
        
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        
        // Clear entry
        unsafe {
            let entry_ptr = self.entries.as_ptr() as *mut PwidEntry;
            let entry = &mut *entry_ptr.add(slot);
            entry.pwid.store(0, Ordering::Release);
            entry.level.store(0, Ordering::Release);
            entry.flags.store(0, Ordering::Release);
            entry.capability_mask = [0; 16];
        }
        
        self.count.fetch_sub(1, Ordering::Relaxed);
        self.set_modified();
        audit::get_audit().log(pwid, 3, 1, 0, 0);  // action=3: delete, result=1: success
        
        serial_println!("[PWID] Deleted: 0x{:016X}", pwid);
        
        self.release_lock();
        Ok(())
    }

    /// Disable a PWID entry
    pub fn disable(&self, pwid: u64) -> Result<(), PwidError> {
        self.acquire_lock();
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        self.entries[slot].add_flags(PwidFlags::DISABLED);
        self.set_modified();
        audit::get_audit().log(pwid, 4, 1, 0, 0);  // action=4: disable, result=1: success
        self.release_lock();
        Ok(())
    }

    /// Enable a previously disabled PWID entry
    pub fn enable(&self, pwid: u64) -> Result<(), PwidError> {
        self.acquire_lock();
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        self.entries[slot].remove_flags(PwidFlags::DISABLED);
        self.set_modified();
        audit::get_audit().log(pwid, 5, 1, 0, 0);  // action=5: enable, result=1: success
        self.release_lock();
        Ok(())
    }

    /// Change password for an entry
    pub fn change_password(&self, pwid: u64, old_password: &str, new_password: &str) -> Result<(), PwidError> {
        self.acquire_lock();
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        
        // Verify old password
        if !self.verify_password(pwid, old_password) {
            serial_println!("[PWID] Error: old password incorrect");
            self.release_lock();
            return Err(PwidError::PasswordIncorrect);
        }
        
        // Re-hash with the existing salt (or generate new salt)
        let stored = &self.entries[slot].password_hash;
        let mut salt = [0u8; PWID_SALT_LEN];
        salt.copy_from_slice(&stored[PWID_DIGEST_LEN..PWID_HASH_LEN]);
        let hash = hash_with_salt(new_password, &salt);
        unsafe {
            let entry_ptr = self.entries[slot].password_hash.as_ptr() as *mut u8;
            core::ptr::copy_nonoverlapping(hash.as_ptr(), entry_ptr, PWID_DIGEST_LEN);
        }
        
        self.entries[slot].add_flags(PwidFlags::MODIFIED);
        self.entries[slot].remove_flags(PwidFlags::DEFAULT_PW);
        self.set_modified();
        
        serial_println!("[PWID] Password changed for 0x{:016X}", pwid);
        
        audit::get_audit().log(pwid, 6, 1, 0, 0);  // action=6: change_password, result=1: success
        self.release_lock();
        Ok(())
    }

    /// Change note/description for an entry (v4: all identities can change notes)
    pub fn change_note(&self, pwid: u64, new_note: &str) -> Result<(), PwidError> {
        self.acquire_lock();
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        unsafe {
            let entry_ptr = &self.entries[slot] as *const PwidEntry as *mut PwidEntry;
            (*entry_ptr).set_note(new_note);
        }
        audit::get_audit().log(pwid, 7, 1, 0, 0);  // action=7: change_note, result=1: success
        self.release_lock();
        Ok(())
    }

    /// Find entry by PWID
    pub fn find(&self, pwid: u64) -> Option<&PwidEntry> {
        self.find_slot(pwid).map(|i| &self.entries[i])
    }

    /// Find entry by note string
    pub fn find_by_note(&self, note: &str) -> Option<&PwidEntry> {
        for i in 0..MAX_PWID_ENTRIES {
            if self.entries[i].is_valid() && self.entries[i].get_note_str() == note {
                return Some(&self.entries[i]);
            }
        }
        None
    }

    /// Get trust level for a PWID
    pub fn get_level(&self, pwid: u64) -> Option<u8> {
        self.find(pwid).map(|e| e.get_level())
    }

    /// Check if PWID is root level (@deprecated v4 — use capability check)
    pub fn is_root(&self, pwid: u64) -> bool {
        self.get_level(pwid) == Some(PwidLevel::Root.as_u8())
    }

    /// Check if entry uses default password
    pub fn has_default_password(&self, pwid: u64) -> bool {
        self.find(pwid)
            .map(|e| e.has_flag(PwidFlags::DEFAULT_PW))
            .unwrap_or(false)
    }

    /// Clear default password flag
    pub fn clear_default_password_flag(&self, pwid: u64) {
        if let Some(entry) = self.find(pwid) {
            entry.remove_flags(PwidFlags::DEFAULT_PW);
        }
    }

    /// Check permission based on trust level
    pub fn check_permission(&self, pwid: u64, required_level: u8) -> bool {
        match self.find(pwid) {
            Some(entry) => {
                if entry.has_flag(PwidFlags::DISABLED) {
                    false
                } else {
                    entry.get_level() <= required_level
                }
            }
            None => false,
        }
    }

    /// Check if creator can create target level
    pub fn can_create_level(creator_level: u8, target_level: u8) -> bool {
        match PwidLevel::from_u8(creator_level) {
            Some(PwidLevel::Root) => true,
            Some(PwidLevel::Trusted) => target_level == PwidLevel::Untrustworthy.as_u8(),
            _ => false,
        }
    }

    /// Check if modifier can modify target
    pub fn can_modify(&self, modifier_pwid: u64, target_pwid: u64) -> bool {
        let modifier = match self.find(modifier_pwid) {
            Some(e) => e,
            None => return false,
        };
        
        let _target = match self.find(target_pwid) {
            Some(e) => e,
            None => return false,
        };
        
        if modifier.has_flag(PwidFlags::DISABLED) {
            return false;
        }
        
        // v4: Require SYS_ADMIN capability (domain 0, full mask)
        if let Some(caps) = self.get_caps(modifier.pwid.load(Ordering::Acquire)) {
            if caps[0] != u64::MAX {
                return false;
            }
        } else {
            return false;
        }
        
        true
    }

    /// Get capability mask for a PWID
    pub fn get_caps(&self, pwid: u64) -> Option<[u64; 16]> {
        self.find(pwid).map(|e| e.capability_mask)
    }

    /// Create identity with full capabilities (via First Token or existing all-cap holder)
    pub fn create_full_cap(&self, password: &str, note: &str) -> Result<u64, PwidError> {
        let all_caps: [u64; 16] = [u64::MAX; 16];
        self.create(password, note, PwidLevel::Root.as_u8(), &all_caps)
    }

    /// Create a new PWID with subset of creator's caps
    pub fn create_with_subset(&self, password: &str, note: &str, level: u8, caps: &[u64; 16], creator_pwid: u64) -> Result<u64, PwidError> {
        let creator_caps = self.get_caps(creator_pwid).ok_or(PwidError::NotFound)?;
        
        // Creator must hold all requested capabilities
        for i in 0..16 {
            if (creator_caps[i] & caps[i]) != caps[i] {
                return Err(PwidError::PermissionDenied);
            }
        }
        
        self.create(password, note, level, caps)
    }

    /// Delete derived root user
    pub fn delete_derived_root(&self, pwid: u64) -> Result<(), PwidError> {
        self.delete(pwid)
    }

    /// Create first identity (via First Token — no prior auth needed)
    pub fn create_first_identity(&self, password: &str) -> Result<u64, PwidError> {
        if self.any_identity_exists.load(Ordering::Acquire) {
            return Err(PwidError::AlreadyExists);
        }

        let all_caps: [u64; 16] = [u64::MAX; 16];
        let pwid = self.create_internal(password, "root", PwidLevel::Root.as_u8(), &all_caps)?;

        if let Some(entry) = self.find_by_note("root") {
            entry.add_flags(PwidFlags::DEFAULT_PW);
            self.any_identity_exists.store(true, Ordering::Release);
            self.set_modified();
        }

        Ok(pwid)
    }

    /// Check if any identity exists in the system
    pub fn any_identity_exists(&self) -> bool {
        self.any_identity_exists.load(Ordering::Acquire)
    }

    /// List all PWID entries
    pub fn list_all(&self) {
        serial_println!("\n=== PWID List ===");
        
        for i in 0..MAX_PWID_ENTRIES {
            if self.entries[i].is_valid() {
                let entry = &self.entries[i];
                
                serial_println!("  PWID: 0x{:016X} Level: {} Note: '{}'",
                    entry.pwid.load(Ordering::Acquire),
                    entry.get_level(),
                    entry.get_note_str()
                );
                
                if entry.has_flag(PwidFlags::DISABLED) {
                    serial_println!("    [DISABLED]");
                }
            }
        }
        
        serial_println!("=================");
    }

    /// Get number of active entries
    pub fn get_count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    /// Check if database has been modified
    pub fn is_modified(&self) -> bool {
        self.modified.load(Ordering::Acquire)
    }

    /// Mark database as modified (triggers auto-save if identities exist)
    pub fn set_modified(&self) {
        self.modified.store(true, Ordering::Release);
        // Auto-save if there are identities to persist
        if self.any_identity_exists.load(Ordering::Acquire) {
            storage::save_database();
        }
    }

    /// Clear modification flag
    pub fn clear_modified(&self) {
        self.modified.store(false, Ordering::Release);
    }

    // ==================== Private Methods ====================

    /// Find slot index by PWID
    fn find_slot(&self, pwid: u64) -> Option<usize> {
        for i in 0..MAX_PWID_ENTRIES {
            if self.entries[i].pwid.load(Ordering::Acquire) == pwid {
                return Some(i);
            }
        }
        None
    }

    /// Find first free slot (for internal use)
    fn find_free_slot(&self) -> Option<usize> {
        for i in 0..MAX_PWID_ENTRIES {
            if !self.entries[i].is_valid() {
                return Some(i);
            }
        }
        None
    }

    /// Find first free slot without lock acquisition (for storage loading)
    pub fn find_free_slot_lockless(&self) -> Option<usize> {
        self.find_free_slot()
    }

    /// Reference to entries array (for storage module access)
    pub fn get_entries_ptr(&self) -> *const PwidEntry {
        self.entries.as_ptr()
    }

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
        tsc / 3_000_000_000u64  // Convert to seconds (approximate)
    }
}

// Global instance
static mut GLOBAL_MANAGER: PwidManager = PwidManager::new();

/// Get reference to global PWID manager
pub fn get_manager() -> &'static PwidManager {
    unsafe { &GLOBAL_MANAGER }
}

/// Get mutable reference to global PWID manager (for init operations)
///
/// # Safety
/// Should only be called during kernel initialization
pub unsafe fn get_manager_mut() -> &'static mut PwidManager {
    &mut GLOBAL_MANAGER
}
