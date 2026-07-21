pub mod api;
pub mod brk;
pub mod canary;
pub mod clone;
pub mod epoll;
pub mod eventfd;
pub mod firmware;
pub mod signalfd;
pub mod timerfd;
pub mod futex;
pub mod info;
pub mod io;
pub mod madvise_mlock;
pub mod mmap;
pub mod mprotect;
pub mod sendfile;
pub mod ftrace_kgdb;
pub mod posix_timer;
pub mod linuxulator;
pub mod wait4;
/// T-03: 系统调用分发决策 trait
pub mod dispatch_trait;

/// Syscall 模块 — QueenX 原生系统调用分发
///
/// 编号空间 (遵循 queenx-naming-standpoint.md):
///   0-299   : 保留给未来 linuxulator (与 Linux 1:1 映射)
///   300-399 : 保留
///   400-499 : Credo 私有 syscall
///   500-599 : 进程 / 内存 / 文件基础 (QX_*)
///   600-699 : 网络 / IPC (QX_*)
///   700-799 : 设备 / 系统 (QX_*)
///   800-899 : 扩展 (QX_*)
///
/// Linux 兼容二进制通过 linuxulator 模块将架构特定编号翻译为 QX_* 编号。

// 公共接口 re-export — 避免跨子系统直接访问内部子模块
pub use epoll::{EPOLLIN, EPOLLOUT, EPOLLERR, EPOLLHUP, EPOLLRDHUP, epoll_pwake};
pub use types::{Errno, SyscallHandler};
pub use types::*;
pub use sendfile::{sys_sendfile, sys_splice, SPLICE_F_MOVE, SPLICE_F_NONBLOCK, SPLICE_F_MORE, SPLICE_F_GIFT};

// dispatch_trait 公共接口 re-export — T-03 策略-机制分离
pub use dispatch_trait::{SyscallDispatch, FallbackSyscallDispatch, register_syscall_dispatch, current_syscall_dispatch};
pub mod types;

#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::idt::InterruptFrame;
// types 已通过 pub use types::* re-export, 此处不再 use
use core::sync::atomic::Ordering;

const USER_ADDR_MAX: u64 = 0x7FFFFFFFE000;

pub fn validate_user_ptr(ptr: u64) -> bool {
    crate::kernel::framework::userptr::validate_user_ptr(ptr)
}

pub fn validate_user_buf(ptr: u64, len: u64) -> bool {
    crate::kernel::framework::userptr::validate_user_buf(ptr, len)
}

