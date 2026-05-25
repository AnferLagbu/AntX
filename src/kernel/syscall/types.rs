/// Syscall 类型定义和常量 — POSIX 原生接口
///
/// syscall 编号采用 POSIX 标准约定，QueenX 私有 syscall 分配在 400+。

pub const SYSCALL_INT: u8 = 0x80;
pub const MAX_SYSCALLS: u64 = 512;

// ==================== POSIX 标准 syscall 编号 ====================

// 文件 I/O
pub const SYS_read: u64 = 0;
pub const SYS_write: u64 = 1;
pub const SYS_open: u64 = 2;
pub const SYS_close: u64 = 3;
pub const SYS_stat: u64 = 4;
pub const SYS_fstat: u64 = 5;
pub const SYS_lstat: u64 = 6;
pub const SYS_poll: u64 = 7;
pub const SYS_lseek: u64 = 8;

// 内存管理
pub const SYS_mmap: u64 = 9;
pub const SYS_mprotect: u64 = 10;
pub const SYS_munmap: u64 = 11;
pub const SYS_brk: u64 = 12;

// 信号 (基础存根)
pub const SYS_rt_sigaction: u64 = 13;
pub const SYS_rt_sigprocmask: u64 = 14;
pub const SYS_rt_sigreturn: u64 = 15;

// 设备 I/O
pub const SYS_ioctl: u64 = 16;

// 文件系统预读
pub const SYS_readahead: u64 = 18;

// 文件访问
pub const SYS_access: u64 = 21;
pub const SYS_pipe: u64 = 22;
pub const SYS_select: u64 = 23;
pub const SYS_sched_yield: u64 = 24;

// 内存重映射
pub const SYS_mremap: u64 = 25;

// 文件描述符
pub const SYS_dup: u64 = 32;
pub const SYS_dup2: u64 = 33;

// 暂停
pub const SYS_nanosleep: u64 = 35;

// ITIMER
pub const SYS_getitimer: u64 = 36;
pub const SYS_alarm: u64 = 37;
pub const SYS_setitimer: u64 = 38;

// 进程基础
pub const SYS_getpid: u64 = 39;

// 网络 socket
pub const SYS_socket: u64 = 41;
pub const SYS_connect: u64 = 42;
pub const SYS_accept: u64 = 43;
pub const SYS_sendto: u64 = 44;
pub const SYS_recvfrom: u64 = 45;
pub const SYS_sendmsg: u64 = 46;
pub const SYS_recvmsg: u64 = 47;
pub const SYS_shutdown: u64 = 48;
pub const SYS_bind: u64 = 49;
pub const SYS_listen: u64 = 50;
pub const SYS_getsockname: u64 = 51;
pub const SYS_getpeername: u64 = 52;
pub const SYS_socketpair: u64 = 53;
pub const SYS_setsockopt: u64 = 54;
pub const SYS_getsockopt: u64 = 55;

// 进程
pub const SYS_clone: u64 = 56;
pub const SYS_fork: u64 = 57;
pub const SYS_vfork: u64 = 58;
pub const SYS_execve: u64 = 59;
pub const SYS_exit: u64 = 60;
pub const SYS_wait4: u64 = 61;
pub const SYS_kill: u64 = 62;

// 系统信息
pub const SYS_uname: u64 = 63;

// IPC
pub const SYS_semget: u64 = 64;
pub const SYS_semop: u64 = 65;

// 文件描述符操作
pub const SYS_fcntl: u64 = 72;

// 文件截断
pub const SYS_truncate: u64 = 76;
pub const SYS_ftruncate: u64 = 77;

// 目录
pub const SYS_getdents: u64 = 78;

// 路径
pub const SYS_getcwd: u64 = 79;
pub const SYS_chdir: u64 = 80;

// 文件重命名
pub const SYS_rename: u64 = 82;

// 目录操作
pub const SYS_mkdir: u64 = 83;
pub const SYS_rmdir: u64 = 84;

