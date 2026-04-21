pub type CapDomain = u16;
pub type CapBits = u64;

pub const CAP_DOMAIN_SYSTEM: CapDomain = 0x0000;
pub const CAP_DOMAIN_FS: CapDomain = 0x0001;
pub const CAP_DOMAIN_NET: CapDomain = 0x0002;
pub const CAP_DOMAIN_PROC: CapDomain = 0x0003;
pub const CAP_DOMAIN_DEVICE: CapDomain = 0x0004;
pub const CAP_DOMAIN_USER_MGMT: CapDomain = 0x0005;
pub const CAP_DOMAIN_CUSTOM_START: CapDomain = 0x0100;

pub const FS_CAP_READ: CapBits = 1 << 0;
pub const FS_CAP_WRITE: CapBits = 1 << 1;
pub const FS_CAP_EXECUTE: CapBits = 1 << 2;
pub const FS_CAP_CREATE: CapBits = 1 << 3;
pub const FS_CAP_DELETE: CapBits = 1 << 4;
pub const FS_CAP_CHMOD: CapBits = 1 << 5;
pub const FS_CAP_CHOWN: CapBits = 1 << 6;
pub const FS_CAP_MOUNT: CapBits = 1 << 7;
pub const FS_CAP_LINK: CapBits = 1 << 8;

pub const PROC_CAP_FORK: CapBits = 1 << 0;
pub const PROC_CAP_EXEC: CapBits = 1 << 1;
pub const PROC_CAP_KILL: CapBits = 1 << 2;
pub const PROC_CAP_DEBUG: CapBits = 1 << 3;
pub const PROC_CAP_NICE: CapBits = 1 << 4;
pub const PROC_CAP_SCHED: CapBits = 1 << 5;

pub const SYS_CAP_ALL: CapBits = !0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PwidLevel {
    Root = 0,
    Trustworthy = 1,
    Untrustworthy = 2,
}

impl PwidLevel {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => PwidLevel::Root,
            1 => PwidLevel::Trustworthy,
            _ => PwidLevel::Untrustworthy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum TrustLevel {
    None = 0,
    Basic = 1,
    Operate = 2,
    Delegate = 3,
    Full = 4,
}

impl TrustLevel {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => TrustLevel::None,
            1 => TrustLevel::Basic,
            2 => TrustLevel::Operate,
            3 => TrustLevel::Delegate,
            _ => TrustLevel::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AuditLevel {
    None = 0,
    Critical = 1,
    Important = 2,
    All = 3,
    Full = 4,
}

impl AuditLevel {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => AuditLevel::None,
            1 => AuditLevel::Critical,
            2 => AuditLevel::Important,
            3 => AuditLevel::All,
            _ => AuditLevel::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum TimeOfDay {
    Any = 0,
    WorkHours = 1,
    OffHours = 2,
    Maintenance = 3,
    Emergency = 4,
}

impl TimeOfDay {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => TimeOfDay::Any,
            1 => TimeOfDay::WorkHours,
            2 => TimeOfDay::OffHours,
            3 => TimeOfDay::Maintenance,
            _ => TimeOfDay::Emergency,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionType {
    Local = 0,
    SSH = 1,
    Serial = 2,
    GUI = 3,
    API = 4,
}

impl SessionType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => SessionType::Local,
            1 => SessionType::SSH,
            2 => SessionType::Serial,
            3 => SessionType::GUI,
            _ => SessionType::API,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LoginMethod {
    Password = 0,
    Token = 1,
    Key = 2,
    Biometric = 3,
    Elevated = 4,
}

impl LoginMethod {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => LoginMethod::Password,
            1 => LoginMethod::Token,
            2 => LoginMethod::Key,
            3 => LoginMethod::Biometric,
            _ => LoginMethod::Elevated,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TokenType {
    Elevation = 0,
    Delegation = 1,
    Session = 2,
    OneTime = 3,
    Scoped = 4,
}

impl TokenType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => TokenType::Elevation,
            1 => TokenType::Delegation,
            2 => TokenType::Session,
            3 => TokenType::OneTime,
            _ => TokenType::Scoped,
        }
    }
}

pub const PWID_FLAG_DISABLED: u8 = 0x01;
pub const PWID_FLAG_LOCKED: u8 = 0x02;
pub const PWID_FLAG_EXPIRED: u8 = 0x04;
pub const PWID_FLAG_2FA_REQUIRED: u8 = 0x08;

pub const TRUST_COND_TIME_LIMITED: u32 = 0x01;
pub const TRUST_COND_IP_RESTRICTED: u32 = 0x02;
pub const TRUST_COND_SINGLE_USE: u32 = 0x04;
pub const TRUST_COND_REQUIRES_2FA: u32 = 0x08;

pub const TOKEN_FLAG_SINGLE_COMMAND: u32 = 0x01;
pub const TOKEN_FLAG_NO_TTY: u32 = 0x02;
pub const TOKEN_FLAG_REQUIRE_CONFIRM: u32 = 0x04;
pub const TOKEN_FLAG_AUDIT_ALL: u32 = 0x08;
pub const TOKEN_FLAG_EXPIRE_ON_IDLE: u32 = 0x10;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct InheritFlags: u8 {
        const INHERIT_PERMS = 0x01;
        const INHERIT_TRUST_CHAIN = 0x02;
        const INHERIT_CONTEXT_POLICY = 0x04;
        const INHERIT_ACL = 0x08;
        const NONE = 0x00;
        const ALL = 0x0F;
    }
}