#[unsafe(no_mangle)]
///
/// # Safety
///
/// 调用者处于内核上下文. `ptr` 是已校验的用户态指针.
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

    // 注册 epoll 的 fd 关闭通知回调, 解耦 fs→syscall 依赖
    // SAFETY: epoll_pwake 是 'static 函数指针, 在内核运行期间始终有效.
    unsafe {
        crate::kernel::framework::fd_notify::register_pwake(
            crate::kernel::framework::syscall::epoll::epoll_pwake,
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
///
/// # Safety
///
/// 调用者处于内核上下文. `ptr` 是已校验的用户态字符串指针.
pub unsafe extern "C" fn syscall_dispatch_from_frame(frame: *mut InterruptFrame) { unsafe {
    if frame.is_null() {
        return;
    }
    let f = &mut *frame;
    let syscall_num = f.rax;

    // rt_sigreturn 特殊处理: 需要直接修改 frame, 不走正常 dispatch
    // 使用架构无关的翻译层判断
    if linuxulator::is_rt_sigreturn(syscall_num) {
        // 从用户栈上的 SignalFrame 恢复寄存器
        // 布局: rsp+0=返回地址, rsp+8=SignalFrame
        let sigframe_ptr = (f.rsp + 8) as *const crate::kernel::framework::proc::SignalFrame;
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
    crate::kernel::framework::proc::do_signal_deliver(frame);
}}

macro_rules! dispatch {
    ($num:expr_2021, $name:expr_2021) => {{
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

#[unsafe(no_mangle)]
///
/// # Safety
///
/// 从中断上下文 (int 0x80) 调用. 所有寄存器值来自被打断的用户上下文.
pub unsafe extern "C" fn syscall_dispatch(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    // TD-10: 进入内核态, tick 期间 sys_time 累加.
    crate::kernel::framework::proc::proc_set_in_kern(1);
    let result = syscall_dispatch_impl(num, a0, a1, a2, a3, a4, a5);
    // 出口恢复用户态, tick 期间 user_time 累加.
    crate::kernel::framework::proc::proc_set_in_kern(0);
    result
}

fn syscall_dispatch_impl(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    // 直接 Linux ABI: syscall 编号直接使用 Linux 标准编号, 无需翻译

    // C7: Seccomp 过滤检查 (在 dispatch 之前)
    let args = [a0, a1, a2, a3, a4, a5];
    if let Some(ret) = crate::kernel::framework::proc::seccomp_check(num, &args) {
        return ret;
    }

    // L-01: 优先委托 services 层策略分发
    // services 可处理已迁移的 syscall; 返回 -ENOSYS (38) 表示未处理
    let svc_ret = dispatch_trait::current_syscall_dispatch().dispatch(num, args);
    if svc_ret != -38 {
        return svc_ret;
    }

    // framework 回退: 处理尚未迁移到 services 的 syscall
    match num {
        // ==================== 文件 I/O ====================
        SYS_read => dispatch!(sys_read(a0 as i32, a1 as *mut u8, a2), b"read\0"),
        SYS_write => dispatch!(sys_write(a0 as i32, a1 as *const u8, a2), b"write\0"),
        // 已迁移: SYS_poll, SYS_lseek

        // ==================== 内存管理 ====================
        // 已迁移: SYS_mmap, SYS_munmap
        SYS_mremap => {
            // 从当前 task 取 MmStruct; 验证后委托 services/mm/mremap
            use crate::kernel::framework::mm::vma_get_current_mm;
            match vma_get_current_mm() {
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
        QX_RT_SIGRETURN => dispatch!(sys_rt_sigreturn(), b"rt_sigreturn\0"),
        // P1-I-45: 替代栈注册/查询系统调用

        // ==================== 设备 ====================
        // 已迁移: QX_IOCTL

        // 730-733: 设备固件加载
        QX_FW_LOAD => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_load(a0, a1, a2, a3),
            b"fw_load\0"
        ),
        QX_FW_GET => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_get(a0, a1, a2, a3),
            b"fw_get\0"
        ),
        QX_FW_GET_INFO => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_get_info(a0, a1),
            b"fw_get_info\0"
        ),
        QX_FW_DETACH => dispatch!(
            crate::kernel::framework::syscall::firmware::sys_fw_detach(a0),
            b"fw_detach\0"
        ),

        // ==================== POSIX Timer (740-745) ====================
        // 已迁移: QX_TIMER_CREATE, QX_TIMER_SETTIME, QX_TIMER_GETTIME, QX_TIMER_DELETE,
        //         QX_TIMER_GETOVERRUN, QX_CLOCK_GETRES (T5-1 → 服务层系统调用分发)

        // ==================== 熵源 / Stack Canary (746-747) ====================
        // 已迁移: QX_GETRANDOM, QX_GET_CANARY (T5-1 → 服务层系统调用分发)

        // ==================== 内存建议与锁定 (760-765, P1 #15) ====================
        // 已迁移: QX_MADVISE, QX_MLOCK, QX_MUNLOCK, QX_MLOCKALL, QX_MUNLOCKALL, QX_MINCORE

        // ==================== 调试 / 跟踪 (800-809) ====================
        QX_FTRACE_ENABLE => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_enable(),
            b"ftrace_enable\0"
        ),
        QX_FTRACE_DISABLE => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_disable(),
            b"ftrace_disable\0"
        ),
        QX_FTRACE_READ => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_read(a0),
            b"ftrace_read\0"
        ),
        QX_FTRACE_STAT => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_ftrace_stat(a0),
            b"ftrace_stat\0"
        ),
        QX_KGDB_ENTER => dispatch!(
            crate::kernel::framework::syscall::ftrace_kgdb::sys_kgdb_enter(),
            b"kgdb_enter\0"
        ),

        // ==================== C7: Seccomp / prctl ====================
        QX_SECCOMP => dispatch!(
            crate::kernel::framework::proc::sys_seccomp(a0 as u32, a1 as u32, a2),
            b"seccomp\0"
        ),
        QX_PRCTL => dispatch!(
            crate::kernel::framework::proc::sys_prctl_prctl(a0 as i64, a1, a2, a3, a4),
            b"prctl\0"
        ),

        // ==================== C5: 路由表 ====================
        QX_ROUTE_ADD => dispatch!(
            crate::kernel::framework::net::route::sys_route_add(a0, a1, a2),
            b"route_add\0"
        ),
        QX_ROUTE_DEL => dispatch!(
            crate::kernel::framework::net::route::sys_route_del(a0, a1, a2),
            b"route_del\0"
        ),
        QX_ROUTE_QUERY => dispatch!(
            crate::kernel::framework::net::route::sys_route_query(a0),
            b"route_query\0"
        ),

        // ==================== C5: Netfilter ====================
        QX_NF_ADD_RULE => dispatch!(
            crate::kernel::framework::net::netfilter::sys_nf_add_rule(a0, a1, a2, a3, a4, a5),
            b"nf_add_rule\0"
        ),
        QX_NF_DEL_RULE => dispatch!(
            crate::kernel::framework::net::netfilter::sys_nf_del_rule(a0, a1),
            b"nf_del_rule\0"
        ),

        // ==================== C4: io_uring ====================
        QX_IO_URING_SETUP => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_setup(a0),
            b"io_uring_setup\0"
        ),
        QX_IO_URING_ENTER => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_enter(a0, a1, a2),
            b"io_uring_enter\0"
        ),
        QX_IO_URING_REGISTER => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_register(a0, a1, a2, a3),
            b"io_uring_register\0"
        ),
        QX_IO_URING_SUBMIT => dispatch!(
            crate::kernel::framework::io::iouring::sys_io_uring_submit_sqe(a0, a1, a2, a3, a4, a5),
            b"io_uring_submit\0"
        ),

        // ==================== D1: Namespace ====================
        QX_UNSHARE => dispatch!(
            crate::kernel::framework::proc::sys_unshare(a0),
            b"unshare\0"
        ),
        QX_SETNS => dispatch!(
            crate::kernel::framework::proc::sys_setns(a0, a1),
            b"setns\0"
        ),

        // ==================== D2: cgroup ====================
        QX_CGROUP_CREATE => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_create(a0, a1, a2),
            b"cgroup_create\0"
        ),
        QX_CGROUP_DESTROY => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_destroy(a0),
            b"cgroup_destroy\0"
        ),
        QX_CGROUP_ATTACH => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_attach(a0, a1),
            b"cgroup_attach\0"
        ),
        QX_CGROUP_SET_LIMIT => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_set_limit(a0, a1, a2),
            b"cgroup_set_limit\0"
        ),
        QX_CGROUP_GET_STAT => dispatch!(
            crate::kernel::framework::proc::sys_cgroup_get_stat(a0, a1),
            b"cgroup_get_stat\0"
        ),

        // ==================== D3: NUMA ====================
        // 已迁移: QX_GET_MEMPOLICY, QX_SET_MEMPOLICY, QX_MIGRATE_PAGES, QX_GETCPU
        //         (T5-1 → 服务层系统调用分发)

        // ==================== D4: eBPF ====================
        QX_BPF => dispatch!(
            crate::kernel::framework::debug::sys_bpf(a0, a1, a2),
            b"bpf\0"
        ),

        // ==================== D5: 电源管理 ====================
        QX_PM => dispatch!(
            crate::kernel::framework::driver::sys_pm(a0, a1, a2),
            b"pm\0"
        ),

        // ==================== D6: 安全启动 + TPM ====================
        QX_SECURE_BOOT => dispatch!(
            crate::kernel::framework::credo::sys_secure_boot(a0, a1, a2, a3),
            b"secure_boot\0"
        ),
        QX_TPM => dispatch!(
            crate::kernel::framework::credo::sys_tpm(a0, a1, a2, a3),
            b"tpm\0"
        ),

        // ==================== D7: Shadow Stack (CET) ====================
        QX_CET => dispatch!(
            crate::kernel::framework::arch::shadow_stack::sys_cet(a0, a1, a2),
            b"cet\0"
        ),

        // ==================== D8: Tickless (NO_HZ) ====================  // 动态时钟节拍模式
        QX_TICKLESS => dispatch!(
            crate::kernel::framework::timer::sys_tickless(a0, a1, a2),
            b"tickless\0"
        ),

        // ==================== D9: NTP/PTP 时钟同步 ====================
        QX_TIMESYNC => dispatch!(
            crate::kernel::framework::timer::sys_timesync(a0, a1, a2),
            b"timesync\0"
        ),

        // ==================== D10: kexec ====================
        QX_KEXEC => dispatch!(
            crate::kernel::framework::driver::sys_kexec(a0, a1, a2, a3),
            b"kexec\0"
        ),

        // ==================== D11: UEFI ====================
        QX_UEFI => dispatch!(
            crate::kernel::framework::driver::sys_uefi(a0, a1, a2),
            b"uefi\0"
        ),

        // ==================== 文件访问 ====================
        // 已迁移: QX_SELECT, QX_SCHED_YIELD, QX_SCHED_SETAFFINITY, QX_SCHED_GETAFFINITY

        // ==================== 文件描述符 ====================

        // 已迁移: 进程优先级 (getpriority/setpriority)

        // 已迁移: QX_NANOSLEEP, QX_GETITIMER, QX_ALARM, QX_SETITIMER

        // ==================== 进程 (getpriority/setpriority 已迁移到 services) ====================
        QX_TCGETPGRP => dispatch!(
            crate::kernel::framework::proc::session::sys_tcgetpgrp(a0 as i32),
            b"tcgetpgrp\0"
        ),
        QX_TCSETPGRP => dispatch!(
            crate::kernel::framework::proc::session::sys_tcsetpgrp(a0 as i32, a1 as i32),
            b"tcsetpgrp\0"
        ),

        // ==================== 网络 (services 代理) ====================
        #[cfg(feature = "net")]
        QX_GETSOCKNAME => dispatch!(sys_getsockname(a0 as i32, a1, a2), b"getsockname\0"),
        #[cfg(feature = "net")]
        QX_GETPEERNAME => dispatch!(sys_getpeername(a0 as i32, a1, a2), b"getpeername\0"),
        // 已迁移: QX_GETRUSAGE
        #[cfg(not(feature = "net"))]
        QX_SOCKET | QX_CONNECT | QX_ACCEPT | QX_SENDTO | QX_RECVFROM | QX_SHUTDOWN
        | QX_BIND | QX_LISTEN | QX_SENDMSG | QX_RECVMSG | QX_SETSOCKOPT | QX_GETSOCKOPT
        | QX_GETSOCKNAME | QX_GETPEERNAME => {
            dispatch!(Errno::ENOSYS.as_ret(), b"net_nosys\0")
        }

        // ==================== 进程创建 ====================
        // 已迁移: QX_FORK, QX_CLONE
        QX_EXECVE => dispatch!(
            crate::kernel::services::proc::execve::ExecveResult::from_ret(
                sys_execve(
                    a0 as *const u8,
                    a1 as *const *const u8,
                    a2 as *const *const u8
                )
            ).as_ret(),
            b"execve\0"
        ),
        // 已迁移: QX_EXIT, QX_WAIT4

        // ==================== 系统信息 ====================
        // 已迁移: QX_UNAME, QX_GETTIMEOFDAY

        // ==================== 文件描述符操作 ====================

        // 已迁移: QX_FLOCK

        // 已迁移: QX_TRUNCATE, QX_FTRUNCATE

        // 已迁移: QX_GETDENTS

        // ==================== 路径 (services 代理) ====================

        // ==================== 文件操作 ====================
        // 已迁移: QX_LINK

        // ==================== 文件权限 ====================
        // 已迁移: QX_CHOWN

        // ==================== 时间 ====================
        // 已迁移: QX_GETTIMEOFDAY, QX_GETRLIMIT
        QX_SETRLIMIT => dispatch!(
            crate::kernel::framework::proc::sys_setrlimit(a0 as i32, a1),
            b"setrlimit\0"
        ),
        // 已迁移: QX_SYSINFO, QX_TIMES

        // ==================== 用户/组 (services 代理) ====================

        // ==================== 文件同步/挂载 ====================

        // 已迁移: QX_TIME, QX_CLOCK_GETTIME, QX_EXIT_GROUP
        QX_TGKILL => dispatch!(sys_tgkill(a0 as i32, a1 as i32, a2 as i32), b"tgkill\0"),

        // ==================== 同步 ====================

        // ==================== 事件轮询 ====================
        // 已迁移: QX_EPOLL_CREATE, QX_EPOLL_CTL, QX_EPOLL_WAIT

        // ==================== eventfd / signalfd / timerfd ====================
        // 已迁移: QX_EVENTFD, QX_EVENTFD2, QX_SIGNALFD, QX_SIGNALFD4, QX_TIMERFD_CREATE, QX_TIMERFD_SETTIME, QX_TIMERFD_GETTIME

        // 已迁移: inotify syscall (640+)

        // ==================== sendfile / splice syscall (650+) ====================  // 高效文件传输
        QX_SENDFILE => dispatch!(
            crate::kernel::framework::syscall::sendfile::sys_sendfile(
                a0 as i32, a1 as i32, a2, a3 as usize
            ),
            b"sendfile\0"
        ),
        QX_SPLICE => dispatch!(
            crate::kernel::framework::syscall::sendfile::sys_splice(
                a0 as i32, a1, a2 as i32, a3, a4 as usize, a5 as u32
            ),
            b"splice\0"
        ),

        // ==================== Credo 私有 syscall (400+) ====================
        // 已迁移: SYS_CREDO_LOGIN ~ SYS_CREDO_SET_PWM, SYS_CREDO_DISK_LIST/INFO/FORMAT/PARTITION/FAT_FORMAT

        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_INSTALL => dispatch!(sys_boot_install(a0 as u32), b"credo_diskinst\0"),

        #[cfg(feature = "kernel_test")]
        SYS_CREDO_DISK_INSTALL => dispatch!(Errno::ENOSYS.as_ret(), b"credo_disk_nosys\0"),

        // 已迁移: SYS_CREDO_PROC_LIST, SYS_CREDO_PROC_SETPRI, SYS_CREDO_PROC_CPUTIME, SYS_CREDO_PROC_SLEEP
        // 已迁移: SYS_CREDO_GETHOSTNAME, SYS_CREDO_SETHOSTNAME, SYS_CREDO_BOOT_CHECK, SYS_CREDO_REBOOT
        SYS_CREDO_HOTPLUG_STATUS => dispatch!(
            sys_hotplug_status(a0 as *mut u8, a1 as u32),
            b"credo_hotplug_status\0"
        ),

        // ==================== 帧缓冲设备 ====================
        SYS_FB_OPEN => dispatch!(sys_fb_open(a0, a1), b"fb_open\0"),
        SYS_FB_MMAP => dispatch!(sys_fb_mmap(a0, a1, a2), b"fb_mmap\0"),
        SYS_FB_RELEASE => dispatch!(sys_fb_release(a0), b"fb_release\0"),

        // 未匹配的 syscall 编号 — framework 和 services 均未处理
        _ => Errno::ENOSYS.as_ret(),
    }
}

