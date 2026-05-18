/// Syscall 类型定义和常量
/// 
/// 提供系统调用的错误码、参数结构和处理器类型定义

/// 系统调用中断号
pub const SYSCALL_INT: u8 = 0x80;

/// 最大系统调用数量
pub const MAX_SYSCALLS: u64 = 128;

// ==================== 进程管理 syscall ====================
pub const SYS_PROC_CREATE: u64 = 0;
pub const SYS_PROC_EXEC: u64 = 1;
pub const SYS_PROC_EXIT: u64 = 2;
pub const SYS_PROC_WAIT: u64 = 3;
pub const SYS_PROC_GETID: u64 = 4;
pub const SYS_PROC_GETPPID: u64 = 5;
pub const SYS_PROC_GETPWID: u64 = 6;
pub const SYS_PROC_SETPWID: u64 = 7;
pub const SYS_PROC_SETPRI: u64 = 8;
pub const SYS_PROC_YIELD: u64 = 9;
pub const SYS_PROC_SLEEP: u64 = 10;

// ==================== 文件系统 syscall ====================
pub const SYS_FS_OPEN: u64 = 20;
pub const SYS_FS_CLOSE: u64 = 21;
pub const SYS_FS_READ: u64 = 22;
pub const SYS_FS_WRITE: u64 = 23;
pub const SYS_FS_SEEK: u64 = 24;
pub const SYS_FS_STAT: u64 = 25;
pub const SYS_FS_FSTAT: u64 = 26;
pub const SYS_FS_CHMOD: u64 = 27;
pub const SYS_FS_CHOWN: u64 = 28;
pub const SYS_FS_UNLINK: u64 = 29;
pub const SYS_FS_RENAME: u64 = 30;
pub const SYS_FS_MKDIR: u64 = 31;
pub const SYS_FS_RMDIR: u64 = 32;
pub const SYS_FS_READDIR: u64 = 33;

// ==================== 认证/权限 syscall (PWID) ====================
pub const SYS_AUTH_LOGIN: u64 = 40;
pub const SYS_AUTH_LOGOUT: u64 = 41;
pub const SYS_AUTH_ELEVATE: u64 = 42;
pub const SYS_AUTH_CREATE: u64 = 43;
pub const SYS_AUTH_DELETE: u64 = 44;
pub const SYS_AUTH_LIST: u64 = 45;
pub const SYS_AUTH_INFO: u64 = 46;
pub const SYS_AUTH_SETNOTE: u64 = 47;
pub const SYS_AUTH_CHANGEPW: u64 = 48;
pub const SYS_AUTH_VERIFY: u64 = 49;
pub const SYS_AUTH_CREATE_FIRST: u64 = 50;
pub const SYS_AUTH_TOKEN_CREATE: u64 = 51;
pub const SYS_AUTH_TOKEN_USE: u64 = 52;
pub const SYS_AUTH_TOKEN_REVOKE: u64 = 53;
pub const SYS_AUTH_TRUST_ADD: u64 = 54;
pub const SYS_AUTH_TRUST_REMOVE: u64 = 55;
pub const SYS_AUTH_CHECK: u64 = 56;
pub const SYS_AUTH_CREATE_WITH_CAPS: u64 = 57;

// ==================== 内存管理 syscall ====================
pub const SYS_MEM_BRK: u64 = 60;
pub const SYS_MEM_MAP: u64 = 61;
pub const SYS_MEM_UNMAP: u64 = 62;
pub const SYS_MEM_PROTECT: u64 = 63;

// ==================== IPC syscall ====================
pub const SYS_IPC_PIPE: u64 = 80;

// ==================== 网络 syscall ====================
pub const SYS_NET_SOCKET: u64 = 81;
pub const SYS_NET_BIND: u64 = 82;
pub const SYS_NET_LISTEN: u64 = 83;
pub const SYS_NET_ACCEPT: u64 = 84;
pub const SYS_NET_CONNECT: u64 = 85;
pub const SYS_NET_SEND: u64 = 86;
pub const SYS_NET_RECV: u64 = 87;
pub const SYS_NET_SHUTDOWN: u64 = 88;

