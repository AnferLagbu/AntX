use super::types::*;
use super::sha256;
use super::audit;
use super::capability::VIABLE_FLOOR;
use super::bootstrap;
use super::grant;
use core::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, AtomicU8, AtomicU16, AtomicU32, Ordering};

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff: u8 = 0;
    for i in 0..a.len() { diff |= a[i] ^ b[i]; }
    diff == 0
}

pub(crate) fn hash_with_salt(password: &str, salt: &[u8; PWM_SALT_LEN]) -> [u8; 32] {
    const STRETCH_ROUNDS: usize = 4096;
    let mut input = [0u8; 256];
    let mut pos = 0usize;
    for byte in salt.iter() { input[pos] = *byte; pos += 1; }
    for byte in password.bytes().take(255 - pos) { input[pos] = byte; pos += 1; }
    let full_hash = sha256::sha256(&input[..pos]);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&full_hash[..32]);
    for round in 1..STRETCH_ROUNDS {
        let mut stretch_input = [0u8; 64];
        stretch_input[..32].copy_from_slice(&hash);
        let round_bytes = (round as u64).to_le_bytes();
        stretch_input[32..40].copy_from_slice(&round_bytes);
        for byte in salt.iter() { stretch_input[40 + (*byte & 0xF) as usize] ^= *byte; }
        let full = sha256::sha256(&stretch_input[..48]);
        hash.copy_from_slice(&full[..32]);
    }
    hash
}

pub(crate) fn generate_salt() -> [u8; PWM_SALT_LEN] {
    static COUNTER: AtomicU64 = AtomicU64::new(0x5A3C_9E17_F2D8_4B61);
    let tsc = crate::arch!(timestamp());
    let stack_addr = &tsc as *const _ as u64;
    let counter = COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::AcqRel);
    let heap_addr = core::ptr::null::<u8>() as u64;
    let mut v = tsc
        .wrapping_mul(0x2545_F491_4F6C_DD1D)
        .wrapping_add(stack_addr)
        .wrapping_mul(0x1329_4A6B_3C7D_8E0F)
        .wrapping_add(counter)
        .wrapping_add(heap_addr);
    let mut salt = [0u8; PWM_SALT_LEN];
    for i in 0..PWM_SALT_LEN {
        v = v.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(tsc.rotate_left(i as u32));
        salt[i] = (v >> 56) as u8;
        salt[i] ^= (v >> 40) as u8;
    }
    let extra = sha256::sha256(&salt);
    for i in 0..PWM_SALT_LEN.min(32) {
        salt[i] ^= extra[i];
    }
    salt
}

pub struct IdentityTable {
    pub entries: [PwmEntry; MAX_PWM_ENTRIES],
    pub count: AtomicUsize,
    pub any_identity_exists: AtomicBool,
    pub next_uid: AtomicU32,
    modified: AtomicBool,
    lock: AtomicBool,
}

