//! AntX 用户态运行时 — 系统调用层
//!
//! 所有 syscall 通过 `int 0x80` 触发，使用标准 x86_64 调用约定:
//!   rax = syscall 号, rdi/rsi/rdx/r10 = 参数 1-4, 返回值在 rax

use core::arch::asm;

pub const O_RDONLY: i32 = 0;
pub const O_WRONLY: i32 = 1;
pub const O_RDWR: i32 = 2;
pub const O_CREAT: i32 = 0o100;
pub const O_TRUNC: i32 = 0o1000;

pub const FT_FILE: u8 = 0;
pub const FT_DIR: u8 = 1;
pub const FT_DEV: u8 = 2;

pub const SYS_PROC_EXEC: u64 = 1;
pub const SYS_PROC_EXIT: u64 = 2;
pub const SYS_PROC_GETID: u64 = 4;
pub const SYS_PROC_GETPWID: u64 = 6;
pub const SYS_PROC_YIELD: u64 = 9;

pub const SYS_FS_OPEN: u64 = 20;
pub const SYS_FS_CLOSE: u64 = 21;
pub const SYS_FS_READ: u64 = 22;
pub const SYS_FS_WRITE: u64 = 23;
pub const SYS_FS_UNLINK: u64 = 29;
pub const SYS_FS_MKDIR: u64 = 31;
pub const SYS_FS_RMDIR: u64 = 32;
pub const SYS_FS_READDIR: u64 = 33;
pub const SYS_FS_SYNC: u64 = 102;
pub const SYS_FS_MOUNT: u64 = 111;
pub const SYS_FS_UNMOUNT: u64 = 112;

pub const SYS_AUTH_LOGIN: u64 = 40;
pub const SYS_AUTH_LOGOUT: u64 = 41;
pub const SYS_AUTH_CREATE: u64 = 43;
pub const SYS_AUTH_CHANGEPW: u64 = 48;
pub const SYS_AUTH_VERIFY: u64 = 49;
pub const SYS_AUTH_CREATE_FIRST: u64 = 50;

pub const SYS_ENV_GETCWD: u64 = 100;
pub const SYS_ENV_CHDIR: u64 = 101;
pub const SYS_REBOOT: u64 = 103;
pub const SYS_GETHOSTNAME: u64 = 108;
pub const SYS_SETHOSTNAME: u64 = 109;
pub const SYS_DISK_LIST: u64 = 113;
pub const SYS_DISK_INFO: u64 = 114;
pub const SYS_DISK_FORMAT: u64 = 115;
pub const SYS_DISK_PARTITION: u64 = 116;
pub const SYS_DISK_INSTALL_GRUB: u64 = 117;

#[repr(C)]
pub struct UserDirent {
    pub inode: u32,
    pub file_type: u8,
    pub name: [u8; 256],
}

#[repr(C)]
pub struct UserDiskInfo {
    pub disk_id: u32,
    pub present: u32,
    pub total_sectors: u32,
    pub sectors: u32,
    pub model: [u8; 64],
}

unsafe fn sys0(num: u64) -> i64 {
    let ret: i64;
    asm!("int 0x80", in("rax") num, lateout("rax") ret);
    ret
}

unsafe fn sys1(num: u64, a1: u64) -> i64 {
    let ret: i64;
    asm!("int 0x80", in("rax") num, in("rdi") a1, lateout("rax") ret);
    ret
}

unsafe fn sys2(num: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    asm!("int 0x80", in("rax") num, in("rdi") a1, in("rsi") a2, lateout("rax") ret);
    ret
}

unsafe fn sys3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    asm!("int 0x80", in("rax") num, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret);
    ret
}

unsafe fn sys4(num: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    asm!("int 0x80", in("rax") num, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, lateout("rax") ret);
    ret
}

pub fn proc_exec(path: &[u8], argv: &[*const u8]) -> i64 {
    unsafe { sys3(SYS_PROC_EXEC, path.as_ptr() as u64, argv.as_ptr() as u64, 0) }
}

pub fn proc_exit(code: i32) -> ! {
    unsafe { sys1(SYS_PROC_EXIT, code as u64); }
    loop { unsafe { asm!("hlt", options(nomem, nostack)); } }
}

pub fn proc_get_pwid() -> u64 {
    unsafe { sys0(SYS_PROC_GETPWID) as u64 }
}

pub fn proc_yield() {
    unsafe { sys0(SYS_PROC_YIELD); }
}