// ==================== 环境/系统信息 syscall ====================
pub const SYS_ENV_GETCWD: u64 = 100;
pub const SYS_ENV_CHDIR: u64 = 101;
pub const SYS_FS_SYNC: u64 = 102;
pub const SYS_REBOOT: u64 = 103;
pub const SYS_TIME: u64 = 104;
pub const SYS_INFO: u64 = 105;
pub const SYS_ENV_GETVAR: u64 = 106;
pub const SYS_ENV_SETVAR: u64 = 107;
pub const SYS_GETHOSTNAME: u64 = 108;
pub const SYS_SETHOSTNAME: u64 = 109;
pub const SYS_BOOT_CHECK: u64 = 110;
pub const SYS_FS_MOUNT: u64 = 111;
pub const SYS_FS_UNMOUNT: u64 = 112;
pub const SYS_DISK_LIST: u64 = 113;
pub const SYS_DISK_INFO: u64 = 114;
pub const SYS_DISK_FORMAT: u64 = 115;
pub const SYS_DISK_PARTITION: u64 = 116;
pub const SYS_DISK_INSTALL_GRUB: u64 = 117;
pub const SYS_BOOT_INSTALL: u64 = 117;
pub const SYS_FAT_FORMAT: u64 = 118;

// ==================== 设备 I/O syscall ====================
pub const SYS_DEV_IOCTL: u64 = 120;
pub const SYS_DEV_READ: u64 = 121;
pub const SYS_DEV_WRITE: u64 = 122;

// ==================== 错误码定义 ====================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum SyscallError {
    /// Operation not permitted
    E_PERM = -1,
    /// No such file or directory
    E_NOTFOUND = -2,
    /// Function not implemented
    E_NOSYS = -3,
    /// Interrupted system call
    E_INTR = -4,
    /// I/O error
    E_IO = -5,
    /// Exec format error
    E_NOEXEC = -8,
    /// Bad file descriptor
    E_BADFD = -9,
    /// No child processes
    E_CHILD = -10,
    /// Resource temporarily unavailable
    E_AGAIN = -11,
    /// Cannot allocate memory
    E_NOMEM = -12,
    /// Permission denied
    E_ACCES = -13,
    /// Bad address
    E_FAULT = -14,
    /// Device or resource busy
    E_BUSY = -16,
    /// File exists
    E_EXIST = -17,
    /// Not a directory
    E_NOTDIR = -20,
    /// Is a directory
    E_ISDIR = -21,
    /// Invalid argument
    E_INVAL = -22,
    /// No space left on device
    E_NOSPC = -28,
    /// Read-only file system
    E_ROFS = -30,
    /// Result too large
    E_RANGE = -34,
    /// File name too long
    E_NAMETOOLONG = -36,
    /// Directory not empty
    E_NOTEMPTY = -39,
    
    // ==================== 认证错误码 ====================
    /// Invalid authentication request
    E_AUTH_INVALID = -100,
    /// Identity not found
    E_AUTH_NOTFOUND = -101,
    /// Account disabled
    E_AUTH_DISABLED = -102,
    /// Authentication expired
    E_AUTH_EXPIRED = -103,
    /// Incorrect password
    E_AUTH_PWERR = -104,
    /// Insufficient capability
    E_AUTH_CAP = -105,
    /// Access denied
    E_AUTH_DENY = -106,
}