impl IdentityTable {
    pub const fn new() -> Self {
        const DEFAULT_ENTRY: PwmEntry = PwmEntry {
            pwm: AtomicU64::new(0),
            posix_uid: AtomicU32::new(0),
            posix_gid: AtomicU32::new(0),
            creator_pwm: AtomicU64::new(0),
            privilege_level: AtomicU8::new(0xFF),
            flags: AtomicU16::new(0),
            caps: [
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
            note: [0u8; PWM_NOTE_LEN],
            password_hash: [0u8; PWM_HASH_LEN],
            created_time: AtomicU64::new(0),
            expires_at: AtomicU64::new(0),
            lockout_until: AtomicU64::new(0),
            failed_attempts: AtomicU32::new(0),
            last_login_time: AtomicU64::new(0),
        };
        Self {
            entries: [DEFAULT_ENTRY; MAX_PWM_ENTRIES],
            count: AtomicUsize::new(0),
            any_identity_exists: AtomicBool::new(false),
            next_uid: AtomicU32::new(1000),
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
            entry.pwm.store(0, Ordering::Release);
        }
        self.count.store(0, Ordering::Release);
        self.any_identity_exists.store(false, Ordering::Release);
        self.modified.store(false, Ordering::Release);
        self.release();
    }

    pub fn generate_pwm(&self, password: &str, note: &str) -> u64 {
        let mut input = [0u8; 256];
        let mut pos = 0;
        for b in password.bytes().take(128) { input[pos] = b; pos += 1; }
        input[pos] = b':'; pos += 1;
        for b in note.bytes().take(127) { input[pos] = b; pos += 1; }
        let hash = sha256::sha256(&input[..pos]);
        let mut pwm: u64 = 0;
        for i in 0..8 {
            pwm = (pwm << 8) | (hash[i] as u64);
        }
        if pwm == 0 { pwm = 1; }
        pwm
    }

    pub fn find(&self, pwm: u64) -> Option<&PwmEntry> {
        if pwm == 0 { return None; }
        for entry in self.entries.iter() {
            if entry.pwm.load(Ordering::Acquire) == pwm {
                return Some(entry);
            }
        }
        None
    }

    pub fn find_by_note(&self, note: &str) -> Option<&PwmEntry> {
        for entry in self.entries.iter() {
            if !entry.is_valid() { continue; }
            if entry.get_note_str() == note {
                return Some(entry);
            }
        }
        None
    }

    pub fn verify_password(&self, pwm: u64, password: &str) -> bool {
        let entry = match self.find(pwm) {
            Some(e) => e,
            None => return false,
        };
        let stored = &entry.password_hash;
        let salt: [u8; PWM_SALT_LEN] = stored[PWM_DIGEST_LEN..PWM_HASH_LEN].try_into().unwrap_or([0u8; PWM_SALT_LEN]);
        let computed = hash_with_salt(password, &salt);
        constant_time_eq(&computed, &stored[..PWM_DIGEST_LEN])
    }

    pub fn create(
        &self,
        password: &str,
        note: &str,
        creator_pwm: u64,
    ) -> Result<u64, PwmError> {
        let privilege_level = if creator_pwm == 0 {
            0u8
        } else {
            let creator = self.find(creator_pwm).ok_or(PwmError::NotFound)?;
            let creator_level = creator.privilege_level.load(Ordering::Acquire);
            if creator_level >= 254 {
                return Err(PwmError::PrivilegeOverflow);
            }
            creator_level + 1
        };

        let pwm = self.generate_pwm(password, note);

        self.acquire();
        for i in 0..MAX_PWM_ENTRIES {
            if self.entries[i].pwm.load(Ordering::Acquire) == pwm {
                self.release();
                return Err(PwmError::AlreadyExists);
            }
        }

        let slot = {
            let mut s = None;
            for i in 0..MAX_PWM_ENTRIES {
                if !self.entries[i].is_valid() {
                    s = Some(i);
                    break;
                }
            }
            s
        };

        let slot = match slot {
            Some(s) => s,
            None => { self.release(); return Err(PwmError::TableFull); }
        };

        let entry = &self.entries[slot];
        entry.pwm.store(pwm, Ordering::Release);
        entry.creator_pwm.store(creator_pwm, Ordering::Release);
        entry.privilege_level.store(privilege_level, Ordering::Release);
        entry.flags.store(0, Ordering::Release);

        let salt = generate_salt();
        let digest = hash_with_salt(password, &salt);
        let hash_ptr = entry.password_hash.as_ptr() as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(digest.as_ptr(), hash_ptr, PWM_DIGEST_LEN);
            core::ptr::copy_nonoverlapping(salt.as_ptr(), hash_ptr.add(PWM_DIGEST_LEN), PWM_SALT_LEN);
        }

        {
            let note_bytes = note.as_bytes();
            let len = note_bytes.len().min(PWM_NOTE_LEN - 1);
            let note_ptr = entry.note.as_ptr() as *mut u8;
            unsafe {
                core::ptr::copy_nonoverlapping(note_bytes.as_ptr(), note_ptr, len);
                *note_ptr.add(len) = 0;
            }
        }

        for i in 0..16 {
            entry.caps[i].store(VIABLE_FLOOR[i], Ordering::Release);
        }

        let uid = if creator_pwm == 0 {
            0
        } else {
            self.next_uid.fetch_add(1, Ordering::AcqRel)
        };
        entry.set_uid(uid);
        entry.set_gid(uid);

        entry.created_time.store(bootstrap::pwm_now(), Ordering::Release);
        entry.expires_at.store(0, Ordering::Release);
        entry.lockout_until.store(0, Ordering::Release);
        entry.failed_attempts.store(0, Ordering::Release);
        entry.last_login_time.store(0, Ordering::Release);

        self.count.fetch_add(1, Ordering::AcqRel);
        self.any_identity_exists.store(true, Ordering::Release);
        self.set_modified();
        self.release();

        audit::log(creator_pwm, AuditAction::Create, pwm, 0, privilege_level as u64);

        Ok(pwm)
    }

