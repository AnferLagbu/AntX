/// Syscall 模块 — POSIX 原生系统调用分发
///
/// POSIX 标准 syscall 编号 (0-399) + QueenX 私有 syscall (400+).
/// 内核能力层 (VFS/PWM/PROC/NET/MM) 不变，仅 syscall ABI 层替换。

pub mod types;
pub mod ffi;
pub mod mmap;

use crate::kernel::syscall::types::*;
#[cfg(target_arch = "x86_64")]
use crate::kernel::idt::types::InterruptFrame;
use spin::Mutex;

const USER_ADDR_MAX: u64 = 0x7FFFFFFFE000;

fn validate_user_ptr(ptr: u64) -> bool {
    ptr > 0 && ptr < USER_ADDR_MAX
}

fn validate_user_buf(ptr: u64, len: u64) -> bool {
    if len == 0 { return true; }
    validate_user_ptr(ptr) && ptr + len <= USER_ADDR_MAX
}

#[no_mangle]
pub unsafe extern "C" fn syscall_init() {
    unsafe { crate::kernel::klog::klog_write(1, 7, core::ptr::null(), core::ptr::null(), 0, b"POSIX syscall subsystem ready\0".as_ptr() as *const core::ffi::c_char); }
}

#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn syscall_dispatch_from_frame(frame: *mut InterruptFrame) {
    if frame.is_null() { return; }
    let f = &mut *frame;
    let syscall_num = f.rax;
    let a0 = f.rdi;
    let a1 = f.rsi;
    let a2 = f.rdx;
    let a3 = f.r10;
    let result = syscall_dispatch(syscall_num, a0, a1, a2, a3);
    f.rax = result as u64;
}

macro_rules! dispatch {
    ($num:expr, $name:expr) => {
        {
            let ret = $num;
            unsafe { crate::kernel::klog::klog_write(0, 7, core::ptr::null(), core::ptr::null(), 0, $name.as_ptr() as *const core::ffi::c_char); }
            ret
        }
    };
}

