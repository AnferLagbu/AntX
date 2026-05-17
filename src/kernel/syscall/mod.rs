/// Syscall 模块 - 系统调用接口定义与分发
///
/// 通过直接匹配分发实现核心系统调用。
/// 已连接 VFS、PWID、proc 等子系统的 FFI 接口。

pub mod types;
pub mod ffi;

use crate::kernel::syscall::types::*;
use crate::kernel::idt::types::InterruptFrame;

#[no_mangle]
pub unsafe extern "C" fn syscall_init() {
    unsafe { crate::kernel::klog::klog_write(1, 7, core::ptr::null(), core::ptr::null(), 0, b"syscall subsystem ready\0".as_ptr() as *const i8); }
}

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

/// 系统调用分发宏 — 每个分发点记一条日志
macro_rules! dispatch {
    ($num:expr, $name:expr) => {
        {
            let ret = $num;
            unsafe { crate::kernel::klog::klog_write(0, 7, core::ptr::null(), core::ptr::null(), 0, $name.as_ptr() as *const i8); }
            ret
        }
    };
}

#[no_mangle]
pub unsafe extern "C" fn syscall_dispatch(num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    match num {
        SYS_PROC_GETID   => dispatch!(sys_proc_getid(), b"proc_getid\0"),
        SYS_PROC_GETPPID => dispatch!(sys_proc_getppid(), b"proc_getppid\0"),
        SYS_PROC_YIELD   => dispatch!(sys_proc_yield(), b"proc_yield\0"),
        SYS_PROC_EXIT    => dispatch!(sys_proc_exit(a0 as i32), b"proc_exit\0"),
        SYS_PROC_CREATE  => dispatch!(sys_proc_create(a0 as *const i8, a1 as *const *const u8, a2 as u32), b"proc_create\0"),
        SYS_PROC_EXEC    => dispatch!(sys_proc_exec(a0 as *const i8, a1 as *const *const u8, a2 as u32), b"proc_exec\0"),
        SYS_PROC_WAIT    => dispatch!(sys_proc_wait(a0 as u32), b"proc_wait\0"),
        SYS_PROC_GETPWID => dispatch!(sys_proc_getpwid(), b"proc_getpwid\0"),
        SYS_PROC_SETPWID => dispatch!(sys_proc_setpwid(a0), b"proc_setpwid\0"),
        SYS_PROC_SETPRI  => dispatch!(sys_proc_setpri(a0 as u32, a1 as u32), b"proc_setpri\0"),
        SYS_PROC_SLEEP   => dispatch!(sys_proc_sleep(a0), b"proc_sleep\0"),

        SYS_FS_OPEN      => dispatch!(sys_fs_open(a0 as *const i8, a1 as i32, a2 as i32), b"fs_open\0"),
        SYS_FS_CLOSE     => dispatch!(sys_fs_close(a0 as i32), b"fs_close\0"),
        SYS_FS_READ      => dispatch!(sys_fs_read(a0 as i32, a1 as *mut u8, a2), b"fs_read\0"),
        SYS_FS_WRITE     => dispatch!(sys_fs_write(a0 as i32, a1 as *const u8, a2), b"fs_write\0"),
        SYS_FS_MKDIR     => dispatch!(sys_fs_mkdir(a0 as *const i8, a1 as i32), b"fs_mkdir\0"),
        SYS_FS_RMDIR     => dispatch!(sys_fs_rmdir(a0 as *const i8), b"fs_rmdir\0"),
        SYS_FS_MOUNT     => dispatch!(sys_fs_mount(a0 as *const i8, a1 as *const i8, a2 as *const i8, a3 as *const i8), b"fs_mount\0"),
        SYS_FS_SEEK      => dispatch!(sys_fs_seek(a0 as i32, a1 as i64, a2 as i32), b"fs_seek\0"),
        SYS_FS_STAT      => dispatch!(sys_fs_stat(a0 as *const i8, a1 as *mut core::ffi::c_void), b"fs_stat\0"),
        SYS_FS_READDIR   => dispatch!(sys_fs_readdir(a0 as i32, a1 as *mut core::ffi::c_void), b"fs_readdir\0"),
        SYS_FS_UNLINK    => dispatch!(sys_fs_unlink(a0 as *const i8), b"fs_unlink\0"),
        SYS_FS_SYNC      => dispatch!(sys_fs_sync(), b"fs_sync\0"),

        SYS_AUTH_LOGIN        => dispatch!(sys_auth_login(a0 as *const i8, a1 as *const i8), b"auth_login\0"),
        SYS_AUTH_LOGOUT       => dispatch!(sys_auth_logout(), b"auth_logout\0"),
        SYS_AUTH_CREATE       => dispatch!(sys_auth_create(a0 as *const i8, a1 as *const i8, a2 as u8), b"auth_create\0"),
        SYS_AUTH_DELETE       => dispatch!(sys_auth_delete(a0), b"auth_delete\0"),
        SYS_AUTH_INFO         => dispatch!(sys_auth_info(a0), b"auth_info\0"),
        SYS_AUTH_CHANGEPW     => dispatch!(sys_auth_changepw(a0 as *const i8, a1 as *const i8), b"auth_changepw\0"),
        SYS_AUTH_VERIFY       => dispatch!(sys_auth_verify(a0 as *const i8), b"auth_verify\0"),
        SYS_AUTH_TOKEN_CREATE => dispatch!(sys_auth_token_create(a0, a1 as u16, a2, a3, 1), b"auth_token_create\0"),
        SYS_AUTH_TOKEN_USE    => dispatch!(sys_auth_token_use(a0), b"auth_token_use\0"),
        SYS_AUTH_TOKEN_REVOKE => dispatch!(sys_auth_token_revoke(a0), b"auth_token_revoke\0"),

        SYS_ENV_GETCWD    => dispatch!(sys_env_getcwd(a0 as *mut i8, a1), b"env_getcwd\0"),
        SYS_ENV_CHDIR     => dispatch!(sys_env_chdir(a0 as *const i8), b"env_chdir\0"),
        SYS_GETHOSTNAME   => dispatch!(sys_gethostname(a0 as *mut i8, a1), b"gethostname\0"),
        SYS_SETHOSTNAME   => dispatch!(sys_sethostname(a0 as *const i8, a1), b"sethostname\0"),
        SYS_BOOT_CHECK    => dispatch!(sys_boot_check(a0 as i32), b"boot_check\0"),

        #[cfg(not(feature = "kernel_test"))]
        SYS_DISK_LIST    => dispatch!(sys_disk_list(a0 as *mut u64, a1 as u32), b"disk_list\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_DISK_INFO    => dispatch!(sys_disk_info(a0 as u32, a1 as *mut u8), b"disk_info\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_DISK_FORMAT  => dispatch!(sys_disk_format(a0 as u32, a1 as *const i8), b"disk_format\0"),

        SYS_MEM_BRK      => dispatch!(sys_mem_brk(a0), b"mem_brk\0"),
        SYS_MEM_MAP      => dispatch!(sys_mem_map(a0, a1, a2), b"mem_map\0"),
        SYS_MEM_UNMAP    => dispatch!(sys_mem_unmap(a0, a1), b"mem_unmap\0"),
        SYS_MEM_PROTECT  => dispatch!(sys_mem_protect(a0, a1, a2), b"mem_protect\0"),

        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_SOCKET   => dispatch!(sys_net_socket(a0 as i32, a1 as i32, a2 as i32), b"net_socket\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_BIND     => dispatch!(sys_net_bind(a0 as i32, a1, a2 as u32), b"net_bind\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_LISTEN   => dispatch!(sys_net_listen(a0 as i32, a1 as i32), b"net_listen\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_ACCEPT   => dispatch!(sys_net_accept(a0 as i32, a1, a2), b"net_accept\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_CONNECT  => dispatch!(sys_net_connect(a0 as i32, a1, a2 as u32), b"net_connect\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_SEND     => dispatch!(sys_net_send(a0 as i32, a1, a2 as u32, a3 as i32), b"net_send\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_RECV     => dispatch!(sys_net_recv(a0 as i32, a1, a2 as u32, a3 as i32), b"net_recv\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_NET_SHUTDOWN => dispatch!(sys_net_shutdown(a0 as i32, a1 as i32), b"net_shutdown\0"),

        SYS_IPC_PIPE     => dispatch!(sys_ipc_pipe(a0 as *mut i32, a1 as *mut i32), b"ipc_pipe\0"),

        #[cfg(not(feature = "kernel_test"))]
        SYS_DEV_IOCTL    => dispatch!(sys_dev_ioctl(a0 as i32, a1, a2), b"dev_ioctl\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_DEV_READ     => dispatch!(sys_dev_read(a0 as i32, a1, a2), b"dev_read\0"),
        #[cfg(not(feature = "kernel_test"))]
        SYS_DEV_WRITE    => dispatch!(sys_dev_write(a0 as i32, a1, a2), b"dev_write\0"),

        _ => SyscallError::E_NOSYS.as_i64(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn syscall_register(_num: u64, _handler: SyscallHandler) {}

// ============================================================================
// 进程管理 syscall
// ============================================================================

unsafe fn sys_proc_getid() -> i64 {
    crate::kernel::proc::ffi::process_get_current_pid() as i64
}

unsafe fn sys_proc_yield() -> i64 {
    crate::kernel::proc::ffi::scheduler_yield();
    0
}

unsafe fn sys_proc_exit(status: i32) -> i64 {
    crate::kernel::proc::ffi::process_exit(status as u32);
    0
}

// ============================================================================
// 文件系统 syscall
// ============================================================================

unsafe fn sys_fs_open(path: *const i8, flags: i32, _mode: i32) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::fs::vfs::ffi::vfs_open(path, flags as u32, pwid) as i64
}

unsafe fn sys_fs_close(fd: i32) -> i64 {
    if fd < 0 { return SyscallError::E_BADFD.as_i64(); }
    crate::kernel::fs::vfs::ffi::vfs_close(fd as u32) as i64
}

unsafe fn sys_fs_read(fd: i32, buf: *mut u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 { return -1; }
    if fd == 1 || fd == 2 { return SyscallError::E_BADFD.as_i64(); }
    if fd == 0 {
        #[cfg(not(feature = "kernel_test"))]
        {
            extern "C" { fn keyboard_has_data() -> bool; fn keyboard_get_char() -> i32; }
            extern "C" { fn serial_has_data(com: i32) -> bool; fn serial_getc(com: i32) -> i32; }
            if keyboard_has_data() { let c = keyboard_get_char(); if c > 0 { *buf = c as u8; return 1; } }
            if serial_has_data(0) { let c = serial_getc(0); if c > 0 { *buf = c as u8; return 1; } }
        }
        return 0;
    }
    crate::kernel::fs::vfs::ffi::vfs_read(fd as u32, buf as *mut u8, count as u32) as i64
}

unsafe fn sys_fs_write(fd: i32, buf: *const u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 { return -1; }
    if fd == 1 || fd == 2 {
        #[cfg(not(feature = "kernel_test"))]
        {
            extern "C" { fn serial_write(com: i32, buf: *const core::ffi::c_void, count: u64); }
            serial_write(0, buf as *const core::ffi::c_void, count);
        }
        return count as i64;
    }
    crate::kernel::fs::vfs::ffi::vfs_write(fd as u32, buf as *const u8, count as u32) as i64
}

unsafe fn sys_fs_mkdir(path: *const i8, _mode: i32) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    let pwid = if pwid == 0 { 0x0020F45A8B978417 } else { pwid };
    crate::kernel::fs::vfs::ffi::vfs_mkdir(path, pwid) as i64
}

unsafe fn sys_fs_rmdir(path: *const i8) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::fs::vfs::ffi::vfs_rmdir(path, pwid) as i64
}

unsafe fn sys_fs_mount(_source: *const i8, target: *const i8, fstype: *const i8, _options: *const i8) -> i64 {
    if target.is_null() || fstype.is_null() { return SyscallError::E_INVAL.as_i64(); }
    crate::kernel::fs::vfs::ffi::vfs_mount(target, fstype) as i64
}

unsafe fn sys_fs_seek(fd: i32, offset: i64, whence: i32) -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_seek(fd as u32, offset as i32, whence as u32) as i64
}

unsafe fn sys_fs_stat(path: *const i8, st_buf: *mut core::ffi::c_void) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::fs::vfs::ffi::vfs_stat(path, st_buf as *mut crate::kernel::fs::vfs::types::VfsStat, pwid) as i64
}

unsafe fn sys_fs_readdir(fd: i32, entry: *mut core::ffi::c_void) -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_readdir(fd as u32, entry as *mut crate::kernel::fs::vfs::types::VfsDirent) as i64
}

unsafe fn sys_fs_unlink(path: *const i8) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::fs::vfs::ffi::vfs_unlink(path, pwid) as i64
}

unsafe fn sys_fs_sync() -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_sync() as i64
}

// ============================================================================
// PWID 认证/权限 syscall
// ============================================================================

unsafe fn sys_auth_login(password: *const i8, note: *const i8) -> i64 {
    crate::kernel::pwid::ffi::pwid_login(note, password) as i64
}

unsafe fn sys_auth_logout() -> i64 {
    crate::kernel::pwid::ffi::pwid_logout();
    0
}

unsafe fn sys_auth_create(password: *const i8, note: *const i8, _level: u8) -> i64 {
    let creator = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_create(password, note, creator) as i64
}

unsafe fn sys_auth_delete(target: u64) -> i64 {
    crate::kernel::pwid::ffi::pwid_delete(target) as i64
}

unsafe fn sys_auth_info(target: u64) -> i64 {
    crate::kernel::pwid::ffi::pwid_get_privilege_level(target) as i64
}

unsafe fn sys_auth_changepw(old_pw: *const i8, new_pw: *const i8) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_change_password(pwid, old_pw, new_pw) as i64
}

