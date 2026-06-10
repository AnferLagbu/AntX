//! 系统调用 API 层
//!
//! QueenX 原生 syscall (QX_*) + Linux 兼容 (SYS_*) + Credo 私有 syscall 的统一分发入口,
//! 用户态→内核态的唯一合法路径。
//!
//! ## 编号空间
//! - 0-299   : Linux 兼容编号 (SYS_*), 由 linuxulator 模块翻译为 QX_*
//! - 400-499 : Credo 私有 syscall
//! - 500+    : QueenX 原生编号 (QX_*)
//!
//! ## 调用方契约
//! - `boot::isr.asm` —— 中断/异常入口 (int 0x80 / syscall 指令)
//! - `idt::handlers` —— ISR 存根调用 `syscall_dispatch_from_frame`
//! - `proc::exec::load_elf` —— execve 时验证用户指针
//! - `credo::api` —— 能力检查路径复用 `validate_user_ptr`
//! - `chitin::user_driver` —— 用户态驱动透传
//!
//! ## 内部接口
//! - `types.rs` —— SyscallHandler 函数指针类型, Errno, syscall 编号常量
//! - `linuxulator.rs` —— Linux ABI 兼容层 (编号翻译 + 参数转换)
//! - `mmap.rs` —— mmap/munmap/mprotect 实现
//! - `mod.rs` —— syscall_dispatch() 核心分发器 (所有 sys_* 实现)
//!
//! ## 安全约束
//! - 所有公开函数均通过 validate_user_ptr / validate_user_buf 检查用户指针
//! - 用户指针必须在 [1, 0x7FFFFFFFE000) 范围内
//! - syscall_dispatch / syscall_dispatch_from_frame 必须在中断上下文调用
//! - syscall_register 仅在启动阶段单线程调用
//!
//! ## 性能特征
//! - 分发路径: O(1) match 分支, 编译器优化为跳转表
//! - 指针验证: 两次比较, ≤ 5ns
//! - 覆盖 70+ POSIX syscall + 40+ Credo 私有 syscall

pub use super::types::{SyscallHandler, Errno};

// ============================================================================
// QueenX 原生 syscall 编号 (QX_*)
// ============================================================================

pub use super::types::{
    QX_EXIT, QX_WRITE, QX_READ, QX_OPEN, QX_CLOSE, QX_STAT, QX_FSTAT, QX_LSTAT, QX_LSEEK,
    QX_MMAP, QX_BRK, QX_MPROTECT, QX_MUNMAP, QX_MREMAP,
    QX_GETPID, QX_FORK, QX_EXECVE, QX_CLONE, QX_WAIT4, QX_EXIT_GROUP,
    QX_GETPPID, QX_GETTID, QX_GETPGID, QX_SETPGID, QX_GETSID, QX_SETSID,
    QX_NICE, QX_SCHED_YIELD, QX_SCHED_SETAFFINITY, QX_SCHED_GETAFFINITY,
    QX_GETPRIORITY, QX_SETPRIORITY,
    QX_RT_SIGACTION, QX_RT_SIGPROCMASK, QX_RT_SIGRETURN, QX_KILL, QX_TGKILL,
    QX_MKDIR, QX_RMDIR, QX_RENAME, QX_LINK, QX_UNLINK, QX_SYMLINK, QX_READLINK,
    QX_CHMOD, QX_FCHMOD, QX_CHOWN, QX_FCHOWN, QX_UMASK, QX_ACCESS,
    QX_TRUNCATE, QX_FTRUNCATE, QX_GETDENTS, QX_GETCWD, QX_CHDIR, QX_CREAT, QX_PIPE,
    QX_DUP, QX_DUP2, QX_FCNTL, QX_FLOCK, QX_IOCTL, QX_SYNC, QX_FSYNC, QX_MOUNT, QX_UMOUNT2,
    QX_POLL, QX_SELECT,
    QX_GETUID, QX_GETGID, QX_SETUID, QX_SETGID, QX_GETEUID, QX_GETEGID,
    QX_SETEUID, QX_SETEGID, QX_SETREUID,
    QX_SOCKET, QX_BIND, QX_LISTEN, QX_ACCEPT, QX_CONNECT,
    QX_SENDTO, QX_RECVFROM, QX_SENDMSG, QX_RECVMSG, QX_SHUTDOWN,
    QX_SETSOCKOPT, QX_GETSOCKOPT, QX_GETSOCKNAME, QX_GETPEERNAME,
    QX_FUTEX, QX_EPOLL_CREATE, QX_EPOLL_CTL, QX_EPOLL_WAIT,
    QX_EVENTFD, QX_EVENTFD2, QX_SIGNALFD, QX_SIGNALFD4,
    QX_TIMERFD_CREATE, QX_TIMERFD_SETTIME, QX_TIMERFD_GETTIME,
    QX_INOTIFY_INIT1, QX_INOTIFY_ADD_WATCH, QX_INOTIFY_RM_WATCH,
    QX_UNAME, QX_SYSINFO, QX_GETRLIMIT, QX_GETRUSAGE,
    QX_CLOCK_GETTIME, QX_GETTIMEOFDAY, QX_NANOSLEEP, QX_ALARM,
    QX_GETITIMER, QX_SETITIMER, QX_TIME, QX_TIMES,
    QX_FB_OPEN, QX_FB_MMAP, QX_FB_RELEASE,
    QX_FW_LOAD, QX_FW_GET, QX_FW_GET_INFO, QX_FW_DETACH,
    QX_TIMER_CREATE, QX_TIMER_SETTIME, QX_TIMER_GETTIME, QX_TIMER_DELETE,
    QX_TIMER_GETOVERRUN, QX_CLOCK_GETRES,
    QX_GETRANDOM, QX_GET_CANARY,
    QX_FTRACE_ENABLE, QX_FTRACE_DISABLE, QX_FTRACE_READ, QX_FTRACE_STAT,
    QX_KGDB_ENTER,
    QX_SECCOMP, QX_PRCTL,
    QX_ROUTE_ADD, QX_ROUTE_DEL, QX_ROUTE_QUERY,
    QX_NF_ADD_RULE, QX_NF_DEL_RULE,
    QX_IO_URING_SETUP, QX_IO_URING_ENTER, QX_IO_URING_REGISTER, QX_IO_URING_SUBMIT,
    QX_UNSHARE, QX_SETNS,
    QX_CGROUP_CREATE, QX_CGROUP_DESTROY, QX_CGROUP_ATTACH,
    QX_CGROUP_SET_LIMIT, QX_CGROUP_GET_STAT,
    QX_GET_MEMPOLICY, QX_SET_MEMPOLICY, QX_MIGRATE_PAGES, QX_GETCPU,
    QX_BPF,
    QX_PM,
    QX_SECURE_BOOT, QX_TPM,
    QX_CET,
};