#[no_mangle]
pub unsafe extern "C" fn syscall_dispatch(num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    match num {
        // ==================== 文件 I/O ====================
        SYS_read            => dispatch!(sys_read(a0 as i32, a1 as *mut u8, a2), b"read\0"),
        SYS_write           => dispatch!(sys_write(a0 as i32, a1 as *const u8, a2), b"write\0"),
        SYS_open            => dispatch!(sys_open(a0 as *const core::ffi::c_char, a1 as i32, a2 as i32), b"open\0"),
        SYS_close           => dispatch!(sys_close(a0 as i32), b"close\0"),
        SYS_stat            => dispatch!(sys_stat(a0 as *const core::ffi::c_char, a1 as *mut core::ffi::c_void), b"stat\0"),
        SYS_fstat           => dispatch!(sys_fstat(a0 as i32, a1 as *mut core::ffi::c_void), b"fstat\0"),
        SYS_lstat           => dispatch!(sys_stat(a0 as *const core::ffi::c_char, a1 as *mut core::ffi::c_void), b"lstat\0"),
        SYS_poll            => dispatch!(sys_poll(a0 as *mut core::ffi::c_void, a1 as u32, a2 as i32), b"poll\0"),
        SYS_lseek           => dispatch!(sys_lseek(a0 as i32, a1 as i64, a2 as i32), b"lseek\0"),

        // ==================== 内存管理 ====================
        SYS_mmap            => dispatch!(sys_mmap(a0, a1, a2 as i32, a3 as i32), b"mmap\0"),
        SYS_mprotect        => dispatch!(Errno::ENOSYS.as_ret(), b"mprotect\0"),
        SYS_munmap          => dispatch!(sys_munmap(a0, a1), b"munmap\0"),
        SYS_brk             => dispatch!(sys_brk(a0), b"brk\0"),

        // ==================== 信号 ====================
        SYS_rt_sigaction    => dispatch!(sys_rt_sigaction(a0 as i32, a1, a2), b"rt_sigaction\0"),
        SYS_rt_sigprocmask  => dispatch!(sys_rt_sigprocmask(a0 as i32, a1, a2), b"rt_sigprocmask\0"),
        SYS_rt_sigreturn    => dispatch!(sys_rt_sigreturn(), b"rt_sigreturn\0"),

        // ==================== 设备 ====================
        SYS_ioctl           => dispatch!(sys_ioctl(a0 as i32, a1, a2), b"ioctl\0"),

        // ==================== 文件访问 ====================
        SYS_access          => dispatch!(sys_access(a0 as *const core::ffi::c_char, a1 as i32), b"access\0"),
        SYS_pipe            => dispatch!(sys_pipe(a0 as *mut i32), b"pipe\0"),
        SYS_select          => dispatch!(sys_poll(a0 as *mut core::ffi::c_void, a1 as u32, a2 as i32), b"select\0"),
        SYS_sched_yield     => dispatch!(sys_sched_yield(), b"sched_yield\0"),

        // ==================== 文件描述符 ====================
        SYS_dup             => dispatch!(sys_dup(a0 as i32), b"dup\0"),
        SYS_dup2            => dispatch!(sys_dup2(a0 as i32, a1 as i32), b"dup2\0"),

        // ==================== 定时器 ====================
        SYS_nanosleep       => dispatch!(sys_nanosleep(a0, a1), b"nanosleep\0"),
        SYS_alarm           => dispatch!(Errno::ENOSYS.as_ret(), b"alarm\0"),

        // ==================== 进程 ====================
        SYS_getpid          => dispatch!(sys_getpid(), b"getpid\0"),
        SYS_getppid         => dispatch!(sys_getppid(), b"getppid\0"),
        SYS_getpgid         => dispatch!(sys_getpgid(a0 as i32), b"getpgid\0"),
        SYS_setsid          => dispatch!(sys_setsid(), b"setsid\0"),
        SYS_gettid          => dispatch!(sys_gettid(), b"gettid\0"),

        // ==================== 网络 ====================
        #[cfg(feature = "net")]
        SYS_socket      => dispatch!(sys_socket(a0 as i32, a1 as i32, a2 as i32), b"socket\0"),
        #[cfg(feature = "net")]
        SYS_connect     => dispatch!(sys_connect(a0 as i32, a1, a2 as u32), b"connect\0"),
        #[cfg(feature = "net")]
        SYS_accept      => dispatch!(sys_accept(a0 as i32, a1, a2), b"accept\0"),
        #[cfg(feature = "net")]
        SYS_sendto      => dispatch!(sys_sendto(a0 as i32, a1, a2 as u32, a3 as i32), b"sendto\0"),
        #[cfg(feature = "net")]
        SYS_recvfrom    => dispatch!(sys_recvfrom(a0 as i32, a1, a2 as u32, a3 as i32), b"recvfrom\0"),
        #[cfg(feature = "net")]
        SYS_shutdown    => dispatch!(sys_shutdown(a0 as i32, a1 as i32), b"shutdown\0"),
        #[cfg(feature = "net")]
        SYS_bind        => dispatch!(sys_bind(a0 as i32, a1, a2 as u32), b"bind\0"),
        #[cfg(feature = "net")]
        SYS_listen      => dispatch!(sys_listen(a0 as i32, a1 as i32), b"listen\0"),
        #[cfg(not(feature = "net"))]
        SYS_socket | SYS_connect | SYS_accept | SYS_sendto | SYS_recvfrom |
        SYS_shutdown | SYS_bind | SYS_listen => dispatch!(Errno::ENOSYS.as_ret(), b"net_nosys\0"),

        // ==================== 进程创建 ====================
        SYS_fork            => dispatch!(sys_fork(), b"fork\0"),
        SYS_execve          => dispatch!(sys_execve(a0 as *const core::ffi::c_char, a1 as *const *const u8, a2 as *const *const u8), b"execve\0"),
        SYS_exit            => dispatch!(sys_exit(a0 as i32), b"exit\0"),
        SYS_wait4            => dispatch!(sys_wait4(a0 as i32), b"wait4\0"),
        SYS_kill            => dispatch!(sys_kill(a0 as i32, a1 as i32), b"kill\0"),

        // ==================== 系统信息 ====================
        SYS_uname           => dispatch!(sys_uname(a0 as *mut core::ffi::c_void), b"uname\0"),

        // ==================== 文件描述符操作 ====================
        SYS_fcntl           => dispatch!(sys_fcntl(a0 as i32, a1 as i32, a2), b"fcntl\0"),

        // ==================== 文件截断 ====================
        SYS_truncate        => dispatch!(sys_truncate(a0 as *const core::ffi::c_char, a1 as i64), b"truncate\0"),
        SYS_ftruncate       => dispatch!(sys_ftruncate(a0 as i32, a1 as i64), b"ftruncate\0"),

        // ==================== 目录 ====================
        SYS_getdents        => dispatch!(sys_getdents(a0 as i32, a1 as *mut core::ffi::c_void, a2), b"getdents\0"),

        // ==================== 路径 ====================
        SYS_getcwd          => dispatch!(sys_getcwd(a0 as *mut core::ffi::c_char, a1), b"getcwd\0"),
        SYS_chdir           => dispatch!(sys_chdir(a0 as *const core::ffi::c_char), b"chdir\0"),

        // ==================== 文件操作 ====================
        SYS_rename          => dispatch!(sys_rename(a0 as *const core::ffi::c_char, a1 as *const core::ffi::c_char), b"rename\0"),
        SYS_mkdir           => dispatch!(sys_mkdir(a0 as *const core::ffi::c_char, a1 as i32), b"mkdir\0"),
        SYS_rmdir           => dispatch!(sys_rmdir(a0 as *const core::ffi::c_char), b"rmdir\0"),
        SYS_creat           => dispatch!(sys_open(a0 as *const core::ffi::c_char, 0o101, 0o666), b"creat\0"),
        SYS_unlink          => dispatch!(sys_unlink(a0 as *const core::ffi::c_char), b"unlink\0"),
        SYS_readlink        => dispatch!(sys_readlink(a0 as *const core::ffi::c_char, a1 as *mut core::ffi::c_char, a2), b"readlink\0"),

        // ==================== 文件权限 ====================
        SYS_chmod           => dispatch!(sys_chmod(a0 as *const core::ffi::c_char, a1 as u32), b"chmod\0"),
        SYS_fchmod          => dispatch!(sys_fchmod(a0 as i32, a1 as u32), b"fchmod\0"),
        SYS_chown           => dispatch!(sys_chown(a0 as *const core::ffi::c_char, a1 as u32, a2 as u32), b"chown\0"),
        SYS_umask           => dispatch!(sys_umask(a0 as u32), b"umask\0"),

        // ==================== 时间 ====================
        SYS_gettimeofday    => dispatch!(sys_gettimeofday(a0 as *mut core::ffi::c_void, a1 as *mut core::ffi::c_void), b"gettimeofday\0"),
        SYS_getrlimit       => dispatch!(sys_getrlimit(a0 as i32, a1 as *mut core::ffi::c_void), b"getrlimit\0"),
        SYS_sysinfo         => dispatch!(sys_sysinfo(a0 as *mut core::ffi::c_void), b"sysinfo\0"),

        // ==================== 用户/组 ====================
        SYS_getuid          => dispatch!(sys_getuid(), b"getuid\0"),
        SYS_getgid          => dispatch!(sys_getgid(), b"getgid\0"),
        SYS_setuid          => dispatch!(Errno::EPERM.as_ret(), b"setuid\0"),
        SYS_setgid          => dispatch!(Errno::EPERM.as_ret(), b"setgid\0"),
        SYS_geteuid         => dispatch!(sys_getuid(), b"geteuid\0"),
        SYS_getegid         => dispatch!(sys_getgid(), b"getegid\0"),

        // ==================== 文件同步/挂载 ====================
        SYS_sync            => dispatch!(sys_sync(), b"sync\0"),
        SYS_fsync           => dispatch!(sys_fsync(a0 as i32), b"fsync\0"),
        SYS_mount           => dispatch!(sys_mount(a0 as *const core::ffi::c_char, a1 as *const core::ffi::c_char, a2 as *const core::ffi::c_char), b"mount\0"),
        SYS_umount2          => dispatch!(sys_umount2(a0 as *const core::ffi::c_char, a1 as i32), b"umount2\0"),

        SYS_time            => dispatch!(sys_time(a0 as *mut u64), b"time\0"),
        SYS_clock_gettime   => dispatch!(sys_clock_gettime(a0 as i32, a1 as *mut core::ffi::c_void), b"clock_gettime\0"),
        SYS_exit_group      => dispatch!(sys_exit(a0 as i32), b"exit_group\0"),
        SYS_tgkill          => dispatch!(sys_tgkill(a0 as i32, a1 as i32, a2 as i32), b"tgkill\0"),

        // ==================== QueenX 私有 syscall (400+) ====================
        SYS_QX_LOGIN             => dispatch!(sys_auth_login(a0 as *const core::ffi::c_char, a1 as *const core::ffi::c_char), b"qx_login\0"),
        SYS_QX_LOGOUT            => dispatch!(sys_auth_logout(), b"qx_logout\0"),
        SYS_QX_CREATE_IDENTITY   => dispatch!(sys_auth_create(a0 as *const core::ffi::c_char, a1 as *const core::ffi::c_char, a2 as u8), b"qx_create\0"),
        SYS_QX_DELETE_IDENTITY   => dispatch!(sys_auth_delete(a0), b"qx_delete\0"),
        SYS_QX_IDENTITY_INFO     => dispatch!(sys_auth_info(a0), b"qx_info\0"),
        SYS_QX_CHANGE_PASSWORD   => dispatch!(sys_auth_changepw(a0 as *const core::ffi::c_char, a1 as *const core::ffi::c_char), b"qx_chpw\0"),
        SYS_QX_VERIFY_PASSWORD   => dispatch!(sys_auth_verify(a0 as *const core::ffi::c_char), b"qx_verify\0"),
        SYS_QX_CREATE_FIRST      => dispatch!(sys_auth_create_first(a0 as *const core::ffi::c_char), b"qx_first\0"),
        SYS_QX_TOKEN_CREATE      => dispatch!(sys_auth_token_create(a0, a1 as u16, a2, a3, 1), b"qx_tcreate\0"),
        SYS_QX_TOKEN_USE         => dispatch!(0, b"qx_tuse\0"),
        SYS_QX_TOKEN_REVOKE      => dispatch!(0, b"qx_trevoke\0"),
        SYS_QX_GRANT             => dispatch!(sys_auth_grant(a0, a1, a2 as u16, a3), b"qx_grant\0"),
        SYS_QX_REVOKE            => dispatch!(sys_auth_revoke(a0, a1, a2 as u16, a3), b"qx_revoke\0"),
        SYS_QX_CHECK_CAP         => dispatch!(sys_auth_check_cap(a0, a1 as u16, a2), b"qx_checkcap\0"),
        SYS_QX_GET_CAPS          => dispatch!(sys_auth_get_caps(a0, a1 as u16), b"qx_getcaps\0"),
        SYS_QX_GET_PWM           => dispatch!(sys_pwm_get(), b"qx_getpwm\0"),
        SYS_QX_SET_PWM           => dispatch!(sys_pwm_set(a0), b"qx_setpwm\0"),

        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_QX_DISK_LIST      => dispatch!(sys_disk_list(a0 as *mut u64, a1 as u32), b"qx_disklist\0"),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_QX_DISK_INFO      => dispatch!(sys_disk_info(a0 as u32, a1 as *mut u8), b"qx_diskinfo\0"),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_QX_DISK_FORMAT    => dispatch!(sys_disk_format(a0 as u32, a1 as *const core::ffi::c_char), b"qx_diskfmt\0"),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_QX_DISK_PARTITION => dispatch!(sys_disk_partition(a0 as u32, a1), b"qx_diskpart\0"),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_QX_DISK_INSTALL   => dispatch!(sys_boot_install(a0 as u32), b"qx_diskinst\0"),
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        SYS_QX_FAT_FORMAT     => dispatch!(sys_fat_format(a0 as u32), b"qx_fatfmt\0"),

        #[cfg(feature = "kernel_test")]
        SYS_QX_DISK_LIST | SYS_QX_DISK_INFO | SYS_QX_DISK_FORMAT |
        SYS_QX_DISK_PARTITION | SYS_QX_DISK_INSTALL | SYS_QX_FAT_FORMAT =>
            dispatch!(Errno::ENOSYS.as_ret(), b"qx_disk_nosys\0"),

        SYS_QX_PROC_LIST     => dispatch!(sys_proc_list(a0 as *mut u8, a1 as u32), b"qx_proclist\0"),
        SYS_QX_PROC_SETPRI   => dispatch!(sys_proc_setpri(a0 as u32, a1 as u32), b"qx_procpri\0"),
        SYS_QX_PROC_SLEEP    => dispatch!(sys_proc_sleep(a0), b"qx_procsleep\0"),
        SYS_QX_GETHOSTNAME   => dispatch!(sys_gethostname(a0 as *mut core::ffi::c_char, a1), b"qx_gethost\0"),
        SYS_QX_SETHOSTNAME   => dispatch!(sys_sethostname(a0 as *const core::ffi::c_char, a1), b"qx_sethost\0"),
        SYS_QX_BOOT_CHECK    => dispatch!(sys_boot_check(a0 as i32), b"qx_bootchk\0"),
        SYS_QX_REBOOT        => dispatch!(sys_reboot(a0 as i32), b"qx_reboot\0"),
        SYS_QX_HOTPLUG_STATUS => dispatch!(sys_hotplug_status(a0 as *mut u8, a1 as u32), b"qx_hotplug_status\0"),

        _ => Errno::ENOSYS.as_ret(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn syscall_register(_num: u64, _handler: SyscallHandler) {}

// ============================================================================
// 文件 I/O — read / write / open / close
// ============================================================================

unsafe fn sys_read(fd: i32, buf: *mut u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 { return Errno::EINVAL.as_ret(); }
    if !validate_user_buf(buf as u64, count) { return Errno::EFAULT.as_ret(); }
    if fd == 1 || fd == 2 { return Errno::EBADF.as_ret(); }
    if fd == 0 {
        #[cfg(not(feature = "kernel_test"))]
        {
            extern "C" { fn serial_has_data(com: i32) -> bool; fn serial_getc(com: i32) -> i32; }
            #[cfg(target_arch = "x86_64")]
            {
                extern "C" { fn keyboard_has_data() -> bool; fn keyboard_get_char() -> i32; }
                if keyboard_has_data() { let c = keyboard_get_char(); if c > 0 { *buf = c as u8; return 1; } }
            }
            if serial_has_data(0) { let c = serial_getc(0); if c > 0 { *buf = c as u8; return 1; } }
        }
        return 0;
    }
    crate::kernel::fs::vfs::ffi::vfs_read(fd as u32, buf as *mut u8, count as u32) as i64
}

unsafe fn sys_write(fd: i32, buf: *const u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 { return Errno::EINVAL.as_ret(); }
    if !validate_user_buf(buf as u64, count) { return Errno::EFAULT.as_ret(); }
    if fd == 1 || fd == 2 {
        if count > 0 {
            let data = unsafe { core::slice::from_raw_parts(buf, count as usize) };
            crate::kernel::klog::serial_write_bytes(data);
        }
        return count as i64;
    }
    crate::kernel::fs::vfs::ffi::vfs_write(fd as u32, buf as *const u8, count as u32) as i64
}

unsafe fn sys_open(path: *const core::ffi::c_char, flags: i32, _mode: i32) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_open(path, flags as u32, pwm) as i64
}

unsafe fn sys_close(fd: i32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    crate::kernel::fs::vfs::ffi::vfs_close(fd as u32) as i64
}

unsafe fn sys_stat(path: *const core::ffi::c_char, st_buf: *mut core::ffi::c_void) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_stat(path, st_buf as *mut crate::kernel::fs::vfs::types::VfsStat, pwm) as i64
}

unsafe fn sys_fstat(fd: i32, st_buf: *mut core::ffi::c_void) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_fstat(fd as u32, st_buf as *mut crate::kernel::fs::vfs::types::VfsStat, pwm) as i64
}