unsafe fn sys_auth_verify(password: *const i8) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_verify_password(pwid, password) as i64
}

unsafe fn sys_auth_token_create(holder: u64, domain: u16, caps: u64, _duration: u64, _max_uses: u32) -> i64 {
    let creator = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_grant(creator, holder, domain, caps) as i64
}

unsafe fn sys_auth_token_use(_token_id: u64) -> i64 {
    0
}

unsafe fn sys_auth_token_revoke(_token_id: u64) -> i64 {
    0
}

// ============================================================================
// 系统信息 syscall
// ============================================================================

unsafe fn sys_env_getcwd(buf: *mut i8, size: u64) -> i64 {
    if buf.is_null() || size == 0 { return SyscallError::E_INVAL.as_i64(); }
    crate::kernel::fs::vfs::ffi::vfs_get_cwd(buf, size as u32) as i64
}

unsafe fn sys_env_chdir(path: *const i8) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    crate::kernel::fs::vfs::ffi::vfs_set_cwd(path);
    0
}

unsafe fn sys_gethostname(buf: *mut i8, size: u64) -> i64 {
    if buf.is_null() || size == 0 { return SyscallError::E_INVAL.as_i64(); }
    let hostname = b"localhost\0";
    let copy_len = hostname.len().min(size as usize - 1);
    core::ptr::copy_nonoverlapping(hostname.as_ptr(), buf as *mut u8, copy_len);
    *buf.add(copy_len) = 0;
    0
}