impl core::fmt::Display for SyscallError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::E_PERM => write!(f, "Operation not permitted"),
            Self::E_NOTFOUND => write!(f, "No such file or directory"),
            Self::E_NOSYS => write!(f, "Function not implemented"),
            Self::E_INTR => write!(f, "Interrupted system call"),
            Self::E_IO => write!(f, "I/O error"),
            Self::E_NOEXEC => write!(f, "Exec format error"),
            Self::E_BADFD => write!(f, "Bad file descriptor"),
            Self::E_CHILD => write!(f, "No child processes"),
            Self::E_AGAIN => write!(f, "Resource temporarily unavailable"),
            Self::E_NOMEM => write!(f, "Cannot allocate memory"),
            Self::E_ACCES => write!(f, "Permission denied"),
            Self::E_FAULT => write!(f, "Bad address"),
            Self::E_BUSY => write!(f, "Device or resource busy"),
            Self::E_EXIST => write!(f, "File exists"),
            Self::E_NOTDIR => write!(f, "Not a directory"),
            Self::E_ISDIR => write!(f, "Is a directory"),
            Self::E_INVAL => write!(f, "Invalid argument"),
            Self::E_NOSPC => write!(f, "No space left on device"),
            Self::E_ROFS => write!(f, "Read-only file system"),
            Self::E_RANGE => write!(f, "Result too large"),
            Self::E_NAMETOOLONG => write!(f, "File name too long"),
            Self::E_NOTEMPTY => write!(f, "Directory not empty"),
            
            Self::E_AUTH_INVALID => write!(f, "Invalid authentication request"),
            Self::E_AUTH_NOTFOUND => write!(f, "Identity not found"),
            Self::E_AUTH_DISABLED => write!(f, "Account disabled"),
            Self::E_AUTH_EXPIRED => write!(f, "Authentication expired"),
            Self::E_AUTH_PWERR => write!(f, "Incorrect password"),
            Self::E_AUTH_CAP => write!(f, "Insufficient capability"),
            Self::E_AUTH_DENY => write!(f, "Access denied"),
        }
    }
}

impl SyscallError {
    /// 转换为 i64 错误码
    pub fn as_i64(self) -> i64 {
        self as i64
    }
    
    /// 从 i64 错误码创建
    pub fn from_i64(code: i64) -> Option<Self> {
        match code {
            -1 => Some(Self::E_PERM),
            -2 => Some(Self::E_NOTFOUND),
            -3 => Some(Self::E_NOSYS),
            -4 => Some(Self::E_INTR),
            -5 => Some(Self::E_IO),
            -8 => Some(Self::E_NOEXEC),
            -9 => Some(Self::E_BADFD),
            -10 => Some(Self::E_CHILD),
            -11 => Some(Self::E_AGAIN),
            -12 => Some(Self::E_NOMEM),
            -13 => Some(Self::E_ACCES),
            -14 => Some(Self::E_FAULT),
            -16 => Some(Self::E_BUSY),
            -17 => Some(Self::E_EXIST),
            -20 => Some(Self::E_NOTDIR),
            -21 => Some(Self::E_ISDIR),
            -22 => Some(Self::E_INVAL),
            -28 => Some(Self::E_NOSPC),
            -30 => Some(Self::E_ROFS),
            -34 => Some(Self::E_RANGE),
            -36 => Some(Self::E_NAMETOOLONG),
            -39 => Some(Self::E_NOTEMPTY),
            
            -100 => Some(Self::E_AUTH_INVALID),
            -101 => Some(Self::E_AUTH_NOTFOUND),
            -102 => Some(Self::E_AUTH_DISABLED),
            -103 => Some(Self::E_AUTH_EXPIRED),
            -104 => Some(Self::E_AUTH_PWERR),
            -105 => Some(Self::E_AUTH_CAP),
            -106 => Some(Self::E_AUTH_DENY),
            
            _ => None,
        }
    }
}

/// 系统调用寄存器上下文 (用于保存/恢复)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallRegs {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

/// 系统调用处理器函数类型
pub type SyscallHandler = extern "C" fn(u64, u64, u64, u64) -> i64;

/// 系统调用结果类型 (用于安全的错误处理)
pub type SyscallResult<T> = Result<T, SyscallError>;
