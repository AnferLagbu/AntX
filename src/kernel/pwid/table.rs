//! PWID v5 Table - Core Identity Management
//!
//! Manages PWID entries: create, find, grant, revoke, transfer_creator, bootstrap.

use super::types::*;
use super::sha256;
use super::audit;
use super::capability::VIABLE_FLOOR;
use super::first_token;
use super::grant_record;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, AtomicU8, AtomicU16, AtomicU32, Ordering};

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff: u8 = 0;
    for i in 0..a.len() { diff |= a[i] ^ b[i]; }
    diff == 0
}

pub(crate) fn hash_with_salt(password: &str, salt: &[u8; PWID_SALT_LEN]) -> [u8; 32] {
    let mut input = [0u8; 256];
    let mut pos = 0usize;
    for byte in salt.iter() { input[pos] = *byte; pos += 1; }
    for byte in password.bytes().take(255 - pos) { input[pos] = byte; pos += 1; }
    let hash = sha256::sha256(&input[..pos]);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash[..32.min(hash.len())]);
    result
}

pub(crate) fn generate_salt() -> [u8; PWID_SALT_LEN] {
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    let mut salt = [0u8; PWID_SALT_LEN];
    let mut v = tsc;
    for i in 0..PWID_SALT_LEN {
        v = v.wrapping_mul(0x9e3779b97f4a7c15);
        salt[i] = (v >> 56) as u8;
    }
    salt
}

pub struct PwidTable {
    pub entries: [PwidEntry; MAX_PWID_ENTRIES],
    pub count: AtomicUsize,
    pub any_identity_exists: AtomicBool,
    modified: AtomicBool,
    lock: AtomicBool,
}