unsafe fn sys_lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_seek(fd as u32, offset as i32, whence as u32) as i64
}

unsafe fn sys_getdents(fd: i32, buf: *mut core::ffi::c_void, _count: u64) -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_readdir(fd as u32, buf as *mut crate::kernel::fs::vfs::types::VfsDirEntry) as i64
}

// ============================================================================
// 目录/文件操作
// ============================================================================

unsafe fn sys_getcwd(buf: *mut core::ffi::c_char, size: u64) -> i64 {
    if buf.is_null() || size == 0 { return Errno::EINVAL.as_ret(); }
    if !validate_user_buf(buf as u64, size) { return Errno::EFAULT.as_ret(); }
    crate::kernel::fs::vfs::ffi::vfs_get_cwd(buf, size as u32) as i64
}

unsafe fn sys_chdir(path: *const core::ffi::c_char) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    crate::kernel::fs::vfs::ffi::vfs_set_cwd(path);
    0
}

unsafe fn sys_mkdir(path: *const core::ffi::c_char, _mode: i32) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    let pwm = if pwm == 0 { 0x0020F45A8B978417 } else { pwm };
    crate::kernel::fs::vfs::ffi::vfs_mkdir(path, pwm) as i64
}

unsafe fn sys_rmdir(path: *const core::ffi::c_char) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_rmdir(path, pwm) as i64
}

unsafe fn sys_unlink(path: *const core::ffi::c_char) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_unlink(path, pwm) as i64
}

unsafe fn sys_rename(old: *const core::ffi::c_char, new: *const core::ffi::c_char) -> i64 {
    if old.is_null() || new.is_null() || !validate_user_ptr(old as u64) || !validate_user_ptr(new as u64) {
        return Errno::EFAULT.as_ret();
    }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_rename(old, new, pwm) as i64
}

unsafe fn sys_access(path: *const core::ffi::c_char, _mode: i32) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    let stat_ptr: *mut crate::kernel::fs::vfs::types::VfsStat = &mut core::mem::zeroed();
    let result = crate::kernel::fs::vfs::ffi::vfs_stat(path, stat_ptr, pwm);
    if result < 0 { return result as i64; }
    0
}

unsafe fn sys_sync() -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_sync() as i64
}

unsafe fn sys_mount(_source: *const core::ffi::c_char, target: *const core::ffi::c_char, fstype: *const core::ffi::c_char) -> i64 {
    if target.is_null() || !validate_user_ptr(target as u64) { return Errno::EFAULT.as_ret(); }
    if fstype.is_null() { return Errno::EINVAL.as_ret(); }
    crate::kernel::fs::vfs::ffi::vfs_mount(target, fstype) as i64
}

// ============================================================================
// 进程 — fork / execve / exit / wait / getpid
// ============================================================================

unsafe fn sys_getpid() -> i64 {
    crate::kernel::proc::ffi::process_get_current_pid() as i64
}

unsafe fn sys_getppid() -> i64 {
    let pid = crate::kernel::proc::ffi::process_get_current_pid();
    crate::kernel::proc::ffi::proc_get_ppid(pid) as i64
}

unsafe fn sys_sched_yield() -> i64 {
    crate::kernel::proc::ffi::scheduler_yield();
    0
}

unsafe fn sys_fork() -> i64 {
    crate::kernel::proc::ffi::sys_fork() as i64
}

unsafe fn sys_execve(path: *const core::ffi::c_char, argv: *const *const u8, _envp: *const *const u8) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let mut argc: u32 = 0;
    if !argv.is_null() {
        let mut p = argv;
        while !(*p).is_null() { argc += 1; p = p.add(1); }
    }
    let result = crate::kernel::proc::ffi::proc_exec_replace(path, argv, argc);
    if result < 0 { Errno::ENOENT.as_ret() } else { 0 }
}

unsafe fn sys_exit(status: i32) -> i64 {
    crate::kernel::proc::ffi::process_exit(status as u32);
    0
}

unsafe fn sys_wait4(_pid: i32) -> i64 {
    let result = crate::kernel::proc::ffi::proc_wait_child(0);
    result as i64
}

// ============================================================================
// 用户/组 — getuid / getgid / geteuid / getegid
// ============================================================================

unsafe fn sys_getuid() -> i64 {
    crate::kernel::pwm::session::get_current_uid() as i64
}

unsafe fn sys_getgid() -> i64 {
    crate::kernel::pwm::session::get_current_gid() as i64
}

