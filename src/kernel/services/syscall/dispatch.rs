#![deny(unsafe_code)]
//! 系统调用分发策略 — services 层
//!
//! T5-1: 将 syscall 号 → 处理函数映射表 (分发策略) 从 framework 提取到 services.
//! framework 仅保留入口汇编 + unsafe 边界, services 拥有完整的分发映射.
//!
//! ## 架构
//!
//! ```text
//! 用户态 → framework 入口 (syscall/sysret 汇编)
//!        → framework::syscall_dispatch_from_frame (unsafe 边界)
//!        → services::syscall::dispatch::ServicesSyscallDispatch::dispatch (策略)
//!        → framework 回退 (未迁移的 syscall)
//! ```
//!
//! ## 迁移状态
//!
//! - 已迁移: 文件 I/O, 文件系统, 内存管理, 进程, 信号, 网络, 凭证, 同步, 定时器,
//!   事件轮询, eventfd/signalfd/timerfd, Credo 私有 syscall, 存储设备, inotify,
//!   内存建议与锁定, 进程创建/等待, 系统信息, CPU 亲和性, 进程优先级
//! - 待迁移: read/write, execve, firmware, ftrace/kgdb, seccomp/prctl,
//!   路由/Netfilter, io_uring, namespace, cgroup, NUMA, eBPF, PM, TPM,
//!   CET, tickless, timesync, kexec, UEFI, 帧缓冲, sendfile/splice 等
//!
//! 评估日期: 2026-06-19

use crate::kernel::framework::syscall::dispatch_trait::{register_syscall_dispatch, SyscallDispatch};
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 辅助函数
// ============================================================================

