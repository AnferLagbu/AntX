use super::types::*;

// ============================================================
// 能力常量定义 (v4 规范)
// ============================================================

/// 全能力掩码 (所有位为 1)
pub const SYS_CAP_ALL: u64 = 0xFFFFFFFFFFFFFFFF;

/// 领域索引常量
pub const CAP_DOMAIN_SYSTEM: u16 = 0;
pub const CAP_DOMAIN_FS: u16     = 1;
pub const CAP_DOMAIN_NET: u16    = 2;
pub const CAP_DOMAIN_PROC: u16   = 3;
pub const CAP_DOMAIN_DEVICE: u16 = 4;
pub const CAP_DOMAIN_USER_MGMT: u16 = 5;

/// 文件系统领域操作权限
pub const FS_CAP_READ:    u64 = 0x0000000000000001;  // 位 0: 读
pub const FS_CAP_WRITE:   u64 = 0x0000000000000002;  // 位 1: 写
pub const FS_CAP_EXECUTE: u64 = 0x0000000000000004;  // 位 2: 执行
pub const FS_CAP_CREATE:  u64 = 0x0000000000000008;  // 位 3: 创建
pub const FS_CAP_DELETE:  u64 = 0x0000000000000010;  // 位 4: 删除

/// 进程领域操作权限
pub const PROC_CAP_FORK:  u64 = 0x0000000000000001;  // 位 0: fork
pub const PROC_CAP_EXEC:  u64 = 0x0000000000000002;  // 位 1: exec
pub const PROC_CAP_KILL:  u64 = 0x0000000000000004;  // 位 2: kill
pub const PROC_CAP_DEBUG: u64 = 0x0000000000000008;  // 位 3: debug

/// 用户管理领域操作权限
pub const CAP_DOMAIN_USER_LIST: u64 = 0x0000000000000001;  // 列出用户

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CapabilityMatrix {
    pub domains: [CapBits; 16],
}

impl Default for CapabilityMatrix {
    fn default() -> Self {
        Self { domains: [0; 16] }
    }
}

impl CapabilityMatrix {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root_capabilities() -> Self {
        let mut matrix = Self::default();
        for domain in matrix.domains.iter_mut() {
            *domain = SYS_CAP_ALL;
        }
        matrix
    }

    pub fn trustworthy_default() -> Self {
        let mut matrix = Self::default();
        matrix.domains[CAP_DOMAIN_FS as usize] = FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE | FS_CAP_CREATE;
        matrix.domains[CAP_DOMAIN_PROC as usize] = PROC_CAP_FORK | PROC_CAP_EXEC;
        matrix
    }

    pub fn untrustworthy_default() -> Self {
        let mut matrix = Self::default();
        matrix.domains[CAP_DOMAIN_FS as usize] = FS_CAP_READ | FS_CAP_EXECUTE;
        matrix
    }

    pub fn has_capability(&self, domain: CapDomain, cap: CapBits) -> bool {
        let idx = (domain as usize) % 16;
        (self.domains[idx] & cap) == cap
    }

    pub fn grant(&mut self, domain: CapDomain, caps: CapBits) {
        let idx = (domain as usize) % 16;
        self.domains[idx] |= caps;
    }

    pub fn revoke(&mut self, domain: CapDomain, caps: CapBits) {
        let idx = (domain as usize) % 16;
        self.domains[idx] &= !caps;
    }

    pub fn get_domain_caps(&self, domain: CapDomain) -> CapBits {
        let idx = (domain as usize) % 16;
        self.domains[idx]
    }

    pub fn set_domain_caps(&mut self, domain: CapDomain, caps: CapBits) {
        let idx = (domain as usize) % 16;
        self.domains[idx] = caps;
    }

    pub fn is_empty(&self) -> bool {
        self.domains.iter().all(|&d| d == 0)
    }

    pub fn intersection(&self, other: &Self) -> Self {
        let mut result = Self::default();
        for i in 0..16 {
            result.domains[i] = self.domains[i] & other.domains[i];
        }
        result
    }

    pub fn union(&self, other: &Self) -> Self {
        let mut result = Self::default();
        for i in 0..16 {
            result.domains[i] = self.domains[i] | other.domains[i];
        }
        result
    }

    pub fn to_string(&self) -> alloc::string::String {
        use alloc::format;
        let mut s = alloc::string::String::from("[");
        for i in 0..16 {
            if self.domains[i] != 0 {
                if s.len() > 1 { s.push_str(", "); }
                s.push_str(&format!("D{}:{:016X}", i, self.domains[i]));
            }
        }
        s.push(']');
        s
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DomainPermission {
    pub domain: CapDomain,
    pub cap_bits: CapBits,
    pub is_set: bool,
}

impl Default for DomainPermission {
    fn default() -> Self {
        Self {
            domain: 0,
            cap_bits: 0,
            is_set: false,
        }
    }
}

impl DomainPermission {
    pub fn new(domain: CapDomain, caps: CapBits) -> Self {
        Self {
            domain,
            cap_bits: caps,
            is_set: true,
        }
    }

    pub fn check(&self, required: CapBits) -> bool {
        if !self.is_set {
            return false;
        }
        (self.cap_bits & required) == required
    }
}