// ============================================================================
// 管道 — pipe / dup / dup2
// ============================================================================

unsafe fn sys_pipe(fds: *mut i32) -> i64 {
    if fds.is_null() { return Errno::EINVAL.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 6, 0x01) {
        return Errno::EACCES.as_ret();
    }
    let mut pipefd: [i32; 2] = [0; 2];
    let result = crate::kernel::ipc::pipe::ipc_pipe_create(pipefd.as_mut_ptr());
    if result < 0 { return Errno::EBUSY.as_ret(); }
    if pipefd[0] < 0 || pipefd[1] < 0 { return Errno::EBUSY.as_ret(); }
    *fds = pipefd[0];
    *fds.offset(1) = pipefd[1];
    0
}

unsafe fn sys_dup(oldfd: i32) -> i64 {
    if oldfd < 0 { return Errno::EBADF.as_ret(); }
    crate::kernel::fs::vfs::ffi::vfs_dup(oldfd as u32) as i64
}

unsafe fn sys_dup2(oldfd: i32, newfd: i32) -> i64 {
    if oldfd < 0 || newfd < 0 { return Errno::EBADF.as_ret(); }
    if oldfd == newfd { return newfd as i64; }
    let result = crate::kernel::fs::vfs::ffi::vfs_dup2(oldfd as u32, newfd as u32);
    if result < 0 { return Errno::EBADF.as_ret(); }
    result as i64
}

// ============================================================================
// 内存 — brk / mmap / munmap
// ============================================================================

unsafe fn sys_brk(addr: u64) -> i64 {
    use core::sync::atomic::AtomicU64;
    static BRK: AtomicU64 = AtomicU64::new(0x400000 + 65536);
    if addr == 0 {
        BRK.load(core::sync::atomic::Ordering::SeqCst) as i64
    } else if addr > USER_ADDR_MAX {
        Errno::ENOMEM.as_ret()
    } else {
        let current = BRK.load(core::sync::atomic::Ordering::SeqCst);
        if addr > current {
            let extra = addr - current;
            let pages = (extra + 4095) / 4096;
            extern "C" { fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void; }
            let ptr = pmm_alloc_pages(pages);
            if ptr.is_null() { return Errno::ENOMEM.as_ret(); }
        }
        BRK.store(addr, core::sync::atomic::Ordering::SeqCst);
        addr as i64
    }
}

unsafe fn sys_mmap(addr: u64, size: u64, prot: i32, flags: i32) -> i64 {
    if size == 0 { return Errno::EINVAL.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 7, 0x01) {
        return Errno::EACCES.as_ret();
    }

    let mm = match crate::kernel::mm::vma::get_current_mm() {
        Some(m) => m,
        None => {
            extern "C" { fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void; }
            let pages = (size + 4095) / 4096;
            let ptr = pmm_alloc_pages(pages);
            return if ptr.is_null() { Errno::ENOMEM.as_ret() } else { ptr as i64 };
        }
    };

    match crate::kernel::syscall::mmap::mmap_syscall(mm, addr, size, prot, flags) {
        Ok(a) => a as i64,
        Err(e) => e.as_ret(),
    }
}

unsafe fn sys_munmap(addr: u64, size: u64) -> i64 {
    if addr == 0 || size == 0 { return Errno::EINVAL.as_ret(); }

    let mm = match crate::kernel::mm::vma::get_current_mm() {
        Some(m) => m,
        None => {
            extern "C" { fn pmm_free_pages(addr: *mut core::ffi::c_void, count: u64); }
            let pages = (size + 4095) / 4096;
            pmm_free_pages(addr as *mut core::ffi::c_void, pages);
            return 0;
        }
    };

    match crate::kernel::syscall::mmap::munmap_syscall(mm, addr, size) {
        Ok(()) => 0,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// 系统信息 — uname
// ============================================================================

unsafe fn sys_uname(buf: *mut core::ffi::c_void) -> i64 {
    if buf.is_null() || !validate_user_buf(buf as u64, 390) { return Errno::EFAULT.as_ret(); }
    #[repr(C)]
    struct Utsname {
        sysname: [u8; 65],
        nodename: [u8; 65],
        release: [u8; 65],
        version: [u8; 65],
        machine: [u8; 65],
        domainname: [u8; 65],
    }
    fn str65(s: &[u8]) -> [u8; 65] {
        let mut buf = [0u8; 65];
        let len = s.len().min(64);
        buf[..len].copy_from_slice(&s[..len]);
        buf
    }
    let un = Utsname {
        sysname: str65(b"QueenX"),
        nodename: str65(b"localhost"),
        release: str65(b"0.1.0"),
        version: str65(b"QueenX POSIX Kernel"),
        machine: {
            #[cfg(target_arch = "x86_64")] { str65(b"x86_64") }
            #[cfg(target_arch = "aarch64")] { str65(b"aarch64") }
        },
        domainname: [0u8; 65],
    };
    let dst = buf as *mut u8;
    let src = &un as *const Utsname as *const u8;
    core::ptr::copy_nonoverlapping(src, dst, core::mem::size_of::<Utsname>());
    0
}

// ============================================================================
// 时间 — time
// ============================================================================

unsafe fn sys_time(buf: *mut u64) -> i64 {
    if buf.is_null() { return Errno::EINVAL.as_ret(); }
    extern "C" { fn timer_get_ticks() -> u64; }
    let ticks = timer_get_ticks();
    *buf = ticks;
    ticks as i64
}

// ============================================================================
// 网络 — socket / bind / listen / accept / connect / sendto / recvfrom / shutdown
// ============================================================================

#[cfg(feature = "net")]
unsafe fn sys_socket(domain: i32, sock_type: i32, protocol: i32) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 2, 0x01) {
        return Errno::EACCES.as_ret();
    }
    extern "C" { fn lwip_socket(domain: i32, sock_type: i32, protocol: i32) -> i32; }
    lwip_socket(domain, sock_type, protocol) as i64
}

#[cfg(feature = "net")]
unsafe fn sys_bind(sockfd: i32, addr: u64, addrlen: u32) -> i64 {
    extern "C" { fn lwip_bind(sockfd: i32, addr: *const u8, addrlen: u32) -> i32; }
    lwip_bind(sockfd, addr as *const u8, addrlen) as i64
}

#[cfg(feature = "net")]
unsafe fn sys_listen(sockfd: i32, backlog: i32) -> i64 {
    extern "C" { fn lwip_listen(sockfd: i32, backlog: i32) -> i32; }
    lwip_listen(sockfd, backlog) as i64
}

#[cfg(feature = "net")]
unsafe fn sys_accept(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    extern "C" { fn lwip_accept(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32; }
    lwip_accept(sockfd, addr as *mut u8, addrlen as *mut u32) as i64
}

#[cfg(feature = "net")]
unsafe fn sys_connect(sockfd: i32, addr: u64, addrlen: u32) -> i64 {
    extern "C" { fn lwip_connect(sockfd: i32, addr: *const u8, addrlen: u32) -> i32; }
    lwip_connect(sockfd, addr as *const u8, addrlen) as i64
}

#[cfg(feature = "net")]
unsafe fn sys_sendto(sockfd: i32, buf: u64, len: u32, flags: i32) -> i64 {
    extern "C" { fn lwip_send(sockfd: i32, buf: *const u8, len: u32, flags: i32) -> i32; }
    lwip_send(sockfd, buf as *const u8, len, flags) as i64
}

#[cfg(feature = "net")]
unsafe fn sys_recvfrom(sockfd: i32, buf: u64, len: u32, flags: i32) -> i64 {
    extern "C" { fn lwip_recv(sockfd: i32, buf: *mut u8, len: u32, flags: i32) -> i32; }
    lwip_recv(sockfd, buf as *mut u8, len, flags) as i64
}

#[cfg(feature = "net")]
unsafe fn sys_shutdown(sockfd: i32, _how: i32) -> i64 {
    extern "C" { fn lwip_close(sockfd: i32) -> i32; }
    lwip_close(sockfd) as i64
}

// ============================================================================
// PWM 认证/权限 (QueenX 私有 syscall)
// ============================================================================

unsafe fn sys_auth_login(password: *const core::ffi::c_char, note: *const core::ffi::c_char) -> i64 {
    crate::kernel::pwm::ffi::pwm_login(note, password) as i64
}

unsafe fn sys_auth_logout() -> i64 {
    crate::kernel::pwm::ffi::pwm_logout();
    0
}

unsafe fn sys_auth_create(password: *const core::ffi::c_char, note: *const core::ffi::c_char, _level: u8) -> i64 {
    let creator = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::pwm::ffi::pwm_create(password, note, creator) as i64
}

unsafe fn sys_auth_delete(target: u64) -> i64 {
    crate::kernel::pwm::ffi::pwm_delete(target) as i64
}

unsafe fn sys_auth_info(target: u64) -> i64 {
    crate::kernel::pwm::ffi::pwm_get_privilege_level(target) as i64
}

unsafe fn sys_auth_changepw(old_pw: *const core::ffi::c_char, new_pw: *const core::ffi::c_char) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::pwm::ffi::pwm_change_password(pwm, old_pw, new_pw) as i64
}