/// 将 services 层 Result 转为 i64 返回码
#[inline]
fn as_ret(r: Result<usize, Errno>) -> i64 {
    match r {
        Ok(v) => v as i64,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// services 层系统调用分发策略
// ============================================================================

/// services 层系统调用分发策略
///
/// L-01: 已从 framework 迁移的 syscall 分支在此分发.
/// 返回 -ENOSYS (-38) 表示未处理, framework 回退处理.
pub struct ServicesSyscallDispatch;

impl SyscallDispatch for ServicesSyscallDispatch {
    fn dispatch(&self, num: u64, args: [u64; 6]) -> i64 {
        use crate::kernel::services::syscall::types::*;
        let [a0, a1, a2, a3, a4, a5] = args;

        match num {
            // ==================== 文件 I/O (已迁移) ====================
            QX_OPEN => as_ret(crate::kernel::services::fs::open::open_syscall(a0, a1 as i32, a2 as i32)),
            QX_CLOSE => as_ret(crate::kernel::services::fs::open::close_syscall(a0 as i32)),
            QX_STAT => as_ret(crate::kernel::services::fs::stat::stat_syscall(a0, a1)),
            QX_FSTAT => as_ret(crate::kernel::services::fs::stat::fstat_syscall(a0 as i32, a1)),
            QX_LSTAT => as_ret(crate::kernel::services::fs::stat::lstat_syscall(a0, a1)),
            QX_CREAT => as_ret(crate::kernel::services::fs::open::creat_syscall(a0, a2 as i32)),

            // ==================== 文件系统操作 (已迁移) ====================
            QX_MKDIR => as_ret(crate::kernel::services::fs::mode::mkdir_syscall(a0, a1 as i32)),
            QX_RMDIR => as_ret(crate::kernel::services::fs::mode::rmdir_syscall(a0)),
            QX_CHMOD => as_ret(crate::kernel::services::fs::mode::chmod_syscall(a0, a1 as u32)),
            QX_FCHMOD => as_ret(crate::kernel::services::fs::mode::fchmod_syscall(a0 as i32, a1 as u32)),
            QX_UMASK => as_ret(crate::kernel::services::fs::mode::umask_syscall(a0 as u32)),
            QX_ACCESS => as_ret(crate::kernel::services::fs::access::access_syscall(a0, a1 as i32)),
            QX_UNLINK => as_ret(crate::kernel::services::fs::access::unlink_syscall(a0)),
            QX_RENAME => as_ret(crate::kernel::services::fs::misc::rename_syscall(a0, a1)),
            QX_SYMLINK => as_ret(crate::kernel::services::fs::link::symlink_syscall(a0, a1)),
            QX_READLINK => as_ret(crate::kernel::services::fs::link::readlink_syscall(a0, a1, a2)),
            QX_FCHOWN => as_ret(crate::kernel::services::fs::misc::fchown_syscall(a0 as i32, a1, a2)),
            QX_SYNC => as_ret(crate::kernel::services::fs::misc::sync_syscall()),
            QX_FSYNC => as_ret(crate::kernel::services::fs::misc::fsync_syscall(a0 as i32)),
            QX_MOUNT => as_ret(crate::kernel::services::fs::mount::mount_syscall(a0, a1, a2)),
            QX_UMOUNT2 => as_ret(crate::kernel::services::fs::mount::umount2_syscall(a0, a1 as i32)),
            QX_GETCWD => as_ret(crate::kernel::services::fs::path::getcwd_syscall(a0, a1)),
            QX_CHDIR => as_ret(crate::kernel::services::fs::path::chdir_syscall(a0)),

            // ==================== 文件描述符操作 (已迁移) ====================
            QX_PIPE => as_ret(crate::kernel::services::fs::io::pipe_syscall(a0)),
            QX_DUP => as_ret(crate::kernel::services::fs::io::dup_syscall(a0 as i32)),
            QX_DUP2 => as_ret(crate::kernel::services::fs::io::dup2_syscall(a0 as i32, a1 as i32)),
            QX_FCNTL => as_ret(crate::kernel::services::fs::io::fcntl_syscall(a0 as i32, a1 as i32, a2)),

            // ==================== 内存管理 (已迁移) ====================
            QX_MPROTECT => as_ret(crate::kernel::services::mm::mprotect::mprotect_syscall(a0, a1, a2 as i32)),
            QX_BRK => as_ret(crate::kernel::services::mm::brk::brk_syscall(a0)),

            // ==================== 进程信息 (已迁移) ====================
            QX_GETPID => crate::kernel::services::proc::info::getpid_syscall() as i64,
            QX_GETPPID => crate::kernel::services::proc::info::getppid_syscall() as i64,
            QX_GETPGID => as_ret(crate::kernel::services::proc::info::getpgid_syscall(a0 as i32)),
            QX_GETTID => crate::kernel::services::proc::info::gettid_syscall() as i64,
            QX_SETSID => crate::kernel::services::proc::session::proc_setsid(),
            QX_GETSID => crate::kernel::services::proc::session::proc_getsid(a0 as i32),
            QX_SETPGID => crate::kernel::services::proc::session::proc_setpgid(a0 as i32, a1 as i32),

            // ==================== 信号 (已迁移) ====================
            QX_RT_SIGACTION => as_ret(crate::kernel::services::proc::signal::rt_sigaction_syscall(a0 as i32, a1, a2)),
            QX_RT_SIGPROCMASK => as_ret(crate::kernel::services::proc::signal::rt_sigprocmask_syscall(a0 as i32, a1, a2)),
            QX_SIGALTSTACK => as_ret(crate::kernel::services::proc::signal::sigaltstack_syscall(a0, a1)),
            QX_KILL => as_ret(crate::kernel::services::proc::signal::kill_syscall(a0 as i32, a1 as i32)),

            // ==================== 网络 (已迁移) ====================
            QX_SOCKET => as_ret(crate::kernel::services::net::syscall::socket_syscall(a0 as i32, a1 as i32, a2 as i32)),
            QX_CONNECT => as_ret(crate::kernel::services::net::syscall::connect_syscall(a0 as i32, a1, a2 as u32)),
            QX_ACCEPT => as_ret(crate::kernel::services::net::syscall::accept_syscall(a0 as i32, a1, a2)),
            QX_SENDTO => as_ret(crate::kernel::services::net::syscall::sendto_syscall(a0 as i32, a1, a2 as u32, a3 as i32, args[4], args[5] as u32)),
            QX_RECVFROM => as_ret(crate::kernel::services::net::syscall::recvfrom_syscall(a0 as i32, a1, a2 as u32, a3 as i32, args[4], args[5])),
            QX_SHUTDOWN => as_ret(crate::kernel::services::net::syscall::shutdown_syscall(a0 as i32, a1 as i32)),
            QX_BIND => as_ret(crate::kernel::services::net::syscall::bind_syscall(a0 as i32, a1, a2 as u32)),
            QX_LISTEN => as_ret(crate::kernel::services::net::syscall::listen_syscall(a0 as i32, a1 as i32)),
            QX_SENDMSG => as_ret(crate::kernel::services::net::syscall::sendmsg_syscall(a0 as i32, a1, a2 as i32)),
            QX_RECVMSG => as_ret(crate::kernel::services::net::syscall::recvmsg_syscall(a0 as i32, a1, a2 as i32)),
            QX_SETSOCKOPT => as_ret(crate::kernel::services::net::syscall::setsockopt_syscall(a0 as i32, a1 as i32, a2 as i32, a3, a4 as u32)),
            QX_GETSOCKOPT => as_ret(crate::kernel::services::net::syscall::getsockopt_syscall(a0 as i32, a1 as i32, a2 as i32, a3, a4)),

            // ==================== 凭证 (已迁移) ====================
            QX_GETUID => as_ret(crate::kernel::services::credo::uid::getuid_syscall()),
            QX_GETGID => as_ret(crate::kernel::services::credo::uid::getgid_syscall()),
            QX_SETUID => as_ret(crate::kernel::services::credo::uid::setuid_syscall(a0 as u32)),
            QX_SETGID => as_ret(crate::kernel::services::credo::uid::setgid_syscall(a0 as u32)),
            QX_GETEUID => as_ret(crate::kernel::services::credo::uid::geteuid_syscall()),
            QX_GETEGID => as_ret(crate::kernel::services::credo::uid::getegid_syscall()),
            QX_SETEUID => as_ret(crate::kernel::services::credo::uid::seteuid_syscall(a0 as u32)),
            QX_SETEGID => as_ret(crate::kernel::services::credo::uid::setegid_syscall(a0 as u32)),
            QX_SETREUID => as_ret(crate::kernel::services::credo::uid::setreuid_syscall(a0 as u32, a1 as u32)),
            // QX_SETREGID 与 QX_SETREUID 共享编号 599, 由 framework 回退处理

            // ==================== 进程优先级 (已迁移) ====================
            QX_NICE => crate::kernel::services::proc::priority::nice_syscall(a0 as i32),
            QX_GETPRIORITY => crate::kernel::services::proc::priority::getpriority_syscall(a0 as i32, a1 as u32),
            QX_SETPRIORITY => crate::kernel::services::proc::priority::setpriority_syscall(a0 as i32, a1 as u32, a2 as i32),

            // ==================== CPU 亲和性 (已迁移) ====================
            QX_SCHED_SETAFFINITY => crate::kernel::services::proc::affinity::sched_setaffinity_syscall(a0 as i32, a1 as u32, a2),
            QX_SCHED_GETAFFINITY => crate::kernel::services::proc::affinity::sched_getaffinity_syscall(a0 as i32, a1 as u32, a2),

            // ==================== 进程生命周期 (已迁移) ====================
            QX_FORK => crate::kernel::services::proc::lifecycle::fork_syscall(),
            QX_EXIT => crate::kernel::services::proc::lifecycle::exit_syscall(a0 as i32),
            QX_EXIT_GROUP => crate::kernel::services::proc::lifecycle::exit_syscall(a0 as i32),
            QX_SCHED_YIELD => crate::kernel::services::proc::lifecycle::sched_yield_syscall(),

            // ==================== 同步原语 (已迁移) ====================
            QX_FUTEX => {
                match crate::kernel::services::sync::futex::futex_syscall(a0, a1 as i32, a2 as i32, a3, a4 as u32) {
                    Ok(crate::kernel::services::sync::futex::FutexResult::Woken) => 0,
                    Ok(crate::kernel::services::sync::futex::FutexResult::WokenCount(n)) => n as i64,
                    Ok(crate::kernel::services::sync::futex::FutexResult::Requeued { woken, .. }) => woken as i64,
                    Ok(crate::kernel::services::sync::futex::FutexResult::Pending) => 0,
                    Err(e) => e.as_ret(),
                }
            }

            // ==================== 系统信息 (已迁移) ====================
            QX_GETRUSAGE => crate::kernel::services::proc::sysinfo::getrusage_syscall(a0 as i32, a1),
            QX_SYSINFO => crate::kernel::services::proc::sysinfo::sysinfo_syscall(a0),
            QX_GETRLIMIT => crate::kernel::services::proc::sysinfo::getrlimit_syscall(a0 as i32, a1),
            QX_CLOCK_GETTIME => crate::kernel::services::fs::file_ops::clock_gettime_syscall(a0 as i32, a1),

            // ==================== 文件操作 (已迁移) ====================
            QX_IOCTL => crate::kernel::services::fs::file_ops::ioctl_syscall(a0 as i32, a1, a2),
            QX_POLL => crate::kernel::services::fs::file_ops::poll_syscall(a0, a1 as u32, a2 as i32),
            QX_SELECT => crate::kernel::services::fs::file_ops::poll_syscall(a0, a1 as u32, a2 as i32),
            QX_CHOWN => crate::kernel::services::fs::file_ops::chown_syscall(a0, a1 as u32, a2 as u32),
            QX_TRUNCATE => crate::kernel::services::fs::file_ops::truncate_syscall(a0, a1 as i64),
            QX_FTRUNCATE => crate::kernel::services::fs::file_ops::ftruncate_syscall(a0 as i32, a1 as i64),
            QX_FLOCK => crate::kernel::services::fs::file_ops::flock_syscall(a0 as i32, a1 as i32),
            QX_LSEEK => crate::kernel::services::fs::dir_ops::lseek_syscall(a0 as i32, a1 as i64, a2 as i32),
            QX_GETDENTS => crate::kernel::services::fs::dir_ops::getdents_syscall(a0 as i32, a1, a2),

            // ==================== inotify (已迁移) ====================
            QX_INOTIFY_INIT1 => crate::kernel::services::fs::inotify::sys_inotify_init1(a0 as i32),
            QX_INOTIFY_ADD_WATCH => crate::kernel::services::fs::inotify::sys_inotify_add_watch(a0 as i64, a1 as u32, a2 as u32),
            QX_INOTIFY_RM_WATCH => crate::kernel::services::fs::inotify::sys_inotify_rm_watch(a0 as i64, a1 as i32),

            // ==================== 内存建议与锁定 (已迁移) ====================
            QX_MMAP => crate::kernel::services::mm::mmap::mmap_syscall_entry(a0, a1, a2 as i32, a3 as i32, a4 as i32, a5),
            QX_MUNMAP => crate::kernel::services::mm::mmap::munmap_syscall_entry(a0, a1),
            QX_MADVISE => crate::kernel::services::mm::madvise_mlock::sys_madvise(a0, a1, a2),
            QX_MLOCK => crate::kernel::services::mm::madvise_mlock::sys_mlock(a0, a1),
            QX_MUNLOCK => crate::kernel::services::mm::madvise_mlock::sys_munlock(a0, a1),
            QX_MLOCKALL => crate::kernel::services::mm::madvise_mlock::sys_mlockall(a0),
            QX_MUNLOCKALL => crate::kernel::services::mm::madvise_mlock::sys_munlockall(),
            QX_MINCORE => crate::kernel::services::mm::madvise_mlock::sys_mincore(a0, a1, a2),

            // ==================== 定时器 (已迁移) ====================
            QX_NANOSLEEP => as_ret(crate::kernel::services::proc::sleep::nanosleep_syscall(a0, a1)),
            QX_GETITIMER => as_ret(crate::kernel::services::fs::misc::getitimer_syscall(a0 as i32, a1)),
            QX_ALARM => as_ret(crate::kernel::services::fs::misc::alarm_syscall(a0 as u32)),
            QX_SETITIMER => as_ret(crate::kernel::services::fs::misc::setitimer_syscall(a0 as i32, a1, a2)),

            // ==================== 进程创建/等待 (已迁移) ====================
            QX_CLONE => as_ret(crate::kernel::services::proc::clone::clone_syscall(a0, a1, a2, a3, a4)),
            QX_WAIT4 => as_ret(crate::kernel::services::proc::wait4::wait4_syscall(a0 as i32, a1, a2 as i32)),

            // ==================== 系统信息 (已迁移) ====================
            QX_UNAME => as_ret(crate::kernel::services::proc::info::uname_syscall(a0)),
            QX_GETTIMEOFDAY => as_ret(crate::kernel::services::proc::info::gettimeofday_syscall(a0)),

            // ==================== 文件操作 (已迁移) ====================
            QX_LINK => as_ret(crate::kernel::services::fs::link::link_syscall(a0, a1)),
            QX_TIMES => as_ret(crate::kernel::services::fs::misc::times_syscall(a0)),
            QX_TIME => as_ret(crate::kernel::services::fs::misc::time_syscall(a0)),

            // ==================== 事件轮询 (已迁移) ====================
            QX_EPOLL_CREATE => as_ret(crate::kernel::services::sync::epoll::epoll_create_syscall(a0 as i32)),
            QX_EPOLL_CTL => as_ret(crate::kernel::services::sync::epoll::epoll_ctl_syscall(a0 as i64, a1 as i32, a2 as i32, a3)),
            QX_EPOLL_WAIT => as_ret(crate::kernel::services::sync::epoll::epoll_wait_syscall(a0 as i64, a1, a2 as i32, a3 as i32)),

            // ==================== eventfd / signalfd / timerfd (已迁移) ====================
            QX_EVENTFD => as_ret(crate::kernel::services::sync::eventfd::eventfd_syscall(a0, a1 as i32)),
            QX_EVENTFD2 => as_ret(crate::kernel::services::sync::eventfd::eventfd_syscall(a0, a1 as i32)),
            QX_SIGNALFD => as_ret(crate::kernel::services::sync::signalfd::signalfd_syscall(a0 as i32, a1, a2 as i32)),
            QX_SIGNALFD4 => as_ret(crate::kernel::services::sync::signalfd::signalfd_syscall(a0 as i32, a1, a2 as i32)),
            QX_TIMERFD_CREATE => as_ret(crate::kernel::services::sync::timerfd::timerfd_create_syscall(a0 as i32, a1 as i32)),
            QX_TIMERFD_SETTIME => as_ret(crate::kernel::services::sync::timerfd::timerfd_settime_syscall(a0 as i32, a1 as i32, a2, a3)),
            QX_TIMERFD_GETTIME => as_ret(crate::kernel::services::sync::timerfd::timerfd_gettime_syscall(a0 as i32, a1)),

            // ==================== Credo 私有 syscall (已迁移) ====================
            SYS_CREDO_LOGIN => crate::kernel::services::credo::auth::auth_login_syscall(a0, a1),
            SYS_CREDO_LOGOUT => crate::kernel::services::credo::auth::auth_logout_syscall(),
            SYS_CREDO_CREATE_IDENTITY => crate::kernel::services::credo::auth::auth_create_syscall(a0, a1, a2 as u8),
            SYS_CREDO_DELETE_IDENTITY => crate::kernel::services::credo::auth::auth_delete_syscall(a0),
            SYS_CREDO_IDENTITY_INFO => crate::kernel::services::credo::auth::auth_info_syscall(a0),
            SYS_CREDO_CHANGE_PASSWORD => crate::kernel::services::credo::auth::auth_changepw_syscall(a0, a1),
            SYS_CREDO_VERIFY_PASSWORD => crate::kernel::services::credo::auth::auth_verify_syscall(a0),
            SYS_CREDO_CREATE_FIRST => crate::kernel::services::credo::auth::auth_create_first_syscall(a0),
            SYS_CREDO_GRANT => crate::kernel::services::credo::auth::auth_grant_syscall(a0, a1, a2 as u16, a3),
            SYS_CREDO_REVOKE => crate::kernel::services::credo::auth::auth_revoke_syscall(a0, a1, a2 as u16, a3),
            SYS_CREDO_CHECK_CAP => crate::kernel::services::credo::auth::auth_check_cap_syscall(a0, a1 as u16, a2),
            SYS_CREDO_GET_CAPS => crate::kernel::services::credo::auth::auth_get_caps_syscall(a0, a1 as u16),
            SYS_CREDO_GET_PWM => crate::kernel::services::credo::auth::pwm_get_syscall(),
            SYS_CREDO_SET_PWM => crate::kernel::services::credo::auth::pwm_set_syscall(a0),
            SYS_CREDO_GETHOSTNAME => crate::kernel::services::proc::sysinfo::gethostname_syscall(a0, a1),
            SYS_CREDO_SETHOSTNAME => crate::kernel::services::proc::sysinfo::sethostname_syscall(a0, a1),
            SYS_CREDO_BOOT_CHECK => crate::kernel::services::proc::sysinfo::boot_check_syscall(a0 as i32),
            SYS_CREDO_PROC_LIST => crate::kernel::services::proc::proc_mgmt::proc_list_syscall(a0, a1 as u32),
            SYS_CREDO_PROC_SETPRI => crate::kernel::services::proc::proc_mgmt::proc_setpri_syscall(a0 as u32, a1 as u32),
            SYS_CREDO_PROC_CPUTIME => crate::kernel::services::proc::proc_mgmt::credo_proc_cputime_syscall(a0 as u32),
            SYS_CREDO_PROC_SLEEP => {
                let ns = a0 * 1_000_000;
                as_ret(crate::kernel::services::proc::sleep::nanosleep_syscall(ns, a1))
            }

            SYS_CREDO_REBOOT => crate::kernel::services::proc::sysinfo::reboot_syscall(a0 as i32),

            // ==================== 存储设备 (已迁移) ====================
            SYS_CREDO_DISK_LIST => as_ret(crate::kernel::services::storage::disk::disk_list(a0, a1 as u32).map(|n| n as usize)),
            SYS_CREDO_DISK_INFO => match crate::kernel::services::storage::disk::disk_info(a0 as u32, a1) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            },
            SYS_CREDO_DISK_FORMAT => match crate::kernel::services::storage::disk::disk_format(a0 as u32, a1) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            },
            SYS_CREDO_DISK_PARTITION => match crate::kernel::services::storage::disk::disk_partition(a0 as u32, a1) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            },
            SYS_CREDO_FAT_FORMAT => match crate::kernel::services::storage::disk::fat_format(a0 as u32) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            },

            // ==================== POSIX Timer (T5-1: 从 framework 回退迁移) ====================
            QX_TIMER_CREATE => crate::kernel::framework::syscall::api::sys_timer_create(a0, a1, a2),
            QX_TIMER_SETTIME => crate::kernel::framework::syscall::api::sys_timer_settime(a0, a1, a2, a3),
            QX_TIMER_GETTIME => crate::kernel::framework::syscall::api::sys_timer_gettime(a0, a1),
            QX_TIMER_DELETE => crate::kernel::framework::syscall::api::sys_timer_delete(a0),
            QX_TIMER_GETOVERRUN => crate::kernel::framework::syscall::api::sys_timer_getoverrun(a0),
            QX_CLOCK_GETRES => crate::kernel::framework::syscall::api::sys_clock_getres(a0, a1),

            // ==================== 熵源 / Stack Canary (T5-1: 从 framework 回退迁移) ====================
            QX_GETRANDOM => crate::kernel::framework::syscall::api::sys_getrandom(a0, a1, a2),
            QX_GET_CANARY => crate::kernel::framework::syscall::api::sys_get_canary(a0, a1),

            // ==================== NUMA (T5-1: 从 framework 回退迁移) ====================
            QX_GET_MEMPOLICY => crate::kernel::services::mm::numa::sys_get_mempolicy(a0, a1),
            QX_SET_MEMPOLICY => crate::kernel::services::mm::numa::sys_set_mempolicy(a0, a1),
            QX_MIGRATE_PAGES => crate::kernel::services::mm::numa::sys_migrate_pages(a0),
            QX_GETCPU => crate::kernel::services::mm::numa::sys_getcpu(),

            // 未迁移的 syscall — 返回 -ENOSYS 让 framework 回退处理
            _ => -38,
        }
    }
}

// ============================================================================
// 注册
// ============================================================================

/// 注册 services 层分发策略到 framework
pub fn register_services_dispatch() -> Result<(), ()> {
    static POLICY: ServicesSyscallDispatch = ServicesSyscallDispatch;
    register_syscall_dispatch(&POLICY).map_err(|_| ())
}