// 文件创建
pub const SYS_creat: u64 = 85;

// 文件链接
pub const SYS_link: u64 = 86;
pub const SYS_unlink: u64 = 87;
pub const SYS_symlink: u64 = 88;
pub const SYS_readlink: u64 = 89;

// 文件权限
pub const SYS_chmod: u64 = 90;
pub const SYS_fchmod: u64 = 91;
pub const SYS_chown: u64 = 92;
pub const SYS_fchown: u64 = 93;

// 文件属性
pub const SYS_umask: u64 = 95;

// 时间
pub const SYS_gettimeofday: u64 = 96;
pub const SYS_getrlimit: u64 = 97;
pub const SYS_getrusage: u64 = 98;
pub const SYS_sysinfo: u64 = 99;

// 系统
pub const SYS_times: u64 = 100;

// 用户/组
pub const SYS_getuid: u64 = 102;
pub const SYS_getgid: u64 = 104;
pub const SYS_setuid: u64 = 105;
pub const SYS_setgid: u64 = 106;
pub const SYS_geteuid: u64 = 107;
pub const SYS_getegid: u64 = 108;

// 进程组
pub const SYS_getppid: u64 = 110;
pub const SYS_getpgid: u64 = 111;
pub const SYS_setsid: u64 = 112;

// 文件同步
pub const SYS_sync: u64 = 162;
pub const SYS_fsync: u64 = 170;

// 挂载
pub const SYS_mount: u64 = 165;
pub const SYS_umount2: u64 = 166;

// 其他 POSIX
pub const SYS_gettid: u64 = 186;
pub const SYS_time: u64 = 201;
pub const SYS_clock_gettime: u64 = 228;
pub const SYS_exit_group: u64 = 231;
pub const SYS_tgkill: u64 = 234;

// ==================== QueenX 私有 syscall (400+ 不与 POSIX 冲突) ====================

pub const SYS_QX_LOGIN: u64 = 400;
pub const SYS_QX_LOGOUT: u64 = 401;
pub const SYS_QX_CREATE_IDENTITY: u64 = 402;
pub const SYS_QX_DELETE_IDENTITY: u64 = 403;
pub const SYS_QX_IDENTITY_INFO: u64 = 404;
pub const SYS_QX_CHANGE_PASSWORD: u64 = 405;
pub const SYS_QX_VERIFY_PASSWORD: u64 = 406;
pub const SYS_QX_CREATE_FIRST: u64 = 407;
pub const SYS_QX_TOKEN_CREATE: u64 = 408;
pub const SYS_QX_TOKEN_USE: u64 = 409;
pub const SYS_QX_TOKEN_REVOKE: u64 = 410;
pub const SYS_QX_GRANT: u64 = 411;
pub const SYS_QX_REVOKE: u64 = 412;
pub const SYS_QX_CHECK_CAP: u64 = 413;
pub const SYS_QX_GET_CAPS: u64 = 414;
pub const SYS_QX_GET_PWM: u64 = 415;
pub const SYS_QX_SET_PWM: u64 = 416;
pub const SYS_QX_DISK_LIST: u64 = 420;
pub const SYS_QX_DISK_INFO: u64 = 421;
pub const SYS_QX_DISK_FORMAT: u64 = 422;
pub const SYS_QX_DISK_PARTITION: u64 = 423;
pub const SYS_QX_DISK_INSTALL: u64 = 424;
pub const SYS_QX_FAT_FORMAT: u64 = 425;
pub const SYS_QX_PROC_LIST: u64 = 430;
pub const SYS_QX_PROC_SETPRI: u64 = 431;
pub const SYS_QX_PROC_SLEEP: u64 = 432;
pub const SYS_QX_GETHOSTNAME: u64 = 433;
pub const SYS_QX_SETHOSTNAME: u64 = 434;
pub const SYS_QX_BOOT_CHECK: u64 = 435;
pub const SYS_QX_REBOOT: u64 = 436;
pub const SYS_QX_HOTPLUG_STATUS: u64 = 437;