impl PwidTable {
    pub const fn new() -> Self {
        const DEFAULT_ENTRY: PwidEntry = PwidEntry {
            pwid: AtomicU64::new(0),
            creator_pwid: AtomicU64::new(0),
            privilege_level: AtomicU8::new(0xFF),
            flags: AtomicU16::new(0),
            caps: [
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
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

    fn acquire(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }

    pub fn set_modified(&self) {
        self.modified.store(true, Ordering::Release);
    }

    pub fn is_modified(&self) -> bool {
        self.modified.load(Ordering::Acquire)
    }

    pub fn clear_modified(&self) {
        self.modified.store(false, Ordering::Release);
    }

    pub fn init(&self) {
        self.acquire();
        for entry in self.entries.iter() {
            entry.pwid.store(0, Ordering::Release);
        }
        self.count.store(0, Ordering::Release);
        self.any_identity_exists.store(false, Ordering::Release);
        self.modified.store(false, Ordering::Release);
        self.release();
    }

    pub fn generate_pwid(&self, password: &str, note: &str) -> u64 {
        let mut input = [0u8; 256];
        let mut pos = 0;
        for b in password.bytes().take(128) { input[pos] = b; pos += 1; }
        input[pos] = b':'; pos += 1;
        for b in note.bytes().take(127) { input[pos] = b; pos += 1; }
        let hash = sha256::sha256(&input[..pos]);
        let mut pwid: u64 = 0;
        for i in 0..8 {
            pwid = (pwid << 8) | (hash[i] as u64);
        }
        if pwid == 0 { pwid = 1; }
        pwid
    }

    pub fn find(&self, pwid: u64) -> Option<&PwidEntry> {
        if pwid == 0 { return None; }
        for entry in self.entries.iter() {
            if entry.pwid.load(Ordering::Acquire) == pwid {
                return Some(entry);
            }
        }
        None
    }

    pub fn find_by_note(&self, note: &str) -> Option<&PwidEntry> {
        for entry in self.entries.iter() {
            if !entry.is_valid() { continue; }
            if entry.get_note_str() == note {
                return Some(entry);
            }
        }
        None
    }

    pub fn verify_password(&self, pwid: u64, password: &str) -> bool {
        let entry = match self.find(pwid) {
            Some(e) => e,
            None => return false,
        };
        let stored = &entry.password_hash;
        let salt: [u8; PWID_SALT_LEN] = stored[PWID_DIGEST_LEN..PWID_HASH_LEN].try_into().unwrap_or([0u8; PWID_SALT_LEN]);
        let computed = hash_with_salt(password, &salt);
        constant_time_eq(&computed, &stored[..PWID_DIGEST_LEN])
    }

    pub fn create(
        &self,
        password: &str,
        note: &str,
        creator_pwid: u64,
    ) -> Result<u64, PwidError> {
        let privilege_level = if creator_pwid == 0 {
            0u8
        } else {
            let creator = self.find(creator_pwid).ok_or(PwidError::NotFound)?;
            let creator_level = creator.privilege_level.load(Ordering::Acquire);
            if creator_level >= 254 {
                return Err(PwidError::PrivilegeOverflow);
            }
            creator_level + 1
        };

        let pwid = self.generate_pwid(password, note);

        self.acquire();
        for i in 0..MAX_PWID_ENTRIES {
            if self.entries[i].pwid.load(Ordering::Acquire) == pwid {
                self.release();
                return Err(PwidError::AlreadyExists);
            }
        }

        let slot = {
            let mut s = None;
            for i in 0..MAX_PWID_ENTRIES {
                if !self.entries[i].is_valid() {
                    s = Some(i);
                    break;
                }
            }
            s
        };

        let slot = match slot {
            Some(s) => s,
            None => { self.release(); return Err(PwidError::TableFull); }
        };

        let entry = &self.entries[slot];
        entry.pwid.store(pwid, Ordering::Release);
        entry.creator_pwid.store(creator_pwid, Ordering::Release);
        entry.privilege_level.store(privilege_level, Ordering::Release);
        entry.flags.store(0, Ordering::Release);

        let salt = generate_salt();
        let digest = hash_with_salt(password, &salt);
        let hash_ptr = entry.password_hash.as_ptr() as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(digest.as_ptr(), hash_ptr, PWID_DIGEST_LEN);
            core::ptr::copy_nonoverlapping(salt.as_ptr(), hash_ptr.add(PWID_DIGEST_LEN), PWID_SALT_LEN);
        }

        {
            let note_bytes = note.as_bytes();
            let len = note_bytes.len().min(PWID_NOTE_LEN - 1);
            let note_ptr = entry.note.as_ptr() as *mut u8;
            unsafe {
                core::ptr::copy_nonoverlapping(note_bytes.as_ptr(), note_ptr, len);
                *note_ptr.add(len) = 0;
            }
        }

        for i in 0..16 {
            entry.caps[i].store(VIABLE_FLOOR[i], Ordering::Release);
        }

        entry.created_time.store(first_token::pwid_now(), Ordering::Release);
        entry.expires_at.store(0, Ordering::Release);
        entry.lockout_until.store(0, Ordering::Release);
        entry.failed_attempts.store(0, Ordering::Release);
        entry.last_login_time.store(0, Ordering::Release);

        self.count.fetch_add(1, Ordering::AcqRel);
        self.any_identity_exists.store(true, Ordering::Release);
        self.set_modified();
        self.release();

        audit::log(creator_pwid, AuditAction::Create, pwid, 0, privilege_level as u64);

        Ok(pwid)
    }

    pub fn grant(
        &self,
        grantor_pwid: u64,
        grantee_pwid: u64,
        domain: CapDomain,
        caps: CapBits,
    ) -> Result<(), PwidError> {
        let grantor = self.find(grantor_pwid).ok_or(PwidError::NotFound)?;
        let grantee = self.find(grantee_pwid).ok_or(PwidError::NotFound)?;

        let grantor_caps = grantor.load_caps(domain);
        if (grantor_caps & caps) != caps {
            return Err(PwidError::PermissionDenied);
        }

        let grantor_level = grantor.privilege_level.load(Ordering::Acquire);
        let grantee_level = grantee.privilege_level.load(Ordering::Acquire);
        if grantor_level > grantee_level {
            return Err(PwidError::InsufficientPrivilege);
        }

        if grantee.has_flag(PwidFlags::DISABLED) {
            return Err(PwidError::Disabled);
        }

        grantee.fetch_or_caps(domain, caps);

        grant_record::add_record(GrantRecord {
            grantor_pwid,
            grantee_pwid,
            domain,
            caps,
            granted_at: first_token::pwid_now(),
        })?;

        audit::log(grantor_pwid, AuditAction::Grant, grantee_pwid, domain as u64, caps);

        Ok(())
    }

    pub fn revoke(
        &self,
        revoker_pwid: u64,
        target_pwid: u64,
        domain: CapDomain,
        caps: CapBits,
    ) -> Result<(), PwidError> {
        let revoker = self.find(revoker_pwid).ok_or(PwidError::NotFound)?;
        let target = self.find(target_pwid).ok_or(PwidError::NotFound)?;

        let revoker_level = revoker.privilege_level.load(Ordering::Acquire);
        let target_level = target.privilege_level.load(Ordering::Acquire);
        if revoker_level >= target_level {
            return Err(PwidError::InsufficientPrivilege);
        }

        let creator_pwid = target.creator_pwid.load(Ordering::Acquire);
        let is_creator = revoker_pwid == creator_pwid;

        let is_grantor = grant_record::is_grantor(revoker_pwid, target_pwid, domain, caps);

        if !is_creator && !is_grantor {
            return Err(PwidError::NotAuthorized);
        }

        let current = target.load_caps(domain);
        let after_revoke = current & !caps;
        if (after_revoke & VIABLE_FLOOR[domain as usize]) != VIABLE_FLOOR[domain as usize] {
            return Err(PwidError::WouldBreakFloor);
        }

        target.fetch_and_caps(domain, !caps);

        grant_record::clear_records(revoker_pwid, target_pwid, domain, caps);

        self.set_modified();

        audit::log(revoker_pwid, AuditAction::Revoke, target_pwid, domain as u64, caps);

        Ok(())
    }

    pub fn transfer_creator(
        &self,
        current_creator_pwid: u64,
        target_pwid: u64,
        new_creator_pwid: u64,
    ) -> Result<(), PwidError> {
        let current_creator = self.find(current_creator_pwid).ok_or(PwidError::NotFound)?;
        let target = self.find(target_pwid).ok_or(PwidError::NotFound)?;
        let new_creator = self.find(new_creator_pwid).ok_or(PwidError::NotFound)?;

        let creator = target.creator_pwid.load(Ordering::Acquire);
        if creator != current_creator_pwid {
            return Err(PwidError::NotCreator);
        }

        let current_level = current_creator.privilege_level.load(Ordering::Acquire);
        let target_level = target.privilege_level.load(Ordering::Acquire);
        if current_level >= target_level {
            return Err(PwidError::InsufficientPrivilege);
        }

        let new_level = new_creator.privilege_level.load(Ordering::Acquire);
        if new_level >= target_level {
            return Err(PwidError::InsufficientPrivilege);
        }

        target.creator_pwid.store(new_creator_pwid, Ordering::Release);

        self.set_modified();

        audit::log(current_creator_pwid, AuditAction::TransferCreator, target_pwid, 0, new_creator_pwid);

        Ok(())
    }

    pub fn bootstrap(&self, password: &str, note: &str) -> Result<u64, PwidError> {
        first_token::generate_first_token();

        let pwid = self.create(password, note, 0)?;

        for i in 0..16 {
            first_token::grant_from_first_token(pwid, i as CapDomain, 0xFFFFFFFFFFFFFFFF)?;
        }

        Ok(pwid)
    }

    pub fn recover_with_first(&self, password: &str, note: &str) -> Result<u64, PwidError> {
        let pwid_entry = self.find_by_note(note).ok_or(PwidError::NotFound)?;
        let pwid = pwid_entry.pwid.load(Ordering::Acquire);

        if !self.verify_password(pwid, password) {
            return Err(PwidError::InvalidPassword);
        }

        first_token::generate_first_token();

        for i in 0..16 {
            first_token::grant_from_first_token(pwid, i as CapDomain, 0xFFFFFFFFFFFFFFFF)?;
        }

        Ok(pwid)
    }

    pub fn delete(&self, pwid: u64) -> Result<(), PwidError> {
        self.acquire();
        for entry in self.entries.iter() {
            if entry.pwid.load(Ordering::Acquire) == pwid {
                entry.pwid.store(0, Ordering::Release);
                self.count.fetch_sub(1, Ordering::AcqRel);
                self.set_modified();
                self.release();
                return Ok(());
            }
        }
        self.release();
        Err(PwidError::NotFound)
    }

    pub fn disable(&self, pwid: u64) -> Result<(), PwidError> {
        let entry = self.find(pwid).ok_or(PwidError::NotFound)?;
        entry.add_flags(PwidFlags::DISABLED);
        self.set_modified();
        Ok(())
    }

    pub fn enable(&self, pwid: u64) -> Result<(), PwidError> {
        let entry = self.find(pwid).ok_or(PwidError::NotFound)?;
        entry.remove_flags(PwidFlags::DISABLED);
        self.set_modified();
        Ok(())
    }

    pub fn change_password(&self, pwid: u64, old: &str, new: &str) -> Result<(), PwidError> {
        if !self.verify_password(pwid, old) {
            return Err(PwidError::PasswordIncorrect);
        }
        let entry = self.find(pwid).ok_or(PwidError::NotFound)?;
        let salt = generate_salt();
        let digest = hash_with_salt(new, &salt);
        let hash_ptr = entry.password_hash.as_ptr() as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(digest.as_ptr(), hash_ptr, PWID_DIGEST_LEN);
            core::ptr::copy_nonoverlapping(salt.as_ptr(), hash_ptr.add(PWID_DIGEST_LEN), PWID_SALT_LEN);
        }
        self.set_modified();
        Ok(())
    }

    pub fn any_identity_exists(&self) -> bool {
        self.any_identity_exists.load(Ordering::Acquire)
    }
}

static mut GLOBAL_TABLE: PwidTable = PwidTable::new();

pub fn get_table() -> &'static PwidTable {
    unsafe { &GLOBAL_TABLE }
}

pub unsafe fn get_table_mut() -> &'static mut PwidTable {
    &mut GLOBAL_TABLE
}

pub fn find(pwid: u64) -> Option<&'static PwidEntry> {
    get_table().find(pwid)
}