#[unsafe(no_mangle)]
// 此处保留 #[no_mangle] 符号由 api.rs 的实现提供.

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
    // eventfd read: fd ∈ [200, 216)
    if crate::kernel::framework::syscall::eventfd::is_eventfd_fd(fd) {
        return crate::kernel::framework::syscall::eventfd::sys_eventfd_read(fd, buf as u64);
    }
    // signalfd read: fd ∈ [220, 236)
    if crate::kernel::framework::syscall::signalfd::is_signalfd_fd(fd) {
        return crate::kernel::framework::syscall::signalfd::sys_signalfd_read(fd, buf as u64);
    }
    // timerfd read: fd ∈ [240, 256)
    if crate::kernel::framework::syscall::timerfd::is_timerfd_fd(fd) {
        return crate::kernel::framework::syscall::timerfd::sys_timerfd_read(fd, buf as u64);
    }
    // inotify read: fd ∈ [260, 268)
    if crate::kernel::framework::fs::is_inotify_fd(fd) {
        return crate::kernel::framework::fs::sys_inotify_read(fd as i64, buf, count as usize);
    }
    crate::kernel::framework::fs::vfs_read(fd as u32, buf, count as u32) as i64
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
    // eventfd write: fd ∈ [200, 216), buf 指向 u64 值
    if crate::kernel::framework::syscall::eventfd::is_eventfd_fd(fd) {
        if count < 8 {
            return Errno::EINVAL.as_ret();
        }
        // SAFETY: buf 由 check_user_buf 验证, 读取 8 字节 u64
        let value = unsafe { core::ptr::read(buf as *const u64) };
        return crate::kernel::framework::syscall::eventfd::sys_eventfd_write(fd, value);
    }
    crate::kernel::framework::fs::vfs_write(fd as u32, buf, count as u32) as i64
}