unsafe fn sys_auth_verify(password: *const core::ffi::c_char) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::pwm::ffi::pwm_verify_password(pwm, password) as i64
}

unsafe fn sys_auth_create_first(password: *const core::ffi::c_char) -> i64 {
    if password.is_null() { return Errno::EINVAL.as_ret(); }
    crate::kernel::pwm::ffi::pwm_create_first_identity(password) as i64
}

unsafe fn sys_auth_token_create(holder: u64, domain: u16, caps: u64, _duration: u64, _max_uses: u32) -> i64 {
    let creator = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::pwm::ffi::pwm_grant(creator, holder, domain, caps) as i64
}

unsafe fn sys_auth_grant(grantor: u64, grantee: u64, domain: u16, caps: u64) -> i64 {
    crate::kernel::pwm::ffi::pwm_grant(grantor, grantee, domain, caps) as i64
}

unsafe fn sys_auth_revoke(revoker: u64, target: u64, domain: u16, caps: u64) -> i64 {
    crate::kernel::pwm::ffi::pwm_revoke(revoker, target, domain, caps) as i64
}

unsafe fn sys_auth_check_cap(pwm: u64, domain: u16, required: u64) -> i64 {
    if crate::kernel::pwm::ffi::pwm_has_capability(pwm, domain, required) { 1 } else { 0 }
}

unsafe fn sys_auth_get_caps(pwm: u64, domain: u16) -> i64 {
    crate::kernel::pwm::ffi::pwm_get_capability_raw(pwm, domain) as i64
}

unsafe fn sys_pwm_get() -> i64 {
    crate::kernel::pwm::ffi::pwm_get_current() as i64
}

unsafe fn sys_pwm_set(pwm: u64) -> i64 {
    let pid = crate::kernel::proc::ffi::process_get_current_pid();
    crate::kernel::proc::ffi::proc_set_pwm(pid, pwm) as i64
}

// ============================================================================
// 系统信息 / 环境 (QueenX 私有 syscall)
// ============================================================================

unsafe fn sys_gethostname(buf: *mut core::ffi::c_char, size: u64) -> i64 {
    if buf.is_null() || size == 0 || !validate_user_buf(buf as u64, size) { return Errno::EFAULT.as_ret(); }
    let hostname = b"localhost\0";
    let copy_len = hostname.len().min(size as usize - 1);
    core::ptr::copy_nonoverlapping(hostname.as_ptr(), buf as *mut u8, copy_len);
    *buf.add(copy_len) = 0;
    0
}

unsafe fn sys_sethostname(name: *const core::ffi::c_char, len: u64) -> i64 {
    if name.is_null() || len == 0 || len > 63 { return Errno::EINVAL.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 0, 9) {
        return Errno::EACCES.as_ret();
    }
    0
}

unsafe fn sys_reboot(cmd: i32) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 0, 0x01) {
        return Errno::EACCES.as_ret();
    }
    match cmd {
        0 => { loop {} }
        1 => {
            #[cfg(target_arch = "x86_64")]
            unsafe {
                core::arch::asm!(
                    "lidt [rdi]",
                    "int 0",
                    in("rdi") &[0u16; 4],
                    options(nostack, nomem)
                );
            }
            #[cfg(target_arch = "aarch64")]
            {
                core::arch::asm!("svc #0", in("x0") 0u64, options(nostack));
            }
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            loop {}
            loop {}
        }
        _ => Errno::EINVAL.as_ret(),
    }
}

unsafe fn sys_boot_check(check_type: i32) -> i64 {
    match check_type {
        0 => {
            if crate::kernel::pwm::ffi::pwm_any_identity_exists() { 1 } else { 0 }
        }
        _ => -1,
    }
}

// ============================================================================
// 磁盘管理 (QueenX 私有 syscall) — 通过 BlockDevice 注册表统一访问
// ============================================================================

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
unsafe fn sys_disk_list(disks: *mut u64, max_count: u32) -> i64 {
    if disks.is_null() || max_count == 0 { return Errno::EINVAL.as_ret(); }
    let count = crate::kernel::driver::block::block_device_count();
    let limit = max_count.min(count as u32);
    for i in 0..limit {
        *disks.add(i as usize) = i as u64;
    }
    limit as i64
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
unsafe fn sys_disk_info(disk_id: u32, info: *mut u8) -> i64 {
    if info.is_null() { return Errno::EINVAL.as_ret(); }
    let present = if crate::kernel::driver::block::hdd_is_present(disk_id as u8) { 1u32 } else { 0u32 };
    let sectors = if present != 0 {
        crate::kernel::driver::block::hdd_total_sectors(disk_id as u8) as u32
    } else {
        0
    };
    let model_bytes = b"Block Dev";
    let mut model = [0u8; 64];
    let copy_len = if model_bytes.len() < 63 { model_bytes.len() } else { 63 };
    core::ptr::copy_nonoverlapping(model_bytes.as_ptr(), model.as_mut_ptr(), copy_len);
    #[repr(C)]
    struct UserDiskInfo {
        disk_id: u32, present: u32, total_sectors: u32, sectors: u32, model: [u8; 64],
    }
    let disk_info = UserDiskInfo { disk_id, present, total_sectors: sectors, sectors, model };
    core::ptr::copy_nonoverlapping(
        &disk_info as *const UserDiskInfo as *const u8, info, core::mem::size_of::<UserDiskInfo>(),
    );
    0
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
unsafe fn sys_disk_format(disk_id: u32, fstype: *const core::ffi::c_char) -> i64 {
    if fstype.is_null() { return Errno::EINVAL.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 4, 0) { return Errno::EACCES.as_ret(); }
    if !crate::kernel::driver::block::hdd_is_present(disk_id as u8) { return Errno::ENOENT.as_ret(); }
    let hvfs_start_lba: u32 = 18432;
    let mut sector_buf = [0u8; 512];
    sector_buf[0] = 0x48; sector_buf[1] = 0x56; sector_buf[2] = 0x46; sector_buf[3] = 0x53;
    sector_buf[8] = 0x02; sector_buf[9] = 0x00;
    if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, hvfs_start_lba as u64, &sector_buf) < 0 {
        return Errno::EIO.as_ret();
    }
    0
}

fn write_le32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset] = val as u8;
    buf[offset+1] = (val >> 8) as u8;
    buf[offset+2] = (val >> 16) as u8;
    buf[offset+3] = (val >> 24) as u8;
}

fn write_le16(buf: &mut [u8], offset: usize, val: u16) {
    buf[offset] = val as u8;
    buf[offset+1] = (val >> 8) as u8;
}