// ============================================================================
// Linux 兼容编号 (SYS_*) — 保留给 linuxulator, 由 linuxulator 模块翻译
// ============================================================================

pub const SYS_read: u64 = 0;
pub const SYS_write: u64 = 1;
pub const SYS_open: u64 = 2;
pub const SYS_close: u64 = 3;
pub const SYS_stat: u64 = 4;
pub const SYS_fstat: u64 = 5;
pub const SYS_lseek: u64 = 8;
pub const SYS_mmap: u64 = 9;
pub const SYS_munmap: u64 = 11;
pub const SYS_brk: u64 = 12;
pub const SYS_rt_sigaction: u64 = 13;
pub const SYS_rt_sigprocmask: u64 = 14;
pub const SYS_ioctl: u64 = 16;
pub const SYS_pipe: u64 = 22;
pub const SYS_dup: u64 = 32;
pub const SYS_dup2: u64 = 33;
pub const SYS_nanosleep: u64 = 35;
pub const SYS_getpid: u64 = 39;
pub const SYS_fork: u64 = 57;
pub const SYS_execve: u64 = 59;
pub const SYS_exit: u64 = 60;
pub const SYS_wait4: u64 = 61;
pub const SYS_kill: u64 = 62;
pub const SYS_getdents: u64 = 78;
pub const SYS_getcwd: u64 = 79;
pub const SYS_chdir: u64 = 80;
pub const SYS_rename: u64 = 82;
pub const SYS_mkdir: u64 = 83;
pub const SYS_rmdir: u64 = 84;
pub const SYS_unlink: u64 = 87;
pub const SYS_readlink: u64 = 89;
pub const SYS_chmod: u64 = 90;
pub const SYS_gettimeofday: u64 = 96;
pub const SYS_getuid: u64 = 102;
pub const SYS_getgid: u64 = 104;
pub const SYS_sync: u64 = 162;
pub const SYS_mount: u64 = 165;
pub const SYS_umount2: u64 = 166;
pub const SYS_sched_yield: u64 = 24;
pub const SYS_exit_group: u64 = 231;
pub const SYS_futex: u64 = 202;
pub const SYS_clock_gettime: u64 = 228;
pub const SYS_CREDO_BASE: u64 = 400;
pub const MAX_SYSCALLS: u64 = 800;

// ============================================================================
// 契约: 注册机制
// ============================================================================

/// 动态注册 syscall 处理器。
///
/// # 安全约束
/// - 仅在启动阶段单线程调用
/// - `num` 不可与已有注册冲突
/// - `handler` 必须在中断上下文可调用 (无 sleep / 无 lock 等待)
///
/// # Safety
/// 调用方确保 num 合法且 handler 在中断上下文安全。
pub unsafe fn syscall_register(num: u64, handler: SyscallHandler) {
    unsafe { super::syscall_register(num, handler) }
}

/// 验证用户态指针是否在合法范围内
pub fn validate_user_ptr(ptr: u64) -> bool {
    super::validate_user_ptr(ptr)
}

/// 验证用户态缓冲区是否在合法范围内
pub fn validate_user_buf(ptr: u64, len: u64) -> bool {
    super::validate_user_buf(ptr, len)
}

/// nanosleep 系统调用实现 (TCB: 操作 hrtimer + 调度器)
pub fn sys_nanosleep(req: u64, rem: u64) -> i64 {
    super::sys_nanosleep(req, rem)
}

/// kill 系统调用实现 (TCB: 操作进程信号位)
pub fn sys_kill(pid: i32, sig: i32) -> i64 {
    super::sys_kill(pid, sig)
}

/// rt_sigaction 系统调用实现 (TCB: 操作 sigaction 表)
pub fn sys_rt_sigaction(signum: i32, act: u64, oact: u64) -> i64 {
    super::sys_rt_sigaction(signum, act, oact)
}

/// rt_sigprocmask 系统调用实现 (TCB: 操作信号掩码)
pub fn sys_rt_sigprocmask(how: i32, set: u64, oset: u64) -> i64 {
    super::sys_rt_sigprocmask(how, set, oset)
}