// ============================================================================
// 目录/文件操作
// ============================================================================

// 已迁移到 services: nice_to_priority, priority_to_nice, sys_nice, sys_getpriority, sys_setpriority

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
    let mut stat_buf = core::mem::MaybeUninit::<crate::kernel::framework::fs::VfsStat>::uninit();
    let current_pwm = crate::kernel::framework::credo::get_current_pwm();
    let stat_result =
        crate::kernel::framework::fs::vfs_stat_internal(path, stat_buf.as_mut_ptr(), current_pwm);
    if stat_result == 0 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let st = unsafe { stat_buf.assume_init() };
        if (st.perm & 0o4000) != 0 && st.owner_pwm != 0 {
            crate::kernel::framework::credo::elevate_for_suid(st.owner_pwm);
        }
    }

    let result = crate::kernel::framework::proc::proc_exec_replace(path, argv, argc);
    if result < 0 {
        Errno::ENOENT.as_ret()
    } else {
        0
    }
}

// 已迁移到 services: sys_setregid, sys_mmap, sys_munmap, sys_time,
// sys_sched_setaffinity, sys_sched_getaffinity

#[cfg(feature = "net")]
fn sys_getsockname(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    raw::sm_getsockname_call(sockfd, addr, addrlen) as i64
}

#[cfg(feature = "net")]
fn sys_getpeername(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    raw::sm_getpeername_call(sockfd, addr, addrlen) as i64
}