const BOOT_PART_SECTORS: u32 = 16384;

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
unsafe fn sys_disk_partition(disk_id: u32, total_sectors: u64) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 4, 0) { return Errno::EACCES.as_ret(); }
    if !crate::kernel::driver::block::hdd_is_present(disk_id as u8) { return Errno::ENOENT.as_ret(); }
    let hvfs_start = BOOT_PART_SECTORS;
    let hvfs_sectors = if total_sectors > hvfs_start as u64 + 1 { total_sectors - hvfs_start as u64 } else { 0xFFFFFFFFu64 };
    let mut mbr = [0u8; 512];
    write_le32(&mut mbr, 446, 0x00000800);
    write_le32(&mut mbr, 450, 0x06FEFFFF);
    write_le32(&mut mbr, 454, 64u32);
    write_le32(&mut mbr, 458, BOOT_PART_SECTORS - 64);
    write_le32(&mut mbr, 462, (hvfs_start & 0xFFFFFFFF) as u32);
    write_le32(&mut mbr, 466, 0x83FEFFFF);
    let hvfs_len = if hvfs_sectors > 0xFFFFFFFF { 0xFFFFFFFFu32 } else { hvfs_sectors as u32 };
    write_le32(&mut mbr, 470, hvfs_start);
    write_le32(&mut mbr, 474, hvfs_len);
    mbr[510] = 0x55; mbr[511] = 0xAA;
    if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, 0, &mbr) < 0 { return Errno::EIO.as_ret(); }
    0
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
unsafe fn sys_fat_format(disk_id: u32) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 4, 0) { return Errno::EACCES.as_ret(); }
    if !crate::kernel::driver::block::hdd_is_present(disk_id as u8) { return Errno::ENOENT.as_ret(); }
    let fat_start_lba: u32 = 2048;
    let total_sectors: u16 = BOOT_PART_SECTORS as u16 - 64;
    let sectors_per_cluster: u8 = 8;
    let reserved_sectors: u16 = 1;
    let num_fats: u8 = 2;
    let root_entries: u16 = 512;
    let sectors_per_fat: u16 = ((total_sectors as u32 - 1 - 32) / (sectors_per_cluster as u32 * 256 + 2) + 1) as u16;
    let mut bpb = [0u8; 512];
    bpb[0] = 0xEB; bpb[1] = 0x3C; bpb[2] = 0x90;
    bpb[3] = b'A'; bpb[4] = b'N'; bpb[5] = b'T'; bpb[6] = b'X'; bpb[7] = b'B'; bpb[8] = b'O'; bpb[9] = b'O'; bpb[10] = b'T';
    write_le16(&mut bpb, 11, 512);
    bpb[13] = sectors_per_cluster;
    write_le16(&mut bpb, 14, reserved_sectors);
    bpb[16] = num_fats;
    write_le16(&mut bpb, 17, root_entries);
    write_le16(&mut bpb, 19, total_sectors);
    bpb[21] = 0xF8;
    write_le16(&mut bpb, 22, sectors_per_fat);
    bpb[36] = 0x80; bpb[38] = 0x29;
    bpb[510] = 0x55; bpb[511] = 0xAA;
    if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, fat_start_lba as u64, &bpb) < 0 { return Errno::EIO.as_ret(); }
    let fat_begin = fat_start_lba + reserved_sectors as u32;
    let mut fat_sector = [0u8; 512];
    fat_sector[0] = 0xF8; fat_sector[1] = 0xFF; fat_sector[2] = 0xFF; fat_sector[3] = 0xFF;
    for i in 0..num_fats {
        let lba = fat_begin + i as u32 * sectors_per_fat as u32;
        if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, lba as u64, &fat_sector) < 0 { return Errno::EIO.as_ret(); }
        let zero = [0u8; 512];
        for s in 1..sectors_per_fat as u32 { if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, (lba + s) as u64, &zero) < 0 { return Errno::EIO.as_ret(); } }
    }
    let root_dir_lba = fat_begin + num_fats as u32 * sectors_per_fat as u32;
    let root_dir_sectors = (root_entries as u32 * 32 + 511) / 512;
    let zero = [0u8; 512];
    for s in 0..root_dir_sectors { if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, (root_dir_lba + s) as u64, &zero) < 0 { return Errno::EIO.as_ret(); } }
    0
}

#[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
unsafe fn sys_boot_install(disk_id: u32) -> i64 {
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 4, 0) { return Errno::EACCES.as_ret(); }
    let stage1 = include_bytes!("../../../build/stage1.bin");
    if !crate::kernel::driver::block::hdd_is_present(disk_id as u8) { return Errno::ENOENT.as_ret(); }
    let mut mbr = [0u8; 512];
    if crate::kernel::driver::block::hdd_read_sector(disk_id as u8, 0, &mut mbr) < 0 { return Errno::EIO.as_ret(); }
    core::ptr::copy_nonoverlapping(stage1.as_ptr(), mbr.as_mut_ptr(), 440);
    let total_sectors = crate::kernel::driver::block::hdd_total_sectors(disk_id as u8);
    let hvfs_start = BOOT_PART_SECTORS;
    let hvfs_sectors = if total_sectors > hvfs_start as u64 + 1 { total_sectors - hvfs_start as u64 } else { 0xFFFFFFFFu64 };
    write_le32(&mut mbr, 446, 0x00000800);
    write_le32(&mut mbr, 450, 0x06FEFFFF);
    write_le32(&mut mbr, 454, 64u32);
    write_le32(&mut mbr, 458, BOOT_PART_SECTORS - 64);
    write_le32(&mut mbr, 462, (hvfs_start & 0xFFFFFFFF) as u32);
    write_le32(&mut mbr, 466, 0x83FEFFFF);
    write_le32(&mut mbr, 470, hvfs_start);
    let hvfs_len = if hvfs_sectors > 0xFFFFFFFF { 0xFFFFFFFFu32 } else { hvfs_sectors as u32 };
    write_le32(&mut mbr, 474, hvfs_len);
    mbr[510] = 0x55; mbr[511] = 0xAA;
    if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, 0, &mbr) < 0 { return Errno::EIO.as_ret(); }
    extern "C" { static _kernel_start: u8; static _kernel_end: u8; }
    let kernel_ptr = unsafe { &_kernel_start as *const u8 };
    let kernel_len = {
        let vma_end = unsafe { &_kernel_end as *const u8 as usize };
        const HHDM_OFFSET: usize = 0xFFFF_8000_0000_0000;
        let phys_end = vma_end.wrapping_sub(HHDM_OFFSET);
        phys_end - (kernel_ptr as usize)
    };
    let total_kernel_sectors = ((kernel_len + 511) / 512) as u32;
    let max_sectors = 2047u32;
    let copy_sectors = if total_kernel_sectors > max_sectors { max_sectors } else { total_kernel_sectors };
    for s in 0..copy_sectors {
        let offset = s as usize * 512;
        let remaining = kernel_len.saturating_sub(offset);
        if remaining == 0 { break; }
        let n = if remaining < 512 { remaining } else { 512 };
        let mut buf = [0u8; 512];
        unsafe { core::ptr::copy_nonoverlapping(kernel_ptr.add(offset), buf.as_mut_ptr(), n); }
        if crate::kernel::driver::block::hdd_write_sector(disk_id as u8, (1 + s) as u64, &buf) < 0 { return Errno::EIO.as_ret(); }
    }
    let mut cfg = [0u8; 512];
    cfg[0] = b'A'; cfg[1] = b'N'; cfg[2] = b'T'; cfg[3] = b'X';
    write_le32(&mut cfg, 4, BOOT_PART_SECTORS);
    cfg[510] = 0x55; cfg[511] = 0xAA;
    crate::kernel::driver::block::hdd_write_sector(disk_id as u8, 2046, &cfg);
    0
}

// ============================================================================
// 进程列表 (QueenX 私有 syscall)
// ============================================================================

#[repr(C)]
struct ProcListEntry {
    pid: u32, state: u8, _pad: [u8; 3], pwm: u64, priority: u32, _pad2: u32, name: [u8; 48],
}

