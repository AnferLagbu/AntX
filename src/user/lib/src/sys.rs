/// AntX 用户态运行时 — 系统调用层
/// x86_64: 通过 `int 0x80` 触发
/// aarch64: 通过 `svc #0` 触发, 约定 x0=syscall_num, x1-x4=args, x0=返回

use core::arch::asm;

// ============================================================
// 系统调用编号 — 与内核 syscall/mod.rs 对应
// ============================================================

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
pub const SYS_PROC_GETPWID: u64 = 6;
pub const SYS_PROC_YIELD: u64 = 9;
pub const SYS_FS_OPEN: u64 = 20;
pub const SYS_FS_CLOSE: u64 = 21;
pub const SYS_FS_READ: u64 = 22;
pub const SYS_FS_WRITE: u64 = 23;
pub const SYS_FS_UNLINK: u64 = 29;
pub const SYS_FS_RENAME: u64 = 30;
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
pub const SYS_PROC_LIST: u64 = 11;
pub const SYS_GETHOSTNAME: u64 = 108;
pub const SYS_SETHOSTNAME: u64 = 109;
pub const SYS_DISK_LIST: u64 = 113;
pub const SYS_DISK_INFO: u64 = 114;
pub const SYS_DISK_FORMAT: u64 = 115;
pub const SYS_DISK_PARTITION: u64 = 116;
pub const SYS_DISK_INSTALL_GRUB: u64 = 117;
pub const SYS_FAT_FORMAT: u64 = 118;

#[repr(C)] pub struct UserDirEntry { pub node: u32, pub file_type: u8, pub name: [u8; 256] }
#[repr(C)] pub struct UserDiskInfo { pub disk_id: u32, pub present: u32, pub total_sectors: u32, pub sectors: u32, pub model: [u8; 64] }

// ============================================================
// 系统调用门 — 双架构
// ============================================================

