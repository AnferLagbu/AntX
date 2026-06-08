#![allow(dead_code)]
pub mod api;
pub mod brk;
pub mod clone;
pub mod epoll;
pub mod futex;
pub mod info;
pub mod io;
pub mod mmap;
pub mod mprotect;
pub mod wait4;

/// Syscall 模块 — POSIX 原生系统调用分发
///
/// POSIX 标准 syscall 编号 (0-399) + Credo 私有 syscall (400+).
/// 内核能力层 (VFS/PWM/PROC/NET/MM) 不变，仅 syscall ABI 层替换。
pub mod types;

#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::idt::types::InterruptFrame;
use crate::kernel::framework::syscall::types::*;
use core::sync::atomic::Ordering;

const USER_ADDR_MAX: u64 = 0x7FFFFFFFE000;

pub fn validate_user_ptr(ptr: u64) -> bool {
    ptr > 0 && ptr < USER_ADDR_MAX
}

pub fn validate_user_buf(ptr: u64, len: u64) -> bool {
    if len == 0 {
        return true;
    }
    validate_user_ptr(ptr) && ptr + len <= USER_ADDR_MAX
}

#[no_mangle]
///
/// # Safety
///
/// Caller is in kernel context. `ptr` is a validated user-space pointer.
pub unsafe extern "C" fn syscall_init() {
    // SAFETY: klog_write 是 C-ABI 日志函数；byte string literal 是 'static
    // 字节切片，传递给 C 时按指针 + 长度使用。
    unsafe {
        crate::kernel::framework::klog::klog_write(
            1,
            7,
            core::ptr::null(),
            core::ptr::null(),
            0,
            b"POSIX syscall subsystem ready".as_ptr(),
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[no_mangle]
///
/// # Safety
///
/// Caller is in kernel context. `ptr` is a validated user-space string pointer.
pub unsafe extern "C" fn syscall_dispatch_from_frame(frame: *mut InterruptFrame) {
    if frame.is_null() {
        return;
    }
    let f = &mut *frame;
    let syscall_num = f.rax;

    // rt_sigreturn 特殊处理: 需要直接修改 frame, 不走正常 dispatch
    if syscall_num == SYS_rt_sigreturn {
        // 从用户栈上的 SignalFrame 恢复寄存器
        // 布局: rsp+0=返回地址, rsp+8=SignalFrame
        let sigframe_ptr = (f.rsp + 8) as *const crate::kernel::framework::proc::signal::SignalFrame;
        if !sigframe_ptr.is_null() {
            let sigframe = core::ptr::read_unaligned(sigframe_ptr);
            f.r15 = sigframe.r15;
            f.r14 = sigframe.r14;
            f.r13 = sigframe.r13;
            f.r12 = sigframe.r12;
            f.r11 = sigframe.r11;
            f.r10 = sigframe.r10;
            f.r9 = sigframe.r9;
            f.r8 = sigframe.r8;
            f.rdi = sigframe.rdi;
            f.rsi = sigframe.rsi;
            f.rbp = sigframe.rbp;
            f.rdx = sigframe.rdx;
            f.rcx = sigframe.rcx;
            f.rbx = sigframe.rbx;
            f.rax = sigframe.rax;
            f.rip = sigframe.rip;
            f.cs = sigframe.cs;
            f.rflags = sigframe.rflags;
            f.rsp = sigframe.rsp;
            f.ss = sigframe.ss;
        }
        // sigreturn 不返回值, rax 保持原值
        return;
    }

    let a0 = f.rdi;
    let a1 = f.rsi;
    let a2 = f.rdx;
    let a3 = f.r10;
    let a4 = f.r8;
    let a5 = f.r9;
    let result = syscall_dispatch(syscall_num, a0, a1, a2, a3, a4, a5);
    f.rax = result as u64;

    // 返回用户态前检查待投递信号
    // SAFETY: frame 有效, 当前在当前 CPU 的 syscall 上下文
    crate::kernel::framework::proc::signal::do_signal_deliver(frame);
}

macro_rules! dispatch {
    ($num:expr, $name:expr) => {{
        let ret = $num;
        // SAFETY: klog_write 是 C-ABI 日志函数，$name 是 Rust 静态字符串
        // (字节切片)，传给 C 时按指针 + 长度传递。
        unsafe {
            crate::kernel::framework::klog::klog_write(
                0,
                7,
                core::ptr::null(),
                core::ptr::null(),
                0,
                $name.as_ptr() as *const u8,
            );
        }
        ret
    }};
}

#[no_mangle]
///
/// # Safety
///
/// Called from interrupt context (int 0x80). All register values come from the interrupted user context.
pub unsafe extern "C" fn syscall_dispatch(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    match num {
        // ==================== 文件 I/O ====================
        SYS_read => dispatch!(sys_read(a0 as i32, a1 as *mut u8, a2), b"read\0"),
        SYS_write => dispatch!(sys_write(a0 as i32, a1 as *const u8, a2), b"write\0"),
        SYS_open => dispatch!(
            match crate::kernel::services::fs::open::open_syscall(a0, a1 as i32, a2 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"open\0"
        ),
        SYS_close => dispatch!(
            match crate::kernel::services::fs::open::close_syscall(a0 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"close\0"
        ),
        SYS_stat => dispatch!(
            match crate::kernel::services::fs::stat::stat_syscall(a0, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"stat\0"
        ),
        SYS_fstat => dispatch!(
            match crate::kernel::services::fs::stat::fstat_syscall(a0 as i32, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"fstat\0"
        ),
        SYS_lstat => dispatch!(
            match crate::kernel::services::fs::stat::lstat_syscall(a0, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"lstat\0"
        ),
        SYS_poll => dispatch!(
            sys_poll(a0 as *mut u8, a1 as u32, a2 as i32),
            b"poll\0"
        ),
        SYS_lseek => dispatch!(sys_lseek(a0 as i32, a1 as i64, a2 as i32), b"lseek\0"),

        // ==================== 内存管理 ====================
        SYS_mmap => dispatch!(sys_mmap(a0, a1, a2 as i32, a3 as i32, a4 as i32, a5), b"mmap\0"),
        SYS_mprotect => dispatch!(
            match crate::kernel::services::mm::mprotect::mprotect_syscall(a0, a1, a2 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"mprotect\0"
        ),
        SYS_munmap => dispatch!(sys_munmap(a0, a1), b"munmap\0"),
        SYS_brk => dispatch!(
            match crate::kernel::services::mm::brk::brk_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"brk\0"
        ),
        SYS_mremap => {
            // 从当前 task 取 MmStruct; 验证后委托 services/mm/mremap
            use crate::kernel::framework::mm::vma::get_current_mm;
            match get_current_mm() {
                Some(mm) => {
                    dispatch!(
                        match crate::kernel::services::mm::mremap::mremap_syscall(
                            mm, a0, a1, a2, a3 as i32,
                        ) {
                            Ok(addr) => addr as i64,
                            Err(e) => e.as_ret(),
                        },
                        b"mremap\0"
                    )
                }
                None => -1, // EFAULT
            }
        }

        // ==================== 信号 ====================
        SYS_rt_sigaction => dispatch!(
            match crate::kernel::services::proc::signal::rt_sigaction_syscall(a0 as i32, a1, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"rt_sigaction\0"
        ),
        SYS_rt_sigprocmask => dispatch!(
            match crate::kernel::services::proc::signal::rt_sigprocmask_syscall(a0 as i32, a1, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"rt_sigprocmask\0"
        ),
        SYS_rt_sigreturn => dispatch!(sys_rt_sigreturn(), b"rt_sigreturn\0"),

        // ==================== 设备 ====================
        SYS_ioctl => dispatch!(sys_ioctl(a0 as i32, a1, a2), b"ioctl\0"),

        // ==================== 文件访问 ====================
        SYS_access => dispatch!(
            match crate::kernel::services::fs::access::access_syscall(a0, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"access\0"
        ),
        SYS_pipe => dispatch!(
            match crate::kernel::services::fs::io::pipe_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"pipe\0"
        ),
        SYS_select => dispatch!(
            sys_poll(a0 as *mut u8, a1 as u32, a2 as i32),
            b"select\0"
        ),
        SYS_sched_yield => dispatch!(sys_sched_yield(), b"sched_yield\0"),
        SYS_sched_setaffinity => dispatch!(
            sys_sched_setaffinity(a0 as i32, a1 as u32, a2),
            b"sched_setaffinity\0"
        ),
        SYS_sched_getaffinity => dispatch!(
            sys_sched_getaffinity(a0 as i32, a1 as u32, a2),
            b"sched_getaffinity\0"
        ),

        // ==================== 文件描述符 ====================
        SYS_dup => dispatch!(
            match crate::kernel::services::fs::io::dup_syscall(a0 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"dup\0"
        ),
        SYS_dup2 => dispatch!(
            match crate::kernel::services::fs::io::dup2_syscall(a0 as i32, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"dup2\0"
        ),

        // ==================== 进程优先级 ====================
        SYS_nice => dispatch!(sys_nice(a0 as i32), b"nice\0"),

        // ==================== 定时器 ====================
        SYS_nanosleep => dispatch!(
            match crate::kernel::services::proc::sleep::nanosleep_syscall(a0, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"nanosleep\0"
        ),
        SYS_getitimer => dispatch!(
            match crate::kernel::services::fs::misc::getitimer_syscall(a0 as i32, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getitimer\0"
        ),
        SYS_alarm => dispatch!(
            match crate::kernel::services::fs::misc::alarm_syscall(a0 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"alarm\0"
        ),
        SYS_setitimer => dispatch!(
            match crate::kernel::services::fs::misc::setitimer_syscall(a0 as i32, a1, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setitimer\0"
        ),

        // ==================== 进程 ====================
        SYS_getpid => dispatch!(
            crate::kernel::services::proc::info::getpid_syscall() as i64,
            b"getpid\0"
        ),
        SYS_getppid => dispatch!(
            crate::kernel::services::proc::info::getppid_syscall() as i64,
            b"getppid\0"
        ),
        SYS_getpgid => dispatch!(
            match crate::kernel::services::proc::info::getpgid_syscall(a0 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getpgid\0"
        ),
        SYS_setsid => dispatch!(
            match crate::kernel::services::proc::session::setsid_syscall() {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setsid\0"
        ),
        SYS_getsid => dispatch!(
            match crate::kernel::services::proc::session::getsid_syscall(a0 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getsid\0"
        ),
        SYS_setpgid => dispatch!(
            match crate::kernel::services::proc::session::setpgid_syscall(a0 as i32, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setpgid\0"
        ),
        SYS_getpriority => dispatch!(sys_getpriority(a0 as i32, a1 as u32), b"getpriority\0"),
        SYS_setpriority => dispatch!(
            sys_setpriority(a0 as i32, a1 as u32, a2 as i32),
            b"setpriority\0"
        ),
        SYS_gettid => dispatch!(
            crate::kernel::services::proc::info::gettid_syscall() as i64,
            b"gettid\0"
        ),

        // ==================== 网络 (services 代理) ====================
        #[cfg(feature = "net")]
        SYS_socket => dispatch!(
            match crate::kernel::services::net::syscall::socket_syscall(a0 as i32, a1 as i32, a2 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"socket\0"
        ),
        #[cfg(feature = "net")]
        SYS_connect => dispatch!(
            match crate::kernel::services::net::syscall::connect_syscall(a0 as i32, a1, a2 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"connect\0"
        ),
        #[cfg(feature = "net")]
        SYS_accept => dispatch!(
            match crate::kernel::services::net::syscall::accept_syscall(a0 as i32, a1, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"accept\0"
        ),
        #[cfg(feature = "net")]
        SYS_sendto => dispatch!(
            match crate::kernel::services::net::syscall::sendto_syscall(a0 as i32, a1, a2 as u32, a3 as i32, a4, a5 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"sendto\0"
        ),
        #[cfg(feature = "net")]
        SYS_recvfrom => dispatch!(
            match crate::kernel::services::net::syscall::recvfrom_syscall(a0 as i32, a1, a2 as u32, a3 as i32, a4, a5) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"recvfrom\0"
        ),
        #[cfg(feature = "net")]
        SYS_shutdown => dispatch!(
            match crate::kernel::services::net::syscall::shutdown_syscall(a0 as i32, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"shutdown\0"
        ),
        #[cfg(feature = "net")]
        SYS_bind => dispatch!(
            match crate::kernel::services::net::syscall::bind_syscall(a0 as i32, a1, a2 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"bind\0"
        ),
        #[cfg(feature = "net")]
        SYS_listen => dispatch!(
            match crate::kernel::services::net::syscall::listen_syscall(a0 as i32, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"listen\0"
        ),
        #[cfg(feature = "net")]
        SYS_sendmsg => dispatch!(
            match crate::kernel::services::net::syscall::sendmsg_syscall(a0 as i32, a1, a2 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"sendmsg\0"
        ),
        #[cfg(feature = "net")]
        SYS_recvmsg => dispatch!(
            match crate::kernel::services::net::syscall::recvmsg_syscall(a0 as i32, a1, a2 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"recvmsg\0"
        ),
        #[cfg(feature = "net")]
        SYS_setsockopt => dispatch!(
            match crate::kernel::services::net::syscall::setsockopt_syscall(a0 as i32, a1 as i32, a2 as i32, a3, a4 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setsockopt\0"
        ),
        #[cfg(feature = "net")]
        SYS_getsockopt => dispatch!(
            match crate::kernel::services::net::syscall::getsockopt_syscall(a0 as i32, a1 as i32, a2 as i32, a3, a4) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getsockopt\0"
        ),
        #[cfg(feature = "net")]
        SYS_getsockname => dispatch!(sys_getsockname(a0 as i32, a1, a2), b"getsockname\0"),
        #[cfg(feature = "net")]
        SYS_getpeername => dispatch!(sys_getpeername(a0 as i32, a1, a2), b"getpeername\0"),
        #[cfg(feature = "net")]
        SYS_getrusage => dispatch!(sys_getrusage(a0 as i32, a1), b"getrusage\0"),
        #[cfg(not(feature = "net"))]
        SYS_socket | SYS_connect | SYS_accept | SYS_sendto | SYS_recvfrom | SYS_shutdown
        | SYS_bind | SYS_listen | SYS_sendmsg | SYS_recvmsg | SYS_setsockopt | SYS_getsockopt
        | SYS_getsockname | SYS_getpeername | SYS_getrusage => {
            dispatch!(Errno::ENOSYS.as_ret(), b"net_nosys\0")
        }

        // ==================== 进程创建 ====================
        SYS_fork => dispatch!(sys_fork(), b"fork\0"),
        SYS_clone => dispatch!(
            match crate::kernel::services::proc::clone::clone_syscall(a0, a1, a2, a3, a4) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"clone\0"
        ),
        SYS_execve => dispatch!(
            crate::kernel::services::proc::execve::ExecveResult::from_ret(
                sys_execve(
                    a0 as *const u8,
                    a1 as *const *const u8,
                    a2 as *const *const u8
                )
            ).as_ret(),
            b"execve\0"
        ),
        SYS_exit => dispatch!(sys_exit(a0 as i32), b"exit\0"),
        SYS_wait4 => dispatch!(
            match crate::kernel::services::proc::wait4::wait4_syscall(a0 as i32, a1, a2 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"wait4\0"
        ),
        SYS_kill => dispatch!(
            match crate::kernel::services::proc::signal::kill_syscall(a0 as i32, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"kill\0"
        ),

        // ==================== 系统信息 ====================
        SYS_uname => dispatch!(
            match crate::kernel::services::proc::info::uname_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"uname\0"
        ),

        // ==================== 文件描述符操作 ====================
        SYS_fcntl => dispatch!(
            match crate::kernel::services::fs::io::fcntl_syscall(a0 as i32, a1 as i32, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"fcntl\0"
        ),

        // ==================== 文件截断 ====================
        SYS_truncate => dispatch!(
            sys_truncate(a0 as *const u8, a1 as i64),
            b"truncate\0"
        ),
        SYS_ftruncate => dispatch!(sys_ftruncate(a0 as i32, a1 as i64), b"ftruncate\0"),

        // ==================== 目录 ====================
        SYS_getdents => dispatch!(
            sys_getdents(a0 as i32, a1 as *mut u8, a2),
            b"getdents\0"
        ),

        // ==================== 路径 (services 代理) ====================
        SYS_getcwd => dispatch!(
            match crate::kernel::services::fs::path::getcwd_syscall(a0, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getcwd\0"
        ),
        SYS_chdir => dispatch!(
            match crate::kernel::services::fs::path::chdir_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"chdir\0"
        ),

        // ==================== 文件操作 ====================
        SYS_rename => dispatch!(
            match crate::kernel::services::fs::misc::rename_syscall(a0, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"rename\0"
        ),
        SYS_mkdir => dispatch!(
            match crate::kernel::services::fs::mode::mkdir_syscall(a0, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"mkdir\0"
        ),
        SYS_rmdir => dispatch!(
            match crate::kernel::services::fs::mode::rmdir_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"rmdir\0"
        ),
        SYS_creat => dispatch!(
            match crate::kernel::services::fs::open::creat_syscall(a0, a2 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"creat\0"
        ),
        SYS_link => dispatch!(
            match crate::kernel::services::fs::link::link_syscall(a0, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"link\0"
        ),
        SYS_unlink => dispatch!(
            match crate::kernel::services::fs::access::unlink_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"unlink\0"
        ),
        SYS_symlink => dispatch!(
            match crate::kernel::services::fs::link::symlink_syscall(a0, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"symlink\0"
        ),
        SYS_readlink => dispatch!(
            match crate::kernel::services::fs::link::readlink_syscall(a0, a1, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"readlink\0"
        ),

        // ==================== 文件权限 ====================
        SYS_chmod => dispatch!(
            match crate::kernel::services::fs::mode::chmod_syscall(a0, a1 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"chmod\0"
        ),
        SYS_fchmod => dispatch!(
            match crate::kernel::services::fs::mode::fchmod_syscall(a0 as i32, a1 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"fchmod\0"
        ),
        SYS_chown => dispatch!(
            sys_chown(a0 as *const u8, a1 as u32, a2 as u32),
            b"chown\0"
        ),
        SYS_fchown => dispatch!(
            match crate::kernel::services::fs::misc::fchown_syscall(a0 as i32, a1, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"fchown\0"
        ),
        SYS_umask => dispatch!(
            match crate::kernel::services::fs::mode::umask_syscall(a0 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"umask\0"
        ),

        // ==================== 时间 ====================
        SYS_gettimeofday => dispatch!(
            match crate::kernel::services::proc::info::gettimeofday_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"gettimeofday\0"
        ),
        SYS_getrlimit => dispatch!(
            match crate::kernel::services::proc::rlimit::getrlimit_syscall(a0 as i32, a1) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getrlimit\0"
        ),
        SYS_sysinfo => dispatch!(sys_sysinfo(a0 as *mut u8), b"sysinfo\0"),
        SYS_times => dispatch!(
            match crate::kernel::services::fs::misc::times_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"times\0"
        ),

        // ==================== 用户/组 (services 代理) ====================
        SYS_getuid => dispatch!(
            match crate::kernel::services::credo::uid::getuid_syscall() {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getuid\0"
        ),
        SYS_getgid => dispatch!(
            match crate::kernel::services::credo::uid::getgid_syscall() {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getgid\0"
        ),
        SYS_setuid => dispatch!(
            match crate::kernel::services::credo::uid::setuid_syscall(a0 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setuid\0"
        ),
        SYS_setgid => dispatch!(
            match crate::kernel::services::credo::uid::setgid_syscall(a0 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setgid\0"
        ),
        SYS_geteuid => dispatch!(
            match crate::kernel::services::credo::uid::geteuid_syscall() {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"geteuid\0"
        ),
        SYS_getegid => dispatch!(
            match crate::kernel::services::credo::uid::getegid_syscall() {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"getegid\0"
        ),
        SYS_seteuid => dispatch!(
            match crate::kernel::services::credo::uid::seteuid_syscall(a0 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"seteuid\0"
        ),
        SYS_setegid => dispatch!(
            match crate::kernel::services::credo::uid::setegid_syscall(a0 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setegid\0"
        ),
        SYS_setreuid => dispatch!(
            match crate::kernel::services::credo::uid::setreuid_syscall(a0 as u32, a1 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setreuid\0"
        ),
        SYS_setregid => dispatch!(
            match crate::kernel::services::credo::uid::setregid_syscall(a0 as u32, a1 as u32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"setregid\0"
        ),

        // ==================== 文件同步/挂载 ====================
        SYS_sync => dispatch!(
            match crate::kernel::services::fs::misc::sync_syscall() {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"sync\0"
        ),
        SYS_fsync => dispatch!(
            match crate::kernel::services::fs::misc::fsync_syscall(a0 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"fsync\0"
        ),
        SYS_mount => dispatch!(
            match crate::kernel::services::fs::mount::mount_syscall(a0, a1, a2) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"mount\0"
        ),
        SYS_umount2 => dispatch!(
            match crate::kernel::services::fs::mount::umount2_syscall(a0, a1 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"umount2\0"
        ),

        SYS_time => dispatch!(
            match crate::kernel::services::fs::misc::time_syscall(a0) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"time\0"
        ),
        SYS_clock_gettime => dispatch!(
            sys_clock_gettime(a0 as i32, a1 as *mut u8),
            b"clock_gettime\0"
        ),
        SYS_exit_group => dispatch!(sys_exit(a0 as i32), b"exit_group\0"),
        SYS_tgkill => dispatch!(sys_tgkill(a0 as i32, a1 as i32, a2 as i32), b"tgkill\0"),

        // ==================== 同步 ====================
        SYS_futex => dispatch!(
            match crate::kernel::services::sync::futex::futex_syscall(
                a0, a1 as i32, a2 as i32, a3, 0,
            ) {
                Ok(crate::kernel::services::sync::futex::FutexResult::Woken) => 0,
                Ok(crate::kernel::services::sync::futex::FutexResult::WokenCount(n)) => n as i64,
                Ok(crate::kernel::services::sync::futex::FutexResult::Requeued { woken, .. }) => woken as i64,
                Ok(crate::kernel::services::sync::futex::FutexResult::Pending) => 0,
                Err(e) => e.as_ret(),
            },
            b"futex\0"
        ),

        // ==================== 事件轮询 ====================
        SYS_epoll_create => dispatch!(
            match crate::kernel::services::sync::epoll::epoll_create_syscall(a0 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"epoll_create\0"
        ),
        SYS_epoll_ctl => dispatch!(
            match crate::kernel::services::sync::epoll::epoll_ctl_syscall(a0 as i64, a1 as i32, a2 as i32, a3) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"epoll_ctl\0"
        ),
        SYS_epoll_wait => dispatch!(
            match crate::kernel::services::sync::epoll::epoll_wait_syscall(a0 as i64, a1, a2 as i32, a3 as i32) {
                Ok(v) => v as i64,
                Err(e) => e.as_ret(),
            },
            b"epoll_wait\0"
        ),

        // ==================== Credo 私有 syscall (400+) ====================
        SYS_CREDO_LOGIN => dispatch!(
            sys_auth_login(
                a0 as *const u8,
                a1 as *const u8
            ),
            b"credo_login\0"
        ),
        SYS_CREDO_LOGOUT => dispatch!(sys_auth_logout(), b"credo_logout\0"),
        SYS_CREDO_CREATE_IDENTITY => dispatch!(
            sys_auth_create(
                a0 as *const u8,
                a1 as *const u8,
                a2 as u8
            ),
            b"credo_create\0"
        ),
        SYS_CREDO_DELETE_IDENTITY => dispatch!(sys_auth_delete(a0), b"credo_delete\0"),
        SYS_CREDO_IDENTITY_INFO => dispatch!(sys_auth_info(a0), b"credo_info\0"),
        SYS_CREDO_CHANGE_PASSWORD => dispatch!(
            sys_auth_changepw(
                a0 as *const u8,
                a1 as *const u8
            ),
            b"credo_chpw\0"
        ),
        SYS_CREDO_VERIFY_PASSWORD => dispatch!(
            sys_auth_verify(a0 as *const u8),
            b"credo_verify\0"
        ),
        SYS_CREDO_CREATE_FIRST => dispatch!(
            sys_auth_create_first(a0 as *const u8),
            b"credo_first\0"
        ),
        SYS_CREDO_GRANT => dispatch!(sys_auth_grant(a0, a1, a2 as u16, a3), b"credo_grant\0"),
        SYS_CREDO_REVOKE => dispatch!(sys_auth_revoke(a0, a1, a2 as u16, a3), b"credo_revoke\0"),
        SYS_CREDO_CHECK_CAP => {
            dispatch!(sys_auth_check_cap(a0, a1 as u16, a2), b"credo_checkcap\0")
        }
        SYS_CREDO_GET_CAPS => dispatch!(sys_auth_get_caps(a0, a1 as u16), b"credo_getcaps\0"),
        SYS_CREDO_GET_PWM => dispatch!(sys_pwm_get(), b"credo_getpwm\0"),
        SYS_CREDO_SET_PWM => dispatch!(sys_pwm_set(a0), b"credo_setpwm\0"),

        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_LIST => dispatch!(
            match crate::kernel::services::storage::disk::disk_list(a0, a1 as u32) {
                Ok(n) => n as i64,
                Err(e) => e.as_ret(),
            },
            b"credo_disklist\0"
        ),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_INFO => {
            dispatch!(
                match crate::kernel::services::storage::disk::disk_info(a0 as u32, a1) {
                    Ok(()) => 0,
                    Err(e) => e.as_ret(),
                },
                b"credo_diskinfo\0"
            )
        }
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_FORMAT => dispatch!(
            match crate::kernel::services::storage::disk::disk_format(a0 as u32, a1) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            },
            b"credo_diskfmt\0"
        ),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_PARTITION => {
            dispatch!(
                match crate::kernel::services::storage::disk::disk_partition(a0 as u32, a1) {
                    Ok(()) => 0,
                    Err(e) => e.as_ret(),
                },
                b"credo_diskpart\0"
            )
        }
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_INSTALL => dispatch!(sys_boot_install(a0 as u32), b"credo_diskinst\0"),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_FAT_FORMAT => dispatch!(
            match crate::kernel::services::storage::disk::fat_format(a0 as u32) {
                Ok(()) => 0,
                Err(e) => e.as_ret(),
            },
            b"credo_fatfmt\0"
        ),

        #[cfg(feature = "kernel_test")]
        SYS_CREDO_DISK_LIST
        | SYS_CREDO_DISK_INFO
        | SYS_CREDO_DISK_FORMAT
        | SYS_CREDO_DISK_PARTITION
        | SYS_CREDO_DISK_INSTALL
        | SYS_CREDO_FAT_FORMAT => dispatch!(Errno::ENOSYS.as_ret(), b"credo_disk_nosys\0"),

        SYS_CREDO_PROC_LIST => {
            dispatch!(sys_proc_list(a0 as *mut u8, a1 as u32), b"credo_proclist\0")
        }
        SYS_CREDO_PROC_SETPRI => {
            dispatch!(sys_proc_setpri(a0 as u32, a1 as u32), b"credo_procpri\0")
        }
        SYS_CREDO_PROC_SLEEP => {
            let ms = a0;
            let ns = ms * 1_000_000;
            dispatch!(sys_nanosleep(ns, a1), b"credo_procsleep\0")
        }
        SYS_CREDO_GETHOSTNAME => dispatch!(
            sys_gethostname(a0 as *mut u8, a1),
            b"credo_gethost\0"
        ),
        SYS_CREDO_SETHOSTNAME => dispatch!(
            sys_sethostname(a0 as *const u8, a1),
            b"credo_sethost\0"
        ),
        SYS_CREDO_BOOT_CHECK => dispatch!(sys_boot_check(a0 as i32), b"credo_bootchk\0"),
        SYS_CREDO_REBOOT => dispatch!(sys_reboot(a0 as i32), b"credo_reboot\0"),
        SYS_CREDO_HOTPLUG_STATUS => dispatch!(
            sys_hotplug_status(a0 as *mut u8, a1 as u32),
            b"credo_hotplug_status\0"
        ),
        SYS_CREDO_PROC_CPUTIME => dispatch!(sys_credo_proc_cputime(a0 as u32), b"credo_cputime\0"),

        // ==================== 帧缓冲设备 ====================
        SYS_FB_OPEN => dispatch!(sys_fb_open(a0, a1), b"fb_open\0"),
        SYS_FB_MMAP => dispatch!(sys_fb_mmap(a0, a1, a2), b"fb_mmap\0"),
        SYS_FB_RELEASE => dispatch!(sys_fb_release(a0), b"fb_release\0"),

        _ => Errno::ENOSYS.as_ret(),
    }
}

#[no_mangle]
///
/// # Safety
///
/// 动态注册 syscall 处理器的入口,被 asm stub 间接调用。`_handler` 是
/// Rust 函数指针 (`fn(u64, u64, u64, u64) -> i64`),**不是** C ABI 类型,
/// 因此函数本身必须用 `extern "Rust"` 标记,否则编译器会报
/// `improper_ctypes_definitions`。
pub unsafe extern "Rust" fn syscall_register(_num: u64, _handler: SyscallHandler) {}

// ============================================================================
// 文件 I/O — read / write / open / close
// ============================================================================

fn sys_read(fd: i32, buf: *mut u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(buf as u64, count) {
        return Errno::EFAULT.as_ret();
    }
    if fd == 1 || fd == 2 {
        return Errno::EBADF.as_ret();
    }
    if fd == 0 {
        #[cfg(not(feature = "kernel_test"))]
        {
            #[cfg(target_arch = "x86_64")]
            {
                if let Some(c) = raw::read_keyboard_byte() {
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    unsafe { raw::write_u8(buf, c) };
                    return 1;
                }
            }
            if let Some(c) = raw::read_serial_byte(0) {
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe { raw::write_u8(buf, c) };
                return 1;
            }
        }
        return 0;
    }
    crate::kernel::framework::fs::vfs::api::vfs_read(fd as u32, buf, count as u32) as i64
}

fn sys_write(fd: i32, buf: *const u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(buf as u64, count) {
        return Errno::EFAULT.as_ret();
    }
    if fd == 1 || fd == 2 {
        if count > 0 {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            let data = unsafe { raw::read_slice(buf, count as usize) };
            crate::kernel::framework::klog::serial_write_bytes(data);
        }
        return count as i64;
    }
    crate::kernel::framework::fs::vfs::api::vfs_write(fd as u32, buf, count as u32) as i64
}

fn sys_open(path: *const u8, flags: i32, _mode: i32) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_open(path, flags as u32, pwm) as i64
}

fn sys_close(fd: i32) -> i64 {
    if fd < 0 {
        return Errno::EBADF.as_ret();
    }
    // UDS FD 范围 [100, 116) → 走 UDS close, 不进 VFS
    if fd >= 100 && fd < 116 {
        return match crate::kernel::services::net::unix::close(fd) {
            Ok(()) => 0,
            Err(e) => e.to_errno().as_ret(),
        };
    }
    crate::kernel::framework::fs::vfs::api::vfs_close(fd as u32) as i64
}

fn sys_stat(path: *const u8, st_buf: *mut u8) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    if st_buf.is_null()
        || !raw::check_user_buf(
            st_buf as u64,
            core::mem::size_of::<crate::kernel::framework::fs::vfs::types::VfsStat>() as u64,
        )
    {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_stat(
        path,
        st_buf as *mut crate::kernel::framework::fs::vfs::types::VfsStat,
        pwm,
    ) as i64
}

fn sys_fstat(fd: i32, st_buf: *mut u8) -> i64 {
    if st_buf.is_null()
        || !raw::check_user_buf(
            st_buf as u64,
            core::mem::size_of::<crate::kernel::framework::fs::vfs::types::VfsStat>() as u64,
        )
    {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_fstat(
        fd as u32,
        st_buf as *mut crate::kernel::framework::fs::vfs::types::VfsStat,
        pwm,
    ) as i64
}

fn sys_lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    crate::kernel::framework::fs::vfs::api::vfs_seek(fd as u32, offset as i32, whence as u32) as i64
}

fn sys_getdents(fd: i32, buf: *mut u8, _count: u64) -> i64 {
    crate::kernel::framework::fs::vfs::api::vfs_readdir(
        fd as u32,
        buf as *mut crate::kernel::framework::fs::vfs::types::VfsDirEntry,
    ) as i64
}

// ============================================================================
// 目录/文件操作
// ============================================================================

fn sys_getcwd(buf: *mut u8, size: u64) -> i64 {
    if buf.is_null() || size == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(buf as u64, size) {
        return Errno::EFAULT.as_ret();
    }
    crate::kernel::framework::fs::vfs::api::vfs_get_cwd(buf, size as u32) as i64
}

fn sys_chdir(path: *const u8) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    crate::kernel::framework::fs::vfs::api::vfs_set_cwd(path);
    0
}

fn sys_mkdir(path: *const u8, _mode: i32) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    let pwm = if pwm == 0 { 0x0020F45A8B978417 } else { pwm };
    crate::kernel::framework::fs::vfs::api::vfs_mkdir(path, pwm) as i64
}

fn sys_rmdir(path: *const u8) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_rmdir(path, pwm) as i64
}

fn sys_unlink(path: *const u8) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_unlink(path, pwm) as i64
}

fn sys_rename(old: *const u8, new: *const u8) -> i64 {
    if old.is_null()
        || new.is_null()
        || !raw::check_user_ptr(old as u64)
        || !raw::check_user_ptr(new as u64)
    {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_rename(old, new, pwm) as i64
}

fn sys_access(path: *const u8, _mode: i32) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    let stat_ptr: *mut crate::kernel::framework::fs::vfs::types::VfsStat = unsafe { &mut core::mem::zeroed() };
    let result = crate::kernel::framework::fs::vfs::api::vfs_stat(path, stat_ptr, pwm);
    if result < 0 {
        return result as i64;
    }
    0
}

fn sys_sync() -> i64 {
    crate::kernel::framework::fs::vfs::api::vfs_sync() as i64
}

fn sys_mount(
    _source: *const u8,
    target: *const u8,
    fstype: *const u8,
) -> i64 {
    if target.is_null() || !raw::check_user_ptr(target as u64) {
        return Errno::EFAULT.as_ret();
    }
    if fstype.is_null() {
        return Errno::EINVAL.as_ret();
    }
    crate::kernel::framework::fs::vfs::api::vfs_mount(target, fstype) as i64
}

// ============================================================================
// 进程 — fork / execve / exit / wait
// ============================================================================

fn sys_sched_yield() -> i64 {
    crate::kernel::framework::proc::api::scheduler_yield();
    0
}

const PRIO_PROCESS: i32 = 0;

fn nice_to_priority(nice: i32) -> crate::kernel::framework::proc::types::ProcessPriority {
    let clamped = nice.clamp(-20, 19);
    if clamped < -10 {
        crate::kernel::framework::proc::types::ProcessPriority::RealTime
    } else if clamped < 0 {
        crate::kernel::framework::proc::types::ProcessPriority::High
    } else if clamped < 10 {
        crate::kernel::framework::proc::types::ProcessPriority::Normal
    } else if clamped < 19 {
        crate::kernel::framework::proc::types::ProcessPriority::Low
    } else {
        crate::kernel::framework::proc::types::ProcessPriority::Idle
    }
}

fn priority_to_nice(p: crate::kernel::framework::proc::types::ProcessPriority) -> i32 {
    match p {
        crate::kernel::framework::proc::types::ProcessPriority::RealTime => -20,
        crate::kernel::framework::proc::types::ProcessPriority::High => -10,
        crate::kernel::framework::proc::types::ProcessPriority::Normal => 0,
        crate::kernel::framework::proc::types::ProcessPriority::Low => 10,
        crate::kernel::framework::proc::types::ProcessPriority::Idle => 19,
    }
}

fn sys_nice(inc: i32) -> i64 {
    let pid = crate::kernel::framework::proc::api::process_get_current_pid();
    // SAFETY: sys_getpriority 是 libc 兼容的 FFI；传入有效 PRIO_PROCESS
    // 常量与进程 pid (由 process_get_current_pid 返回)。
    let current_nice = unsafe { sys_getpriority(PRIO_PROCESS, pid) as i32 };
    let new_nice = (current_nice + inc).clamp(-20, 19);
    // SAFETY: sys_setpriority 是 libc 兼容的 FFI；new_nice 在合法范围 [-20, 19]。
    unsafe { sys_setpriority(PRIO_PROCESS, pid, new_nice) };
    new_nice as i64
}

fn sys_getpriority(which: i32, who: u32) -> i64 {
    if which != PRIO_PROCESS {
        return Errno::EINVAL.as_ret();
    }
    let pid = if who == 0 {
        crate::kernel::framework::proc::api::process_get_current_pid()
    } else {
        who
    };
    use crate::kernel::framework::proc::process::PROCESS_TABLE;
    let proc = match PROCESS_TABLE.get(pid) {
        Some(p) => p,
        None => return Errno::ESRCH.as_ret(),
    };
    // SAFETY: proc 是 NonNull<Process>，get() 已检查 pid 范围与有效性。
    let pri = unsafe { (*proc).get_priority() };
    priority_to_nice(pri) as i64
}

fn sys_setpriority(which: i32, who: u32, prio: i32) -> i64 {
    if which != PRIO_PROCESS {
        return Errno::EINVAL.as_ret();
    }
    let clamped = prio.clamp(-20, 19);
    let pid = if who == 0 {
        crate::kernel::framework::proc::api::process_get_current_pid()
    } else {
        who
    };
    use crate::kernel::framework::proc::process::PROCESS_TABLE;
    let proc = match PROCESS_TABLE.get(pid) {
        Some(p) => p,
        None => return Errno::ESRCH.as_ret(),
    };
    let new_pri = nice_to_priority(clamped);
    // SAFETY: proc 是 NonNull<Process>，set_priority 修改 Process 内部状态。
    unsafe { (*proc).set_priority(new_pri) };
    0
}

fn sys_fork() -> i64 {
    crate::kernel::framework::proc::api::sys_fork() as i64
}

fn sys_execve(
    path: *const u8,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    // envp 暂不传递给执行体 (POSIX ABI 保留, 当前 exec 仅消费 argv)
    let _ = envp;
    let mut argc: u32 = 0;
    if !argv.is_null() {
        if !raw::check_user_ptr(argv as u64) {
            return Errno::EFAULT.as_ret();
        }
        let mut p = argv;
        loop {
            if !raw::check_user_ptr(p as u64) {
                return Errno::EFAULT.as_ret();
            }
            // SAFETY: p 是经过 check_user_ptr 验证的用户空间指针，指向
            // *mut *const u8 (8 字节)，read_volatile 读取指针值。
            let entry = unsafe { core::ptr::read_volatile(p) };
            if entry.is_null() {
                break;
            }
            if !raw::check_user_ptr(entry as u64) {
                return Errno::EFAULT.as_ret();
            }
            argc += 1;
            // SAFETY: p 指向用户空间数组元素；p.add(1) 前进到下一个元素，
            // 由 argc 计数 + NULL 终止保证不越界。
            p = unsafe { p.add(1) };
        }
    }

    // SUID 处理
    let mut stat_buf = core::mem::MaybeUninit::<crate::kernel::framework::fs::vfs::types::VfsStat>::uninit();
    let current_pwm = crate::kernel::framework::credo::session::get_current_pwm();
    let stat_result =
        crate::kernel::framework::fs::vfs::api::vfs_stat_internal(path, stat_buf.as_mut_ptr(), current_pwm);
    if stat_result == 0 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let st = unsafe { stat_buf.assume_init() };
        if (st.perm & 0o4000) != 0 && st.owner_pwm != 0 {
            crate::kernel::framework::credo::session::elevate_for_suid(st.owner_pwm);
        }
    }

    let result = crate::kernel::framework::proc::api::proc_exec_replace(path, argv, argc);
    if result < 0 {
        Errno::ENOENT.as_ret()
    } else {
        0
    }
}

fn sys_exit(status: i32) -> i64 {
    crate::kernel::framework::proc::api::process_exit(status as u32);
    0
}

// ============================================================================
// 用户/组 — getuid / getgid / geteuid / getegid
// ============================================================================

fn sys_getuid() -> i64 {
    crate::kernel::framework::credo::session::get_current_uid() as i64
}

fn sys_getgid() -> i64 {
    crate::kernel::framework::credo::session::get_current_gid() as i64
}

fn sys_geteuid() -> i64 {
    crate::kernel::framework::credo::session::get_euid() as i64
}

fn sys_getegid() -> i64 {
    crate::kernel::framework::credo::session::get_egid() as i64
}

fn sys_setuid(uid: u32) -> i64 {
    if uid == crate::kernel::framework::credo::session::get_current_uid()
        || uid == crate::kernel::framework::credo::session::get_euid()
        || uid == crate::kernel::framework::credo::session::get_saved_euid()
    {
        return 0;
    }
    if crate::kernel::framework::credo::session::try_setuid(uid) {
        return 0;
    }
    Errno::EPERM.as_ret()
}

fn sys_setgid(gid: u32) -> i64 {
    if gid == crate::kernel::framework::credo::session::get_current_gid()
        || gid == crate::kernel::framework::credo::session::get_egid()
        || gid == crate::kernel::framework::credo::session::get_saved_egid()
    {
        return 0;
    }
    if crate::kernel::framework::credo::session::try_setgid(gid) {
        return 0;
    }
    Errno::EPERM.as_ret()
}

fn sys_seteuid(euid: u32) -> i64 {
    if euid == crate::kernel::framework::credo::session::get_current_uid()
        || euid == crate::kernel::framework::credo::session::get_euid()
        || euid == crate::kernel::framework::credo::session::get_saved_euid()
    {
        return 0;
    }
    if crate::kernel::framework::credo::session::try_seteuid(euid) {
        return 0;
    }
    Errno::EPERM.as_ret()
}

fn sys_setegid(egid: u32) -> i64 {
    if egid == crate::kernel::framework::credo::session::get_current_gid()
        || egid == crate::kernel::framework::credo::session::get_egid()
        || egid == crate::kernel::framework::credo::session::get_saved_egid()
    {
        return 0;
    }
    if crate::kernel::framework::credo::session::try_setegid(egid) {
        return 0;
    }
    Errno::EPERM.as_ret()
}

fn sys_setreuid(ruid: u32, euid: u32) -> i64 {
    if crate::kernel::framework::credo::session::try_setreuid(ruid, euid) {
        return 0;
    }
    Errno::EPERM.as_ret()
}

fn sys_setregid(rgid: u32, egid: u32) -> i64 {
    if crate::kernel::framework::credo::session::try_setregid(rgid, egid) {
        return 0;
    }
    Errno::EPERM.as_ret()
}

// ============================================================================
// 管道 — pipe / dup / dup2
// ============================================================================

fn _unused_sys_pipe_marker() {}
fn _unused_sys_dup_marker() {}
fn _unused_sys_dup2_marker() {}

// ============================================================================
// 内存 — brk / mmap / munmap
// ============================================================================

fn sys_mmap(addr: u64, size: u64, prot: i32, flags: i32, fd: i32, offset: u64) -> i64 {
    if size == 0 {
        return Errno::EINVAL.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 7, 0x01) {
        return Errno::EACCES.as_ret();
    }

    let mm = match crate::kernel::framework::mm::vma::get_current_mm() {
        Some(m) => m,
        None => {
            let pages = size.div_ceil(4096);
            let ptr = raw::alloc_pages(pages);
            return if ptr.is_null() {
                Errno::ENOMEM.as_ret()
            } else {
                ptr as i64
            };
        }
    };

    // 通过 services 层代理: VFS 交互 (fd→inode_id) 在 services 层完成
    // 透传进程当前 pwm, Vma 记录后由 #PF miss 路径用于 vfs_pread_inode 权限校验
    match crate::kernel::services::mm::mmap::mmap_syscall(mm, addr, size, prot, flags, fd, offset, pwm) {
        Ok(a) => a as i64,
        Err(e) => e.as_ret(),
    }
}

fn sys_munmap(addr: u64, size: u64) -> i64 {
    if addr == 0 || size == 0 {
        return Errno::EINVAL.as_ret();
    }

    let mm = match crate::kernel::framework::mm::vma::get_current_mm() {
        Some(m) => m,
        None => {
            let pages = size.div_ceil(4096);
            raw::free_pages(addr as *mut u8, pages);
            return 0;
        }
    };

    match crate::kernel::services::mm::mmap::munmap_syscall(mm, addr, size) {
        Ok(()) => 0,
        Err(e) => e.as_ret(),
    }
}


// ============================================================================
// 时间 — time
// ============================================================================

fn sys_time(buf: *mut u64) -> i64 {
    if buf.is_null() {
        return Errno::EINVAL.as_ret();
    }
    let ticks = raw::get_ticks();
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { raw::write_u64(buf, ticks) };
    ticks as i64
}

// ============================================================================
// 网络 — socket / bind / listen / accept / connect / sendto / recvfrom / shutdown
// ============================================================================

/// C2: sched_setaffinity — 设置进程的 CPU 亲和性掩码
///
/// Linux 兼容 ABI: `sched_setaffinity(pid, cpusetsize, mask)`
///   - `pid == 0` 表示当前进程
///   - `cpusetsize` 必须 >= 8 (u64 掩码大小)
///   - `mask` 用户空间指针, 指向 64-bit 位图
///
/// 返回 0 成功, 负值 -errno 失败
pub fn sys_sched_setaffinity(pid: i32, cpusetsize: u32, mask_ptr: u64) -> i64 {
    if cpusetsize < 8 {
        return Errno::EINVAL.as_ret();
    }
    if mask_ptr == 0 || !validate_user_buf(mask_ptr, 8) {
        return Errno::EFAULT.as_ret();
    }

    // 读 user 空间 mask
    let mask = match raw::read_u64_from_user(mask_ptr) {
        Some(v) => v,
        None => return Errno::EFAULT.as_ret(),
    };

    // 解析 pid (0 = 当前进程)
    let target_pid = if pid == 0 {
        crate::kernel::framework::proc::scheduler::SCHEDULER
            .current()
            .unwrap_or(0)
    } else if pid > 0 {
        pid as u32
    } else {
        return Errno::EINVAL.as_ret();
    };

    if target_pid == 0 {
        return Errno::ESRCH.as_ret();
    }

    // 写入 Process.cpuset_allowed
    let ok = crate::kernel::framework::proc::process::PROCESS_TABLE
        .with_process(target_pid, |p| {
            p.cpuset_allowed.store(mask, Ordering::Release);
        })
        .is_some();

    if !ok {
        return Errno::ESRCH.as_ret();
    }

    crate::klog_debug!(Sync, "[sched] setaffinity pid={} mask=0x{:X}", target_pid, mask);
    0
}

/// C2: sched_getaffinity — 获取进程的 CPU 亲和性掩码
///
/// Linux 兼容 ABI: `sched_getaffinity(pid, cpusetsize, mask)`
///
/// 返回写入的字节数 (8) 成功, 负值 -errno 失败
pub fn sys_sched_getaffinity(pid: i32, cpusetsize: u32, mask_ptr: u64) -> i64 {
    if cpusetsize < 8 {
        return Errno::EINVAL.as_ret();
    }
    if mask_ptr == 0 || !validate_user_buf(mask_ptr, 8) {
        return Errno::EFAULT.as_ret();
    }

    let target_pid = if pid == 0 {
        crate::kernel::framework::proc::scheduler::SCHEDULER
            .current()
            .unwrap_or(0)
    } else if pid > 0 {
        pid as u32
    } else {
        return Errno::EINVAL.as_ret();
    };

    if target_pid == 0 {
        return Errno::ESRCH.as_ret();
    }

    let mask = crate::kernel::framework::proc::process::PROCESS_TABLE
        .with_process(target_pid, |p| p.cpuset_allowed.load(Ordering::Acquire))
        .unwrap_or(u64::MAX);

    if !raw::write_u64_to_user(mask_ptr, mask) {
        return Errno::EFAULT.as_ret();
    }

    crate::klog_debug!(Sync, "[sched] getaffinity pid={} mask=0x{:X}", target_pid, mask);
    8 // 返回写入字节数 (Linux 兼容)
}

#[cfg(feature = "net")]
fn sys_socket(domain: i32, sock_type: i32, protocol: i32) -> i64 {
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 2, 0x01) {
        return Errno::EACCES.as_ret();
    }
    raw::sm_socket_call(domain, sock_type, protocol) as i64
}

#[cfg(feature = "net")]
fn sys_bind(sockfd: i32, addr: u64, addrlen: u32) -> i64 {
    raw::sm_bind_call(sockfd, addr as *const u8, addrlen) as i64
}

#[cfg(feature = "net")]
fn sys_listen(sockfd: i32, backlog: i32) -> i64 {
    raw::sm_listen_call(sockfd, backlog) as i64
}

#[cfg(feature = "net")]
fn sys_accept(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    raw::sm_accept_call(sockfd, addr as *mut u8, addrlen as *mut u32) as i64
}

#[cfg(feature = "net")]
fn sys_connect(sockfd: i32, addr: u64, addrlen: u32) -> i64 {
    raw::sm_connect_call(sockfd, addr as *const u8, addrlen) as i64
}

#[cfg(feature = "net")]
fn sys_sendto(sockfd: i32, buf: u64, len: u32, flags: i32) -> i64 {
    raw::sm_send_call(sockfd, buf as *const u8, len, flags) as i64
}

#[cfg(feature = "net")]
fn sys_recvfrom(sockfd: i32, buf: u64, len: u32, flags: i32) -> i64 {
    raw::sm_recv_call(sockfd, buf as *mut u8, len, flags) as i64
}

#[cfg(feature = "net")]
fn sys_shutdown(sockfd: i32, _how: i32) -> i64 {
    raw::sm_close_call(sockfd) as i64
}

#[cfg(feature = "net")]
fn sys_sendmsg(_sockfd: i32, _msg: u64, _flags: i32) -> i64 {
    Errno::ENOSYS.as_ret()
}

#[cfg(feature = "net")]
fn sys_recvmsg(_sockfd: i32, _msg: u64, _flags: i32) -> i64 {
    Errno::ENOSYS.as_ret()
}

#[cfg(feature = "net")]
fn sys_setsockopt(sockfd: i32, level: i32, optname: i32, optval: u64, optlen: u32) -> i64 {
    raw::sm_setsockopt_call(sockfd, level, optname, optval as *const u8, optlen) as i64
}

#[cfg(feature = "net")]
fn sys_getsockopt(sockfd: i32, level: i32, optname: i32, optval: u64, optlen: u64) -> i64 {
    raw::sm_getsockopt_call(
        sockfd,
        level,
        optname,
        optval as *mut u8,
        optlen as *mut u32,
    ) as i64
}

#[cfg(feature = "net")]
fn sys_getsockname(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    raw::sm_getsockname_call(sockfd, addr, addrlen) as i64
}

#[cfg(feature = "net")]
fn sys_getpeername(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    raw::sm_getpeername_call(sockfd, addr, addrlen) as i64
}

fn sys_getrusage(who: i32, rusage: u64) -> i64 {
    let pid = crate::kernel::framework::proc::api::process_get_current_pid();
    crate::kernel::framework::proc::api::proc_get_rusage(pid, who, rusage as *mut u8, 144) as i64
}

#[cfg(feature = "net")]
fn sys_sendmsg(fd: i32, msg: u64, flags: i32) -> i64 {
    raw::sm_sendmsg_call(fd, msg, flags) as i64
}

#[cfg(feature = "net")]
fn sys_recvmsg(fd: i32, msg: u64, flags: i32) -> i64 {
    raw::sm_recvmsg_call(fd, msg, flags) as i64
}

// ============================================================================
// PWM 认证/权限 (Credo 私有 syscall)
// ============================================================================

fn sys_auth_login(
    password: *const u8,
    note: *const u8,
) -> i64 {
    crate::kernel::framework::credo::api::pwm_login(note, password)
}

fn sys_auth_logout() -> i64 {
    crate::kernel::framework::credo::api::pwm_logout();
    0
}

fn sys_auth_create(
    password: *const u8,
    note: *const u8,
    _level: u8,
) -> i64 {
    let creator = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::credo::api::pwm_create(password, note, creator)
}

fn sys_auth_delete(target: u64) -> i64 {
    crate::kernel::framework::credo::api::pwm_delete(target) as i64
}

fn sys_auth_info(target: u64) -> i64 {
    crate::kernel::framework::credo::api::pwm_get_privilege_level(target) as i64
}

fn sys_auth_changepw(
    old_pw: *const u8,
    new_pw: *const u8,
) -> i64 {
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::credo::api::pwm_change_password(pwm, old_pw, new_pw) as i64
}

fn sys_auth_verify(password: *const u8) -> i64 {
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::credo::api::pwm_verify_password(pwm, password) as i64
}

fn sys_auth_create_first(password: *const u8) -> i64 {
    if password.is_null() {
        return Errno::EINVAL.as_ret();
    }
    crate::kernel::framework::credo::api::pwm_create_first_identity(password)
}

fn sys_auth_grant(grantor: u64, grantee: u64, domain: u16, caps: u64) -> i64 {
    crate::kernel::framework::credo::api::pwm_grant(grantor, grantee, domain, caps) as i64
}

fn sys_auth_revoke(revoker: u64, target: u64, domain: u16, caps: u64) -> i64 {
    crate::kernel::framework::credo::api::pwm_revoke(revoker, target, domain, caps) as i64
}

fn sys_auth_check_cap(pwm: u64, domain: u16, required: u64) -> i64 {
    if crate::kernel::framework::credo::api::pwm_has_capability(pwm, domain, required) {
        1
    } else {
        0
    }
}

fn sys_auth_get_caps(pwm: u64, domain: u16) -> i64 {
    crate::kernel::framework::credo::api::pwm_get_capability_raw(pwm, domain) as i64
}

fn sys_pwm_get() -> i64 {
    crate::kernel::framework::credo::api::pwm_get_current() as i64
}

fn sys_pwm_set(pwm: u64) -> i64 {
    let pid = crate::kernel::framework::proc::api::process_get_current_pid();
    crate::kernel::framework::proc::api::proc_set_pwm(pid, pwm) as i64
}

// ============================================================================
// 系统信息 / 环境 (Credo 私有 syscall)
// ============================================================================

fn sys_gethostname(buf: *mut u8, size: u64) -> i64 {
    if buf.is_null() || size == 0 || !raw::check_user_buf(buf as u64, size) {
        return Errno::EFAULT.as_ret();
    }
    let hostname = b"localhost\0";
    let copy_len = hostname.len().min(size as usize - 1);
    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    unsafe { core::ptr::copy_nonoverlapping(hostname.as_ptr(), buf as *mut u8, copy_len) };
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { raw::write_u8(buf.add(copy_len), 0) };
    0
}

fn sys_sethostname(name: *const u8, len: u64) -> i64 {
    if name.is_null() || len == 0 || len > 63 {
        return Errno::EINVAL.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 0, 9) {
        return Errno::EACCES.as_ret();
    }
    0
}

fn sys_reboot(cmd: i32) -> i64 {
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 0, 0x01) {
        return Errno::EACCES.as_ret();
    }
    // SAFETY: 已在调用方通过 PWM CAP 检查, 重启为合法操作。
    match cmd {
        0 => loop {},
        1 => match () {
            #[cfg(target_arch = "x86_64")]
            () => unsafe { raw::reboot_via_idt() },
            #[cfg(target_arch = "aarch64")]
            () => unsafe { raw::reboot_via_psci() },
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            () => loop {},
        },
        _ => Errno::EINVAL.as_ret(),
    }
}

fn sys_boot_check(check_type: i32) -> i64 {
    match check_type {
        0 => {
            if crate::kernel::framework::credo::api::pwm_any_identity_exists() {
                1
            } else {
                0
            }
        }
        _ => -1,
    }
}

// ============================================================================
// 磁盘管理 (Credo 私有 syscall) — 通过 BlockDevice 注册表统一访问
// ============================================================================

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn sys_disk_list(disks: *mut u64, max_count: u32) -> i64 {
    if disks.is_null() || max_count == 0 {
        return Errno::EINVAL.as_ret();
    }
    let count = crate::kernel::framework::driver::block::block_device_count();
    let limit = max_count.min(count as u32);
    for i in 0..limit {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe { raw::write_u64(disks.add(i as usize), i as u64) };
    }
    limit as i64
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn sys_disk_info(disk_id: u32, info: *mut u8) -> i64 {
    if info.is_null() {
        return Errno::EINVAL.as_ret();
    }
    let present = if crate::kernel::framework::driver::block::hdd_is_present(disk_id as u8) {
        1u32
    } else {
        0u32
    };
    let sectors = if present != 0 {
        crate::kernel::framework::driver::block::hdd_total_sectors(disk_id as u8) as u32
    } else {
        0
    };
    let model_bytes = b"Block Dev";
    let mut model = [0u8; 64];
    let copy_len = if model_bytes.len() < 63 {
        model_bytes.len()
    } else {
        63
    };
    model[..copy_len].copy_from_slice(&model_bytes[..copy_len]);
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct UserDiskInfo {
        disk_id: u32,
        present: u32,
        total_sectors: u32,
        sectors: u32,
        model: [u8; 64],
    }
    let disk_info = UserDiskInfo {
        disk_id,
        present,
        total_sectors: sectors,
        sectors,
        model,
    };
    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    unsafe { raw::write_struct(info as *mut UserDiskInfo, &disk_info) };
    0
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn sys_disk_format(disk_id: u32, fstype: *const u8) -> i64 {
    if fstype.is_null() {
        return Errno::EINVAL.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 4, 0) {
        return Errno::EACCES.as_ret();
    }
    if !crate::kernel::framework::driver::block::hdd_is_present(disk_id as u8) {
        return Errno::ENOENT.as_ret();
    }
    let hvfs_start_lba: u32 = 18432;
    let mut sector_buf = [0u8; 512];
    sector_buf[0] = 0x48;
    sector_buf[1] = 0x56;
    sector_buf[2] = 0x46;
    sector_buf[3] = 0x53;
    sector_buf[8] = 0x02;
    sector_buf[9] = 0x00;
    if crate::kernel::framework::driver::block::hdd_write_sector(
        disk_id as u8,
        hvfs_start_lba as u64,
        &sector_buf,
    ) < 0
    {
        return Errno::EIO.as_ret();
    }
    0
}

fn write_le32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset] = val as u8;
    buf[offset + 1] = (val >> 8) as u8;
    buf[offset + 2] = (val >> 16) as u8;
    buf[offset + 3] = (val >> 24) as u8;
}

fn write_le16(buf: &mut [u8], offset: usize, val: u16) {
    buf[offset] = val as u8;
    buf[offset + 1] = (val >> 8) as u8;
}

const BOOT_PART_SECTORS: u32 = 16384;

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn sys_disk_partition(disk_id: u32, total_sectors: u64) -> i64 {
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 4, 0) {
        return Errno::EACCES.as_ret();
    }
    if !crate::kernel::framework::driver::block::hdd_is_present(disk_id as u8) {
        return Errno::ENOENT.as_ret();
    }
    let hvfs_start = BOOT_PART_SECTORS;
    let hvfs_sectors = if total_sectors > hvfs_start as u64 + 1 {
        total_sectors - hvfs_start as u64
    } else {
        0xFFFFFFFFu64
    };
    let mut mbr = [0u8; 512];
    write_le32(&mut mbr, 446, 0x00000800);
    write_le32(&mut mbr, 450, 0x06FEFFFF);
    write_le32(&mut mbr, 454, 64u32);
    write_le32(&mut mbr, 458, BOOT_PART_SECTORS - 64);
    write_le32(&mut mbr, 462, hvfs_start);
    write_le32(&mut mbr, 466, 0x83FEFFFF);
    let hvfs_len = if hvfs_sectors > 0xFFFFFFFF {
        0xFFFFFFFFu32
    } else {
        hvfs_sectors as u32
    };
    write_le32(&mut mbr, 470, hvfs_start);
    write_le32(&mut mbr, 474, hvfs_len);
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    if crate::kernel::framework::driver::block::hdd_write_sector(disk_id as u8, 0, &mbr) < 0 {
        return Errno::EIO.as_ret();
    }
    0
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn sys_fat_format(disk_id: u32) -> i64 {
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 4, 0) {
        return Errno::EACCES.as_ret();
    }
    if !crate::kernel::framework::driver::block::hdd_is_present(disk_id as u8) {
        return Errno::ENOENT.as_ret();
    }
    let fat_start_lba: u32 = 2048;
    let total_sectors: u16 = BOOT_PART_SECTORS as u16 - 64;
    let sectors_per_cluster: u8 = 8;
    let reserved_sectors: u16 = 1;
    let num_fats: u8 = 2;
    let root_entries: u16 = 512;
    let sectors_per_fat: u16 =
        ((total_sectors as u32 - 1 - 32) / (sectors_per_cluster as u32 * 256 + 2) + 1) as u16;
    let mut bpb = [0u8; 512];
    bpb[0] = 0xEB;
    bpb[1] = 0x3C;
    bpb[2] = 0x90;
    bpb[3] = b'A';
    bpb[4] = b'N';
    bpb[5] = b'T';
    bpb[6] = b'X';
    bpb[7] = b'B';
    bpb[8] = b'O';
    bpb[9] = b'O';
    bpb[10] = b'T';
    write_le16(&mut bpb, 11, 512);
    bpb[13] = sectors_per_cluster;
    write_le16(&mut bpb, 14, reserved_sectors);
    bpb[16] = num_fats;
    write_le16(&mut bpb, 17, root_entries);
    write_le16(&mut bpb, 19, total_sectors);
    bpb[21] = 0xF8;
    write_le16(&mut bpb, 22, sectors_per_fat);
    bpb[36] = 0x80;
    bpb[38] = 0x29;
    bpb[510] = 0x55;
    bpb[511] = 0xAA;
    if crate::kernel::framework::driver::block::hdd_write_sector(disk_id as u8, fat_start_lba as u64, &bpb) < 0
    {
        return Errno::EIO.as_ret();
    }
    let fat_begin = fat_start_lba + reserved_sectors as u32;
    let mut fat_sector = [0u8; 512];
    fat_sector[0] = 0xF8;
    fat_sector[1] = 0xFF;
    fat_sector[2] = 0xFF;
    fat_sector[3] = 0xFF;
    for i in 0..num_fats {
        let lba = fat_begin + i as u32 * sectors_per_fat as u32;
        if crate::kernel::framework::driver::block::hdd_write_sector(disk_id as u8, lba as u64, &fat_sector)
            < 0
        {
            return Errno::EIO.as_ret();
        }
        let zero = [0u8; 512];
        for s in 1..sectors_per_fat as u32 {
            if crate::kernel::framework::driver::block::hdd_write_sector(
                disk_id as u8,
                (lba + s) as u64,
                &zero,
            ) < 0
            {
                return Errno::EIO.as_ret();
            }
        }
    }
    let root_dir_lba = fat_begin + num_fats as u32 * sectors_per_fat as u32;
    let root_dir_sectors = (root_entries as u32 * 32).div_ceil(512);
    let zero = [0u8; 512];
    for s in 0..root_dir_sectors {
        if crate::kernel::framework::driver::block::hdd_write_sector(
            disk_id as u8,
            (root_dir_lba + s) as u64,
            &zero,
        ) < 0
        {
            return Errno::EIO.as_ret();
        }
    }
    0
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn sys_boot_install(disk_id: u32) -> i64 {
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 4, 0) {
        return Errno::EACCES.as_ret();
    }
    let stage1 = include_bytes!("../../../../build/stage1.bin");
    if !crate::kernel::framework::driver::block::hdd_is_present(disk_id as u8) {
        return Errno::ENOENT.as_ret();
    }
    let mut mbr = [0u8; 512];
    if crate::kernel::framework::driver::block::hdd_read_sector(disk_id as u8, 0, &mut mbr) < 0 {
        return Errno::EIO.as_ret();
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { core::ptr::copy_nonoverlapping(stage1.as_ptr(), mbr.as_mut_ptr(), 440) };
    let total_sectors = crate::kernel::framework::driver::block::hdd_total_sectors(disk_id as u8);
    let hvfs_start = BOOT_PART_SECTORS;
    let hvfs_sectors = if total_sectors > hvfs_start as u64 + 1 {
        total_sectors - hvfs_start as u64
    } else {
        0xFFFFFFFFu64
    };
    write_le32(&mut mbr, 446, 0x00000800);
    write_le32(&mut mbr, 450, 0x06FEFFFF);
    write_le32(&mut mbr, 454, 64u32);
    write_le32(&mut mbr, 458, BOOT_PART_SECTORS - 64);
    write_le32(&mut mbr, 462, hvfs_start);
    write_le32(&mut mbr, 466, 0x83FEFFFF);
    write_le32(&mut mbr, 470, hvfs_start);
    let hvfs_len = if hvfs_sectors > 0xFFFFFFFF {
        0xFFFFFFFFu32
    } else {
        hvfs_sectors as u32
    };
    write_le32(&mut mbr, 474, hvfs_len);
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    if crate::kernel::framework::driver::block::hdd_write_sector(disk_id as u8, 0, &mbr) < 0 {
        return Errno::EIO.as_ret();
    }
    let kernel_ptr = raw::kernel_start_ptr();
    let kernel_len = {
        const HHDM_OFFSET: usize = 0xFFFF_8000_0000_0000;
        let phys_end = raw::kernel_end_phys(HHDM_OFFSET);
        phys_end - (kernel_ptr as usize)
    };
    let total_kernel_sectors = kernel_len.div_ceil(512) as u32;
    let max_sectors = 2047u32;
    let copy_sectors = if total_kernel_sectors > max_sectors {
        max_sectors
    } else {
        total_kernel_sectors
    };
    for s in 0..copy_sectors {
        let offset = s as usize * 512;
        let remaining = kernel_len.saturating_sub(offset);
        if remaining == 0 {
            break;
        }
        let n = if remaining < 512 { remaining } else { 512 };
        let mut buf = [0u8; 512];
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::copy_nonoverlapping(kernel_ptr.add(offset), buf.as_mut_ptr(), n);
        }
        if crate::kernel::framework::driver::block::hdd_write_sector(disk_id as u8, (1 + s) as u64, &buf) < 0 {
            return Errno::EIO.as_ret();
        }
    }
    let mut cfg = [0u8; 512];
    cfg[0] = b'A';
    cfg[1] = b'N';
    cfg[2] = b'T';
    cfg[3] = b'X';
    write_le32(&mut cfg, 4, BOOT_PART_SECTORS);
    cfg[510] = 0x55;
    cfg[511] = 0xAA;
    crate::kernel::framework::driver::block::hdd_write_sector(disk_id as u8, 2046, &cfg);
    0
}

// ============================================================================
// 进程列表 (Credo 私有 syscall)
// ============================================================================

#[repr(C)]
struct ProcListEntry {
    pid: u32,
    state: u8,
    _pad: [u8; 3],
    pwm: u64,
    priority: u32,
    _pad2: u32,
    name: [u8; 48],
}

fn sys_proc_list(buf: *mut u8, max_entries: u32) -> i64 {
    if buf.is_null() || !raw::check_user_ptr(buf as u64) {
        return Errno::EFAULT.as_ret();
    }
    let entry_size = core::mem::size_of::<ProcListEntry>() as u32;
    let mut count: i32 = 0;
    let table = &crate::kernel::framework::proc::process::PROCESS_TABLE;
    table.for_each(|proc| {
        if (count as u32) < max_entries {
            let entry_ptr =
                // SAFETY: `entry_size` 由调用方保证为有效指针; 只读访问
                unsafe { buf.add(count as usize * entry_size as usize) as *mut ProcListEntry };
            // SAFETY: `entry_ptr` 由调用方保证为有效指针; 只读访问
            let entry = unsafe { &mut *entry_ptr };
            entry.pid = proc.pid.0;
            entry.state = proc.get_state() as u8;
            entry._pad = [0u8; 3];
            entry.pwm = proc.get_pwm();
            entry.priority = proc.get_priority() as u32;
            entry._pad2 = 0;
            let name = proc.name.lock();
            let name_bytes = name.as_bytes();
            let len = name_bytes.len().min(47);
            entry.name[..len].copy_from_slice(&name_bytes[..len]);
            entry.name[len] = 0;
            count += 1;
        }
        true
    });
    count as i64
}

fn sys_proc_setpri(pid: u32, priority: u32) -> i64 {
    crate::kernel::framework::proc::api::proc_set_priority(pid, priority) as i64
}

// ============================================================================
// ioctl — 设备 I/O 控制
// ============================================================================

const TIOCGWINSZ: u64 = 0x5413;
const TCGETS: u64 = 0x5401;

fn sys_ioctl(_fd: i32, request: u64, arg: u64) -> i64 {
    if arg == 0 {
        return Errno::EINVAL.as_ret();
    }
    match request {
        TIOCGWINSZ => {
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct Winsize {
                ws_row: u16,
                ws_col: u16,
                ws_xpixel: u16,
                ws_ypixel: u16,
            }
            let ws = Winsize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let dst = arg as *mut Winsize;
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe { raw::write_struct(dst, &ws) };
            0
        }
        TCGETS => 0,
        _ => Errno::ENOTTY.as_ret(),
    }
}

// ============================================================================
// nanosleep — 高精度睡眠 (基于 ticks, 1ms 粒度)
// ============================================================================

pub fn sys_nanosleep(req: u64, rem: u64) -> i64 {
    if req == 0 || !raw::check_user_ptr(req) {
        return Errno::EINVAL.as_ret();
    }
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    // SAFETY: `const` 由调用方保证为有效指针; 只读访问
    let ts = unsafe { core::ptr::read_volatile(req as *const Timespec) };
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return Errno::EINVAL.as_ret();
    }

    // 计算总纳秒
    let total_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
    if total_ns == 0 {
        return 0;
    }

    // 短延时 (< 1ms): 使用 hrtimer 忙等, 避免调度开销
    // 长延时 (>= 1ms): 使用调度器阻塞睡眠
    if total_ns < 1_000_000 {
        // 忙等: 使用 hrtimer 时钟源精确等待
        let start = crate::kernel::framework::timer::hrtimer::hrtimer_clock_read();
        let target = start + total_ns;
        while crate::kernel::framework::timer::hrtimer::hrtimer_clock_read() < target {
            core::hint::spin_loop();
        }
    } else {
        // 调度器阻塞: 毫秒级精度
        let total_ms = total_ns / 1_000_000;
        let _ = crate::kernel::framework::timer::sleep::timer_sleep(total_ms);
    }

    // 如果有 rem 指针且被信号中断, 写入剩余时间
    // 当前简化: 总是成功完成, 不处理信号中断
    let _ = rem;

    0
}

// ============================================================================
// gettimeofday / clock_gettime
// ============================================================================

const CLOCK_REALTIME: i32 = 0;
const CLOCK_MONOTONIC: i32 = 1;

fn sys_clock_gettime(clk_id: i32, tp: *mut u8) -> i64 {
    if tp.is_null() {
        return Errno::EINVAL.as_ret();
    }
    if clk_id != CLOCK_REALTIME && clk_id != CLOCK_MONOTONIC {
        return Errno::EINVAL.as_ret();
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    let ticks = raw::get_ticks();
    let t = Timespec {
        tv_sec: (ticks / 1000) as i64,
        tv_nsec: ((ticks % 1000) * 1000000) as i64,
    };
    let dst = tp as *mut Timespec;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { raw::write_struct(dst, &t) };
    0
}

// ============================================================================
// poll — 基础轮询框架
// ============================================================================

const POLLIN: i16 = 1;
const POLLOUT: i16 = 4;

fn sys_poll(fds: *mut u8, nfds: u32, _timeout: i32) -> i64 {
    if fds.is_null() || nfds == 0 {
        return 0;
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    let mut ready: i32 = 0;
    for i in 0..nfds as usize {
        // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
        let pfd_ptr = unsafe { (fds as *mut PollFd).add(i) };
        // SAFETY: `pfd_ptr` 由调用方保证为有效指针; 只读访问
        let pfd = unsafe { &mut *pfd_ptr };
        pfd.revents = 0;
        if pfd.fd < 0 {
            continue;
        }
        if pfd.events & POLLIN != 0 {
            let fd_table = crate::kernel::framework::fs::vfs::vfs::VFS_MANAGER.fd_table.lock();
            if (pfd.fd as usize) < 256 && fd_table[pfd.fd as usize].used {
                pfd.revents |= POLLIN;
                ready += 1;
            }
        }
        if pfd.events & POLLOUT != 0 {
            pfd.revents |= POLLOUT;
            ready += 1;
        }
    }
    ready as i64
}

// ============================================================================
// chmod / fchmod / chown
// ============================================================================

fn sys_chmod(path: *const u8, mode: u32) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_chmod(path, mode as u16, pwm) as i64
}

fn sys_fchmod(fd: i32, mode: u32) -> i64 {
    crate::kernel::framework::fs::vfs::api::vfs_fchmod(fd as u32, mode as u16) as i64
}

fn sys_chown(path: *const u8, uid: u32, gid: u32) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
    let tbl = crate::kernel::framework::credo::identity::get_table();
    let owner_pwm = tbl.find_by_uid(uid).map_or(0, |e| e.get_pwm().0);
    let group_pwm = tbl.find_by_uid(gid).map_or(0, |e| e.get_pwm().0);
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    crate::kernel::framework::fs::vfs::api::vfs_chown_ext(path, owner_pwm, group_pwm, pwm) as i64
}

// ============================================================================
// kill — 信号发送
// ============================================================================

const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;

pub fn sys_kill(pid: i32, sig: i32) -> i64 {
    if sig < 0 || sig > 31 {
        return Errno::EINVAL.as_ret();
    }
    // 解决 TRACK-315B7C: 移除 pid <= 0 阻塞, 接受 POSIX 4 种 pid 语义
    // (pid>0 单进程, pid=0 同组, pid=-1 全部, pid<-1 |pid| 组).
    match crate::kernel::framework::proc::signal::do_signal_send_extended(pid, sig as u8) {
        Ok(_) => 0,
        Err(-1) => Errno::EINVAL.as_ret(),
        Err(-2) => Errno::ESRCH.as_ret(),
        Err(_) => Errno::EPERM.as_ret(),
    }
}

// ============================================================================
// readlink — 读取符号链接的目标路径
//
// 注: HvFS 当前不支持符号链接, 返回 EINVAL 提示调用者此路径非符号链接。
// ============================================================================

fn sys_readlink(
    path: *const u8,
    buf: *mut u8,
    bufsiz: u64,
) -> i64 {
    if path.is_null() || buf.is_null() || bufsiz == 0 {
        return Errno::EINVAL.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut st_buf: crate::kernel::framework::fs::vfs::types::VfsStat = unsafe { core::mem::zeroed() };
    let result = crate::kernel::framework::fs::vfs::api::vfs_stat(path, &mut st_buf, pwm);
    if result < 0 {
        return Errno::ENOENT.as_ret();
    }
    Errno::EINVAL.as_ret()
}

// ============================================================================
// umount2
// ============================================================================

fn sys_umount2(target: *const u8, _flags: i32) -> i64 {
    if target.is_null() || !raw::check_user_ptr(target as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::framework::credo::api::pwm_get_current();
    if !crate::kernel::framework::credo::api::pwm_has_capability(pwm, 0, 0x01) {
        return Errno::EACCES.as_ret();
    }
    0
}

// ============================================================================
// getrlimit / sysinfo
// ============================================================================

fn sys_getrlimit(_resource: i32, rlim: *mut u8) -> i64 {
    if rlim.is_null() {
        return Errno::EINVAL.as_ret();
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct Rlimit {
        rlim_cur: u64,
        rlim_max: u64,
    }
    let r = Rlimit {
        rlim_cur: u64::MAX,
        rlim_max: u64::MAX,
    };
    let dst = rlim as *mut Rlimit;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { raw::write_struct(dst, &r) };
    0
}

fn sys_sysinfo(info: *mut u8) -> i64 {
    if info.is_null() {
        return Errno::EINVAL.as_ret();
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct SysInfo {
        uptime: i64,
        loads: [u64; 3],
        totalram: u64,
        freeram: u64,
        sharedram: u64,
        bufferram: u64,
        totalswap: u64,
        freeswap: u64,
        procs: u16,
        _pad: [u8; 6],
        totalhigh: u64,
        freehigh: u64,
        mem_unit: u32,
    }
    let si = SysInfo {
        uptime: (raw::get_ticks() / 1000) as i64,
        loads: [0, 0, 0],
        totalram: 128 * 1024 * 1024,
        freeram: 97 * 1024 * 1024,
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs: 1,
        _pad: [0u8; 6],
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1,
    };
    let dst = info as *mut SysInfo;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { raw::write_struct(dst, &si) };
    0
}

// ============================================================================
// 文件截断 — truncate / ftruncate
// ============================================================================

fn sys_truncate(path: *const u8, length: i64) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) || length < 0 {
        return Errno::EINVAL.as_ret();
    }
    let fd = crate::kernel::framework::fs::vfs::api::vfs_open(
        path,
        0o2,
        crate::kernel::framework::credo::api::pwm_get_current(),
    );
    if fd < 0 {
        return Errno::ENOENT.as_ret();
    }
    let result = crate::kernel::framework::fs::vfs::api::vfs_truncate_internal(fd as u32, length as u64);
    crate::kernel::framework::fs::vfs::api::vfs_close(fd as u32);
    if result < 0 {
        Errno::EIO.as_ret()
    } else {
        0
    }
}

fn sys_ftruncate(fd: i32, length: i64) -> i64 {
    if fd < 0 || length < 0 {
        return Errno::EINVAL.as_ret();
    }
    let result = crate::kernel::framework::fs::vfs::api::vfs_truncate_internal(fd as u32, length as u64);
    if result < 0 {
        Errno::EIO.as_ret()
    } else {
        0
    }
}

// ============================================================================
// umask — 文件创建模式掩码
// ============================================================================

fn sys_umask(mask: u32) -> i64 {
    static UMASK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0o22);
    let old = UMASK.swap(mask & 0o777, core::sync::atomic::Ordering::SeqCst);
    old as i64
}

// ============================================================================
// 文件同步 — fsync
// ============================================================================

fn sys_fsync(fd: i32) -> i64 {
    if fd < 0 {
        return Errno::EBADF.as_ret();
    }
    crate::kernel::framework::fs::vfs::api::vfs_sync();
    0
}

// ============================================================================
// 进程组/会话 — getpgid / setsid
// ============================================================================

fn sys_setsid() -> i64 {
    let pid = crate::kernel::framework::proc::api::process_get_current_pid();
    pid as i64
}


// ============================================================================
// tgkill — 向指定线程发送信号
// ============================================================================

fn sys_tgkill(_tgid: i32, tid: i32, sig: i32) -> i64 {
    sys_kill(tid, sig)
}

// ============================================================================
// 信号框架 — rt_sigaction / rt_sigprocmask / rt_sigreturn
// ============================================================================

const SIG_DFL_SYSCALL: u64 = 0;
const SIG_IGN_SYSCALL: u64 = 1;
const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SIG_SETMASK: i32 = 2;

pub fn sys_rt_sigaction(signum: i32, act: u64, oact: u64) -> i64 {
    if !(1..=31).contains(&signum) {
        return Errno::EINVAL.as_ret();
    }

    let pid = match crate::kernel::framework::proc::scheduler::SCHEDULER.current() {
        Some(p) => p,
        None => return Errno::ESRCH.as_ret(),
    };

    // 读取旧值
    if oact != 0 {
        if !raw::check_user_buf(oact, 8) {
            return Errno::EFAULT.as_ret();
        }
        let old = crate::kernel::framework::proc::signal::get_sigaction(pid, signum as u8);
        match old {
            // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
            Some(v) => unsafe { raw::write_u64(oact as *mut u64, v) },
            None => return Errno::EINVAL.as_ret(),
        }
    }

    // 设置新值
    if act != 0 {
        if !raw::check_user_buf(act, 8) {
            return Errno::EFAULT.as_ret();
        }
        // SAFETY: `const` 由调用方保证为有效指针; 只读访问
        let new_action = unsafe { raw::read_u64(act as *const u64) };
        match crate::kernel::framework::proc::signal::set_sigaction(pid, signum as u8, new_action) {
            Some(_) => {}
            None => return Errno::EINVAL.as_ret(), // SIGKILL/SIGSTOP
        }
    }

    0
}

pub fn sys_rt_sigprocmask(how: i32, set: u64, oset: u64) -> i64 {
    let pid = match crate::kernel::framework::proc::scheduler::SCHEDULER.current() {
        Some(p) => p,
        None => return Errno::ESRCH.as_ret(),
    };

    // 返回旧屏蔽字
    if oset != 0 {
        if !raw::check_user_buf(oset, 8) {
            return Errno::EFAULT.as_ret();
        }
        let old = crate::kernel::framework::proc::signal::get_blocked_mask(pid);
        // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
        unsafe { raw::write_u64(oset as *mut u64, old) };
    }

    // 设置新屏蔽字
    if set != 0 {
        if !raw::check_user_buf(set, 8) {
            return Errno::EFAULT.as_ret();
        }
        // SAFETY: `const` 由调用方保证为有效指针; 只读访问
        let new_set = unsafe { raw::read_u64(set as *const u64) };
        let old = crate::kernel::framework::proc::signal::get_blocked_mask(pid);
        let updated = match how {
            SIG_BLOCK => old | new_set,
            SIG_UNBLOCK => old & !new_set,
            SIG_SETMASK => new_set,
            _ => return Errno::EINVAL.as_ret(),
        };
        // SIGKILL/SIGSTOP 不可屏蔽
        let updated = updated & !((1u64 << 9) | (1u64 << 19));
        crate::kernel::framework::proc::signal::set_blocked_mask(pid, updated);
    }

    0
}

fn sys_rt_sigreturn() -> i64 {
    // TODO(TRACK-B29335): 架构相关 — 从信号栈帧恢复原始寄存器状态
    // 当前简化实现: 直接返回 0
    0
}

// ============================================================================
// 热插拔状态查询 (Credo 私有 syscall 437)
// ============================================================================

fn sys_hotplug_status(buf: *mut u8, buf_size: u32) -> i64 {
    if buf.is_null() || buf_size == 0 {
        return Errno::EINVAL.as_ret();
    }
    if !raw::check_user_buf(buf as u64, buf_size as u64) {
        return Errno::EFAULT.as_ret();
    }

    let status = crate::kernel::framework::driver::hotplug::HOTPLUG_MANAGER.status();

    let mut offset: u32 = 0;

    // 写入头部: enabled(u8) + slot_count(u32) + blk_device_count(u32)
    let header: [u8; 16] = [
        status.enabled as u8,
        0,
        0,
        0,
        (status.slot_count & 0xFF) as u8,
        ((status.slot_count >> 8) & 0xFF) as u8,
        ((status.slot_count >> 16) & 0xFF) as u8,
        ((status.slot_count >> 24) & 0xFF) as u8,
        (status.blk_device_count & 0xFF) as u8,
        ((status.blk_device_count >> 8) & 0xFF) as u8,
        ((status.blk_device_count >> 16) & 0xFF) as u8,
        ((status.blk_device_count >> 24) & 0xFF) as u8,
        0,
        0,
        0,
        0,
    ];
    if offset + 16 > buf_size {
        return offset as i64;
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::ptr::copy_nonoverlapping(header.as_ptr(), buf.add(offset as usize), 16);
    }
    offset += 16;

    // 写入槽位信息 (每槽 8 字节)
    let slot_size: u32 = 8;
    for slot in &status.slots {
        if offset + slot_size > buf_size {
            break;
        }
        let info: [u8; 8] = [
            slot.bus,
            slot.device,
            slot.function,
            slot.slot_number,
            slot.presence as u8,
            ((slot.surprise_capable as u8) << 1) | (slot.hotplug_capable as u8),
            0,
            0,
        ];
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::copy_nonoverlapping(
                info.as_ptr(),
                buf.add(offset as usize),
                slot_size as usize,
            );
        }
        offset += slot_size;
    }

    // 写入块设备状态 (每设备 16 字节)
    let dev_size: u32 = 16;
    for dev in &status.blk_devices {
        if offset + dev_size > buf_size {
            break;
        }
        let info: [u8; 16] = [
            dev.drive,
            dev.present as u8,
            dev.removing as u8,
            0,
            (dev.io_count & 0xFF) as u8,
            ((dev.io_count >> 8) & 0xFF) as u8,
            ((dev.io_count >> 16) & 0xFF) as u8,
            ((dev.io_count >> 24) & 0xFF) as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::copy_nonoverlapping(
                info.as_ptr(),
                buf.add(offset as usize),
                dev_size as usize,
            );
        }
        offset += dev_size;
    }

    offset as i64
}

fn sys_credo_proc_cputime(pid: u32) -> i64 {
    let target_pid = if pid == 0 {
        crate::kernel::framework::proc::api::process_get_current_pid()
    } else {
        pid
    };
    use crate::kernel::framework::proc::process::PROCESS_TABLE;
    use crate::kernel::framework::proc::scheduler_ex::SCHEDULER_EX;
    match PROCESS_TABLE.get(target_pid) {
        Some(_) => {
            let current = SCHEDULER_EX
                .current
                .load(core::sync::atomic::Ordering::Acquire)
                as *mut crate::kernel::framework::proc::thread::Thread;
            if current.is_null() {
                return -1;
            }
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            let cputime = unsafe {
                (*current)
                    .cpu_time
                    .load(core::sync::atomic::Ordering::Acquire)
            };
            cputime as i64
        }
        None => Errno::ESRCH.as_ret(),
    }
}

// ============================================================================
// 帧缓冲设备 — fb_open / fb_mmap / fb_release
//
// 设计原则：
//   1. fb_open — 查询帧缓冲物理信息，返回 FbInfo 结构体
//   2. fb_mmap — 将帧缓冲物理页映射到当前用户进程地址空间
//   3. fb_release — 标记释放（页表在进程退出时由 destroy_page_table 统一清理）
//
// 安全要点：
//   - 所有用户指针均通过 validate_user_ptr 校验
//   - 物理地址映射前校验大小不越界
//   - 使用 WRITE_THROUGH 缓存策略确保像素写入立即可见
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
struct FbInfo {
    phys_addr: u64,
    size: u64,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u8,
    _pad: [u8; 3],
}

fn sys_fb_open(info_ptr: u64, _flags: u64) -> i64 {
    if info_ptr == 0 || !raw::check_user_ptr(info_ptr) {
        return Errno::EFAULT.as_ret();
    }

    let fb_addr =
        crate::kernel::framework::driver::display::FB_PHYS_ADDR.load(core::sync::atomic::Ordering::Acquire);
    if fb_addr == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_size =
        crate::kernel::framework::driver::display::FB_PHYS_SIZE.load(core::sync::atomic::Ordering::Acquire);

    let (width, height, pitch, bpp) = match crate::kernel::framework::driver::display::get_framebuffer() {
        Some(fb) => (
            fb.width(),
            fb.height(),
            fb.pitch(),
            fb.format().bits_per_pixel() as u8,
        ),
        None => return Errno::ENODEV.as_ret(),
    };

    let info = FbInfo {
        phys_addr: fb_addr,
        size: fb_size,
        width,
        height,
        pitch,
        bpp,
        _pad: [0; 3],
    };

    let dst = info_ptr as *mut FbInfo;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { raw::write_struct(dst, &info) };
    0
}

fn sys_fb_mmap(target_vaddr: u64, size: u64, _prot: u64) -> i64 {
    if target_vaddr == 0 || target_vaddr & 0xFFF != 0 {
        return Errno::EINVAL.as_ret();
    }
    if target_vaddr > USER_ADDR_MAX - size {
        return Errno::EINVAL.as_ret();
    }

    let fb_phys =
        crate::kernel::framework::driver::display::FB_PHYS_ADDR.load(core::sync::atomic::Ordering::Acquire);
    if fb_phys == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_total =
        crate::kernel::framework::driver::display::FB_PHYS_SIZE.load(core::sync::atomic::Ordering::Acquire);
    if size > fb_total {
        return Errno::EINVAL.as_ret();
    }

    let cr3 =
        crate::kernel::framework::proc::user_proc::user_entry_cr3.load(core::sync::atomic::Ordering::SeqCst);
    if cr3 == 0 {
        return Errno::ENODEV.as_ret();
    }

    let vmm = crate::kernel::framework::mm::vmm::get_vmm();
    let flags = crate::kernel::framework::mm::PageFlags::PRESENT
        | crate::kernel::framework::mm::PageFlags::WRITABLE
        | crate::kernel::framework::mm::PageFlags::USER
        | crate::kernel::framework::mm::PageFlags::WRITE_THROUGH;

    let phys_page_aligned = fb_phys & !0xFFF;
    let offset = fb_phys - phys_page_aligned;
    let pages = (size + offset).div_ceil(0x1000);

    for i in 0..pages {
        let pa = crate::kernel::framework::mm::PhysAddr(phys_page_aligned + i * 0x1000);
        let va = crate::kernel::framework::mm::VirtAddr(target_vaddr + i * 0x1000);
        vmm.map_page_in_table(cr3, va, pa, flags);
    }

    target_vaddr as i64
}

fn sys_fb_release(_vaddr: u64) -> i64 {
    0
}

// ============================================================================
// raw 子模块 — 集中所有 unsafe 操作与 FFI 声明
// ============================================================================
//
// 设计目的：
// 1. 隔离 unsafe 到单一文件作用域，降低 sys_* 业务函数的认知负载
// 2. 复用 services/credo、services/barrier 的"raw 子模块"模式
// 3. 为 Phase 2.5.1 的 60+ unsafe 函数提供统一的 SAFETY 注释入口
//
// 调用契约：
// - 所有 read_* / write_* 函数均要求调用方先调用 check_user_ptr 或
//   check_user_buf 完成边界校验；否则会触发 UAF/越界写。
// - 所有 FFI 包装函数 (sm_*_call、read_keyboard_byte 等) 假定在中断
//   上下文中调用，不可在持锁睡眠上下文中调用。
// ============================================================================

pub(crate) mod raw {
    // ============= 集中 FFI 声明 =============
    extern "C" {
        // 时间
        fn timer_get_ticks() -> u64;
        // 串口 (COM1/COM2)
        fn serial_has_data(com: i32) -> bool;
        fn serial_getc(com: i32) -> i32;
        // 物理内存分配 (PMM FFI 桥)
        fn pmm_alloc_pages(count: u64) -> *mut u8;
        fn pmm_free_pages(addr: *mut u8, count: u64);
        // smoltcp 网络栈
        fn sm_socket(domain: i32, sock_type: i32, protocol: i32) -> i32;
        fn sm_bind(sockfd: i32, addr: *const u8, addrlen: u32) -> i32;
        fn sm_listen(sockfd: i32, backlog: i32) -> i32;
        fn sm_accept(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32;
        fn sm_connect(sockfd: i32, addr: *const u8, addrlen: u32) -> i32;
        fn sm_send(sockfd: i32, buf: *const u8, len: u32, flags: i32) -> i32;
        fn sm_recv(sockfd: i32, buf: *mut u8, len: u32, flags: i32) -> i32;
        fn sm_close(sockfd: i32) -> i32;
        fn sm_setsockopt(
            sockfd: i32,
            level: i32,
            optname: i32,
            optval: *const u8,
            optlen: u32,
        ) -> i32;
        fn sm_getsockopt(
            sockfd: i32,
            level: i32,
            optname: i32,
            optval: *mut u8,
            optlen: *mut u32,
        ) -> i32;
        fn sm_getsockname(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32;
        fn sm_getpeername(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32;
        fn sm_sendmsg(fd: i32, msg: *const u8, flags: i32) -> i32;
        fn sm_recvmsg(fd: i32, msg: *mut u8, flags: i32) -> i32;
        // 链接器符号
        static _kernel_start: u8;
        static _kernel_end: u8;
    }

    // x86_64 专属: 键盘
    #[cfg(target_arch = "x86_64")]
    extern "C" {
        fn keyboard_has_data() -> bool;
        fn keyboard_get_char() -> i32;
    }

    // ============= 用户指针校验（safe 包装） =============

    /// 校验单个用户指针是否在合法范围 [1, USER_ADDR_MAX)
    pub fn check_user_ptr(ptr: u64) -> bool {
        super::validate_user_ptr(ptr)
    }

    /// 校验用户缓冲区 [ptr, ptr+len) 是否完全在用户空间
    pub fn check_user_buf(ptr: u64, len: u64) -> bool {
        super::validate_user_buf(ptr, len)
    }

    // ============= 用户态读写助手（unsafe 集中点） =============

    /// 写一个 u8 到用户指针。
    /// # Safety
    /// 调用方必须先调用 `check_user_ptr(ptr as u64)` 验证指针合法。
    pub unsafe fn write_u8(ptr: *mut u8, val: u8) {
        // SAFETY: 调用方已通过 `check_user_ptr` 验证 ptr 指向有效且对齐的
        // 用户空间地址 (1 字节自然对齐)；write_volatile 防止编译器优化掉
        // 设备/共享内存访问。
        unsafe { core::ptr::write_volatile(ptr, val) }
    }

    /// 写一个 u32 到用户指针。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 4)` 验证。
    pub unsafe fn write_u32(ptr: *mut u32, val: u32) {
        // SAFETY: 调用方已验证 ptr 对齐到 4 字节且指向 4 字节可写用户空间。
        unsafe { core::ptr::write_volatile(ptr, val) }
    }

    /// 写一个 u64 到用户指针。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 8)` 验证。
    pub unsafe fn write_u64(ptr: *mut u64, val: u64) {
        // SAFETY: 调用方已验证 ptr 对齐到 8 字节且指向 8 字节可写用户空间。
        unsafe { core::ptr::write_volatile(ptr, val) }
    }

    /// 写两个 u64 到用户指针 (用于 rlimit cur/max)。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 16)` 验证。
    pub unsafe fn write_u64_pair(ptr: *mut u64, cur: u64, max: u64) {
        // SAFETY: 调用方已验证 ptr 对齐到 8 字节且指向 16 字节可写用户空间。
        unsafe {
            core::ptr::write_volatile(ptr, cur);
            core::ptr::write_volatile(ptr.add(1), max);
        }
    }

    /// 读一个 u8。
    /// # Safety
    /// 调用方必须先调用 `check_user_ptr(ptr as u64)` 验证。
    pub unsafe fn read_u8(ptr: *const u8) -> u8 {
        // SAFETY: 调用方已验证 ptr 指向有效可读用户地址。
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// 读一个 u64。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 8)` 验证。
    pub unsafe fn read_u64(ptr: *const u64) -> u64 {
        // SAFETY: 调用方已验证 ptr 对齐到 8 字节且指向 8 字节可读用户空间。
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// 从 src 复制 len 字节到用户指针 dst。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(dst as u64, len)` 验证。
    pub unsafe fn write_bytes(dst: *mut u8, src: &[u8]) {
        // SAFETY: 调用方已验证 dst 指向 len 字节可写用户空间；src 是有效
        // Rust 切片不会越界；copy_nonoverlapping 要求两区域不重叠，
        // src 来自内核栈/数据段，与用户空间地址不会重叠。
        unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len()) }
    }

    /// 从用户指针读取 len 字节构造 slice。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, len)` 验证。
    pub unsafe fn read_slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
        // SAFETY: 调用方已验证 [ptr, ptr+len) 完全在合法可读用户空间。
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }

    /// 复制结构体到用户指针（repr(C) 类型）。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, size_of::<T>())` 验证。
    pub unsafe fn write_struct<T: Copy>(dst: *mut T, src: &T) {
        // SAFETY: 调用方已验证 dst 对齐到 align_of::<T>() 且 size_of::<T>()
        // 字节可写；src 是有效 T 引用。write_volatile 保留顺序语义。
        unsafe { core::ptr::write_volatile(dst, *src) }
    }

    /// Safe 包装: 在 services 层用, 写一个 repr(C) 结构体到 user 指针.
    ///
    /// 调用方无需 unsafe 块. 内部先 `check_user_buf` 验证后写.
    pub fn write_struct_to_user<T: Copy>(dst_ptr: u64, src: &T) -> bool {
        if dst_ptr == 0 {
            return false;
        }
        let size = core::mem::size_of::<T>() as u64;
        if !check_user_buf(dst_ptr, size) {
            return false;
        }
        // SAFETY: 上方 check_user_buf 已验证 dst_ptr 指向的 user 缓冲
        // 至少有 size_of::<T>() 字节可写, 且 src 持有有效 T 值.
        unsafe { write_struct(dst_ptr as *mut T, src) }
        true
    }

    /// Safe 包装: 写两个 u64 到 user 指针 (rlim_cur, rlim_max).
    pub fn write_rlimit_to_user(ptr: u64, cur: u64, max: u64) -> bool {
        if ptr == 0 {
            return false;
        }
        if !check_user_buf(ptr, 16) {
            return false;
        }
        // SAFETY: check_user_buf 已验证 16 字节可写
        unsafe { write_u64_pair(ptr as *mut u64, cur, max) }
        true
    }

    /// Safe 包装: 在 services 层用, 写一个 u64 到 user 指针.
    ///
    /// 调用方无需 unsafe 块. 内部先 `check_user_buf(dst, 8)` 验证.
    pub fn write_u64_to_user(dst_ptr: u64, val: u64) -> bool {
        if dst_ptr == 0 {
            return false;
        }
        if !check_user_buf(dst_ptr, 8) {
            return false;
        }
        // SAFETY: check_user_buf 已验证 8 字节可写
        unsafe { core::ptr::write_unaligned(dst_ptr as *mut u64, val) };
        true
    }

    /// Safe 包装: 从 user 指针读一个 repr(C) 结构体.
    /// 调用方无需 unsafe 块. 内部先 `check_user_buf` 验证后读.
    pub fn read_struct_from_user<T: Copy>(src_ptr: u64, dst: &mut T) -> bool {
        if src_ptr == 0 {
            return false;
        }
        let size = core::mem::size_of::<T>() as u64;
        if !check_user_buf(src_ptr, size) {
            return false;
        }
        // SAFETY: check_user_buf 已验证 src_ptr 指向的 user 缓冲
        // 至少有 size_of::<T>() 字节可读.
        *dst = unsafe { core::ptr::read_unaligned(src_ptr as *const T) };
        true
    }

    // ============= 设备输入抽象 =============

    /// 从键盘读取一个字节（x86_64 专属）。None 表示无数据。
    /// # Safety
    /// FFI 调用，需在中断上下文。
    #[cfg(target_arch = "x86_64")]
    pub fn read_keyboard_byte() -> Option<u8> {
        // SAFETY: keyboard_has_data 与 keyboard_get_char 是 C-ABI 函数，
        // 调用方保证在中断上下文 (disable_interrupts 已持有)。
        unsafe {
            if keyboard_has_data() {
                let c = keyboard_get_char();
                if c > 0 {
                    Some(c as u8)
                } else {
                    None
                }
            } else {
                None
            }
        }
    }

    /// 从串口读取一个字节。None 表示无数据。
    /// # Safety
    /// FFI 调用，需在中断上下文。
    pub fn read_serial_byte(com: i32) -> Option<u8> {
        // SAFETY: serial_has_data 与 serial_getc 是 C-ABI 函数，调用方
        // 保证 com 端口已通过 ioport_register 注册。
        unsafe {
            if serial_has_data(com) {
                let c = serial_getc(com);
                if c > 0 {
                    Some(c as u8)
                } else {
                    None
                }
            } else {
                None
            }
        }
    }

    // ============= 物理内存分配 =============

    /// 分配 count 个连续物理页。
    /// # Safety
    /// FFI 调用，返回的指针是物理地址（需 HHDM 转换）。
    pub fn alloc_pages(count: u64) -> *mut u8 {
        // SAFETY: pmm_alloc_pages 是 C-ABI 物理页分配器，count 个连续
        // 4KiB 页的分配请求；若失败返回 null。
        unsafe { pmm_alloc_pages(count) }
    }

    /// 释放 count 个连续物理页。
    /// # Safety
    /// FFI 调用，addr 必须是 alloc_pages 返回的同一基址。
    pub fn free_pages(addr: *mut u8, count: u64) {
        // SAFETY: 调用方已验证 addr 是先前 alloc_pages 的返回值且未释放。
        unsafe { pmm_free_pages(addr, count) }
    }

    // ============= 时间 =============

    /// 从用户指针读一个 u64. None 表示失败.
    /// 调用方无需 unsafe 块. 内部校验 + 读.
    pub fn read_u64_from_user(src_ptr: u64) -> Option<u64> {
        if src_ptr == 0 {
            return None;
        }
        if !check_user_buf(src_ptr, 8) {
            return None;
        }
        // SAFETY: check_user_buf 已验证 src_ptr 8 字节可读.
        Some(unsafe { core::ptr::read_unaligned(src_ptr as *const u64) })
    }

    /// 读取 tick 计数（1ms 粒度）。
    /// # Safety
    /// FFI 调用，硬件定时器寄存器读取。
    pub fn get_ticks() -> u64 {
        // SAFETY: timer_get_ticks 是 C-ABI 函数，读取 PIT/HPET 计数器，
        // 无副作用。
        unsafe { timer_get_ticks() }
    }

    // ============= smoltcp 网络栈 FFI 包装 =============

    /// # Safety
    /// FFI 调用，需在中断上下文。
    pub fn sm_socket_call(domain: i32, sock_type: i32, protocol: i32) -> i32 {
        // SAFETY: sm_socket 是 C-ABI 套接字创建函数，无指针参数。
        unsafe { sm_socket(domain, sock_type, protocol) }
    }

    /// # Safety
    /// FFI 调用，addr/addrlen 由调用方负责用户态校验。
    pub fn sm_bind_call(sockfd: i32, addr: *const u8, addrlen: u32) -> i32 {
        // SAFETY: 调用方已通过 check_user_buf 验证 addr 指向 addrlen 字节
        // 可读用户空间。
        unsafe { sm_bind(sockfd, addr, addrlen) }
    }

    /// # Safety
    /// FFI 调用。
    pub fn sm_listen_call(sockfd: i32, backlog: i32) -> i32 {
        // SAFETY: sm_listen 是 C-ABI 函数，无指针参数。
        unsafe { sm_listen(sockfd, backlog) }
    }

    /// # Safety
    /// FFI 调用，addr/addrlen 由调用方负责用户态校验。
    pub fn sm_accept_call(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
        // SAFETY: 调用方已验证 addr/addrlen 是合法可写用户指针。
        unsafe { sm_accept(sockfd, addr, addrlen) }
    }

    /// # Safety
    /// FFI 调用，addr/addrlen 由调用方负责用户态校验。
    pub fn sm_connect_call(sockfd: i32, addr: *const u8, addrlen: u32) -> i32 {
        // SAFETY: 调用方已验证 addr 指向合法可读用户空间。
        unsafe { sm_connect(sockfd, addr, addrlen) }
    }

    /// # Safety
    /// FFI 调用，buf 由调用方负责用户态校验。
    pub fn sm_send_call(sockfd: i32, buf: *const u8, len: u32, flags: i32) -> i32 {
        // SAFETY: 调用方已验证 buf 指向 len 字节可读用户空间。
        unsafe { sm_send(sockfd, buf, len, flags) }
    }

    /// # Safety
    /// FFI 调用，buf 由调用方负责用户态校验。
    pub fn sm_recv_call(sockfd: i32, buf: *mut u8, len: u32, flags: i32) -> i32 {
        // SAFETY: 调用方已验证 buf 指向 len 字节可写用户空间。
        unsafe { sm_recv(sockfd, buf, len, flags) }
    }

    /// # Safety
    /// FFI 调用。
    pub fn sm_close_call(sockfd: i32) -> i32 {
        // SAFETY: sm_close 是 C-ABI 函数，无指针参数。
        unsafe { sm_close(sockfd) }
    }

    /// # Safety
    /// FFI 调用，optval 由调用方负责用户态校验。
    pub fn sm_setsockopt_call(
        sockfd: i32,
        level: i32,
        optname: i32,
        optval: *const u8,
        optlen: u32,
    ) -> i32 {
        // SAFETY: 调用方已验证 optval 指向 optlen 字节可读用户空间。
        unsafe { sm_setsockopt(sockfd, level, optname, optval, optlen) }
    }

    /// # Safety
    /// FFI 调用，optval/optlen 由调用方负责用户态校验。
    pub fn sm_getsockopt_call(
        sockfd: i32,
        level: i32,
        optname: i32,
        optval: *mut u8,
        optlen: *mut u32,
    ) -> i32 {
        // SAFETY: 调用方已验证 optval/optlen 是合法可写用户指针。
        unsafe { sm_getsockopt(sockfd, level, optname, optval, optlen) }
    }

    /// # Safety
    /// FFI 调用，addr/addrlen 由调用方负责用户态校验 (可写)。
    pub fn sm_getsockname_call(
        sockfd: i32,
        addr: u64,
        addrlen: u64,
    ) -> i32 {
        // SAFETY: addr/addrlen 经 services 校验, 至少 sizeof(SockaddrIn)=16 字节可写.
        unsafe { sm_getsockname(sockfd, addr as *mut u8, addrlen as *mut u32) }
    }

    /// # Safety
    /// FFI 调用，addr/addrlen 由调用方负责用户态校验 (可写)。
    pub fn sm_getpeername_call(
            sockfd: i32,
            addr: u64,
            addrlen: u64,
        ) -> i32 {
            // SAFETY: addr/addrlen 经 services 校验, 至少 sizeof(SockaddrIn)=16 字节可写.
            unsafe { sm_getpeername(sockfd, addr as *mut u8, addrlen as *mut u32) }
        }

        /// # Safety
        /// FFI 调用，`msg` 由 services 校验 msghdr 布局 (iov 范围、iovlen 个 iovec 可读)。
        pub fn sm_sendmsg_call(fd: i32, msg: u64, flags: i32) -> i32 {
            // SAFETY: msg 经 services 校验完整 Msghdr (56 字节) 可读, iovlen 个 Iovec 可读.
            unsafe { sm_sendmsg(fd, msg as *const u8, flags) }
        }

        /// # Safety
        /// FFI 调用，`msg` 由 services 校验 msghdr 布局 (iov 范围、iovlen 个 iovec 可写)。
        pub fn sm_recvmsg_call(fd: i32, msg: u64, flags: i32) -> i32 {
            // SAFETY: msg 经 services 校验完整 Msghdr (56 字节) 可写, iovlen 个 Iovec 可写.
            unsafe { sm_recvmsg(fd, msg as *mut u8, flags) }
        }

    // ============= 链接器符号访问 =============

    /// 内核映像起始虚拟地址。
    /// # Safety
    /// 链接器符号，仅在 boot 后有效。
    pub fn kernel_start_ptr() -> *const u8 {
        // SAFETY: _kernel_start 是链接器符号 (extern "C")，是静态地址，
        // boot 后由 VMM 建立映射可读。
        unsafe { &_kernel_start as *const u8 }
    }

    /// 内核映像结束物理地址（已减 HHDM_OFFSET）。
    /// # SAFETY: 链接器符号，hhdm_offset 必须与启动时一致。
    pub fn kernel_end_phys(hhdm_offset: usize) -> usize {
        unsafe { (&_kernel_end as *const u8 as usize).wrapping_sub(hhdm_offset) }
    }

    /// 用户地址空间上限（导出方便 raw 内部判断）。
    pub const fn user_addr_max() -> u64 {
        super::USER_ADDR_MAX
    }

    // ============= CPU 控制指令集中点 =============

    /// 加载空 IDT 后触发异常，重启 CPU（x86_64）。
    /// # SAFETY: 不返回；调用方须确保已关闭其他 CPU。
    #[cfg(target_arch = "x86_64")]
    pub unsafe fn reboot_via_idt() -> ! {
        core::arch::asm!(
            "lidt [rdi]",
            "int 0",
            in("rdi") &[0u16; 4],
            options(nostack, nomem)
        );
        loop {}
    }

    /// 通过 SVC 触发 PSCI reset（aarch64）。
    /// # SAFETY: 不返回；调用方须确保已关闭其他 CPU。
    #[cfg(target_arch = "aarch64")]
    pub unsafe fn reboot_via_psci() -> ! {
        core::arch::asm!("svc #0", in("x0") 0u64, options(nostack));
        loop {}
    }
}