pub fn fs_open(path: &[u8], flags: i32, _mode: i32) -> i32 {
    unsafe { sys3(SYS_FS_OPEN, path.as_ptr() as u64, flags as u64, _mode as u64) as i32 }
}

pub fn fs_close(fd: i32) -> i32 {
    unsafe { sys1(SYS_FS_CLOSE, fd as u64) as i32 }
}

pub fn fs_read(fd: i32, buf: &mut [u8]) -> i32 {
    unsafe { sys3(SYS_FS_READ, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 }
}

pub fn fs_write(fd: i32, buf: &[u8]) -> i32 {
    unsafe { sys3(SYS_FS_WRITE, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as i32 }
}

pub fn fs_mkdir(path: &[u8]) -> i32 {
    unsafe { sys2(SYS_FS_MKDIR, path.as_ptr() as u64, 0o755) as i32 }
}

pub fn fs_rmdir(path: &[u8]) -> i32 {
    unsafe { sys1(SYS_FS_RMDIR, path.as_ptr() as u64) as i32 }
}

pub fn fs_unlink(path: &[u8]) -> i32 {
    unsafe { sys1(SYS_FS_UNLINK, path.as_ptr() as u64) as i32 }
}

pub fn fs_readdir(fd: i32, entry: &mut UserDirent) -> i32 {
    unsafe { sys2(SYS_FS_READDIR, fd as u64, entry as *mut UserDirent as u64) as i32 }
}

pub fn fs_sync() {
    unsafe { sys0(SYS_FS_SYNC); }
}

pub fn fs_mount(source: &[u8], target: &[u8], fstype: &[u8], options: &[u8]) -> i32 {
    unsafe { sys4(SYS_FS_MOUNT, source.as_ptr() as u64, target.as_ptr() as u64, fstype.as_ptr() as u64, options.as_ptr() as u64) as i32 }
}

pub fn fs_unmount(target: &[u8]) -> i32 {
    unsafe { sys1(SYS_FS_UNMOUNT, target.as_ptr() as u64) as i32 }
}

pub fn auth_login(note: &[u8], password: &[u8]) -> i64 {
    unsafe { sys2(SYS_AUTH_LOGIN, note.as_ptr() as u64, password.as_ptr() as u64) }
}

pub fn auth_logout() {
    unsafe { sys0(SYS_AUTH_LOGOUT); }
}

pub fn auth_create_first(password: &[u8]) -> i32 {
    unsafe { sys1(SYS_AUTH_CREATE_FIRST, password.as_ptr() as u64) as i32 }
}

pub fn auth_change_password(old: &[u8], new: &[u8]) -> i32 {
    unsafe { sys2(SYS_AUTH_CHANGEPW, old.as_ptr() as u64, new.as_ptr() as u64) as i32 }
}

pub fn env_getcwd(buf: &mut [u8]) -> i32 {
    unsafe { sys2(SYS_ENV_GETCWD, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 }
}

pub fn env_chdir(path: &[u8]) -> i32 {
    unsafe { sys1(SYS_ENV_CHDIR, path.as_ptr() as u64) as i32 }
}

pub fn gethostname(buf: &mut [u8]) -> i32 {
    unsafe { sys2(SYS_GETHOSTNAME, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 }
}

pub fn sethostname(name: &[u8]) -> i32 {
    unsafe { sys2(SYS_SETHOSTNAME, name.as_ptr() as u64, name.len() as u64) as i32 }
}

pub fn reboot(cmd: i32) -> i64 {
    unsafe { sys1(SYS_REBOOT, cmd as u64) }
}

pub fn disk_list(disks: &mut [u64]) -> i32 {
    unsafe { sys2(SYS_DISK_LIST, disks.as_mut_ptr() as u64, disks.len() as u64) as i32 }
}

pub fn disk_info(disk_id: u32, info: &mut UserDiskInfo) -> i32 {
    unsafe { sys2(SYS_DISK_INFO, disk_id as u64, info as *mut UserDiskInfo as u64) as i32 }
}

pub fn disk_format(disk_id: u32) -> i32 {
    let fstype = b"hvfs\0";
    unsafe { sys2(SYS_DISK_FORMAT, disk_id as u64, fstype.as_ptr() as u64) as i32 }
}

pub fn disk_partition(disk_id: u32, sectors: u64) -> i64 {
    unsafe { sys2(SYS_DISK_PARTITION, disk_id as u64, sectors) }
}

pub fn disk_install_grub(disk_id: u32) -> i64 {
    unsafe { sys1(SYS_DISK_INSTALL_GRUB, disk_id as u64) }
}
