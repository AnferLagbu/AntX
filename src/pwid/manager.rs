//! PWID Manager - Core Identity Management
//!
//! Manages PWID entries, provides CRUD operations, permission checking,
//! and user lifecycle management.

use super::types::*;
use super::sha256;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, AtomicU8, AtomicU16, AtomicU32, Ordering};

/// Serial print macro (placeholder)
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

/// Global PWID manager instance
pub struct PwidManager {
    /// Table of all PWID entries
    entries: [PwidEntry; MAX_PWID_ENTRIES],
    
    /// Number of active entries
    count: AtomicUsize,
    
    /// Flag indicating if original root has been created
    original_root_created: AtomicBool,
    
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
            note: [0u8; 128],
            password_hash: [0u8; 32],
            created_time: AtomicU64::new(0),
            expires_at: AtomicU64::new(0),
            lockout_until: AtomicU64::new(0),
            failed_attempts: AtomicU32::new(0),
            last_login_time: AtomicU64::new(0),
        };
        
        Self {
            entries: [DEFAULT_ENTRY; MAX_PWID_ENTRIES],
            count: AtomicUsize::new(0),
            original_root_created: AtomicBool::new(false),
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
        self.original_root_created.store(false, Ordering::Release);
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

    /// Verify a password against stored hash
    pub fn verify_password(&self, pwid: u64, password: &str) -> bool {
        match self.find(pwid) {
            Some(entry) => {
                let hash = sha256::sha256(password.as_bytes());
                
                // Compare hashes
                &entry.password_hash == &hash
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
            
            // Hash password
            let hash = sha256::sha256(password.as_bytes());
            entry.password_hash.copy_from_slice(&hash);
            
            // Set creation time
            entry.created_time.store(get_current_time(), Ordering::Release);
        }
        
        // Update count
        self.count.fetch_add(1, Ordering::Relaxed);
        self.set_modified();
        
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
        
        serial_println!("[PWID] Deleted: 0x{:016X}", pwid);
        
        self.release_lock();
        Ok(())
    }

    /// Disable a PWID entry
    pub fn disable(&self, pwid: u64) -> Result<(), PwidError> {
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        self.entries[slot].add_flags(PwidFlags::DISABLED);
        self.set_modified();
        Ok(())
    }

    /// Enable a previously disabled PWID entry
    pub fn enable(&self, pwid: u64) -> Result<(), PwidError> {
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        
        self.entries[slot].remove_flags(PwidFlags::DISABLED);
        self.set_modified();
        
        Ok(())
    }

    /// Change password for an entry
    pub fn change_password(&self, pwid: u64, old_password: &str, new_password: &str) -> Result<(), PwidError> {
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        
        // Verify old password
        if !self.verify_password(pwid, old_password) {
            serial_println!("[PWID] Error: old password incorrect");
            return Err(PwidError::PasswordIncorrect);
        }
        
        // Update hash
        let hash = sha256::sha256(new_password.as_bytes());
        unsafe {
            let entry_ptr = self.entries[slot].password_hash.as_ptr() as *mut u8;
            core::ptr::copy_nonoverlapping(hash.as_ptr(), entry_ptr, PWID_HASH_LEN);
        }
        
        self.entries[slot].add_flags(PwidFlags::MODIFIED);
        self.entries[slot].remove_flags(PwidFlags::DEFAULT_PW);
        self.set_modified();
        
        serial_println!("[PWID] Password changed for 0x{:016X}", pwid);
        
        Ok(())
    }

    /// Change note/description for an entry
    pub fn change_note(&self, pwid: u64, new_note: &str) -> Result<(), PwidError> {
        let slot = self.find_slot(pwid).ok_or(PwidError::NotFound)?;
        
        if self.entries[slot].has_flag(PwidFlags::ORIGINAL_ROOT) {
            return Err(PwidError::CannotDeleteOriginalRoot);
        }
        
        unsafe {
            let entry_ptr = &self.entries[slot] as *const PwidEntry as *mut PwidEntry;
            (*entry_ptr).set_note(new_note);
        }
        
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

    /// Check if PWID is original root
    pub fn is_original_root(&self, pwid: u64) -> bool {
        self.find(pwid)
            .map(|e| e.has_flag(PwidFlags::ORIGINAL_ROOT))
            .unwrap_or(false)
    }

    /// Check if PWID is root level
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
        
        // v4: Any PWID with USER_MGMT caps can modify
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
    pub fn create_original_root(&self, password: &str) -> Result<u64, PwidError> {
        if self.original_root_created.load(Ordering::Acquire) && !self.wants_genesis() {
            return Err(PwidError::AlreadyExists);
        }

        let all_caps: [u64; 16] = [u64::MAX; 16];
        let pwid = self.create_internal(password, "root", PwidLevel::Root.as_u8(), &all_caps)?;

        if let Some(entry) = self.find_by_note("root") {
            entry.add_flags(PwidFlags::DEFAULT_PW);
            self.original_root_created.store(true, Ordering::Release);
            self.set_modified();
        }

        Ok(pwid)
    }

    fn wants_genesis(&self) -> bool {
        false
    }

    /// Check if original root exists
    pub fn has_original_root(&self) -> bool {
        self.original_root_created.load(Ordering::Acquire)
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
                
                if entry.has_flag(PwidFlags::ORIGINAL_ROOT) {
                    serial_println!("    [ORIG]");
                }
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

    /// Mark database as modified
    pub fn set_modified(&self) {
        self.modified.store(true, Ordering::Release);
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

    /// Find first free slot
    fn find_free_slot(&self) -> Option<usize> {
        for i in 0..MAX_PWID_ENTRIES {
            if !self.entries[i].is_valid() {
                return Some(i);
            }
        }
        None
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
