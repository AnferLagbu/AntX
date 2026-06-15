#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! Linux ABI 兼容层 (linuxulator) — services 层策略主体
//!
//! ## T5-2 迁移记录
//!
//! 原属 framework/syscall/linuxulator.rs, 2026-06-16 提取到 services.
//! 纯数据映射 (编号翻译表 + 参数转换), 无 unsafe 操作.
//!
//! ## 架构
//!
//! ```text
//! Linux ELF 二进制
//!     │
//!     ▼
//! ┌─────────────────────────────────┐
//! │  linuxulator                    │
//! │  1. 编号翻译 (Linux → QX_*)     │
//! │  2. 参数转换 (Linux ABI → QX)   │
//! │  3. 结构体适配 (stat, sigaction)│
//! │  4. at 系列路径拼接             │
//! └─────────────────────────────────┘
//!     │
//!     ▼
//! syscall_dispatch (QX_* 编号)
//! ```
//!
//! ## 编号空间
//!
//! | 范围     | 用途                                |
//! |----------|-------------------------------------|
//! | 0-299    | Linux 兼容 (linuxulator 1:1 映射)   |
//! | 300-399  | 保留                                |
//! | 400-499  | Credo 私有 syscall                  |
//! | 500+     | QueenX 原生 (QX_*)                  |
//!
//! ## 翻译规则
//!
//! - Linux 编号 (0-299): 查架构翻译表, 未命中返回原值 (dispatch 走 ENOSYS)
//! - Credo/FB (400-499): 直接透传
//! - QueenX 原生 (500+): 直接透传
//!
//! ## 设计原则 (遵循 queenx-naming-standpoint.md)
//!
//! - 二进制原汁原味: 不修改 Linux ELF 代码段
//! - PT_INTERP 改写: 内核层做, 工具链不需要知道
//! - syscall 翻译: 内核模块, 不动 QueenX 主线
//! - 模块化: linuxulator 可选, 卸载后 QueenX 仍独立运行
//! - 不假装自己是 Linux: 也不假装不是

use crate::kernel::framework::syscall::types::*;

// ============================================================================
// 第一层: 编号翻译
// ============================================================================

/// 判断原始 syscall 号是否为当前架构的 rt_sigreturn。
///
/// rt_sigreturn 需要在架构入口点特殊处理 (恢复信号帧),
/// 因此在翻译前就需要识别。
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn is_rt_sigreturn(num: u64) -> bool {
    num == 15 // Linux x86_64 SYS_rt_sigreturn
}

#[cfg(target_arch = "aarch64")]
#[inline]
pub fn is_rt_sigreturn(num: u64) -> bool {
    num == 139 // Linux aarch64 SYS_rt_sigreturn
}

/// 将原始 syscall 编号翻译为 QueenX 原生编号。
///
/// 对于 Linux 兼容编号 (0-299), 查架构翻译表;
/// 对于 Credo/FB (400-499) 和 QueenX 原生 (500+), 直接透传。
#[inline]
pub fn translate_syscall(num: u64) -> u64 {
    if num >= 400 {
        // Credo/FB/QX 原生: 直接透传
        return num;
    }
    // Linux 兼容编号: 查架构翻译表
    translate_linux(num)
}

// ============================================================================
// 第二层: 参数转换 (预留接口)
// ============================================================================

/// Linux syscall 参数转换结果。
///
/// 当 Linux 和 QueenX 的参数布局一致时, 直接透传;
/// 当不一致时 (如 openat vs open), 需要在此做转换。
pub struct LinuxArgs {
    /// 翻译后的 QueenX syscall 编号
    pub num: u64,
    /// 转换后的参数
    pub args: [u64; 6],
}