// ==================== POSIX errno (使用 Linux 风格: 返回值 = -errno) ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Errno {
    EPERM = 1,
    ENOENT = 2,
    ESRCH = 3,
    EINTR = 4,
    EIO = 5,
    ENXIO = 6,
    E2BIG = 7,
    ENOEXEC = 8,
    EBADF = 9,
    ECHILD = 10,
    EAGAIN = 11,
    ENOMEM = 12,
    EACCES = 13,
    EFAULT = 14,
    ENOTBLK = 15,
    EBUSY = 16,
    EEXIST = 17,
    EXDEV = 18,
    ENODEV = 19,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    ENFILE = 23,
    EMFILE = 24,
    ENOTTY = 25,
    ETXTBSY = 26,
    EFBIG = 27,
    ENOSPC = 28,
    ESPIPE = 29,
    EROFS = 30,
    EMLINK = 31,
    EPIPE = 32,
    EDOM = 33,
    ERANGE = 34,
    EDEADLK = 35,
    ENAMETOOLONG = 36,
    ENOLCK = 37,
    ENOSYS = 38,
    ENOTEMPTY = 39,
    ELOOP = 40,
    EWOULDBLOCK = 41,
    ENOMSG = 42,
    EIDRM = 43,
    ENOSTR = 60,
    ENODATA = 61,
    ETIME = 62,
    ENOSR = 63,
    ENONET = 64,
    EPROTO = 71,
    EBADMSG = 74,
    EOVERFLOW = 75,
    ENOTSOCK = 88,
    EDESTADDRREQ = 89,
    EMSGSIZE = 90,
    EPROTOTYPE = 91,
    ENOPROTOOPT = 92,
    EPROTONOSUPPORT = 93,
    ESOCKTNOSUPPORT = 94,
    ENOTSUP = 95,
    EPFNOSUPPORT = 96,
    EAFNOSUPPORT = 97,
    EADDRINUSE = 98,
    EADDRNOTAVAIL = 99,
    ENETDOWN = 100,
    ENETUNREACH = 101,
    ENETRESET = 102,
    ECONNABORTED = 103,
    ECONNRESET = 104,
    ENOBUFS = 105,
    EISCONN = 106,
    ENOTCONN = 107,
    ESHUTDOWN = 108,
    ETIMEDOUT = 110,
    ECONNREFUSED = 111,
    EHOSTDOWN = 112,
    EHOSTUNREACH = 113,
    EALREADY = 114,
    EINPROGRESS = 115,
}

impl Errno {
    pub const fn as_ret(self) -> i64 {
        -(self as i64)
    }
}

// ==================== 错误码转换 (兼容旧的 SyscallError 语义) ====================

#[deprecated(note = "use Errno::ENOENT.as_ret() instead")]
pub type SyscallError = Errno;

#[allow(deprecated)]
impl SyscallError {
    #[allow(non_upper_case_globals)]
    pub const E_PERM: Self = Self::EPERM;
    pub const E_NOTFOUND: Self = Self::ENOENT;
    pub const E_NOSYS: Self = Self::ENOSYS;
    pub const E_INTR: Self = Self::EINTR;
    pub const E_IO: Self = Self::EIO;
    pub const E_NOEXEC: Self = Self::ENOEXEC;
    pub const E_BADFD: Self = Self::EBADF;
    pub const E_CHILD: Self = Self::ECHILD;
    pub const E_AGAIN: Self = Self::EAGAIN;
    pub const E_NOMEM: Self = Self::ENOMEM;
    pub const E_ACCES: Self = Self::EACCES;
    pub const E_FAULT: Self = Self::EFAULT;
    pub const E_BUSY: Self = Self::EBUSY;
    pub const E_EXIST: Self = Self::EEXIST;
    pub const E_NOTDIR: Self = Self::ENOTDIR;
    pub const E_ISDIR: Self = Self::EISDIR;
    pub const E_INVAL: Self = Self::EINVAL;
    pub const E_NOSPC: Self = Self::ENOSPC;
    pub const E_ROFS: Self = Self::EROFS;
    pub const E_RANGE: Self = Self::ERANGE;
    pub const E_NAMETOOLONG: Self = Self::ENAMETOOLONG;
    pub const E_NOTEMPTY: Self = Self::ENOTEMPTY;
    pub const E_AUTH_INVALID: Self = Self::EPERM;
    pub const E_AUTH_NOTFOUND: Self = Self::ENOENT;
    pub const E_AUTH_DISABLED: Self = Self::EPERM;
    pub const E_AUTH_EXPIRED: Self = Self::EPERM;
    pub const E_AUTH_PWERR: Self = Self::EACCES;
    pub const E_AUTH_CAP: Self = Self::EACCES;
    pub const E_AUTH_DENY: Self = Self::EACCES;

