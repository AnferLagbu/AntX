use super::types::*;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TrustEntry {
    pub subject_pwid: u64,
    pub object_pwid: u64,
    pub trust_level: TrustLevel,
    pub cap_domain: CapDomain,
    pub cap_mask: CapBits,
    pub expires_at: u64,
    pub conditions: u32,
    pub created_time: u64,
}

impl Default for TrustEntry {
    fn default() -> Self {
        Self {
            subject_pwid: 0,
            object_pwid: 0,
            trust_level: TrustLevel::None,
            cap_domain: 0,
            cap_mask: 0,
            expires_at: 0,
            conditions: 0,
            created_time: 0,
        }
    }
}

impl TrustEntry {
    pub fn new(
        subject: u64,
        object: u64,
        level: TrustLevel,
        domain: CapDomain,
        mask: CapBits,
        expires: u64,
        conds: u32,
    ) -> Self {
        let time = get_time();
        Self {
            subject_pwid: subject,
            object_pwid: object,
            trust_level: level,
            cap_domain: domain,
            cap_mask: mask,
            expires_at: expires,
            conditions: conds,
            created_time: time,
        }
    }

    pub fn is_valid(&self) -> bool {
        if self.trust_level == TrustLevel::None {
            return false;
        }
        
        if self.expires_at > 0 && get_time() > self.expires_at {
            return false;
        }

        true
    }

    pub fn has_condition(&self, cond: u32) -> bool {
        (self.conditions & cond) != 0
    }
}

pub const MAX_TRUST_ENTRIES: usize = 256;

#[derive(Debug, Clone)]
pub struct TrustChain {
    entries: [TrustEntry; MAX_TRUST_ENTRIES],
    count: usize,
}

unsafe impl Send for TrustChain {}
unsafe impl Sync for TrustChain {}

impl Default for TrustChain {
    fn default() -> Self {
        Self {
            entries: [TrustEntry::default(); MAX_TRUST_ENTRIES],
            count: 0,
        }
    }
}

impl TrustChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, entry: TrustEntry) -> Result<(), ()> {
        if self.count >= MAX_TRUST_ENTRIES {
            return Err(());
        }

        for i in 0..self.count {
            if self.entries[i].subject_pwid == entry.subject_pwid 
                && self.entries[i].object_pwid == entry.object_pwid
                && self.entries[i].cap_domain == entry.cap_domain {
                self.entries[i] = entry;
                return Ok(());
            }
        }

        self.entries[self.count] = entry;
        self.count += 1;
        Ok(())
    }

    pub fn remove(&mut self, subject: u64, object: u64, domain: CapDomain) -> bool {
        for i in (0..self.count).rev() {
            if self.entries[i].subject_pwid == subject
                && self.entries[i].object_pwid == object
                && self.entries[i].cap_domain == domain {
                
                for j in i..self.count - 1 {
                    self.entries[j] = self.entries[j + 1];
                }
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn find_trust(
        &self,
        subject: u64,
        object: u64,
        domain: CapDomain,
    ) -> Option<&TrustEntry> {
        for i in 0..self.count {
            let e = &self.entries[i];
            if e.subject_pwid == subject
                && e.object_pwid == object
                && e.cap_domain == domain
                && e.is_valid() {
                return Some(e);
            }
        }
        None
    }

    pub fn find_by_subject(&self, subject: u64) -> alloc::vec::Vec<&TrustEntry> {
        let mut result = alloc::vec::Vec::new();
        for i in 0..self.count {
            if self.entries[i].subject_pwid == subject && self.entries[i].is_valid() {
                result.push(&self.entries[i]);
            }
        }
        result
    }

    pub fn find_by_object(&self, object: u64) -> alloc::vec::Vec<&TrustEntry> {
        let mut result = alloc::vec::Vec::new();
        for i in 0..self.count {
            if self.entries[i].object_pwid == object && self.entries[i].is_valid() {
                result.push(&self.entries[i]);
            }
        }
        result
    }

    pub fn check_chain(
        &self,
        subject: u64,
        target: u64,
        domain: CapDomain,
        required_caps: CapBits,
        max_depth: u8,
    ) -> Option<TrustLevel> {
        if max_depth == 0 {
            return None;
        }

        if let Some(trust) = self.find_trust(subject, target, domain) {
            if (trust.cap_mask & required_caps) == required_caps {
                return Some(trust.trust_level);
            }
        }

        for trust in self.find_by_subject(subject) {
            if !trust.is_valid() { continue; }
            
            if let Some(level) = self.check_chain(
                trust.object_pwid,
                target,
                domain,
                required_caps & trust.cap_mask,
                max_depth - 1,
            ) {
                if level <= trust.trust_level {
                    return Some(trust.trust_level);
                }
            }
        }

        None
    }

    pub fn clear_expired(&mut self) {
        let mut write_idx = 0;
        for read_idx in 0..self.count {
            if self.entries[read_idx].is_valid() || read_idx == write_idx {
                if read_idx != write_idx {
                    self.entries[write_idx] = self.entries[read_idx];
                }
                write_idx += 1;
            }
        }
        self.count = write_idx;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

fn get_time() -> u64 {
    let tsc: u64;
    unsafe {
        core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack));
    }
    tsc
}