/// 将 Linux syscall 参数转换为 QueenX 原生参数。
///
/// 当前阶段: 大多数 syscall 参数布局一致, 直接透传。
/// 需要特殊处理的调用 (如 openat, mkdirat 等 at 系列)
/// 将在后续迭代中逐步实现。
#[inline]
pub fn translate_args(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> LinuxArgs {
    let translated_num = translate_syscall(num);
    // 当前阶段: 参数直接透传
    // 后续迭代将在此处添加 at 系列路径拼接、结构体适配等
    LinuxArgs {
        num: translated_num,
        args: [a0, a1, a2, a3, a4, a5],
    }
}

// ============================================================================
// x86_64 Linux 翻译表
// ============================================================================

#[cfg(target_arch = "x86_64")]
fn translate_linux(num: u64) -> u64 {
    match num {
        // 文件 I/O
        0 => QX_READ,
        1 => QX_WRITE,
        2 => QX_OPEN,
        3 => QX_CLOSE,
        4 => QX_STAT,
        5 => QX_FSTAT,
        6 => QX_LSTAT,
        7 => QX_POLL,
        8 => QX_LSEEK,

        // 内存管理
        9 => QX_MMAP,
        10 => QX_MPROTECT,
        11 => QX_MUNMAP,
        12 => QX_BRK,
        25 => QX_MREMAP,
        27 => QX_MINCORE,
        28 => QX_MADVISE,
        149 => QX_MLOCK,
        150 => QX_MUNLOCK,
        151 => QX_MLOCKALL,
        152 => QX_MUNLOCKALL,

        // 信号
        13 => QX_RT_SIGACTION,
        14 => QX_RT_SIGPROCMASK,
        15 => QX_RT_SIGRETURN,

        // 设备
        16 => QX_IOCTL,

        // 文件访问
        21 => QX_ACCESS,
        22 => QX_PIPE,
        23 => QX_SELECT,
        24 => QX_SCHED_YIELD,

        // FD 操作
        32 => QX_DUP,
        33 => QX_DUP2,

        // 优先级
        34 => QX_NICE,

        // 定时器
        35 => QX_NANOSLEEP,
        36 => QX_GETITIMER,
        37 => QX_ALARM,
        38 => QX_SETITIMER,

        // 进程
        39 => QX_GETPID,

        // 网络
        41 => QX_SOCKET,
        42 => QX_CONNECT,
        43 => QX_ACCEPT,
        44 => QX_SENDTO,
        45 => QX_RECVFROM,
        46 => QX_SENDMSG,
        47 => QX_RECVMSG,
        48 => QX_SHUTDOWN,
        49 => QX_BIND,
        50 => QX_LISTEN,
        51 => QX_GETSOCKNAME,
        52 => QX_GETPEERNAME,
        54 => QX_SETSOCKOPT,
        55 => QX_GETSOCKOPT,

        // 进程创建
        56 => QX_CLONE,
        57 => QX_FORK,
        59 => QX_EXECVE,
        60 => QX_EXIT,
        61 => QX_WAIT4,
        62 => QX_KILL,

        // 系统信息
        63 => QX_UNAME,

        // FD 操作
        72 => QX_FCNTL,
        73 => QX_FLOCK,

        // 文件截断
        76 => QX_TRUNCATE,
        77 => QX_FTRUNCATE,

        // 目录
        78 => QX_GETDENTS,

        // 路径
        79 => QX_GETCWD,
        80 => QX_CHDIR,

        // 文件操作
        82 => QX_RENAME,
        83 => QX_MKDIR,
        84 => QX_RMDIR,
        85 => QX_CREAT,
        86 => QX_LINK,
        87 => QX_UNLINK,
        88 => QX_SYMLINK,
        89 => QX_READLINK,

        // 文件权限
        90 => QX_CHMOD,
        91 => QX_FCHMOD,
        92 => QX_CHOWN,
        93 => QX_FCHOWN,

        // 文件属性
        95 => QX_UMASK,

        // 时间
        96 => QX_GETTIMEOFDAY,
        97 => QX_GETRLIMIT,
        160 => QX_SETRLIMIT,
        98 => QX_GETRUSAGE,
        99 => QX_SYSINFO,

        // 系统
        100 => QX_TIMES,

        // 用户/组
        102 => QX_GETUID,
        104 => QX_GETGID,
        105 => QX_SETUID,
        106 => QX_SETGID,
        107 => QX_GETEUID,
        108 => QX_GETEGID,

        // 进程组
        110 => QX_GETPPID,
        111 => QX_GETPGID,
        112 => QX_SETSID,
        113 => QX_SETEUID,
        114 => QX_SETEGID,
        115 => QX_SETREUID,
        116 => QX_SETREUID,

        // 进程调度
        140 => QX_GETPRIORITY,
        141 => QX_SETPRIORITY,

        // 文件同步
        162 => QX_SYNC,
        170 => QX_FSYNC,

        // 挂载
        165 => QX_MOUNT,
        166 => QX_UMOUNT2,

        // 其他 POSIX
        154 => QX_SETPGID,
        156 => QX_GETSID,
        157 => QX_PRCTL,
        186 => QX_GETTID,
        201 => QX_TIME,
        202 => QX_FUTEX,
        203 => QX_SCHED_SETAFFINITY,
        204 => QX_SCHED_GETAFFINITY,
        228 => QX_CLOCK_GETTIME,
        231 => QX_EXIT_GROUP,
        232 => QX_EPOLL_WAIT,
        233 => QX_EPOLL_CTL,
        234 => QX_TGKILL,

        // 事件轮询
        213 => QX_EPOLL_CREATE,

        // eventfd / signalfd / timerfd
        282 => QX_SIGNALFD,
        283 => QX_TIMERFD_CREATE,
        284 => QX_EVENTFD,
        286 => QX_TIMERFD_SETTIME,
        287 => QX_TIMERFD_GETTIME,
        288 => QX_INOTIFY_INIT1,
        289 => QX_SIGNALFD4,
        290 => QX_EVENTFD2,
        29 => QX_INOTIFY_ADD_WATCH,
        30 => QX_INOTIFY_RM_WATCH,
        40 => QX_SENDFILE,
        275 => QX_SPLICE,

        // 设备固件加载
        175 => QX_FW_LOAD,
        313 => QX_FW_GET,

        // POSIX Timer
        222 => QX_TIMER_CREATE,
        223 => QX_TIMER_SETTIME,
        224 => QX_TIMER_GETTIME,
        226 => QX_TIMER_DELETE,
        227 => QX_TIMER_GETOVERRUN,
        229 => QX_CLOCK_GETRES,

        // P1 #14: 熵源 / Stack Canary
        318 => QX_GETRANDOM,

        // C7: Seccomp
        317 => QX_SECCOMP,

        // D1: Namespace
        272 => QX_UNSHARE,
        308 => QX_SETNS,

        // D3: NUMA
        239 => QX_GET_MEMPOLICY,
        238 => QX_SET_MEMPOLICY,
        256 => QX_MIGRATE_PAGES,
        309 => QX_GETCPU,

        // D4: eBPF
        321 => QX_BPF,

        _ => num, // 未识别: 透传, dispatch 走 ENOSYS
    }
}

// ============================================================================
// aarch64 Linux 翻译表
// ============================================================================

#[cfg(target_arch = "aarch64")]
fn translate_linux(num: u64) -> u64 {
    match num {
        // 文件 I/O (aarch64 使用 openat 而非 open)
        56 => QX_OPEN,
        57 => QX_CLOSE,
        62 => QX_LSEEK,
        63 => QX_READ,
        64 => QX_WRITE,

        // 内存管理
        214 => QX_BRK,
        215 => QX_MUNMAP,
        216 => QX_MREMAP,
        222 => QX_MMAP,
        226 => QX_MPROTECT,
        228 => QX_MLOCK,
        229 => QX_MUNLOCK,
        230 => QX_MLOCKALL,
        231 => QX_MUNLOCKALL,
        232 => QX_MINCORE,
        233 => QX_MADVISE,

        // 信号
        129 => QX_KILL,
        130 => QX_TGKILL,
        131 => QX_TKILL,
        134 => QX_RT_SIGACTION,
        135 => QX_RT_SIGPROCMASK,
        139 => QX_RT_SIGRETURN,

        // 设备
        29 => QX_IOCTL,

        // 文件访问 / FS
        45 => QX_TRUNCATE,
        46 => QX_FTRUNCATE,
        48 => QX_ACCESS,
        49 => QX_CHDIR,
        52 => QX_FCHMOD,
        55 => QX_FCHOWN,
        61 => QX_GETDENTS,
        78 => QX_READLINK,
        79 => QX_STAT,
        80 => QX_FSTAT,

        // 文件同步 / 挂载
        39 => QX_UMOUNT2,
        40 => QX_MOUNT,
        81 => QX_SYNC,
        82 => QX_FSYNC,

        // 进程
        93 => QX_EXIT,
        94 => QX_EXIT_GROUP,
        95 => QX_WAIT4,
        172 => QX_GETPID,
        173 => QX_GETPPID,
        174 => QX_GETUID,
        175 => QX_GETEUID,
        176 => QX_GETGID,
        177 => QX_GETEGID,
        178 => QX_GETTID,
        220 => QX_CLONE,
        221 => QX_EXECVE,

        // 进程调度
        122 => QX_SCHED_SETAFFINITY,
        123 => QX_SCHED_GETAFFINITY,
        124 => QX_SCHED_YIELD,
        140 => QX_SETPRIORITY,
        141 => QX_GETPRIORITY,
        154 => QX_SETPGID,
        155 => QX_GETPGID,
        156 => QX_GETSID,
        157 => QX_SETSID,

        // 身份
        143 => QX_SETREUID,
        144 => QX_SETGID,
        145 => QX_SETREUID,
        146 => QX_SETUID,

        // 网络
        198 => QX_SOCKET,
        199 => QX_SOCKETPAIR,
        200 => QX_BIND,
        201 => QX_LISTEN,
        202 => QX_ACCEPT,
        203 => QX_CONNECT,
        204 => QX_GETSOCKNAME,
        205 => QX_GETPEERNAME,
        206 => QX_SENDTO,
        207 => QX_RECVFROM,
        208 => QX_SETSOCKOPT,
        209 => QX_GETSOCKOPT,
        210 => QX_SHUTDOWN,
        211 => QX_SENDMSG,
        212 => QX_RECVMSG,

        // 同步 / IPC
        98 => QX_FUTEX,

        // 事件轮询
        20 => QX_EPOLL_CREATE,
        21 => QX_EPOLL_CTL,
        22 => QX_EPOLL_WAIT,

        // eventfd / signalfd / timerfd
        19 => QX_EVENTFD2,
        74 => QX_SIGNALFD4,
        85 => QX_TIMERFD_CREATE,
        86 => QX_TIMERFD_SETTIME,
        87 => QX_TIMERFD_GETTIME,

        // inotify
        26 => QX_INOTIFY_INIT1,
        27 => QX_INOTIFY_ADD_WATCH,
        28 => QX_INOTIFY_RM_WATCH,

        // sendfile / splice
        71 => QX_SENDFILE,
        76 => QX_SPLICE,

        // 设备固件加载
        271 => QX_FW_LOAD,
        314 => QX_FW_GET,

        // POSIX Timer
        107 => QX_TIMER_CREATE,
        108 => QX_TIMER_GETTIME,
        109 => QX_TIMER_GETOVERRUN,
        110 => QX_TIMER_SETTIME,
        111 => QX_TIMER_DELETE,
        114 => QX_CLOCK_GETRES,

        // P1 #14: 熵源 / Stack Canary
        278 => QX_GETRANDOM,

        // C7: Seccomp / prctl
        277 => QX_SECCOMP,
        167 => QX_PRCTL,

        // D1: Namespace
        97 => QX_UNSHARE,
        432 => QX_SETNS,

        // D3: NUMA
        236 => QX_GET_MEMPOLICY,
        237 => QX_SET_MEMPOLICY,
        238 => QX_MIGRATE_PAGES,
        168 => QX_GETCPU,

        // D4: eBPF
        280 => QX_BPF,

        // FD 操作
        23 => QX_DUP,
        24 => QX_DUP3,
        25 => QX_FCNTL,
        32 => QX_FLOCK,
        59 => QX_PIPE2,

        // 文件系统操作
        34 => QX_MKDIR,
        35 => QX_UNLINK,
        36 => QX_SYMLINK,
        37 => QX_LINK,
        38 => QX_RENAME,
        53 => QX_FCHMODAT,
        166 => QX_UMASK,

        // 系统信息
        160 => QX_UNAME,
        163 => QX_GETRLIMIT,
        164 => QX_SETRLIMIT,
        165 => QX_GETRUSAGE,
        179 => QX_SYSINFO,

        // 时间
        101 => QX_NANOSLEEP,
        102 => QX_GETITIMER,
        103 => QX_SETITIMER,
        112 => QX_CLOCK_SETTIME,
        113 => QX_CLOCK_GETTIME,
        169 => QX_GETTIMEOFDAY,

        // 其他
        17 => QX_GETCWD,
        58 => QX_PIPE,
        99 => QX_NICE,

        _ => num, // 未识别: 透传, dispatch 走 ENOSYS
    }
}

// ============================================================================
// 非 x86_64 / aarch64 架构: 空翻译
// ============================================================================

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn translate_linux(num: u64) -> u64 {
    num // 不翻译, 直接透传
}