// 已迁移到 services: sys_getrusage, sys_auth_*, sys_pwm_*, sys_gethostname,
// sys_sethostname, sys_boot_check, sys_reboot, sys_disk_list/info/format/partition/fat_format

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn write_le32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset] = val as u8;
    buf[offset + 1] = (val >> 8) as u8;
    buf[offset + 2] = (val >> 16) as u8;
    buf[offset + 3] = (val >> 24) as u8;
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
const BOOT_PART_SECTORS: u32 = 16384;

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
fn sys_boot_install(disk_id: u32) -> i64 {
    let pwm = crate::kernel::framework::credo::pwm_get_current();
    if !crate::kernel::framework::credo::pwm_has_capability(pwm, 4, 0) {
        return Errno::EACCES.as_ret();
    }
    let stage1 = include_bytes!("../../../../build/stage1.bin");
    if !crate::kernel::framework::driver::hdd_is_present(disk_id as u8) {
        return Errno::ENOENT.as_ret();
    }
    let mut mbr = [0u8; 512];
    if crate::kernel::framework::driver::hdd_read_sector(disk_id as u8, 0, &mut mbr) < 0 {
        return Errno::EIO.as_ret();
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe { core::ptr::copy_nonoverlapping(stage1.as_ptr(), mbr.as_mut_ptr(), 440) };
    let total_sectors = crate::kernel::framework::driver::hdd_total_sectors(disk_id as u8);
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
    if crate::kernel::framework::driver::hdd_write_sector(disk_id as u8, 0, &mbr) < 0 {
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
        if crate::kernel::framework::driver::hdd_write_sector(disk_id as u8, (1 + s) as u64, &buf) < 0 {
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
    crate::kernel::framework::driver::hdd_write_sector(disk_id as u8, 2046, &cfg);
    0
}

// 已迁移到 services: sys_proc_list, sys_proc_setpri, ProcListEntry, sys_ioctl

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
        let start = crate::kernel::framework::timer::hrtimer_clock_read();
        let target = start + total_ns;
        while crate::kernel::framework::timer::hrtimer_clock_read() < target {
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

// 已迁移到 services: sys_clock_gettime, sys_poll, sys_chown

pub fn sys_kill(pid: i32, sig: i32) -> i64 {
    if !(0..=31).contains(&sig) {
        return Errno::EINVAL.as_ret();
    }
    // 解决 TRACK-315B7C: 移除 pid <= 0 阻塞, 接受 POSIX 4 种 pid 语义
    // (pid>0 单进程, pid=0 同组, pid=-1 全部, pid<-1 |pid| 组).
    match crate::kernel::framework::proc::do_signal_send_extended(pid, sig as u8) {
        Ok(_) => 0,
        Err(-1) => Errno::EINVAL.as_ret(),
        Err(-2) => Errno::ESRCH.as_ret(),
        Err(_) => Errno::EPERM.as_ret(),
    }
}

// 已迁移到 services: readlink, umount2, getrlimit, sysinfo, truncate, ftruncate, flock, sys_inotify_add_watch

fn sys_tgkill(_tgid: i32, tid: i32, sig: i32) -> i64 {
    sys_kill(tid, sig)
}

// 信号框架 — rt_sigaction / rt_sigprocmask / rt_sigreturn

const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SIG_SETMASK: i32 = 2;

pub fn sys_rt_sigaction(signum: i32, act: u64, oact: u64) -> i64 {
    if !(1..=31).contains(&signum) {
        return Errno::EINVAL.as_ret();
    }

    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    // 读取旧值
    if oact != 0 {
        if !raw::check_user_buf(oact, 8) {
            return Errno::EFAULT.as_ret();
        }
        let old = crate::kernel::framework::proc::get_sigaction(pid, signum as u8);
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
        match crate::kernel::framework::proc::set_sigaction(pid, signum as u8, new_action) {
            Some(_) => {}
            None => return Errno::EINVAL.as_ret(), // SIGKILL/SIGSTOP
        }
    }

    0
}

pub fn sys_rt_sigprocmask(how: i32, set: u64, oset: u64) -> i64 {
    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    // 返回旧屏蔽字
    if oset != 0 {
        if !raw::check_user_buf(oset, 8) {
            return Errno::EFAULT.as_ret();
        }
        let old = crate::kernel::framework::proc::get_blocked_mask(pid);
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
        let old = crate::kernel::framework::proc::get_blocked_mask(pid);
        let updated = match how {
            SIG_BLOCK => old | new_set,
            SIG_UNBLOCK => old & !new_set,
            SIG_SETMASK => new_set,
            _ => return Errno::EINVAL.as_ret(),
        };
        // SIGKILL/SIGSTOP 不可屏蔽
        let updated = updated & !((1u64 << 9) | (1u64 << 19));
        crate::kernel::framework::proc::set_blocked_mask(pid, updated);
    }

    0
}

fn sys_rt_sigreturn() -> i64 {
    // P1-I-45 修复: sigreturn 时清除 SS_ONSTACK 标记, 允许下一次信号再次落回替代栈.
    // 必须在恢复寄存器前清, 否则在主栈上再次触发的 SIGSEGV 又会写到已耗尽的替代栈.
    if let Some(pid) = Some(crate::kernel::framework::proc::process_get_current_pid()).filter(|&p| p != 0) {
        crate::kernel::framework::proc::process_with_mut(pid, |proc| {
            let flags = proc.sigaltstack_flags.load(Ordering::Acquire);
            proc.sigaltstack_flags
                .store(flags & !crate::kernel::framework::proc::SS_ONSTACK, Ordering::Release);
        });
    }

    // x86_64: 寄存器恢复在 syscall_dispatch_from_frame 中提前拦截完成
    // (直接操作 InterruptFrame, 不经过此函数).
    // aarch64: 寄存器恢复在 svc_handler 中提前拦截完成.
    // 此函数仅负责 SS_ONSTACK 清除, 返回值被忽略.
    0
}

// ============================================================================
// P1-I-45: sigaltstack 系统调用 (替代栈注册/查询)
// ============================================================================
//
// 用户态传入 stack_t { ss_sp, ss_flags, ss_size }.
// - ss_flags & SS_DISABLE: 禁用替代栈 (清空 addr/size, 保留原 SS_ONSTACK 状态)
//
//   注: POSIX 规定当 SS_DISABLE 置位时, ss_sp/ss_size 被忽略.
// - 否则: 把 ss_sp/ss_size 写入进程 sigaltstack_* 字段, 同时清除 SS_ONSTACK
//   (回到主栈后才允许再次落回替代栈).
//
// 读取时 (old_ss 不为 0): 返回当前 addr/size/flags, 转换 SS_ONSTACK 为
// SS_ONSTACK (POSIX 要求返回时如实反映替代栈状态), 并把 SS_DISABLE 透传.
//
// 用户缓冲有效性: ostack/ss 都走 raw::check_user_buf, 长度 24 (3*u64).
// ============================================================================

fn sys_sigaltstack(ss: u64, old_ss: u64) -> i64 {
    use core::sync::atomic::Ordering;
    use crate::kernel::framework::proc::{SS_DISABLE, SS_ONSTACK};

    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    let result = crate::kernel::framework::proc::process_with_mut(pid, |proc| {
        // 返回旧值
        if old_ss != 0 {
            if !raw::check_user_buf(old_ss, 24) {
                return Errno::EFAULT.as_ret();
            }
            let cur_addr = proc.sigaltstack_addr.load(Ordering::Acquire);
            let cur_size = proc.sigaltstack_size.load(Ordering::Acquire);
            let cur_flags = proc.sigaltstack_flags.load(Ordering::Acquire);
            // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
            unsafe {
                raw::write_u64(old_ss as *mut u64, cur_addr);
                raw::write_u64((old_ss + 8) as *mut u64, cur_flags as u64);
                raw::write_u64((old_ss + 16) as *mut u64, cur_size);
            }
        }

        // 设置新值
        if ss != 0 {
            if !raw::check_user_buf(ss, 24) {
                return Errno::EFAULT.as_ret();
            }
            // SAFETY: `const` 由调用方保证为有效指针; 只读访问
            let new_addr = unsafe { raw::read_u64(ss as *const u64) };
            let new_flags_in = unsafe { raw::read_u64((ss + 8) as *const u64) } as u32;
            let new_size = unsafe { raw::read_u64((ss + 16) as *const u64) };

            if (new_flags_in & SS_DISABLE) != 0 {
                proc.sigaltstack_addr.store(0, Ordering::Release);
                proc.sigaltstack_size.store(0, Ordering::Release);
                let cur = proc.sigaltstack_flags.load(Ordering::Acquire);
                proc.sigaltstack_flags.store(cur | SS_DISABLE, Ordering::Release);
            } else {
                proc.sigaltstack_addr.store(new_addr, Ordering::Release);
                proc.sigaltstack_size.store(new_size, Ordering::Release);
                let cur = proc.sigaltstack_flags.load(Ordering::Acquire);
                proc.sigaltstack_flags
                    .store(cur & !(SS_ONSTACK | SS_DISABLE), Ordering::Release);
            }
        }
        0i64
    });

    result.unwrap_or_else(|| Errno::ESRCH.as_ret())
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

// 已迁移到 services: sys_credo_proc_cputime

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
        crate::kernel::framework::driver::FB_PHYS_ADDR.load(core::sync::atomic::Ordering::Acquire);
    if fb_addr == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_size =
        crate::kernel::framework::driver::FB_PHYS_SIZE.load(core::sync::atomic::Ordering::Acquire);

    let (width, height, pitch, bpp) = match crate::kernel::framework::driver::get_framebuffer() {
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
        crate::kernel::framework::driver::FB_PHYS_ADDR.load(core::sync::atomic::Ordering::Acquire);
    if fb_phys == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_total =
        crate::kernel::framework::driver::FB_PHYS_SIZE.load(core::sync::atomic::Ordering::Acquire);
    if size > fb_total {
        return Errno::EINVAL.as_ret();
    }

    let cr3 =
        crate::kernel::framework::proc::user_proc::user_entry_cr3.load(core::sync::atomic::Ordering::SeqCst);
    if cr3 == 0 {
        return Errno::ENODEV.as_ret();
    }

    let vmm = crate::kernel::framework::mm::get_vmm();
    let flags = crate::kernel::framework::mm::PageFlags::PRESENT
        | crate::kernel::framework::mm::PageFlags::WRITABLE
        | crate::kernel::framework::mm::PageFlags::USER
        | crate::kernel::framework::mm::PageFlags::WRITE_THROUGH;

    let phys_page_aligned = fb_phys & !(crate::kernel::framework::mm::PAGE_SIZE - 1);
    let offset = fb_phys - phys_page_aligned;
    let pages = (size + offset).div_ceil(crate::kernel::framework::mm::PAGE_SIZE);

    for i in 0..pages {
        let pa = crate::kernel::framework::mm::PhysAddr(phys_page_aligned + i * crate::kernel::framework::mm::PAGE_SIZE);
        let va = crate::kernel::framework::mm::VirtAddr(target_vaddr + i * crate::kernel::framework::mm::PAGE_SIZE);
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
    unsafe extern "C" {
        // 时间
        fn timer_get_ticks() -> u64;
        // 串口 (COM1/COM2)
        fn serial_has_data(com: i32) -> bool;
        fn serial_getc(com: i32) -> i32;
        // smoltcp 网络栈 (仅保留仍被调用的 extern)
        #[cfg(feature = "net")]
        fn sm_getsockname(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32;
        #[cfg(feature = "net")]
        fn sm_getpeername(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32;
        // 链接器符号
        static _kernel_start: u8;
        static _kernel_end: u8;
    }

    // x86_64 专属: 键盘
    #[cfg(target_arch = "x86_64")]
    unsafe extern "C" {
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
        crate::kernel::framework::userptr::validate_user_buf(ptr, len)
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

    /// 读一个 u64。
    /// # Safety
    /// 调用方必须先调用 `check_user_buf(ptr as u64, 8)` 验证。
    pub unsafe fn read_u64(ptr: *const u64) -> u64 {
        // SAFETY: 调用方已验证 ptr 对齐到 8 字节且指向 8 字节可读用户空间。
        unsafe { core::ptr::read_volatile(ptr) }
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
    /// 委托到 mm::api::pmm_alloc_pages.
    pub fn alloc_pages(count: u64) -> *mut u8 {
        crate::kernel::framework::mm::pmm_alloc_pages(count as usize)
    }

    /// 释放 count 个连续物理页。
    /// 委托到 mm::api::pmm_free_pages.
    pub fn free_pages(addr: *mut u8, count: u64) {
        crate::kernel::framework::mm::pmm_free_pages(addr, count as usize)
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
    /// FFI 调用，addr/addrlen 由调用方负责用户态校验 (可写)。
    #[cfg(feature = "net")]
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
    #[cfg(feature = "net")]
    pub fn sm_getpeername_call(
            sockfd: i32,
            addr: u64,
            addrlen: u64,
        ) -> i32 {
            // SAFETY: addr/addrlen 经 services 校验, 至少 sizeof(SockaddrIn)=16 字节可写.
            unsafe { sm_getpeername(sockfd, addr as *mut u8, addrlen as *mut u32) }
        }

    // ============= 链接器符号访问 =============

    /// 内核映像起始虚拟地址。
    /// # Safety
    /// 链接器符号，仅在 boot 后有效。
    #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
    pub fn kernel_start_ptr() -> *const u8 {
        // SAFETY: _kernel_start 是链接器符号 (extern "C")，是静态地址，
        // boot 后由 VMM 建立映射可读。
        unsafe { &_kernel_start as *const u8 }
    }

    /// 内核映像结束物理地址（已减 HHDM_OFFSET）。
    /// # SAFETY: 链接器符号，hhdm_offset 必须与启动时一致。
    #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
    pub fn kernel_end_phys(hhdm_offset: usize) -> usize {
        unsafe { (&_kernel_end as *const u8 as usize).wrapping_sub(hhdm_offset) }
    }

    // ============= CPU 控制指令集中点 =============

    /// 加载空 IDT 后触发异常，重启 CPU（x86_64）。
    /// # SAFETY: 不返回；调用方须确保已关闭其他 CPU。
    #[cfg(target_arch = "x86_64")]
    pub unsafe fn reboot_via_idt() -> ! { unsafe {
        core::arch::asm!(
            "lidt [rdi]",
            "int 0",
            in("rdi") &[0u16; 4],
            options(nostack, nomem)
        );
        loop {}
    }}

    /// 通过 SVC 触发 PSCI reset（aarch64）。
    /// # SAFETY: 不返回；调用方须确保已关闭其他 CPU。
    #[cfg(target_arch = "aarch64")]
    pub unsafe fn reboot_via_psci() -> ! { unsafe {
        core::arch::asm!("svc #0", in("x0") 0u64, options(nostack));
        loop {}
    }}
}
