/// Syscall 模块 - 系统调用接口定义与分发
///
/// 通过直接匹配分发实现核心系统调用。
/// 已连接 VFS、PWID、proc 等子系统的 FFI 接口。

pub mod types;

use crate::kernel::syscall::types::*;

#[no_mangle]
pub unsafe extern "C" fn syscall_init() {}

#[no_mangle]
pub unsafe extern "C" fn syscall_dispatch(num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    match num {
        SYS_PROC_GETID   => sys_proc_getid(),
        SYS_PROC_GETPPID => sys_proc_getppid(),
        SYS_PROC_YIELD   => sys_proc_yield(),
        SYS_PROC_EXIT    => sys_proc_exit(a0 as i32),

        SYS_FS_OPEN      => sys_fs_open(a0 as *const i8, a1 as i32, a2 as i32),
        SYS_FS_CLOSE     => sys_fs_close(a0 as i32),
        SYS_FS_READ      => sys_fs_read(a0 as i32, a1 as *mut u8, a2),
        SYS_FS_WRITE     => sys_fs_write(a0 as i32, a1 as *const u8, a2),
        SYS_FS_MKDIR     => sys_fs_mkdir(a0 as *const i8, a1 as i32),
        SYS_FS_RMDIR     => sys_fs_rmdir(a0 as *const i8),
        SYS_FS_MOUNT     => sys_fs_mount(a0 as *const i8, a1 as *const i8, a2 as *const i8, a3 as *const i8),
        SYS_FS_SEEK      => sys_fs_seek(a0 as i32, a1 as i64, a2 as i32),
        SYS_FS_STAT      => sys_fs_stat(a0 as *const i8, a1 as *mut core::ffi::c_void),
        SYS_FS_READDIR   => sys_fs_readdir(a0 as i32, a1 as *mut core::ffi::c_void),
        SYS_FS_UNLINK    => sys_fs_unlink(a0 as *const i8),
        SYS_FS_SYNC      => sys_fs_sync(),

        SYS_AUTH_LOGIN        => sys_auth_login(a0 as *const i8, a1 as *const i8),
        SYS_AUTH_LOGOUT       => sys_auth_logout(),
        SYS_AUTH_CREATE       => sys_auth_create(a0 as *const i8, a1 as *const i8, a2 as u8),
        SYS_AUTH_DELETE       => sys_auth_delete(a0),
        SYS_AUTH_INFO         => sys_auth_info(a0),
        SYS_AUTH_CHANGEPW     => sys_auth_changepw(a0 as *const i8, a1 as *const i8),
        SYS_AUTH_VERIFY       => sys_auth_verify(a0 as *const i8),
        SYS_AUTH_TOKEN_CREATE => sys_auth_token_create(a0, a1 as u16, a2, a3, 1),
        SYS_AUTH_TOKEN_USE    => sys_auth_token_use(a0),
        SYS_AUTH_TOKEN_REVOKE => sys_auth_token_revoke(a0),

        SYS_ENV_GETCWD    => sys_env_getcwd(a0 as *mut i8, a1),
        SYS_ENV_CHDIR     => sys_env_chdir(a0 as *const i8),
        SYS_GETHOSTNAME   => sys_gethostname(a0 as *mut i8, a1),
        SYS_SETHOSTNAME   => sys_sethostname(a0 as *const i8, a1),
        SYS_BOOT_CHECK    => sys_boot_check(a0 as i32),

        SYS_DISK_LIST    => sys_disk_list(a0 as *mut u64, a1 as u32),
        SYS_DISK_INFO    => sys_disk_info(a0 as u32, a1 as *mut u8),
        SYS_DISK_FORMAT  => sys_disk_format(a0 as u32, a1 as *const i8),

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

unsafe fn sys_proc_getppid() -> i64 {
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
        extern "C" { fn keyboard_has_data() -> bool; fn keyboard_get_char() -> i32; }
        extern "C" { fn serial_has_data(com: i32) -> bool; fn serial_getc(com: i32) -> i32; }
        if keyboard_has_data() { let c = keyboard_get_char(); if c > 0 { *buf = c as u8; return 1; } }
        if serial_has_data(0) { let c = serial_getc(0); if c > 0 { *buf = c as u8; return 1; } }
        return 0;
    }
    crate::kernel::fs::vfs::ffi::vfs_read(fd as u32, buf as *mut u8, count as u32) as i64
}

unsafe fn sys_fs_write(fd: i32, buf: *const u8, count: u64) -> i64 {
    if buf.is_null() || count == 0 { return -1; }
    if fd == 1 || fd == 2 {
        extern "C" { fn serial_write(com: i32, buf: *const core::ffi::c_void, count: u64); }
        serial_write(0, buf as *const core::ffi::c_void, count);
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
    crate::kernel::fs::vfs::ffi::vfs_seek(fd as u32, offset as u32, whence as u32) as i64
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

unsafe fn sys_auth_create(password: *const i8, note: *const i8, level: u8) -> i64 {
    crate::kernel::pwid::ffi::pwid_create_user(password, note, level) as i64
}

unsafe fn sys_auth_delete(target: u64) -> i64 {
    crate::kernel::pwid::ffi::pwid_delete(target) as i64
}

unsafe fn sys_auth_info(target: u64) -> i64 {
    crate::kernel::pwid::ffi::pwid_get_level(target) as i64
}

unsafe fn sys_auth_changepw(old_pw: *const i8, new_pw: *const i8) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_change_password(pwid, old_pw, new_pw) as i64
}

unsafe fn sys_auth_verify(password: *const i8) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_verify_password(pwid, password) as i64
}

unsafe fn sys_auth_token_create(holder: u64, domain: u16, caps: u64, duration: u64, _max_uses: u32) -> i64 {
    let creator = crate::kernel::pwid::ffi::pwid_get_current();
    let _ = domain;
    crate::kernel::pwid::ffi::pwid_create_token(creator, holder, caps, duration) as i64
}

unsafe fn sys_auth_token_use(token_id: u64) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_use_token_internal(token_id, pwid) as i64
}

