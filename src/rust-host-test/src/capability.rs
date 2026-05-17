pub const SYS_CAP_ALL: u64 = 0xFFFFFFFFFFFFFFFF;

pub const CAP_DOMAIN_SYSTEM: u16    = 0;
pub const CAP_DOMAIN_FS: u16        = 1;
pub const CAP_DOMAIN_NET: u16       = 2;
pub const CAP_DOMAIN_PROC: u16      = 3;
pub const CAP_DOMAIN_DEVICE: u16    = 4;
pub const CAP_DOMAIN_USER_MGMT: u16 = 5;
pub const CAP_DOMAIN_IPC: u16       = 6;
pub const CAP_DOMAIN_MEM: u16       = 7;
pub const CAP_DOMAIN_TIME: u16      = 8;
pub const CAP_DOMAIN_BARRIER: u16   = 9;
pub const CAP_DOMAIN_SIGNAL: u16    = 10;
pub const CAP_DOMAIN_SHM: u16       = 11;
pub const CAP_DOMAIN_SEM: u16       = 12;
pub const CAP_DOMAIN_MSGQ: u16      = 13;
pub const CAP_DOMAIN_DMA: u16       = 14;
pub const CAP_DOMAIN_RESERVED: u16  = 15;

pub const FS_CAP_READ:    u64 = 1 << 0;
pub const FS_CAP_WRITE:   u64 = 1 << 1;
pub const FS_CAP_EXECUTE: u64 = 1 << 2;
pub const FS_CAP_CREATE:  u64 = 1 << 3;
pub const FS_CAP_DELETE:  u64 = 1 << 4;
pub const FS_CAP_CHOWN:   u64 = 1 << 5;
pub const FS_CAP_CHMOD:   u64 = 1 << 6;

pub const PROC_CAP_FORK:   u64 = 1 << 0;
pub const PROC_CAP_EXEC:   u64 = 1 << 1;
pub const PROC_CAP_KILL:   u64 = 1 << 2;
pub const PROC_CAP_WAIT:   u64 = 1 << 3;
pub const PROC_CAP_CREATE: u64 = 1 << 4;

pub const VIABLE_FLOOR: [u64; 16] = {
    let mut f = [0u64; 16];
    f[CAP_DOMAIN_FS as usize]   = FS_CAP_READ | FS_CAP_EXECUTE;
    f[CAP_DOMAIN_PROC as usize] = PROC_CAP_FORK | PROC_CAP_EXEC;
    f
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapBits(pub u64);

impl CapBits {
    pub fn has(&self, bit: u64) -> bool {
        (self.0 & bit) != 0
    }

    pub fn grant(&mut self, bits: u64) {
        self.0 |= bits;
    }

    pub fn revoke(&mut self, bits: u64) {
        self.0 &= !bits;
    }

    pub fn is_superset_of(&self, other: &CapBits) -> bool {
        (self.0 & other.0) == other.0
    }
}

#[derive(Clone, Debug)]
pub struct CapabilityMatrix {
    caps: [u64; 16],
}

impl CapabilityMatrix {
    pub fn new() -> Self {
        Self { caps: [0; 16] }
    }

    pub fn from_array(caps: [u64; 16]) -> Self {
        Self { caps }
    }

    pub fn all() -> Self {
        Self { caps: [SYS_CAP_ALL; 16] }
    }

    pub fn viable() -> Self {
        Self { caps: VIABLE_FLOOR }
    }

    pub fn has(&self, domain: u16, bit: u64) -> bool {
        if domain as usize >= 16 { return false; }
        (self.caps[domain as usize] & bit) != 0
    }

    pub fn grant(&mut self, domain: u16, bits: u64) {
        if domain as usize >= 16 { return; }
        self.caps[domain as usize] |= bits;
    }

    pub fn revoke(&mut self, domain: u16, bits: u64) {
        if domain as usize >= 16 { return; }
        self.caps[domain as usize] &= !bits;
    }

    pub fn is_superset_of(&self, other: &CapabilityMatrix) -> bool {
        for i in 0..16 {
            if (self.caps[i] & other.caps[i]) != other.caps[i] {
                return false;
            }
        }
        true
    }

    pub fn get_domain(&self, domain: u16) -> CapBits {
        if domain as usize >= 16 { return CapBits(0); }
        CapBits(self.caps[domain as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_bits_has() {
        let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
        assert!(cb.has(FS_CAP_READ));
        assert!(cb.has(FS_CAP_WRITE));
        assert!(!cb.has(FS_CAP_EXECUTE));
    }

    #[test]
    fn cap_bits_grant() {
        let mut cb = CapBits(FS_CAP_READ);
        cb.grant(FS_CAP_WRITE);
        assert!(cb.has(FS_CAP_READ));
        assert!(cb.has(FS_CAP_WRITE));
    }

    #[test]
    fn cap_bits_revoke() {
        let mut cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
        cb.revoke(FS_CAP_READ);
        assert!(!cb.has(FS_CAP_READ));
        assert!(cb.has(FS_CAP_WRITE));
    }

    #[test]
    fn cap_matrix_new_empty() {
        let cm = CapabilityMatrix::new();
        assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
        assert!(!cm.has(CAP_DOMAIN_PROC, PROC_CAP_FORK));
    }

    #[test]
    fn cap_matrix_grant_revoke() {
        let mut cm = CapabilityMatrix::new();
        cm.grant(CAP_DOMAIN_FS, FS_CAP_READ | FS_CAP_WRITE);
        assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
        assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_WRITE));
        cm.revoke(CAP_DOMAIN_FS, FS_CAP_READ);
        assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
        assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_WRITE));
    }

    #[test]
    fn cap_matrix_all() {
        let cm = CapabilityMatrix::all();
        for d in 0..16u16 {
            assert!(cm.has(d, SYS_CAP_ALL));
        }
    }

    #[test]
    fn cap_matrix_viable() {
        let cm = CapabilityMatrix::viable();
        assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
        assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_EXECUTE));
        assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_WRITE));
        assert!(cm.has(CAP_DOMAIN_PROC, PROC_CAP_FORK));
        assert!(cm.has(CAP_DOMAIN_PROC, PROC_CAP_EXEC));
    }

    #[test]
    fn cap_matrix_superset() {
        let mut parent = CapabilityMatrix::all();
        let child = CapabilityMatrix::viable();
        assert!(parent.is_superset_of(&child));
        assert!(!child.is_superset_of(&parent));
    }

    #[test]
    fn cap_matrix_out_of_range() {
        let cm = CapabilityMatrix::new();
        assert!(!cm.has(16, 1));
        assert!(!cm.has(255, 1));
    }

    #[test]
    fn cap_bits_superset() {
        let full = CapBits(FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE);
        let partial = CapBits(FS_CAP_READ);
        assert!(full.is_superset_of(&partial));
        assert!(!partial.is_superset_of(&full));
    }
}
