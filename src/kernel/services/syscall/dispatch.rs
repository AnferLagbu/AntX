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
            SYS_open => as_ret(crate::kernel::services::fs::open::open_syscall(a0, a1 as i32, a2 as i32)),
            SYS_close => as_ret(crate::kernel::services::fs::open::close_syscall(a0 as i32)),
            SYS_stat => as_ret(crate::kernel::services::fs::stat::stat_syscall(a0, a1)),
            SYS_fstat => as_ret(crate::kernel::services::fs::stat::fstat_syscall(a0 as i32, a1)),
            SYS_lstat => as_ret(crate::kernel::services::fs::stat::lstat_syscall(a0, a1)),
            SYS_creat => as_ret(crate::kernel::services::fs::open::creat_syscall(a0, a2 as i32)),

            // ==================== 文件系统操作 (已迁移) ====================
            SYS_mkdir => as_ret(crate::kernel::services::fs::mode::mkdir_syscall(a0, a1 as i32)),
            SYS_rmdir => as_ret(crate::kernel::services::fs::mode::rmdir_syscall(a0)),
            SYS_chmod => as_ret(crate::kernel::services::fs::mode::chmod_syscall(a0, a1 as u32)),
            SYS_fchmod => as_ret(crate::kernel::services::fs::mode::fchmod_syscall(a0 as i32, a1 as u32)),
            SYS_umask => as_ret(crate::kernel::services::fs::mode::umask_syscall(a0 as u32)),
            SYS_access => as_ret(crate::kernel::services::fs::access::access_syscall(a0, a1 as i32)),
            SYS_unlink => as_ret(crate::kernel::services::fs::access::unlink_syscall(a0)),
            SYS_rename => as_ret(crate::kernel::services::fs::misc::rename_syscall(a0, a1)),
            SYS_symlink => as_ret(crate::kernel::services::fs::link::symlink_syscall(a0, a1)),
            SYS_readlink => as_ret(crate::kernel::services::fs::link::readlink_syscall(a0, a1, a2)),

            // *at() 系列 (简化: dirfd 暂不支持, 行为同基本版本)
            SYS_openat => as_ret(crate::kernel::services::fs::open::open_syscall(a1, a2 as i32, a3 as i32)),
            SYS_newfstatat => as_ret(crate::kernel::services::fs::stat::fstat_syscall(a1 as i32, a2)),
            SYS_unlinkat => as_ret(crate::kernel::services::fs::access::unlink_syscall(a1)),
            SYS_renameat => as_ret(crate::kernel::services::fs::misc::rename_syscall(a1, a3)),
            SYS_linkat => as_ret(crate::kernel::services::fs::link::link_syscall(a1, a3)),
            SYS_symlinkat => as_ret(crate::kernel::services::fs::link::symlink_syscall(a0, a2)),
            SYS_readlinkat => as_ret(crate::kernel::services::fs::link::readlink_syscall(a1, a2, a3)),
            SYS_fchmodat => as_ret(crate::kernel::services::fs::mode::chmod_syscall(a1, a2 as u32)),
            SYS_faccessat => as_ret(crate::kernel::services::fs::access::faccessat_syscall(a0 as i32, a1, a2 as i32, a3 as i32)),
            SYS_fchown => as_ret(crate::kernel::services::fs::misc::fchown_syscall(a0 as i32, a1, a2)),
            SYS_sync => as_ret(crate::kernel::services::fs::misc::sync_syscall()),
            SYS_fsync => as_ret(crate::kernel::services::fs::misc::fsync_syscall(a0 as i32)),
            SYS_mount => as_ret(crate::kernel::services::fs::mount::mount_syscall(a0, a1, a2)),
            SYS_umount2 => as_ret(crate::kernel::services::fs::mount::umount2_syscall(a0, a1 as i32)),
            SYS_getcwd => as_ret(crate::kernel::services::fs::path::getcwd_syscall(a0, a1)),
            SYS_chdir => as_ret(crate::kernel::services::fs::path::chdir_syscall(a0)),

            // ==================== 文件描述符操作 (已迁移) ====================
            SYS_pipe => as_ret(crate::kernel::services::fs::io::pipe_syscall(a0)),
            SYS_pipe2 => as_ret(crate::kernel::services::fs::io::pipe_syscall(a0)),
            SYS_dup => as_ret(crate::kernel::services::fs::io::dup_syscall(a0 as i32)),
            SYS_dup2 => as_ret(crate::kernel::services::fs::io::dup2_syscall(a0 as i32, a1 as i32)),
            SYS_dup3 => as_ret(crate::kernel::services::fs::io::dup2_syscall(a0 as i32, a1 as i32)),
            SYS_fcntl => as_ret(crate::kernel::services::fs::io::fcntl_syscall(a0 as i32, a1 as i32, a2)),

            // ==================== 内存管理 (已迁移) ====================
            SYS_mprotect => as_ret(crate::kernel::services::mm::mprotect::mprotect_syscall(a0, a1, a2 as i32)),
            SYS_brk => as_ret(crate::kernel::services::mm::brk::brk_syscall(a0)),

            // ==================== 进程信息 (已迁移) ====================
            SYS_getpid => crate::kernel::services::proc::info::getpid_syscall() as i64,
            SYS_getppid => crate::kernel::services::proc::info::getppid_syscall() as i64,
            SYS_getpgid => as_ret(crate::kernel::services::proc::info::getpgid_syscall(a0 as i32)),
            SYS_gettid => crate::kernel::services::proc::info::gettid_syscall() as i64,
            SYS_setsid => crate::kernel::services::proc::session::proc_setsid(),
            SYS_getsid => crate::kernel::services::proc::session::proc_getsid(a0 as i32),
            SYS_setpgid => crate::kernel::services::proc::session::proc_setpgid(a0 as i32, a1 as i32),

            // ==================== 信号 (已迁移) ====================
            SYS_rt_sigaction => as_ret(crate::kernel::services::proc::signal::rt_sigaction_syscall(a0 as i32, a1, a2)),
            SYS_rt_sigprocmask => as_ret(crate::kernel::services::proc::signal::rt_sigprocmask_syscall(a0 as i32, a1, a2)),
            SYS_kill => as_ret(crate::kernel::services::proc::signal::kill_syscall(a0 as i32, a1 as i32)),

            // ==================== 网络 (已迁移) ====================
            SYS_socket => as_ret(crate::kernel::services::net::syscall::socket_syscall(a0 as i32, a1 as i32, a2 as i32)),
            SYS_connect => as_ret(crate::kernel::services::net::syscall::connect_syscall(a0 as i32, a1, a2 as u32)),
            SYS_accept => as_ret(crate::kernel::services::net::syscall::accept_syscall(a0 as i32, a1, a2)),
            SYS_sendto => as_ret(crate::kernel::services::net::syscall::sendto_syscall(a0 as i32, a1, a2 as u32, a3 as i32, args[4], args[5] as u32)),
            SYS_recvfrom => as_ret(crate::kernel::services::net::syscall::recvfrom_syscall(a0 as i32, a1, a2 as u32, a3 as i32, args[4], args[5])),
            SYS_shutdown => as_ret(crate::kernel::services::net::syscall::shutdown_syscall(a0 as i32, a1 as i32)),
            SYS_bind => as_ret(crate::kernel::services::net::syscall::bind_syscall(a0 as i32, a1, a2 as u32)),
            SYS_listen => as_ret(crate::kernel::services::net::syscall::listen_syscall(a0 as i32, a1 as i32)),
            SYS_sendmsg => as_ret(crate::kernel::services::net::syscall::sendmsg_syscall(a0 as i32, a1, a2 as i32)),
            SYS_recvmsg => as_ret(crate::kernel::services::net::syscall::recvmsg_syscall(a0 as i32, a1, a2 as i32)),
            SYS_setsockopt => as_ret(crate::kernel::services::net::syscall::setsockopt_syscall(a0 as i32, a1 as i32, a2 as i32, a3, a4 as u32)),
            SYS_getsockopt => as_ret(crate::kernel::services::net::syscall::getsockopt_syscall(a0 as i32, a1 as i32, a2 as i32, a3, a4)),

            // ==================== 凭证 (已迁移) ====================
            SYS_getuid => as_ret(crate::kernel::services::credo::uid::getuid_syscall()),
            SYS_getgid => as_ret(crate::kernel::services::credo::uid::getgid_syscall()),
            SYS_setuid => as_ret(crate::kernel::services::credo::uid::setuid_syscall(a0 as u32)),
            SYS_setgid => as_ret(crate::kernel::services::credo::uid::setgid_syscall(a0 as u32)),
            SYS_geteuid => as_ret(crate::kernel::services::credo::uid::geteuid_syscall()),
            SYS_getegid => as_ret(crate::kernel::services::credo::uid::getegid_syscall()),
            SYS_seteuid => as_ret(crate::kernel::services::credo::uid::seteuid_syscall(a0 as u32)),
            SYS_setegid => as_ret(crate::kernel::services::credo::uid::setegid_syscall(a0 as u32)),
            SYS_setreuid => as_ret(crate::kernel::services::credo::uid::setreuid_syscall(a0 as u32, a1 as u32)),

            // ==================== 进程优先级 (已迁移) ====================
            SYS_nice => crate::kernel::services::proc::priority::nice_syscall(a0 as i32),
            SYS_getpriority => crate::kernel::services::proc::priority::getpriority_syscall(a0 as i32, a1 as u32),
            SYS_setpriority => crate::kernel::services::proc::priority::setpriority_syscall(a0 as i32, a1 as u32, a2 as i32),

            // ==================== CPU 亲和性 (已迁移) ====================
            SYS_sched_setaffinity => crate::kernel::services::proc::affinity::sched_setaffinity_syscall(a0 as i32, a1 as u32, a2),
            SYS_sched_getaffinity => crate::kernel::services::proc::affinity::sched_getaffinity_syscall(a0 as i32, a1 as u32, a2),

            // ==================== 进程生命周期 (已迁移) ====================
            SYS_fork => crate::kernel::services::proc::lifecycle::fork_syscall(),
            SYS_exit => crate::kernel::services::proc::lifecycle::exit_syscall(a0 as i32),
            SYS_exit_group => crate::kernel::services::proc::lifecycle::exit_syscall(a0 as i32),
            SYS_sched_yield => crate::kernel::services::proc::lifecycle::sched_yield_syscall(),

            // ==================== 同步原语 (已迁移) ====================
            SYS_futex => {
                match crate::kernel::services::sync::futex::futex_syscall(a0, a1 as i32, a2 as i32, a3, a4 as u32) {
                    Ok(crate::kernel::services::sync::futex::FutexResult::Woken) => 0,
                    Ok(crate::kernel::services::sync::futex::FutexResult::WokenCount(n)) => n as i64,
                    Ok(crate::kernel::services::sync::futex::FutexResult::Requeued { woken, .. }) => woken as i64,
                    Ok(crate::kernel::services::sync::futex::FutexResult::Pending) => 0,
                    Err(e) => e.as_ret(),
                }
            }

            // ==================== 系统信息 (已迁移) ====================
            SYS_getrusage => crate::kernel::services::proc::sysinfo::getrusage_syscall(a0 as i32, a1),
            SYS_sysinfo => crate::kernel::services::proc::sysinfo::sysinfo_syscall(a0),
            SYS_getrlimit => crate::kernel::services::proc::sysinfo::getrlimit_syscall(a0 as i32, a1),
            SYS_clock_gettime => crate::kernel::services::fs::file_ops::clock_gettime_syscall(a0 as i32, a1),

            // ==================== 文件操作 (已迁移) ====================
            SYS_ioctl => crate::kernel::services::fs::file_ops::ioctl_syscall(a0 as i32, a1, a2),
            SYS_poll => crate::kernel::services::fs::file_ops::poll_syscall(a0, a1 as u32, a2 as i32),
            SYS_select => crate::kernel::services::fs::file_ops::poll_syscall(a0, a1 as u32, a2 as i32),
            SYS_chown => crate::kernel::services::fs::file_ops::chown_syscall(a0, a1 as u32, a2 as u32),
            SYS_truncate => crate::kernel::services::fs::file_ops::truncate_syscall(a0, a1 as i64),
            SYS_ftruncate => crate::kernel::services::fs::file_ops::ftruncate_syscall(a0 as i32, a1 as i64),
            SYS_flock => crate::kernel::services::fs::file_ops::flock_syscall(a0 as i32, a1 as i32),
            SYS_lseek => crate::kernel::services::fs::dir_ops::lseek_syscall(a0 as i32, a1 as i64, a2 as i32),
            SYS_getdents => crate::kernel::services::fs::dir_ops::getdents_syscall(a0 as i32, a1, a2),

            // ==================== inotify (已迁移) ====================
            SYS_inotify_init1 => crate::kernel::services::fs::inotify::sys_inotify_init1(a0 as i32),
            SYS_inotify_add_watch => crate::kernel::services::fs::inotify::sys_inotify_add_watch(a0 as i64, a1 as u32, a2 as u32),
            SYS_inotify_rm_watch => crate::kernel::services::fs::inotify::sys_inotify_rm_watch(a0 as i64, a1 as i32),

            // ==================== 内存建议与锁定 (已迁移) ====================
            SYS_mmap => crate::kernel::services::mm::mmap::mmap_syscall_entry(a0, a1, a2 as i32, a3 as i32, a4 as i32, a5),
            SYS_munmap => crate::kernel::services::mm::mmap::munmap_syscall_entry(a0, a1),
            SYS_madvise => crate::kernel::services::mm::madvise_mlock::sys_madvise(a0, a1, a2),
            SYS_mlock => crate::kernel::services::mm::madvise_mlock::sys_mlock(a0, a1),
            SYS_munlock => crate::kernel::services::mm::madvise_mlock::sys_munlock(a0, a1),
            SYS_mlockall => crate::kernel::services::mm::madvise_mlock::sys_mlockall(a0),
            SYS_munlockall => crate::kernel::services::mm::madvise_mlock::sys_munlockall(),
            SYS_mincore => crate::kernel::services::mm::madvise_mlock::sys_mincore(a0, a1, a2),

            // ==================== 定时器 (已迁移) ====================
            SYS_nanosleep => as_ret(crate::kernel::services::proc::sleep::nanosleep_syscall(a0, a1)),
            SYS_getitimer => as_ret(crate::kernel::services::fs::misc::getitimer_syscall(a0 as i32, a1)),
            SYS_alarm => as_ret(crate::kernel::services::fs::misc::alarm_syscall(a0 as u32)),
            SYS_setitimer => as_ret(crate::kernel::services::fs::misc::setitimer_syscall(a0 as i32, a1, a2)),

            // ==================== 进程创建/等待 (已迁移) ====================
            SYS_clone => as_ret(crate::kernel::services::proc::clone::clone_syscall(a0, a1, a2, a3, a4)),
            SYS_wait4 => as_ret(crate::kernel::services::proc::wait4::wait4_syscall(a0 as i32, a1, a2 as i32)),

            // ==================== 系统信息 (已迁移) ====================
            SYS_uname => as_ret(crate::kernel::services::proc::info::uname_syscall(a0)),
            SYS_gettimeofday => as_ret(crate::kernel::services::proc::info::gettimeofday_syscall(a0)),

            // ==================== 文件操作 (已迁移) ====================
            SYS_link => as_ret(crate::kernel::services::fs::link::link_syscall(a0, a1)),
            SYS_times => as_ret(crate::kernel::services::fs::misc::times_syscall(a0)),
            SYS_time => as_ret(crate::kernel::services::fs::misc::time_syscall(a0)),

            // ==================== 事件轮询 (已迁移) ====================
            SYS_epoll_create => as_ret(crate::kernel::services::sync::epoll::epoll_create_syscall(a0 as i32)),
            SYS_epoll_create1 => as_ret(crate::kernel::services::sync::epoll::epoll_create_syscall(a0 as i32)),
            SYS_epoll_ctl => as_ret(crate::kernel::services::sync::epoll::epoll_ctl_syscall(a0 as i64, a1 as i32, a2 as i32, a3)),
            SYS_epoll_wait => as_ret(crate::kernel::services::sync::epoll::epoll_wait_syscall(a0 as i64, a1, a2 as i32, a3 as i32)),

            // ==================== eventfd / signalfd / timerfd (已迁移) ====================
            SYS_eventfd => as_ret(crate::kernel::services::sync::eventfd::eventfd_syscall(a0, a1 as i32)),
            SYS_eventfd2 => as_ret(crate::kernel::services::sync::eventfd::eventfd_syscall(a0, a1 as i32)),

            // memfd_create
            SYS_memfd_create => as_ret(crate::kernel::services::proc::memfd::memfd_create_syscall(a0, a1 as u32)),
            SYS_signalfd => as_ret(crate::kernel::services::sync::signalfd::signalfd_syscall(a0 as i32, a1, a2 as i32)),
            SYS_signalfd4 => as_ret(crate::kernel::services::sync::signalfd::signalfd_syscall(a0 as i32, a1, a2 as i32)),
            SYS_timerfd_create => as_ret(crate::kernel::services::sync::timerfd::timerfd_create_syscall(a0 as i32, a1 as i32)),
            SYS_timerfd_settime => as_ret(crate::kernel::services::sync::timerfd::timerfd_settime_syscall(a0 as i32, a1 as i32, a2, a3)),
            SYS_timerfd_gettime => as_ret(crate::kernel::services::sync::timerfd::timerfd_gettime_syscall(a0 as i32, a1)),

            // ==================== pidfd 系统调用 (已迁移) ====================
            SYS_pidfd_open => as_ret(crate::kernel::services::proc::pidfd::pidfd_open(a0 as u32, a1 as u32)),
            SYS_pidfd_send_signal => as_ret(crate::kernel::services::proc::pidfd::pidfd_send_signal(a0 as u32, a1 as i32, a2, a3 as u32)),
            SYS_pidfd_getfd => as_ret(crate::kernel::services::proc::pidfd::pidfd_getfd(a0 as u32, a1 as u32, a2 as u32)),

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
            SYS_timer_create => crate::kernel::framework::syscall::api::sys_timer_create(a0, a1, a2),
            SYS_timer_settime => crate::kernel::framework::syscall::api::sys_timer_settime(a0, a1, a2, a3),
            SYS_timer_gettime => crate::kernel::framework::syscall::api::sys_timer_gettime(a0, a1),
            SYS_timer_delete => crate::kernel::framework::syscall::api::sys_timer_delete(a0),
            SYS_timer_getoverrun => crate::kernel::framework::syscall::api::sys_timer_getoverrun(a0),
            SYS_clock_getres => crate::kernel::framework::syscall::api::sys_clock_getres(a0, a1),

            // ==================== 熵源 / Stack Canary (T5-1: 从 framework 回退迁移) ====================
            SYS_getrandom => crate::kernel::framework::syscall::api::sys_getrandom(a0, a1, a2),
            QX_GET_CANARY => crate::kernel::framework::syscall::api::sys_get_canary(a0, a1),

            // ==================== NUMA (T5-1: 从 framework 回退迁移) ====================
            SYS_get_mempolicy => crate::kernel::services::mm::numa::sys_get_mempolicy(a0, a1),
            SYS_set_mempolicy => crate::kernel::services::mm::numa::sys_set_mempolicy(a0, a1),
            SYS_migrate_pages => crate::kernel::services::mm::numa::sys_migrate_pages(a0),
            SYS_getcpu => crate::kernel::services::mm::numa::sys_getcpu(),

            // ==================== copy_file_range / name_to_handle ====================
            SYS_copy_file_range => as_ret(crate::kernel::services::fs::io::copy_file_range_syscall(
                a0 as i32, a1, a2 as i32, a3, a4 as usize,
            )),
            SYS_name_to_handle_at => {
                // TODO: 实现 name_to_handle_at
                // 需要: 文件句柄系统
                Errno::ENOSYS.as_ret()
            },
            SYS_open_by_handle_at => {
                // TODO: 实现 open_by_handle_at
                // 需要: 文件句柄系统
                Errno::ENOSYS.as_ret()
            },

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
    let r = register_syscall_dispatch(&POLICY);
    crate::kernel::framework::klog::log_info(
        crate::kernel::framework::klog::LogCategory::Boot,
        format_args!("[SYSCALL] register_services_dispatch result={}", if r.is_ok() { "OK" } else { "ERR" }),
    );
    r.map_err(|_| ())
}
