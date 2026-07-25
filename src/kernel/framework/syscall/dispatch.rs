//! 系统调用分发实现
//!
//! 从 `mod.rs` 拆分而来, 包含主分发函数和所有 `sys_*` 处理函数。

#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::idt::InterruptFrame;
use core::sync::atomic::Ordering;

use super::raw;
use super::types::*;

const USER_ADDR_MAX: u64 = 0x7FFFFFFFE000;

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
///
/// # Safety
///
/// 调用者处于内核上下文. `ptr` 是已校验的用户态字符串指针.
pub unsafe extern "C" fn syscall_dispatch_from_frame(frame: *mut InterruptFrame) {
    unsafe {
        if frame.is_null() {
            return;
        }
        let f = &mut *frame;
        let syscall_num = f.rax;

        // rt_sigreturn 特殊处理: 需要直接修改 frame, 不走正常 dispatch
        #[cfg(target_arch = "x86_64")]
        let is_rt_sigreturn = syscall_num == 15;
        #[cfg(target_arch = "aarch64")]
        let is_rt_sigreturn = syscall_num == 139;

        if is_rt_sigreturn {
            let sigframe_ptr =
                (f.rsp + 8) as *const crate::kernel::framework::proc::SignalFrame;
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
    }
}

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
pub unsafe extern "C" fn syscall_dispatch(
    num: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> i64 {
    // TD-10: 进入内核态, tick 期间 sys_time 累加.
    crate::kernel::framework::proc::proc_set_in_kern(1);
    let result = syscall_dispatch_impl(num, a0, a1, a2, a3, a4, a5);
    // 出口恢复用户态, tick 期间 user_time 累加.
    crate::kernel::framework::proc::proc_set_in_kern(0);
    result
}

fn syscall_dispatch_impl(
    num: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> i64 {
    // 直接 Linux ABI: syscall 编号直接使用 Linux 标准编号, 无需翻译

    // C7: Seccomp 过滤检查 (在 dispatch 之前)
    let args = [a0, a1, a2, a3, a4, a5];
    if let Some(ret) = crate::kernel::framework::proc::seccomp_check(num, &args) {
        return ret;
    }

    // L-01: 优先委托 services 层策略分发
    let svc_ret =
        super::dispatch_trait::current_syscall_dispatch().dispatch(num, args);
    if svc_ret != -38 {
        return svc_ret;
    }

    // framework 回退: 处理尚未迁移到 services 的 syscall
    match num {
        // ==================== 文件 I/O ====================
        SYS_read => dispatch!(sys_read(a0 as i32, a1 as *mut u8, a2), b"read\0"),
        SYS_write => dispatch!(sys_write(a0 as i32, a1 as *const u8, a2), b"write\0"),

        // ==================== 内存管理 ====================
        SYS_mremap => {
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
                None => -1,
            }
        }

        // ==================== 信号 ====================
        QX_RT_SIGRETURN => dispatch!(sys_rt_sigreturn(), b"rt_sigreturn\0"),

        // ==================== 设备固件加载 ====================
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

        // ==================== 调试 / 跟踪 ====================
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

        // ==================== D8: 无 tick 模式 (NO_HZ) ====================
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

        // ==================== 进程 ====================
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
        #[cfg(not(feature = "net"))]
        QX_SOCKET | QX_CONNECT | QX_ACCEPT | QX_SENDTO | QX_RECVFROM | QX_SHUTDOWN
        | QX_BIND | QX_LISTEN | QX_SENDMSG | QX_RECVMSG | QX_SETSOCKOPT | QX_GETSOCKOPT
        | QX_GETSOCKNAME | QX_GETPEERNAME => {
            dispatch!(Errno::ENOSYS.as_ret(), b"net_nosys\0")
        }

        // ==================== 进程创建 ====================
        QX_EXECVE => dispatch!(
            crate::kernel::services::proc::execve::ExecveResult::from_ret(sys_execve(
                a0 as *const u8,
                a1 as *const *const u8,
                a2 as *const *const u8
            ))
            .as_ret(),
            b"execve\0"
        ),

        // ==================== 时间 ====================
        QX_SETRLIMIT => dispatch!(
            crate::kernel::framework::proc::sys_setrlimit(a0 as i32, a1),
            b"setrlimit\0"
        ),
        QX_TGKILL => dispatch!(sys_tgkill(a0 as i32, a1 as i32, a2 as i32), b"tgkill\0"),

        // ==================== sendfile / splice ====================
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

        // ==================== Credo 私有 syscall ====================
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_CREDO_DISK_INSTALL => dispatch!(sys_boot_install(a0 as u32), b"credo_diskinst\0"),
        #[cfg(feature = "kernel_test")]
        SYS_CREDO_DISK_INSTALL => dispatch!(Errno::ENOSYS.as_ret(), b"credo_disk_nosys\0"),

        SYS_CREDO_HOTPLUG_STATUS => dispatch!(
            sys_hotplug_status(a0 as *mut u8, a1 as u32),
            b"credo_hotplug_status\0"
        ),

        // ==================== 帧缓冲设备 ====================
        SYS_FB_OPEN => dispatch!(sys_fb_open(a0, a1), b"fb_open\0"),
        SYS_FB_MMAP => dispatch!(sys_fb_mmap(a0, a1, a2), b"fb_mmap\0"),
        SYS_FB_RELEASE => dispatch!(sys_fb_release(a0), b"fb_release\0"),

        // 未匹配的 syscall 编号
        _ => Errno::ENOSYS.as_ret(),
    }
}

// ============================================================================
// 文件 I/O — read / write
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
    if crate::kernel::framework::syscall::eventfd::is_eventfd_fd(fd) {
        return crate::kernel::framework::syscall::eventfd::sys_eventfd_read(fd, buf as u64);
    }
    if crate::kernel::framework::syscall::signalfd::is_signalfd_fd(fd) {
        return crate::kernel::framework::syscall::signalfd::sys_signalfd_read(fd, buf as u64);
    }
    if crate::kernel::framework::syscall::timerfd::is_timerfd_fd(fd) {
        return crate::kernel::framework::syscall::timerfd::sys_timerfd_read(fd, buf as u64);
    }
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
// execve / 网络 / 时间
// ============================================================================

fn sys_execve(
    path: *const u8,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i64 {
    if path.is_null() || !raw::check_user_ptr(path as u64) {
        return Errno::EFAULT.as_ret();
    }
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
            // SAFETY: p 是经过 check_user_ptr 验证的用户空间指针
            let entry = unsafe { core::ptr::read_volatile(p) };
            if entry.is_null() {
                break;
            }
            if !raw::check_user_ptr(entry as u64) {
                return Errno::EFAULT.as_ret();
            }
            argc += 1;
            // SAFETY: p 指向用户空间数组元素; 由 argc 计数 + NULL 终止保证不越界
            p = unsafe { p.add(1) };
        }
    }

    // SUID 处理
    let mut stat_buf = core::mem::MaybeUninit::<crate::kernel::framework::fs::VfsStat>::uninit();
    let current_pwm = crate::kernel::framework::credo::get_current_pwm();
    let stat_result = crate::kernel::framework::fs::vfs_stat_internal(
        path,
        stat_buf.as_mut_ptr(),
        current_pwm,
    );
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

#[cfg(feature = "net")]
fn sys_getsockname(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    crate::kernel::framework::net::syscall::getsockname_syscall(sockfd, addr, addrlen)
}

#[cfg(feature = "net")]
fn sys_getpeername(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    crate::kernel::framework::net::syscall::getpeername_syscall(sockfd, addr, addrlen)
}

pub(crate) fn sys_nanosleep(req: u64, rem: u64) -> i64 {
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

    let total_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
    if total_ns == 0 {
        return 0;
    }

    if total_ns < 1_000_000 {
        let start = crate::kernel::framework::timer::hrtimer_clock_read();
        let target = start + total_ns;
        while crate::kernel::framework::timer::hrtimer_clock_read() < target {
            core::hint::spin_loop();
        }
    } else {
        let total_ms = total_ns / 1_000_000;
        let _ = crate::kernel::framework::timer::sleep::timer_sleep(total_ms);
    }

    let _ = rem;
    0
}

pub(crate) fn sys_kill(pid: i32, sig: i32) -> i64 {
    if !(0..=31).contains(&sig) {
        return Errno::EINVAL.as_ret();
    }
    match crate::kernel::framework::proc::do_signal_send_extended(pid, sig as u8) {
        Ok(_) => 0,
        Err(-1) => Errno::EINVAL.as_ret(),
        Err(-2) => Errno::ESRCH.as_ret(),
        Err(_) => Errno::EPERM.as_ret(),
    }
}

fn sys_tgkill(_tgid: i32, tid: i32, sig: i32) -> i64 {
    sys_kill(tid, sig)
}

// ============================================================================
// 信号框架
// ============================================================================

const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SIG_SETMASK: i32 = 2;

pub(crate) fn sys_rt_sigaction(signum: i32, act: u64, oact: u64) -> i64 {
    if !(1..=31).contains(&signum) {
        return Errno::EINVAL.as_ret();
    }

    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

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

    if act != 0 {
        if !raw::check_user_buf(act, 8) {
            return Errno::EFAULT.as_ret();
        }
        // SAFETY: `const` 由调用方保证为有效指针; 只读访问
        let new_action = unsafe { raw::read_u64(act as *const u64) };
        match crate::kernel::framework::proc::set_sigaction(pid, signum as u8, new_action) {
            Some(_) => {}
            None => return Errno::EINVAL.as_ret(),
        }
    }

    0
}

pub(crate) fn sys_rt_sigprocmask(how: i32, set: u64, oset: u64) -> i64 {
    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    if oset != 0 {
        if !raw::check_user_buf(oset, 8) {
            return Errno::EFAULT.as_ret();
        }
        let old = crate::kernel::framework::proc::get_blocked_mask(pid);
        // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
        unsafe { raw::write_u64(oset as *mut u64, old) };
    }

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
        let updated = updated & !((1u64 << 9) | (1u64 << 19));
        crate::kernel::framework::proc::set_blocked_mask(pid, updated);
    }

    0
}

fn sys_rt_sigreturn() -> i64 {
    if let Some(pid) = Some(crate::kernel::framework::proc::process_get_current_pid())
        .filter(|&p| p != 0)
    {
        crate::kernel::framework::proc::process_with_mut(pid, |proc| {
            let flags = proc.sigaltstack_flags.load(Ordering::Acquire);
            proc.sigaltstack_flags.store(
                flags & !crate::kernel::framework::proc::SS_ONSTACK,
                Ordering::Release,
            );
        });
    }
    0
}

pub(crate) fn sys_sigaltstack(ss: u64, old_ss: u64) -> i64 {
    use crate::kernel::framework::proc::{SS_DISABLE, SS_ONSTACK};

    let pid = match crate::kernel::framework::proc::process_get_current_pid() {
        0 => return Errno::ESRCH.as_ret(),
        p => p,
    };

    let result = crate::kernel::framework::proc::process_with_mut(pid, |proc| {
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
                proc.sigaltstack_flags
                    .store(cur | SS_DISABLE, Ordering::Release);
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
// 热插拔 / 帧缓冲
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

// ============================================================================
// 帧缓冲设备
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

    let fb_addr = crate::kernel::framework::driver::FB_PHYS_ADDR
        .load(core::sync::atomic::Ordering::Acquire);
    if fb_addr == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_size = crate::kernel::framework::driver::FB_PHYS_SIZE
        .load(core::sync::atomic::Ordering::Acquire);

    let (width, height, pitch, bpp) = match crate::kernel::framework::driver::get_framebuffer() {
        Some(guard) => {
            let fb = guard.as_ref().unwrap();
            (
                fb.width(),
                fb.height(),
                fb.pitch(),
                fb.format().bits_per_pixel() as u8,
            )
        }
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

    let fb_phys = crate::kernel::framework::driver::FB_PHYS_ADDR
        .load(core::sync::atomic::Ordering::Acquire);
    if fb_phys == 0 {
        return Errno::ENODEV.as_ret();
    }

    let fb_total = crate::kernel::framework::driver::FB_PHYS_SIZE
        .load(core::sync::atomic::Ordering::Acquire);
    if size > fb_total {
        return Errno::EINVAL.as_ret();
    }

    let cr3 = crate::kernel::framework::proc::user_proc::user_entry_cr3
        .load(core::sync::atomic::Ordering::SeqCst);
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
        let pa = crate::kernel::framework::mm::PhysAddr(
            phys_page_aligned + i * crate::kernel::framework::mm::PAGE_SIZE,
        );
        let va = crate::kernel::framework::mm::VirtAddr(
            target_vaddr + i * crate::kernel::framework::mm::PAGE_SIZE,
        );
        vmm.map_page_in_table(cr3, va, pa, flags);
    }

    target_vaddr as i64
}

fn sys_fb_release(_vaddr: u64) -> i64 {
    0
}

// ============================================================================
// 引导安装 (仅 x86_64)
// ============================================================================

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
        if crate::kernel::framework::driver::hdd_write_sector(
            disk_id as u8,
            (1 + s) as u64,
            &buf,
        ) < 0
        {
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