    pub fn grant(
        &self,
        grantor_pwm: u64,
        grantee_pwm: u64,
        domain: impl Into<CapDomain>,
        caps: impl Into<CapBits>,
    ) -> Result<(), PwmError> {
        let domain = domain.into();
        let caps = caps.into();

        let grantor = self.find(grantor_pwm).ok_or(PwmError::NotFound)?;
        let grantee = self.find(grantee_pwm).ok_or(PwmError::NotFound)?;

        let grantor_caps = grantor.load_caps(domain);
        if (grantor_caps & caps) != caps {
            return Err(PwmError::PermissionDenied);
        }

        let grantor_level = grantor.privilege_level.load(Ordering::Acquire);
        let grantee_level = grantee.privilege_level.load(Ordering::Acquire);
        if grantor_level > grantee_level {
            return Err(PwmError::InsufficientPrivilege);
        }

        if grantee.has_flag(PwmFlags::DISABLED) {
            return Err(PwmError::Disabled);
        }

        grantee.fetch_or_caps(domain, caps);

        grant::add_record(GrantRecord {
            grantor_pwm: PwmId(grantor_pwm),
            grantee_pwm: PwmId(grantee_pwm),
            domain,
            caps,
            granted_at: bootstrap::pwm_now(),
        })?;

        audit::log(grantor_pwm, AuditAction::Grant, grantee_pwm, domain.as_u16() as u64, caps.as_u64());

        Ok(())
    }

    pub fn revoke(
        &self,
        revoker_pwm: u64,
        target_pwm: u64,
        domain: impl Into<CapDomain>,
        caps: impl Into<CapBits>,
    ) -> Result<(), PwmError> {
        let domain = domain.into();
        let caps = caps.into();

        let revoker = self.find(revoker_pwm).ok_or(PwmError::NotFound)?;
        let target = self.find(target_pwm).ok_or(PwmError::NotFound)?;

        let revoker_level = revoker.privilege_level.load(Ordering::Acquire);
        let target_level = target.privilege_level.load(Ordering::Acquire);
        if revoker_level >= target_level {
            return Err(PwmError::InsufficientPrivilege);
        }

        let creator_pwm = target.creator_pwm.load(Ordering::Acquire);
        let is_creator = revoker_pwm == creator_pwm;

        let is_grantor = grant::is_grantor(revoker_pwm, target_pwm, domain, caps);

        if !is_creator && !is_grantor {
            return Err(PwmError::NotAuthorized);
        }

        let current = target.load_caps(domain);
        let after_revoke = current & !caps;
        if (after_revoke & CapBits(VIABLE_FLOOR[domain.as_usize()])) != CapBits(VIABLE_FLOOR[domain.as_usize()]) {
            return Err(PwmError::WouldBreakFloor);
        }

        target.fetch_and_caps(domain, !caps);

        grant::clear_records(revoker_pwm, target_pwm, domain, caps);

        self.set_modified();

        audit::log(revoker_pwm, AuditAction::Revoke, target_pwm, domain.as_u16() as u64, caps.as_u64());

        Ok(())
    }

    pub fn transfer_creator(
        &self,
        current_creator_pwm: u64,
        target_pwm: u64,
        new_creator_pwm: u64,
    ) -> Result<(), PwmError> {
        let current_creator = self.find(current_creator_pwm).ok_or(PwmError::NotFound)?;
        let target = self.find(target_pwm).ok_or(PwmError::NotFound)?;
        let new_creator = self.find(new_creator_pwm).ok_or(PwmError::NotFound)?;

        let creator = target.creator_pwm.load(Ordering::Acquire);
        if creator != current_creator_pwm {
            return Err(PwmError::NotCreator);
        }

        let current_level = current_creator.privilege_level.load(Ordering::Acquire);
        let target_level = target.privilege_level.load(Ordering::Acquire);
        if current_level >= target_level {
            return Err(PwmError::InsufficientPrivilege);
        }

        let new_level = new_creator.privilege_level.load(Ordering::Acquire);
        if new_level >= target_level {
            return Err(PwmError::InsufficientPrivilege);
        }

        target.creator_pwm.store(new_creator_pwm, Ordering::Release);

        self.set_modified();

        audit::log(current_creator_pwm, AuditAction::TransferCreator, target_pwm, 0, new_creator_pwm);

        Ok(())
    }