unsafe fn sys_auth_token_revoke(token_id: u64) -> i64 {
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    crate::kernel::pwid::ffi::pwid_revoke_token_internal(token_id, pwid) as i64
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
    if crate::kernel::pwid::ffi::pwid_has_capability(pwid, 0, 9) == 0 {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    0
}

unsafe fn sys_boot_check(check_type: i32) -> i64 {
    match check_type {
        0 => {
            if crate::kernel::pwid::ffi::pwid_any_identity_exists() != 0 { 1 } else { 0 }
        }
        _ => -1,
    }
}

// ============================================================================
// 磁盘管理 syscall
// ============================================================================

unsafe fn sys_disk_list(disks: *mut u64, max_count: u32) -> i64 {
    if disks.is_null() || max_count == 0 { return SyscallError::E_INVAL.as_i64(); }
    extern "C" { fn ata_disk_present(drive: u8) -> bool; }
    let mut count: u32 = 0;
    for drive in 0..4u8 {
        if count >= max_count { break; }
        if ata_disk_present(drive) { *disks.add(count as usize) = drive as u64; count += 1; }
    }
    count as i64
}

unsafe fn sys_disk_info(disk_id: u32, info: *mut u8) -> i64 {
    if info.is_null() { return SyscallError::E_INVAL.as_i64(); }
    if disk_id >= 4 { return SyscallError::E_NOTFOUND.as_i64(); }
    0
}

unsafe fn sys_disk_format(disk_id: u32, fstype: *const i8) -> i64 {
    if fstype.is_null() { return SyscallError::E_INVAL.as_i64(); }
    if disk_id >= 4 { return SyscallError::E_NOTFOUND.as_i64(); }
    let pwid = crate::kernel::pwid::ffi::pwid_get_current();
    if crate::kernel::pwid::ffi::pwid_has_capability(pwid, 4, 0) == 0 {
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    extern "C" { fn ata_disk_present(drive: u8) -> bool; }
    if !ata_disk_present(disk_id as u8) { return SyscallError::E_NOTFOUND.as_i64(); }
    SyscallError::E_NOSYS.as_i64()
}