unsafe fn sys_sethostname(name: *const i8, len: u64) -> i64 {
    if name.is_null() || len == 0 || len > 63 { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 0, 9) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    0
}

unsafe fn sys_boot_check(check_type: i32) -> i64 {
    match check_type {
        0 => {
            if crate::kernel::pwid::ffi::pwid_any_identity_exists() { 1 } else { 0 }
        }
        _ => -1,
    }
}

// ============================================================================
// 磁盘管理 syscall
// ============================================================================

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_disk_list(disks: *mut u64, max_count: u32) -> i64 {
    if disks.is_null() || max_count == 0 { return SyscallError::E_INVAL.as_i64(); }
    extern "C" { fn ata_disk_present(drive: u8) -> i32; }
    let mut count: u32 = 0;
    for drive in 0..4u8 {
        if count >= max_count { break; }
        if ata_disk_present(drive) != 0 { *disks.add(count as usize) = drive as u64; count += 1; }
    }
    count as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_disk_info(disk_id: u32, info: *mut u8) -> i64 {
    if info.is_null() { return SyscallError::E_INVAL.as_i64(); }
    if disk_id >= 4 { return SyscallError::E_NOTFOUND.as_i64(); }
    0
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_disk_format(disk_id: u32, fstype: *const i8) -> i64 {
    if fstype.is_null() { return SyscallError::E_INVAL.as_i64(); }
    if disk_id >= 4 { return SyscallError::E_NOTFOUND.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 4, 0) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    extern "C" { fn ata_disk_present(drive: u8) -> i32; }
    if ata_disk_present(disk_id as u8) == 0 { return SyscallError::E_NOTFOUND.as_i64(); }
    SyscallError::E_NOSYS.as_i64()
}

// ============================================================================
// 进程管理 syscall (补全)
// ============================================================================

unsafe fn sys_proc_create(path: *const i8, argv: *const *const u8, argc: u32) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if pwid == 0 { return SyscallError::E_AUTH_NOTFOUND.as_i64(); }
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 3, 0x01) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    let pid = crate::kernel::proc::ffi::proc_create_user(path, argv, argc, pwid);
    if pid == 0 { SyscallError::E_BUSY.as_i64() } else { pid as i64 }
}

