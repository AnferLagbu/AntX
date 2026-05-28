#![allow(dead_code)] // 测试基础设施模块 (CapabilityMatrix 由 #[cfg(test)] 测试使用)

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
        let parent = CapabilityMatrix::all();
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

    #[test]
    fn cap_bits_empty_has_nothing() {
        let cb = CapBits(0);
        assert!(!cb.has(FS_CAP_READ));
        assert!(!cb.has(FS_CAP_WRITE));
        assert!(!cb.has(SYS_CAP_ALL));
    }

    #[test]
    fn cap_bits_grant_all_then_revoke_one() {
        let mut cb = CapBits(0);
        cb.grant(SYS_CAP_ALL);
        assert!(cb.has(FS_CAP_READ));
        assert!(cb.has(SYS_CAP_ALL));
        cb.revoke(FS_CAP_READ);
        assert!(!cb.has(FS_CAP_READ));
        assert!(cb.has(FS_CAP_WRITE));
    }

    #[test]
    fn cap_bits_revoke_nonexistent_is_noop() {
        let mut cb = CapBits(FS_CAP_READ);
        cb.revoke(FS_CAP_WRITE);
        assert!(cb.has(FS_CAP_READ));
        assert!(!cb.has(FS_CAP_WRITE));
    }

    #[test]
    fn cap_bits_grant_idempotent() {
        let mut cb = CapBits(FS_CAP_READ);
        cb.grant(FS_CAP_READ);
        assert!(cb.has(FS_CAP_READ));
        assert_eq!(cb.0, FS_CAP_READ);
    }

    #[test]
    fn cap_matrix_delegation_chain() {
        let root = CapabilityMatrix::all();
        let mut admin = CapabilityMatrix::new();
        admin.grant(CAP_DOMAIN_FS, FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE | FS_CAP_CREATE | FS_CAP_DELETE);
        admin.grant(CAP_DOMAIN_PROC, PROC_CAP_FORK | PROC_CAP_EXEC | PROC_CAP_KILL);
        let mut user = CapabilityMatrix::new();
        user.grant(CAP_DOMAIN_FS, FS_CAP_READ | FS_CAP_EXECUTE);
        user.grant(CAP_DOMAIN_PROC, PROC_CAP_FORK | PROC_CAP_EXEC);
        assert!(root.is_superset_of(&admin));
        assert!(admin.is_superset_of(&user));
        assert!(!user.is_superset_of(&admin));
    }

    #[test]
    fn cap_matrix_revocation_partial() {
        let mut cm = CapabilityMatrix::all();
        cm.revoke(CAP_DOMAIN_FS, FS_CAP_DELETE | FS_CAP_CHOWN);
        assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
        assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_WRITE));
        assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_DELETE));
        assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_CHOWN));
    }

    #[test]
    fn cap_matrix_viable_is_not_all() {
        let viable = CapabilityMatrix::viable();
        let all = CapabilityMatrix::all();
        assert!(all.is_superset_of(&viable));
        assert!(!viable.is_superset_of(&all));
    }

    #[test]
    fn cap_matrix_grant_out_of_range_silent() {
        let mut cm = CapabilityMatrix::new();
        cm.grant(16, 0xFF);
        cm.grant(255, 0xFF);
        assert!(!cm.has(16, 0xFF));
        assert!(!cm.has(255, 0xFF));
    }

    #[test]
    fn cap_matrix_revoke_out_of_range_silent() {
        let mut cm = CapabilityMatrix::all();
        cm.revoke(16, SYS_CAP_ALL);
        cm.revoke(255, SYS_CAP_ALL);
        for d in 0..16u16 {
            assert!(cm.has(d, SYS_CAP_ALL));
        }
    }

    #[test]
    fn cap_matrix_cross_domain_isolation() {
        let mut cm = CapabilityMatrix::new();
        cm.grant(CAP_DOMAIN_FS, FS_CAP_READ);
        assert!(!cm.has(CAP_DOMAIN_PROC, FS_CAP_READ));
        assert!(!cm.has(CAP_DOMAIN_NET, FS_CAP_READ));
    }

    #[test]
    fn cap_matrix_empty_not_superset_of_viable() {
        let empty = CapabilityMatrix::new();
        let viable = CapabilityMatrix::viable();
        assert!(!empty.is_superset_of(&viable));
    }

    #[test]
    fn cap_bits_superset_reflexive() {
        let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
        assert!(cb.is_superset_of(&cb));
    }

    #[test]
    fn cap_matrix_superset_reflexive() {
        let cm = CapabilityMatrix::viable();
        assert!(cm.is_superset_of(&cm));
    }
}