    pub fn bootstrap(&self, password: &str, note: &str) -> Result<u64, PwmError> {
        bootstrap::generate_first_token();

        let pwm = self.create(password, note, 0)?;

        for i in 0..16u16 {
            bootstrap::grant_from_first_token(pwm, CapDomain(i), CapBits::ALL)?;
        }

        Ok(pwm)
    }

    pub fn recover_with_first(&self, password: &str, note: &str) -> Result<u64, PwmError> {
        let pwm_entry = self.find_by_note(note).ok_or(PwmError::NotFound)?;
        let pwm = pwm_entry.pwm.load(Ordering::Acquire);

        if !self.verify_password(pwm, password) {
            return Err(PwmError::InvalidPassword);
        }

        bootstrap::generate_first_token();
        for i in 0..16 {
            bootstrap::grant_from_first_token(pwm, CapDomain(i), CapBits::ALL)?;
        }

        Ok(pwm)
    }

    pub fn delete(&self, pwm: u64) -> Result<(), PwmError> {
        self.acquire();
        for entry in self.entries.iter() {
            if entry.pwm.load(Ordering::Acquire) == pwm {
                entry.pwm.store(0, Ordering::Release);
                self.count.fetch_sub(1, Ordering::AcqRel);
                self.set_modified();
                self.release();
                return Ok(());
            }
        }
        self.release();
        Err(PwmError::NotFound)
    }

    pub fn disable(&self, pwm: u64) -> Result<(), PwmError> {
        let entry = self.find(pwm).ok_or(PwmError::NotFound)?;
        entry.add_flags(PwmFlags::DISABLED);
        self.set_modified();
        Ok(())
    }

    pub fn enable(&self, pwm: u64) -> Result<(), PwmError> {
        let entry = self.find(pwm).ok_or(PwmError::NotFound)?;
        entry.remove_flags(PwmFlags::DISABLED);
        self.set_modified();
        Ok(())
    }

    pub fn change_password(&self, pwm: u64, old: &str, new: &str) -> Result<(), PwmError> {
        if !self.verify_password(pwm, old) {
            return Err(PwmError::PasswordIncorrect);
        }
        let entry = self.find(pwm).ok_or(PwmError::NotFound)?;
        let salt = generate_salt();
        let digest = hash_with_salt(new, &salt);
        let hash_ptr = entry.password_hash.as_ptr() as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(digest.as_ptr(), hash_ptr, PWM_DIGEST_LEN);
            core::ptr::copy_nonoverlapping(salt.as_ptr(), hash_ptr.add(PWM_DIGEST_LEN), PWM_SALT_LEN);
        }
        self.set_modified();
        Ok(())
    }

    pub fn any_identity_exists(&self) -> bool {
        self.any_identity_exists.load(Ordering::Acquire)
    }

    /// uid → PwmEntry (POSIX chown/kill 等 syscall 用)
    pub fn find_by_uid(&self, uid: u32) -> Option<&PwmEntry> {
        if uid == 0xFFFF_FFFF { return None; }
        self.entries.iter().find(|e| e.is_valid() && e.get_uid() == uid)
    }

    pub fn find_by_gid(&self, gid: u32) -> Option<&PwmEntry> {
        if gid == 0xFFFF_FFFF { return None; }
        self.entries.iter().find(|e| e.is_valid() && e.get_gid() == gid)
    }

    /// pwm → uid (stat 填充 st_uid 用)
    pub fn uid_of(&self, pwm: u64) -> u32 {
        self.find(pwm).map_or(0xFFFF_FFFF, |e| e.get_uid())
    }

    /// pwm → gid (stat 填充 st_gid 用)
    pub fn gid_of(&self, pwm: u64) -> u32 {
        self.find(pwm).map_or(0xFFFF_FFFF, |e| e.get_gid())
    }
}

static mut GLOBAL_TABLE: IdentityTable = IdentityTable::new();

pub fn get_table() -> &'static IdentityTable {
    unsafe { &GLOBAL_TABLE }
}

pub unsafe fn get_table_mut() -> &'static mut IdentityTable {
    &mut GLOBAL_TABLE
}

pub fn find(pwm: u64) -> Option<&'static PwmEntry> {
    get_table().find(pwm)
}
