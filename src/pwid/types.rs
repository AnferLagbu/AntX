//! PWID Type Definitions
//!
//! Core data structures for the PWID management system.
//! All types are designed to match C ABI for FFI compatibility.

use core::sync::atomic::{AtomicU64, AtomicBool, AtomicU32, AtomicU16, AtomicU8};

/// Maximum number of PWID entries in the table
pub const MAX_PWID_ENTRIES: usize = 256;

/// Length of the note field (user description)
pub const PWID_NOTE_LEN: usize = 128;

/// Length of SHA-256 hash output (in bytes)
pub const PWID_HASH_LEN: usize = 32;

/// Trust/Permission levels (lower value = higher privilege)
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwidLevel {
    Root = 0,
    Trusted = 1,
    Standard = 2,
    Untrustworthy = 3,
}

impl PwidLevel {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(PwidLevel::Root),
            1 => Some(PwidLevel::Trusted),
            2 => Some(PwidLevel::Standard),
            3 => Some(PwidLevel::Untrustworthy),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// PWID entry flags (bitfield)
bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Clone, Copy, Debug)]
    pub struct PwidFlags: u16 {
        const NONE                = 0;
        const DISABLED            = 1 << 0;
        const ORIGINAL_ROOT       = 1 << 1;
        const DEFAULT_PW          = 1 << 2;
        const MODIFIED            = 1 << 3;
        const LOCKED              = 1 << 4;
        const EXPIRED             = 1 << 5;
    }
}

impl Default for PwidFlags {
    fn default() -> Self {
        Self::NONE
    }
}

/// PWID entry structure (matches C layout for compatibility)
#[repr(C)]
pub struct PwidEntry {
    /// Unique process/user identifier
    pub pwid: AtomicU64,
    
    /// Trust level (0=root, 3=untrustworthy) — v4: label only, not used for permissions
    pub level: AtomicU8,
    
    /// Entry flags
    pub flags: AtomicU16,
    
    /// Capability mask: 16 domains × 64 bits each
    pub capability_mask: [u64; 16],
    
    /// User description/note
    pub note: [u8; PWID_NOTE_LEN],
    
    /// SHA-256 password hash
    pub password_hash: [u8; PWID_HASH_LEN],
    
    /// Creation timestamp
    pub created_time: AtomicU64,
    
    /// Expiration timestamp (0 = never expires)
    pub expires_at: AtomicU64,
    
    /// Lockout end time (for brute-force protection)
    pub lockout_until: AtomicU64,
    
    /// Number of failed login attempts
    pub failed_attempts: AtomicU32,
    
    /// Last successful login time
    pub last_login_time: AtomicU64,
}

impl Default for PwidEntry {
    fn default() -> Self {
        Self {
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
        }
    }
}

impl PwidEntry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if this entry is valid (has a non-zero pwid)
    pub fn is_valid(&self) -> bool {
        self.pwid.load(core::sync::atomic::Ordering::Acquire) != 0
    }

    /// Get current trust level
    pub fn get_level(&self) -> u8 {
        self.level.load(core::sync::atomic::Ordering::Acquire)
    }

    /// Set trust level
    pub fn set_level(&self, level: u8) {
        self.level.store(level, core::sync::atomic::Ordering::Release);
    }

    /// Get flags
    pub fn get_flags(&self) -> PwidFlags {
        PwidFlags::from_bits_truncate(
            self.flags.load(core::sync::atomic::Ordering::Acquire)
        )
    }

    /// Set flags
    pub fn set_flags(&self, flags: PwidFlags) {
        self.flags.store(flags.bits(), core::sync::atomic::Ordering::Release);
    }

    /// Add flag(s)
    pub fn add_flags(&self, flags: PwidFlags) {
        let current = self.get_flags();
        self.set_flags(current | flags);
    }

    /// Remove flag(s)
    pub fn remove_flags(&self, flags: PwidFlags) {
        let current = self.get_flags();
        self.set_flags(current & !flags);
    }

    /// Check if specific flag is set
    pub fn has_flag(&self, flag: PwidFlags) -> bool {
        self.get_flags().contains(flag)
    }

    /// Get note as string slice (returns until first null byte)
    pub fn get_note_str(&self) -> &str {
        let len = self.note.iter().position(|&b| b == 0).unwrap_or(self.note.len());
        unsafe { core::str::from_utf8_unchecked(&self.note[..len]) }
    }

    /// Set note from string (truncates if necessary)
    pub fn set_note(&mut self, note: &str) {
        let bytes = note.as_bytes();
        let len = bytes.len().min(PWID_NOTE_LEN - 1);
        
        self.note[..len].copy_from_slice(&bytes[..len]);
        self.note[len] = 0; // Null terminate
    }
}

/// Session context structure
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct PwidContext {
    /// Pointer to current user's entry (stored as raw pointer for C compat)
    pub current_entry: *const PwidEntry,
    
    /// Current session's pwid
    pub session_pwid: u64,
}

/// Error codes returned by PWID operations
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwidError {
    Ok = 0,
    NotFound = -1,
    Disabled = -2,
    PasswordIncorrect = -3,
    PermissionDenied = -4,
    TableFull = -5,
    AlreadyExists = -6,
    InvalidLevel = -7,
    CannotDeleteOriginalRoot = -8,
}

impl PwidError {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

// ============================================================
// Trust Chain / Capability types (Permission Model v3)
// ============================================================

/// Capability domain (FS=1, NET=2, PROC=3, DEVICE=4, USER_MGMT=5)
pub type CapDomain = u16;

/// Capability bitmask (64 bits per domain)
pub type CapBits = u64;

/// Trust level for delegation chains
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    None = 0,
    Basic = 1,
    Operate = 2,
    Delegate = 3,
    Full = 4,
}

impl Default for TrustLevel {
    fn default() -> Self { TrustLevel::None }
}

/// Audit action types
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum AuditAction {
    Login = 1,
    Logout = 2,
    Create = 3,
    Delete = 4,
    Modify = 5,
    Elevate = 6,
    TokenUse = 7,
    PasswordChange = 8,
}

/// Audit result codes
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum AuditResult {
    Success = 0,
    Failure = 1,
    Denied = 2,
}

/// Audit log entry structure
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AuditEntry {
    /// Timestamp of the event
    pub timestamp: u64,
    
    /// PWID that performed the action
    pub pwid: u64,
    
    /// Action type
    pub action: u32,
    
    /// Result code
    pub result: u32,
    
    /// Target PWID (if applicable)
    pub target_pwid: u64,
    
    /// Additional details
    pub details: u64,
}