    pub fn as_i64(self) -> i64 {
        -(self as i64)
    }

    pub fn from_i64(code: i64) -> Option<Self> {
        let v = (-code) as i32;
        match v {
            1 => Some(Self::EPERM),
            2 => Some(Self::ENOENT),
            3 => Some(Self::ESRCH),
            4 => Some(Self::EINTR),
            5 => Some(Self::EIO),
            8 => Some(Self::ENOEXEC),
            9 => Some(Self::EBADF),
            10 => Some(Self::ECHILD),
            11 => Some(Self::EAGAIN),
            12 => Some(Self::ENOMEM),
            13 => Some(Self::EACCES),
            14 => Some(Self::EFAULT),
            16 => Some(Self::EBUSY),
            17 => Some(Self::EEXIST),
            20 => Some(Self::ENOTDIR),
            21 => Some(Self::EISDIR),
            22 => Some(Self::EINVAL),
            28 => Some(Self::ENOSPC),
            30 => Some(Self::EROFS),
            34 => Some(Self::ERANGE),
            36 => Some(Self::ENAMETOOLONG),
            38 => Some(Self::ENOSYS),
            39 => Some(Self::ENOTEMPTY),
            _ => None,
        }
    }
}

impl core::fmt::Display for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EPERM => write!(f, "Operation not permitted"),
            Self::ENOENT => write!(f, "No such file or directory"),
            Self::ESRCH => write!(f, "No such process"),
            Self::EINTR => write!(f, "Interrupted system call"),
            Self::EIO => write!(f, "I/O error"),
            Self::ENOEXEC => write!(f, "Exec format error"),
            Self::EBADF => write!(f, "Bad file descriptor"),
            Self::ECHILD => write!(f, "No child processes"),
            Self::EAGAIN => write!(f, "Resource temporarily unavailable"),
            Self::ENOMEM => write!(f, "Cannot allocate memory"),
            Self::EACCES => write!(f, "Permission denied"),
            Self::EFAULT => write!(f, "Bad address"),
            Self::EBUSY => write!(f, "Device or resource busy"),
            Self::EEXIST => write!(f, "File exists"),
            Self::ENOTDIR => write!(f, "Not a directory"),
            Self::EISDIR => write!(f, "Is a directory"),
            Self::EINVAL => write!(f, "Invalid argument"),
            Self::ENOSPC => write!(f, "No space left on device"),
            Self::EROFS => write!(f, "Read-only file system"),
            Self::ERANGE => write!(f, "Result too large"),
            Self::ENAMETOOLONG => write!(f, "File name too long"),
            Self::ENOTEMPTY => write!(f, "Directory not empty"),
            Self::ENOSYS => write!(f, "Function not implemented"),
            _ => write!(f, "Error {}", -(*self as i64)),
        }
    }
}

// ==================== 辅助类型 ====================

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

pub type SyscallHandler = extern "C" fn(u64, u64, u64, u64) -> i64;
pub type SyscallResult<T> = Result<T, Errno>;