#[cfg(target_arch = "x86_64")]
unsafe fn sys0(num: u64) -> i64 { let ret: i64; asm!("int 0x80", in("rax") num, lateout("rax") ret); ret }
#[cfg(target_arch = "x86_64")]
unsafe fn sys1(num: u64, a1: u64) -> i64 { let ret: i64; asm!("int 0x80", in("rax") num, in("rdi") a1, lateout("rax") ret); ret }
#[cfg(target_arch = "x86_64")]
unsafe fn sys2(num: u64, a1: u64, a2: u64) -> i64 { let ret: i64; asm!("int 0x80", in("rax") num, in("rdi") a1, in("rsi") a2, lateout("rax") ret); ret }
#[cfg(target_arch = "x86_64")]
unsafe fn sys3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 { let ret: i64; asm!("int 0x80", in("rax") num, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret); ret }
#[cfg(target_arch = "x86_64")]
unsafe fn sys4(num: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 { let ret: i64; asm!("int 0x80", in("rax") num, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, lateout("rax") ret); ret }

#[cfg(target_arch = "aarch64")]
unsafe fn sys0(num: u64) -> i64 { let ret: i64; asm!("svc #0", in("x0") num, lateout("x0") ret); ret }
#[cfg(target_arch = "aarch64")]
unsafe fn sys1(num: u64, a1: u64) -> i64 { let ret: i64; asm!("svc #0", in("x0") num, in("x1") a1, lateout("x0") ret); ret }
#[cfg(target_arch = "aarch64")]
unsafe fn sys2(num: u64, a1: u64, a2: u64) -> i64 { let ret: i64; asm!("svc #0", in("x0") num, in("x1") a1, in("x2") a2, lateout("x0") ret); ret }
#[cfg(target_arch = "aarch64")]
unsafe fn sys3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 { let ret: i64; asm!("svc #0", in("x0") num, in("x1") a1, in("x2") a2, in("x3") a3, lateout("x0") ret); ret }
#[cfg(target_arch = "aarch64")]
unsafe fn sys4(num: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 { let ret: i64; asm!("svc #0", in("x0") num, in("x1") a1, in("x2") a2, in("x3") a3, in("x4") a4, lateout("x0") ret); ret }

pub fn proc_exec(path: &[u8], argv: &[*const u8]) -> i64   { unsafe { sys3(SYS_PROC_EXEC, path.as_ptr() as u64, argv.as_ptr() as u64, 0) } }
pub fn proc_exit(code: i32) -> ! {
    unsafe { sys1(SYS_PROC_EXIT, code as u64); }
    loop {
        if cfg!(target_arch = "x86_64") {
            unsafe { asm!("hlt", options(nomem, nostack)); }
        } else {
            unsafe { asm!("wfi", options(nomem, nostack)); }
        }
    }
}
pub fn proc_get_pwid() -> u64                                { unsafe { sys0(SYS_PROC_GETPWID) as u64 } }
pub fn proc_yield()                                          { unsafe { sys0(SYS_PROC_YIELD); } }
pub fn fs_open(path: &[u8], flags: i32, m: i32) -> i32      { unsafe { sys3(SYS_FS_OPEN, path.as_ptr() as u64, flags as u64, m as u64) as i32 } }
pub fn fs_close(fd: i32) -> i32                              { unsafe { sys1(SYS_FS_CLOSE, fd as u64) as i32 } }
pub fn fs_read(fd: i32, buf: &mut [u8]) -> i32               { unsafe { sys3(SYS_FS_READ, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
pub fn fs_write(fd: i32, buf: &[u8]) -> i32                  { unsafe { sys3(SYS_FS_WRITE, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as i32 } }
pub fn fs_mkdir(path: &[u8]) -> i32                          { unsafe { sys2(SYS_FS_MKDIR, path.as_ptr() as u64, 0o755) as i32 } }
pub fn fs_rmdir(path: &[u8]) -> i32                          { unsafe { sys1(SYS_FS_RMDIR, path.as_ptr() as u64) as i32 } }
pub fn fs_unlink(path: &[u8]) -> i32                         { unsafe { sys1(SYS_FS_UNLINK, path.as_ptr() as u64) as i32 } }
pub fn fs_rename(old: &[u8], new: &[u8]) -> i32              { unsafe { sys2(SYS_FS_RENAME, old.as_ptr() as u64, new.as_ptr() as u64) as i32 } }
pub fn fs_readdir(fd: i32, entry: &mut UserDirEntry) -> i32    { unsafe { sys2(SYS_FS_READDIR, fd as u64, entry as *mut UserDirEntry as u64) as i32 } }
pub fn fs_sync()                                             { unsafe { sys0(SYS_FS_SYNC); } }
pub fn fs_mount(src: &[u8], tgt: &[u8], typ: &[u8], opt: &[u8]) -> i32 { unsafe { sys4(SYS_FS_MOUNT, src.as_ptr() as u64, tgt.as_ptr() as u64, typ.as_ptr() as u64, opt.as_ptr() as u64) as i32 } }
pub fn fs_unmount(t: &[u8]) -> i32                           { unsafe { sys1(SYS_FS_UNMOUNT, t.as_ptr() as u64) as i32 } }
pub fn auth_login(n: &[u8], p: &[u8]) -> i64                 { unsafe { sys2(SYS_AUTH_LOGIN, n.as_ptr() as u64, p.as_ptr() as u64) } }
pub fn auth_logout()                                         { unsafe { sys0(SYS_AUTH_LOGOUT); } }
pub fn auth_create_first(pw: &[u8]) -> i32                   { unsafe { sys1(SYS_AUTH_CREATE_FIRST, pw.as_ptr() as u64) as i32 } }
pub fn auth_change_password(o: &[u8], n: &[u8]) -> i32       { unsafe { sys2(SYS_AUTH_CHANGEPW, o.as_ptr() as u64, n.as_ptr() as u64) as i32 } }
pub fn env_getcwd(buf: &mut [u8]) -> i32                     { unsafe { sys2(SYS_ENV_GETCWD, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
pub fn env_chdir(path: &[u8]) -> i32                         { unsafe { sys1(SYS_ENV_CHDIR, path.as_ptr() as u64) as i32 } }
pub fn gethostname(buf: &mut [u8]) -> i32                    { unsafe { sys2(SYS_GETHOSTNAME, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
pub fn sethostname(name: &[u8]) -> i32                       { unsafe { sys2(SYS_SETHOSTNAME, name.as_ptr() as u64, name.len() as u64) as i32 } }
pub fn reboot(cmd: i32) -> i64                               { unsafe { sys1(SYS_REBOOT, cmd as u64) } }
pub fn proc_list(buf: &mut [u8], max_entries: u32) -> i32     { unsafe { sys2(SYS_PROC_LIST, buf.as_mut_ptr() as u64, max_entries as u64) as i32 } }
pub fn disk_list(disks: &mut [u64]) -> i32                   { unsafe { sys2(SYS_DISK_LIST, disks.as_mut_ptr() as u64, disks.len() as u64) as i32 } }
pub fn disk_info(id: u32, info: &mut UserDiskInfo) -> i32    { unsafe { sys2(SYS_DISK_INFO, id as u64, info as *mut UserDiskInfo as u64) as i32 } }
pub fn disk_format(id: u32) -> i32                           { unsafe { sys2(SYS_DISK_FORMAT, id as u64, b"hvfs\0".as_ptr() as u64) as i32 } }
pub fn disk_partition(id: u32, sectors: u64) -> i64          { unsafe { sys2(SYS_DISK_PARTITION, id as u64, sectors) } }
pub fn boot_install(id: u32) -> i64                          { unsafe { sys1(SYS_DISK_INSTALL_GRUB, id as u64) } }
pub fn fat_format(id: u32) -> i64                            { unsafe { sys1(SYS_FAT_FORMAT, id as u64) } }
