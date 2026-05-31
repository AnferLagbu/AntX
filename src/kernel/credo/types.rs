//! Credo v1 Type Definitions
//!
//! Core data structures for the Credo privilege model.
//! Domain Identity (DID) + capability matrix + identity entry + audit types.
//! Credo: 密码决定身份 | 无预设特权 | 能力来自授予

use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8};

pub const MAX_PWM_ENTRIES: usize = 256;
pub const PWM_NOTE_LEN: usize = 64;
pub const PWM_HASH_LEN: usize = 48;
pub const PWM_SALT_LEN: usize = 16;
pub const PWM_DIGEST_LEN: usize = 32;
pub const MAX_GRANT_RECORDS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PwmId(pub u64);

impl PwmId {
    pub const ZERO: PwmId = PwmId(0);
    pub const TEST: PwmId = PwmId(0x0020F45A8B978417);

    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for PwmId {
    fn default() -> Self {
        Self::ZERO
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct DomainId(pub u64);

impl DomainId {
    pub const ZERO: DomainId = DomainId(0);
    pub const KERNEL: DomainId = DomainId(1);
    pub const ROOT: DomainId = DomainId(1000);
    pub const NOBODY: DomainId = DomainId(65534);

    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn from_uid(uid: u32) -> Self {
        DomainId(uid as u64)
    }

    pub fn to_uid(&self) -> u32 {
        self.0 as u32
    }
}

impl Default for DomainId {
    fn default() -> Self {
        Self::ZERO
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DomainFlags: u32 {
        const NONE       = 0;
        const NO_FORK    = 1 << 0;
        const NO_EXEC    = 1 << 1;
        const NO_NET     = 1 << 2;
        const NO_DEVICE  = 1 << 3;
        const SANDBOX    = 1 << 4;
        const READONLY   = 1 << 5;
        const TEMP       = 1 << 6;
        const SYSTEM     = 1 << 7;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapDomain(pub u16);

impl CapDomain {
    pub const SYSTEM: CapDomain = CapDomain(0);
    pub const FS: CapDomain = CapDomain(1);
    pub const NET: CapDomain = CapDomain(2);
    pub const PROC: CapDomain = CapDomain(3);
    pub const DEVICE: CapDomain = CapDomain(4);
    pub const USER_MGMT: CapDomain = CapDomain(5);
    pub const IPC: CapDomain = CapDomain(6);
    pub const MEM: CapDomain = CapDomain(7);
    pub const TIME: CapDomain = CapDomain(8);
    pub const BARRIER: CapDomain = CapDomain(9);
    pub const SIGNAL: CapDomain = CapDomain(10);
    pub const SHM: CapDomain = CapDomain(11);
    pub const SEM: CapDomain = CapDomain(12);
    pub const MSGQ: CapDomain = CapDomain(13);
    pub const DMA: CapDomain = CapDomain(14);
    pub const RESERVED: CapDomain = CapDomain(15);

    pub fn as_usize(&self) -> usize {
        (self.0 as usize) % 16
    }

    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

impl From<u16> for CapDomain {
    fn from(v: u16) -> Self {
        CapDomain(v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapBits(pub u64);

impl CapBits {
    pub const NONE: CapBits = CapBits(0);
    pub const ALL: CapBits = CapBits(0xFFFFFFFFFFFFFFFF);

    pub fn contains(&self, other: CapBits) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl core::ops::BitOr for CapBits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        CapBits(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for CapBits {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        CapBits(self.0 & rhs.0)
    }
}

impl core::ops::BitOrAssign for CapBits {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl core::ops::BitAndAssign for CapBits {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl core::ops::Not for CapBits {
    type Output = Self;
    fn not(self) -> Self {
        CapBits(!self.0)
    }
}

bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Clone, Copy, Debug)]
    pub struct PwmFlags: u16 {
        const NONE       = 0;
        const DISABLED   = 1 << 0;
        const MODIFIED   = 1 << 3;
        const LOCKED     = 1 << 4;
    }
}

impl Default for PwmFlags {
    fn default() -> Self {
        Self::NONE
    }
}

#[repr(C)]
pub struct PwmEntry {
    pub pwm: AtomicU64,
    pub posix_uid: AtomicU32,
    pub posix_gid: AtomicU32,
    pub creator_pwm: AtomicU64,
    pub privilege_level: AtomicU8,
    pub flags: AtomicU16,
    pub caps: [AtomicU64; 16],
    pub note: [u8; PWM_NOTE_LEN],
    pub password_hash: [u8; PWM_HASH_LEN],
    pub created_time: AtomicU64,
    pub expires_at: AtomicU64,
    pub lockout_until: AtomicU64,
    pub failed_attempts: AtomicU32,
    pub last_login_time: AtomicU64,
}

impl Default for PwmEntry {
    fn default() -> Self {
        Self {
            pwm: AtomicU64::new(0),
            posix_uid: AtomicU32::new(0),
            posix_gid: AtomicU32::new(0),
            creator_pwm: AtomicU64::new(0),
            privilege_level: AtomicU8::new(0xFF),
            flags: AtomicU16::new(0),
            caps: [0; 16].map(AtomicU64::new),
            note: [0u8; PWM_NOTE_LEN],
            password_hash: [0u8; PWM_HASH_LEN],
            created_time: AtomicU64::new(0),
            expires_at: AtomicU64::new(0),
            lockout_until: AtomicU64::new(0),
            failed_attempts: AtomicU32::new(0),
            last_login_time: AtomicU64::new(0),
        }
    }
}

impl PwmEntry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_valid(&self) -> bool {
        self.pwm.load(core::sync::atomic::Ordering::Acquire) != 0
    }

    pub fn get_pwm(&self) -> PwmId {
        PwmId(self.pwm.load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn get_creator_pwm(&self) -> PwmId {
        PwmId(self.creator_pwm.load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn get_flags(&self) -> PwmFlags {
        PwmFlags::from_bits_truncate(self.flags.load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn set_flags(&self, flags: PwmFlags) {
        self.flags
            .store(flags.bits(), core::sync::atomic::Ordering::Release);
    }

    pub fn add_flags(&self, flags: PwmFlags) {
        let current = self.get_flags();
        self.set_flags(current | flags);
    }

    pub fn remove_flags(&self, flags: PwmFlags) {
        let current = self.get_flags();
        self.set_flags(current & !flags);
    }

    pub fn has_flag(&self, flag: PwmFlags) -> bool {
        self.get_flags().contains(flag)
    }

    pub fn get_note_str(&self) -> &str {
        let len = self
            .note
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.note.len());
        unsafe { core::str::from_utf8_unchecked(&self.note[..len]) }
    }

    pub fn set_note(&mut self, note: &str) {
        let bytes = note.as_bytes();
        let len = bytes.len().min(PWM_NOTE_LEN - 1);
        self.note[..len].copy_from_slice(&bytes[..len]);
        self.note[len] = 0;
    }

    pub fn load_caps(&self, domain: CapDomain) -> CapBits {
        let idx = domain.as_usize();
        CapBits(self.caps[idx].load(core::sync::atomic::Ordering::Acquire))
    }

    pub fn store_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = domain.as_usize();
        self.caps[idx].store(caps.0, core::sync::atomic::Ordering::Release);
    }

    pub fn fetch_or_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = domain.as_usize();
        self.caps[idx].fetch_or(caps.0, core::sync::atomic::Ordering::AcqRel);
    }

    pub fn fetch_and_caps(&self, domain: CapDomain, caps: CapBits) {
        let idx = domain.as_usize();
        self.caps[idx].fetch_and(caps.0, core::sync::atomic::Ordering::AcqRel);
    }

    pub fn has_capability(&self, domain: CapDomain, required: CapBits) -> bool {
        let current = self.load_caps(domain);
        current.contains(required)
    }

    pub fn get_uid(&self) -> u32 {
        self.posix_uid.load(core::sync::atomic::Ordering::Acquire)
    }

    pub fn get_gid(&self) -> u32 {
        self.posix_gid.load(core::sync::atomic::Ordering::Acquire)
    }

    pub fn set_uid(&self, uid: u32) {
        self.posix_uid
            .store(uid, core::sync::atomic::Ordering::Release);
    }

    pub fn set_gid(&self, gid: u32) {
        self.posix_gid
            .store(gid, core::sync::atomic::Ordering::Release);
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct PwmContext {
    pub current_entry: *const PwmEntry,
    pub session_pwm: PwmId,
    pub cached_uid: u32,
    pub cached_gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub saved_euid: u32,
    pub saved_egid: u32,
    pub active_domain_id: DomainId,
    pub elevation_granted_pwm: PwmId,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct GrantRecord {
    pub grantor_pwm: PwmId,
    pub grantee_pwm: PwmId,
    pub domain: CapDomain,
    pub caps: CapBits,
    pub granted_at: u64,
}

impl GrantRecord {
    pub const EMPTY: Self = Self {
        grantor_pwm: PwmId::ZERO,
        grantee_pwm: PwmId::ZERO,
        domain: CapDomain(0),
        caps: CapBits::NONE,
        granted_at: 0,
    };

    pub fn is_empty(&self) -> bool {
        self.grantor_pwm == PwmId::ZERO
    }
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwmError {
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

impl PwmError {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default)]
pub enum AuditAction {
    #[default]
    Login = 1,
    Logout = 2,
    Create = 3,
    Delete = 4,
    Modify = 5,
    PasswordChange = 8,
    Grant = 10,
    Revoke = 11,
    TransferCreator = 12,
    FirstTokenGrant = 13,
}

impl AuditAction {
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default)]
pub enum AuditResult {
    #[default]
    Success = 0,
    Failure = 1,
    Denied = 2,
}

impl AuditResult {
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub pwm: PwmId,
    pub action: AuditAction,
    pub result: AuditResult,
    pub target_pwm: PwmId,
    pub details: u64,
}