unsafe fn sys_proc_exec(path: *const i8, argv: *const *const u8, argc: u32) -> i64 {
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let result = crate::kernel::proc::ffi::proc_exec_replace(path, argv, argc);
    if result < 0 { SyscallError::E_NOTFOUND.as_i64() } else { 0 }
}

unsafe fn sys_proc_wait(pid: u32) -> i64 {
    let result = crate::kernel::proc::ffi::proc_wait_child(pid);
    result as i64
}

unsafe fn sys_proc_sleep(ms: u64) -> i64 {
    crate::kernel::proc::ffi::proc_sleep_ms(ms);
    0
}

unsafe fn sys_proc_getppid() -> i64 {
    let pid = crate::kernel::proc::ffi::process_get_current_pid();
    crate::kernel::proc::ffi::proc_get_ppid(pid) as i64
}

unsafe fn sys_proc_getpwid() -> i64 {
    crate::kernel::pwid::ffi::pwid_get_current() as i64
}

unsafe fn sys_proc_setpwid(pwid: u64) -> i64 {
    let pid = crate::kernel::proc::ffi::process_get_current_pid();
    crate::kernel::proc::ffi::proc_set_pwid(pid, pwid) as i64
}

unsafe fn sys_proc_setpri(pid: u32, priority: u32) -> i64 {
    crate::kernel::proc::ffi::proc_set_priority(pid, priority) as i64
}