unsafe fn sys_proc_list(buf: *mut u8, max_entries: u32) -> i64 {
    if buf.is_null() || !validate_user_ptr(buf as u64) { return Errno::EFAULT.as_ret(); }
    let entry_size = core::mem::size_of::<ProcListEntry>() as u32;
    let mut count: i32 = 0;
    let table = &crate::kernel::proc::process::PROCESS_TABLE;
    table.for_each(|proc| {
        if (count as u32) < max_entries {
            let entry = &mut *(buf.add(count as usize * entry_size as usize) as *mut ProcListEntry);
            entry.pid = proc.pid.0 as u32;
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

unsafe fn sys_proc_setpri(pid: u32, priority: u32) -> i64 {
    crate::kernel::proc::ffi::proc_set_priority(pid, priority) as i64
}

unsafe fn sys_proc_sleep(ms: u64) -> i64 {
    crate::kernel::proc::ffi::proc_sleep_ms(ms);
    0
}

// ============================================================================
// fcntl — 文件描述符操作
// ============================================================================

const F_DUPFD: i32 = 0;
const F_GETFD: i32 = 1;
const F_SETFD: i32 = 2;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;

unsafe fn sys_fcntl(fd: i32, cmd: i32, arg: u64) -> i64 {
    match cmd {
        F_GETFD => 0,
        F_SETFD => 0,
        F_GETFL => {
            let fd_table = crate::kernel::fs::vfs::vfs::VFS_MANAGER.fd_table.lock();
            if (fd as usize) < 256 && fd_table[fd as usize].used {
                fd_table[fd as usize].flags as i64
            } else {
                Errno::EBADF.as_ret()
            }
        }
        F_SETFL => 0,
        F_DUPFD => sys_dup2(fd, arg as i32),
        _ => Errno::EINVAL.as_ret(),
    }
}

// ============================================================================
// ioctl — 设备 I/O 控制
// ============================================================================

const TIOCGWINSZ: u64 = 0x5413;
const TCGETS: u64 = 0x5401;

unsafe fn sys_ioctl(_fd: i32, request: u64, arg: u64) -> i64 {
    if arg == 0 { return Errno::EINVAL.as_ret(); }
    match request {
        TIOCGWINSZ => {
            #[repr(C)]
            struct Winsize { ws_row: u16, ws_col: u16, ws_xpixel: u16, ws_ypixel: u16 }
            let ws = Winsize { ws_row: 25, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
            let dst = arg as *mut Winsize;
            core::ptr::write_volatile(dst, ws);
            0
        }
        TCGETS => 0,
        _ => Errno::ENOTTY.as_ret(),
    }
}

// ============================================================================
// nanosleep — 高精度睡眠 (基于 ticks, 1ms 粒度)
// ============================================================================

unsafe fn sys_nanosleep(req: u64, _rem: u64) -> i64 {
    if req == 0 || !validate_user_ptr(req) { return Errno::EINVAL.as_ret(); }
    #[repr(C)] struct Timespec { tv_sec: i64, tv_nsec: i64 }
    let ts = core::ptr::read_volatile(req as *const Timespec);
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return Errno::EINVAL.as_ret();
    }
    let total_ms = ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000;
    if total_ms == 0 { return 0; }
    extern "C" { fn timer_get_ticks() -> u64; }
    let start = timer_get_ticks();
    let target = start + total_ms;
    while timer_get_ticks() < target { core::hint::spin_loop(); }
    0
}

// ============================================================================
// gettimeofday / clock_gettime
// ============================================================================

const CLOCK_REALTIME: i32 = 0;
const CLOCK_MONOTONIC: i32 = 1;

unsafe fn sys_gettimeofday(tv: *mut core::ffi::c_void, _tz: *mut core::ffi::c_void) -> i64 {
    if tv.is_null() { return Errno::EINVAL.as_ret(); }
    extern "C" { fn timer_get_ticks() -> u64; }
    #[repr(C)] struct Timeval { tv_sec: i64, tv_usec: i64 }
    let ticks = timer_get_ticks();
    let t = Timeval { tv_sec: (ticks / 1000) as i64, tv_usec: ((ticks % 1000) * 1000) as i64 };
    let dst = tv as *mut Timeval;
    core::ptr::write_volatile(dst, t);
    0
}

unsafe fn sys_clock_gettime(clk_id: i32, tp: *mut core::ffi::c_void) -> i64 {
    if tp.is_null() { return Errno::EINVAL.as_ret(); }
    if clk_id != CLOCK_REALTIME && clk_id != CLOCK_MONOTONIC { return Errno::EINVAL.as_ret(); }
    extern "C" { fn timer_get_ticks() -> u64; }
    #[repr(C)] struct Timespec { tv_sec: i64, tv_nsec: i64 }
    let ticks = timer_get_ticks();
    let t = Timespec { tv_sec: (ticks / 1000) as i64, tv_nsec: ((ticks % 1000) * 1000000) as i64 };
    let dst = tp as *mut Timespec;
    core::ptr::write_volatile(dst, t);
    0
}

// ============================================================================
// poll — 基础轮询框架
// ============================================================================

const POLLIN: i16 = 1;
const POLLOUT: i16 = 4;

unsafe fn sys_poll(fds: *mut core::ffi::c_void, nfds: u32, _timeout: i32) -> i64 {
    if fds.is_null() || nfds == 0 { return 0; }
    #[repr(C)]
    struct PollFd { fd: i32, events: i16, revents: i16 }
    let mut ready: i32 = 0;
    for i in 0..nfds as usize {
        let pfd = &mut *(fds as *mut PollFd).add(i);
        pfd.revents = 0;
        if pfd.fd < 0 { continue; }
        if pfd.events & POLLIN != 0 {
            let fd_table = crate::kernel::fs::vfs::vfs::VFS_MANAGER.fd_table.lock();
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

unsafe fn sys_chmod(path: *const core::ffi::c_char, mode: u32) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_chmod(path, mode as u16, pwm) as i64
}

unsafe fn sys_fchmod(fd: i32, mode: u32) -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_fchmod(fd as u32, mode as u16) as i64
}

unsafe fn sys_chown(path: *const core::ffi::c_char, uid: u32, gid: u32) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) { return Errno::EFAULT.as_ret(); }
    let tbl = crate::kernel::pwm::table::get_table();
    let owner_pwm = tbl.find_by_uid(uid).map_or(0, |e| e.get_pwm().0);
    let group_pwm = tbl.find_by_uid(gid).map_or(0, |e| e.get_pwm().0);
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    crate::kernel::fs::vfs::ffi::vfs_chown_ext(path, owner_pwm, group_pwm, pwm) as i64
}

// ============================================================================
// kill — 信号发送
// ============================================================================

const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;

unsafe fn sys_kill(pid: i32, sig: i32) -> i64 {
    if pid <= 0 && pid != -1 { return Errno::ESRCH.as_ret(); }
    if sig == 0 {
        let target = crate::kernel::proc::process::PROCESS_TABLE.get(pid as u32);
        if target.is_some() { 0 } else { Errno::ESRCH.as_ret() }
    } else if sig == SIGTERM || sig == SIGKILL {
        let target = crate::kernel::proc::process::PROCESS_TABLE.get(pid as u32);
        match target {
            Some(_proc) => {
                unsafe { crate::kernel::proc::process::PROCESS_TABLE.remove_and_free(pid as u32); }
                0
            }
            None => Errno::ESRCH.as_ret(),
        }
    } else {
        0
    }
}

// ============================================================================
// readlink — 读取符号链接的目标路径
//
// 注: HvFS 当前不支持符号链接, 返回 EINVAL 提示调用者此路径非符号链接。
// ============================================================================

unsafe fn sys_readlink(path: *const core::ffi::c_char, buf: *mut core::ffi::c_char, bufsiz: u64) -> i64 {
    if path.is_null() || buf.is_null() || bufsiz == 0 { return Errno::EINVAL.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    let mut st_buf: crate::kernel::fs::vfs::types::VfsStat = core::mem::zeroed();
    let result = crate::kernel::fs::vfs::ffi::vfs_stat(path, &mut st_buf, pwm);
    if result < 0 { return Errno::ENOENT.as_ret(); }
    Errno::EINVAL.as_ret()
}

// ============================================================================
// umount2
// ============================================================================

unsafe fn sys_umount2(target: *const core::ffi::c_char, _flags: i32) -> i64 {
    if target.is_null() || !validate_user_ptr(target as u64) { return Errno::EFAULT.as_ret(); }
    let pwm = crate::kernel::pwm::ffi::pwm_get_current();
    if !crate::kernel::pwm::ffi::pwm_has_capability(pwm, 0, 0x01) {
        return Errno::EACCES.as_ret();
    }
    0
}

// ============================================================================
// getrlimit / sysinfo
// ============================================================================

unsafe fn sys_getrlimit(_resource: i32, rlim: *mut core::ffi::c_void) -> i64 {
    if rlim.is_null() { return Errno::EINVAL.as_ret(); }
    #[repr(C)] struct Rlimit { rlim_cur: u64, rlim_max: u64 }
    let r = Rlimit { rlim_cur: u64::MAX, rlim_max: u64::MAX };
    let dst = rlim as *mut Rlimit;
    core::ptr::write_volatile(dst, r);
    0
}

unsafe fn sys_sysinfo(info: *mut core::ffi::c_void) -> i64 {
    if info.is_null() { return Errno::EINVAL.as_ret(); }
    #[repr(C)]
    struct SysInfo {
        uptime: i64, loads: [u64; 3], totalram: u64, freeram: u64,
        sharedram: u64, bufferram: u64, totalswap: u64, freeswap: u64,
        procs: u16, _pad: [u8; 6], totalhigh: u64, freehigh: u64, mem_unit: u32,
    }
    let si = SysInfo {
        uptime: {
            extern "C" { fn timer_get_ticks() -> u64; }
            (timer_get_ticks() / 1000) as i64
        },
        loads: [0, 0, 0],
        totalram: 128 * 1024 * 1024,
        freeram: 97 * 1024 * 1024,
        sharedram: 0, bufferram: 0, totalswap: 0, freeswap: 0,
        procs: 1, _pad: [0u8; 6], totalhigh: 0, freehigh: 0, mem_unit: 1,
    };
    let dst = info as *mut SysInfo;
    core::ptr::write_volatile(dst, si);
    0
}

// ============================================================================
// 文件截断 — truncate / ftruncate
// ============================================================================

unsafe fn sys_truncate(path: *const core::ffi::c_char, length: i64) -> i64 {
    if path.is_null() || !validate_user_ptr(path as u64) || length < 0 {
        return Errno::EINVAL.as_ret();
    }
    let fd = crate::kernel::fs::vfs::ffi::vfs_open(path, 0o2, crate::kernel::pwm::ffi::pwm_get_current());
    if fd < 0 { return Errno::ENOENT.as_ret(); }
    let result = crate::kernel::fs::vfs::ffi::vfs_truncate_internal(fd as u32, length as u64);
    crate::kernel::fs::vfs::ffi::vfs_close(fd as u32);
    if result < 0 { Errno::EIO.as_ret() } else { 0 }
}

unsafe fn sys_ftruncate(fd: i32, length: i64) -> i64 {
    if fd < 0 || length < 0 { return Errno::EINVAL.as_ret(); }
    let result = crate::kernel::fs::vfs::ffi::vfs_truncate_internal(fd as u32, length as u64);
    if result < 0 { Errno::EIO.as_ret() } else { 0 }
}

// ============================================================================
// umask — 文件创建模式掩码
// ============================================================================

unsafe fn sys_umask(mask: u32) -> i64 {
    static UMASK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0o22);
    let old = UMASK.swap(mask & 0o777, core::sync::atomic::Ordering::SeqCst);
    old as i64
}

// ============================================================================
// 文件同步 — fsync
// ============================================================================

unsafe fn sys_fsync(fd: i32) -> i64 {
    if fd < 0 { return Errno::EBADF.as_ret(); }
    crate::kernel::fs::vfs::ffi::vfs_sync();
    0
}

// ============================================================================
// 进程组/会话 — getpgid / setsid
// ============================================================================

unsafe fn sys_getpgid(pid: i32) -> i64 {
    if pid < 0 { return Errno::EINVAL.as_ret(); }
    let _target_pid = if pid == 0 {
        crate::kernel::proc::ffi::process_get_current_pid()
    } else {
        pid as u32
    };
    crate::kernel::proc::ffi::process_get_current_pid() as i64
}

unsafe fn sys_setsid() -> i64 {
    let pid = crate::kernel::proc::ffi::process_get_current_pid();
    pid as i64
}

unsafe fn sys_gettid() -> i64 {
    crate::kernel::proc::ffi::process_get_current_pid() as i64
}

// ============================================================================
// tgkill — 向指定线程发送信号
// ============================================================================

unsafe fn sys_tgkill(_tgid: i32, tid: i32, sig: i32) -> i64 {
    sys_kill(tid, sig)
}

// ============================================================================
// 信号框架 — rt_sigaction / rt_sigprocmask / rt_sigreturn
// ============================================================================

const SIG_DFL: u64 = 0;
const SIG_IGN: u64 = 1;
const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SIG_SETMASK: i32 = 2;

static SIGNAL_HANDLERS: Mutex<[u64; 32]> = Mutex::new([SIG_DFL; 32]);
static SIGMASK: Mutex<u64> = Mutex::new(0);

unsafe fn sys_rt_sigaction(signum: i32, act: u64, oact: u64) -> i64 {
    if signum < 1 || signum > 31 { return Errno::EINVAL.as_ret(); }
    let mut handlers = SIGNAL_HANDLERS.lock();
    if oact != 0 {
        let dst = oact as *mut u64;
        core::ptr::write_volatile(dst, handlers[signum as usize]);
    }
    if act != 0 {
        let src = act as *const u64;
        handlers[signum as usize] = core::ptr::read_volatile(src);
    }
    0
}

unsafe fn sys_rt_sigprocmask(how: i32, set: u64, oset: u64) -> i64 {
    let mut mask = SIGMASK.lock();
    if oset != 0 {
        let dst = oset as *mut u64;
        core::ptr::write_volatile(dst, *mask);
    }
    if set != 0 {
        let new_set = core::ptr::read_volatile(set as *const u64);
        match how {
            SIG_BLOCK => *mask |= new_set,
            SIG_UNBLOCK => *mask &= !new_set,
            SIG_SETMASK => *mask = new_set,
            _ => return Errno::EINVAL.as_ret(),
        }
    }
    0
}

unsafe fn sys_rt_sigreturn() -> i64 {
    0
}

// ============================================================================
// 热插拔状态查询 (QueenX 私有 syscall 437)
// ============================================================================

unsafe fn sys_hotplug_status(buf: *mut u8, buf_size: u32) -> i64 {
    if buf.is_null() || buf_size == 0 { return Errno::EINVAL.as_ret(); }
    if !validate_user_buf(buf as u64, buf_size as u64) { return Errno::EFAULT.as_ret(); }

    let status = crate::kernel::driver::hotplug::HOTPLUG_MANAGER.status();

    let mut offset: u32 = 0;

    // 写入头部: enabled(u8) + slot_count(u32) + blk_device_count(u32)
    let header: [u8; 16] = [
        status.enabled as u8, 0, 0, 0,
        (status.slot_count & 0xFF) as u8,
        ((status.slot_count >> 8) & 0xFF) as u8,
        ((status.slot_count >> 16) & 0xFF) as u8,
        ((status.slot_count >> 24) & 0xFF) as u8,
        (status.blk_device_count & 0xFF) as u8,
        ((status.blk_device_count >> 8) & 0xFF) as u8,
        ((status.blk_device_count >> 16) & 0xFF) as u8,
        ((status.blk_device_count >> 24) & 0xFF) as u8,
        0, 0, 0, 0,
    ];
    if offset + 16 > buf_size { return offset as i64; }
    core::ptr::copy_nonoverlapping(header.as_ptr(), buf.add(offset as usize), 16);
    offset += 16;

    // 写入槽位信息 (每槽 8 字节)
    let slot_size: u32 = 8;
    for slot in &status.slots {
        if offset + slot_size > buf_size { break; }
        let info: [u8; 8] = [
            slot.bus, slot.device, slot.function, slot.slot_number,
            slot.presence as u8,
            ((slot.surprise_capable as u8) << 1) | (slot.hotplug_capable as u8),
            0, 0,
        ];
        core::ptr::copy_nonoverlapping(info.as_ptr(), buf.add(offset as usize), slot_size as usize);
        offset += slot_size;
    }

    // 写入块设备状态 (每设备 16 字节)
    let dev_size: u32 = 16;
    for dev in &status.blk_devices {
        if offset + dev_size > buf_size { break; }
        let info: [u8; 16] = [
            dev.drive, dev.present as u8, dev.removing as u8, 0,
            (dev.io_count & 0xFF) as u8,
            ((dev.io_count >> 8) & 0xFF) as u8,
            ((dev.io_count >> 16) & 0xFF) as u8,
            ((dev.io_count >> 24) & 0xFF) as u8,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        core::ptr::copy_nonoverlapping(info.as_ptr(), buf.add(offset as usize), dev_size as usize);
        offset += dev_size;
    }

    offset as i64
}
