//! PWID v5 Type Definitions
//!
//! Core data structures for the PWID v5 privilege model.
//! Design: zero-concept + numeric privilege level + kernel isolation + First Token.
//! PWID初心: 密码决定身份 | 无预设特权 | 能力来自授予

use core::sync::atomic::{AtomicU64, AtomicU32, AtomicU16, AtomicU8};

pub const MAX_PWID_ENTRIES: usize = 256;
pub const PWID_NOTE_LEN: usize = 64;
pub const PWID_HASH_LEN: usize = 48;
pub const PWID_SALT_LEN: usize = 16;
pub const PWID_DIGEST_LEN: usize = 32;
pub const MAX_GRANT_RECORDS: usize = 1024;

pub type CapDomain = u16;
pub type CapBits = u64;

bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Clone, Copy, Debug)]
    pub struct PwidFlags: u16 {
        const NONE       = 0;
        const DISABLED   = 1 << 0;
        const MODIFIED   = 1 << 3;
        const LOCKED     = 1 << 4;
    }
}

impl Default for PwidFlags {
    fn default() -> Self {
        Self::NONE
    }
}

#[repr(C)]
pub struct PwidEntry {
    pub pwid: AtomicU64,
    pub creator_pwid: AtomicU64,
    pub privilege_level: AtomicU8,
    pub flags: AtomicU16,
    pub caps: [AtomicU64; 16],
    pub note: [u8; PWID_NOTE_LEN],
    pub password_hash: [u8; PWID_HASH_LEN],
    pub created_time: AtomicU64,
    pub expires_at: AtomicU64,
    pub lockout_until: AtomicU64,
    pub failed_attempts: AtomicU32,
    pub last_login_time: AtomicU64,
}

impl Default for PwidEntry {
    fn default() -> Self {
        Self {
            pwid: AtomicU64::new(0),
            creator_pwid: AtomicU64::new(0),
            privilege_level: AtomicU8::new(0xFF),
            flags: AtomicU16::new(0),
            caps: [0; 16].map(AtomicU64::new),
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

    pub fn is_valid(&self) -> bool {
        self.pwid.load(core::sync::atomic::Ordering::Acquire) != 0
    }

    pub fn get_flags(&self) -> PwidFlags {
        PwidFlags::from_bits_truncate(
            self.flags.load(core::sync::atomic::Ordering::Acquire)
        )
    }

    pub fn set_flags(&self, flags: PwidFlags) {
        self.flags.store(flags.bits(), core::sync::atomic::Ordering::Release);
    }

    pub fn add_flags(&self, flags: PwidFlags) {
        let current = self.get_flags();
        self.set_flags(current | flags);
    }

    pub fn remove_flags(&self, flags: PwidFlags) {
        let current = self.get_flags();
        self.set_flags(current & !flags);
    }

    pub fn has_flag(&self, flag: PwidFlags) -> bool {
        self.get_flags().contains(flag)
    }

    pub fn get_note_str(&self) -> &str {
        let len = self.note.iter().position(|&b| b == 0).unwrap_or(self.note.len());
        unsafe { core::str::from_utf8_unchecked(&self.note[..len]) }
    }

    pub fn set_note(&mut self, note: &str) {
        let bytes = note.as_bytes();
        let len = bytes.len().min(PWID_NOTE_LEN - 1);
        self.note[..len].copy_from_slice(&bytes[..len]);
        self.note[len] = 0;
    }

    pub fn load_caps(&self, domain: CapDomain) -> CapBits {
        let idx = (domain as usize) % 16;
        self.caps[idx].load(core::sync::atomic::Ordering::Acquire)
    }

    pub fn store_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = (domain as usize) % 16;
        self.caps[idx].store(caps, core::sync::atomic::Ordering::Release);
    }

    pub fn fetch_or_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = (domain as usize) % 16;
        self.caps[idx].fetch_or(caps, core::sync::atomic::Ordering::AcqRel);
    }

    pub fn fetch_and_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = (domain as usize) % 16;
        self.caps[idx].fetch_and(caps, core::sync::atomic::Ordering::AcqRel);
    }

    pub fn has_capability(&self, domain: CapDomain, required: CapBits) -> bool {
        let current = self.load_caps(domain);
        (current & required) == required
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct PwidContext {
    pub current_entry: *const PwidEntry,
    pub session_pwid: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct GrantRecord {
    pub grantor_pwid: u64,
    pub grantee_pwid: u64,
    pub domain: CapDomain,
    pub caps: CapBits,
    pub granted_at: u64,
}

impl GrantRecord {
    pub const EMPTY: Self = Self {
        grantor_pwid: 0,
        grantee_pwid: 0,
        domain: 0,
        caps: 0,
        granted_at: 0,
    };

    pub fn is_empty(&self) -> bool {
        self.grantor_pwid == 0
    }
}

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
    InsufficientPrivilege = -7,
    NotAuthorized = -8,
    NotCreator = -9,
    WouldBreakFloor = -10,
    PrivilegeOverflow = -11,
    TokenUsed = -12,
    NoFirstToken = -13,
    InvalidPassword = -14,
}

impl PwidError {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum AuditAction {
    Create = 3,
    Delete = 4,
    Modify = 5,
    Grant = 10,
    Revoke = 11,
    TransferCreator = 12,
    FirstTokenGrant = 13,
    Login = 1,
    Logout = 2,
    PasswordChange = 8,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum AuditResult {
    Success = 0,
    Failure = 1,
    Denied = 2,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub pwid: u64,
    pub action: u32,
    pub result: u32,
    pub target_pwid: u64,
    pub details: u64,
}