// ============================================================================
// 内存管理 syscall
// ============================================================================

unsafe fn sys_mem_brk(addr: u64) -> i64 {
    extern "C" { fn kmalloc(size: u64) -> *mut core::ffi::c_void; }
    if addr == 0 {
        0
    } else {
        SyscallError::E_NOSYS.as_i64()
    }
}

unsafe fn sys_mem_map(_addr: u64, size: u64, _prot: u64) -> i64 {
    if size == 0 { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 7, 0x01) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    extern "C" { fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void; }
    let pages = (size + 4095) / 4096;
    let ptr = pmm_alloc_pages(pages);
    if ptr.is_null() { SyscallError::E_BUSY.as_i64() } else { ptr as i64 }
}

unsafe fn sys_mem_unmap(addr: u64, size: u64) -> i64 {
    if addr == 0 || size == 0 { return SyscallError::E_INVAL.as_i64(); }
    extern "C" { fn pmm_free_pages(addr: *mut core::ffi::c_void, count: u64); }
    let pages = (size + 4095) / 4096;
    pmm_free_pages(addr as *mut core::ffi::c_void, pages);
    0
}

unsafe fn sys_mem_protect(_addr: u64, _size: u64, _prot: u64) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

// ============================================================================
// 网络 syscall
// ============================================================================

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_socket(domain: i32, sock_type: i32, protocol: i32) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 2, 0x01) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    extern "C" { fn lwip_socket(domain: i32, sock_type: i32, protocol: i32) -> i32; }
    lwip_socket(domain, sock_type, protocol) as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_bind(sockfd: i32, addr: u64, addrlen: u32) -> i64 {
    extern "C" { fn lwip_bind(sockfd: i32, addr: *const u8, addrlen: u32) -> i32; }
    lwip_bind(sockfd, addr as *const u8, addrlen) as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_listen(sockfd: i32, backlog: i32) -> i64 {
    extern "C" { fn lwip_listen(sockfd: i32, backlog: i32) -> i32; }
    lwip_listen(sockfd, backlog) as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_accept(sockfd: i32, addr: u64, addrlen: u64) -> i64 {
    extern "C" { fn lwip_accept(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32; }
    lwip_accept(sockfd, addr as *mut u8, addrlen as *mut u32) as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_connect(sockfd: i32, addr: u64, addrlen: u32) -> i64 {
    extern "C" { fn lwip_connect(sockfd: i32, addr: *const u8, addrlen: u32) -> i32; }
    lwip_connect(sockfd, addr as *const u8, addrlen) as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_send(sockfd: i32, buf: u64, len: u32, flags: i32) -> i64 {
    extern "C" { fn lwip_send(sockfd: i32, buf: *const u8, len: u32, flags: i32) -> i32; }
    lwip_send(sockfd, buf as *const u8, len, flags) as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_recv(sockfd: i32, buf: u64, len: u32, flags: i32) -> i64 {
    extern "C" { fn lwip_recv(sockfd: i32, buf: *mut u8, len: u32, flags: i32) -> i32; }
    lwip_recv(sockfd, buf as *mut u8, len, flags) as i64
}

#[cfg(not(feature = "kernel_test"))]
unsafe fn sys_net_shutdown(sockfd: i32, _how: i32) -> i64 {
    extern "C" { fn lwip_close(sockfd: i32) -> i32; }
    lwip_close(sockfd) as i64
}

// ============================================================================
// IPC syscall
// ============================================================================

unsafe fn sys_ipc_pipe(read_fd: *mut i32, write_fd: *mut i32) -> i64 {
    if read_fd.is_null() || write_fd.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 6, 0x01) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    let (r, w) = {
        let mut pipefd: [i32; 2] = [0; 2];
        let result = crate::kernel::ipc::pipe::ipc_pipe_create(pipefd.as_mut_ptr());
        if result < 0 { return SyscallError::E_BUSY.as_i64(); }
        (pipefd[0], pipefd[1])
    };
    if r < 0 || w < 0 { return SyscallError::E_BUSY.as_i64(); }
    *read_fd = r;
    *write_fd = w;
    0
}

// ============================================================================
// 设备 I/O syscall
// ============================================================================

unsafe fn sys_dev_ioctl(_fd: i32, _cmd: u64, _arg: u64) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_dev_read(fd: i32, buf: u64, count: u64) -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_read(fd as u32, buf as *mut u8, count as u32) as i64
}

unsafe fn sys_dev_write(fd: i32, buf: u64, count: u64) -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_write(fd as u32, buf as *const u8, count as u32) as i64
}

// ============================================================================
// 环境/系统 syscall (补全)
// ============================================================================

unsafe fn sys_reboot(cmd: i32) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 0, 0x01) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    match cmd {
        0 => { loop {} }
        1 => { extern "C" { fn reboot_internal() -> !; } unsafe { reboot_internal(); } }
        _ => SyscallError::E_INVAL.as_i64(),
    }
}

unsafe fn sys_time(buf: *mut u64) -> i64 {
    if buf.is_null() { return SyscallError::E_INVAL.as_i64(); }
    extern "C" { fn timer_get_ticks() -> u64; }
    let ticks = timer_get_ticks();
    *buf = ticks;
    ticks as i64
}

unsafe fn sys_sysinfo(info: *mut u8) -> i64 {
    if info.is_null() { return SyscallError::E_INVAL.as_i64(); }
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_getvar(name: *const i8, buf: *mut i8, _size: u64) -> i64 {
    if name.is_null() || buf.is_null() { return SyscallError::E_INVAL.as_i64(); }
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_setvar(name: *const i8, _value: *const i8) -> i64 {
    if name.is_null() { return SyscallError::E_INVAL.as_i64(); }
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_sync() -> i64 {
    crate::kernel::fs::vfs::ffi::vfs_sync();
    0
}

unsafe fn sys_mount(source: *const i8, target: *const i8, fstype: *const i8) -> i64 {
    if source.is_null() || target.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 0, 0x01) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    crate::kernel::fs::vfs::ffi::vfs_mount(target, fstype) as i64
}

unsafe fn sys_unmount(target: *const i8) -> i64 {
    if target.is_null() { return SyscallError::E_INVAL.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if !crate::kernel::pwid::ffi::pwid_has_capability(pwid, 0, 0x01) {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_install_grub(_disk_id: u32) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_disk_partition(_disk_id: u32) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_fchmod(_fd: i32, _mode: u32) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

unsafe fn sys_rename(old: *const i8, new: *const i8) -> i64 {
    if old.is_null() || new.is_null() { return SyscallError::E_INVAL.as_i64(); }
    SyscallError::E_NOSYS.as_i64()
}
