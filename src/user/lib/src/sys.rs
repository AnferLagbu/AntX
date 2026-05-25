/// AntX 用户态运行时 — POSIX 原生系统调用层
/// x86_64: int 0x80, rax=num, rdi=a1, rsi=a2, rdx=a3, r10=a4
/// aarch64: svc #0, x0=num, x1-x4=args, x0=返回

use core::arch::asm;

// ============================================================
// POSIX syscall 编号 — 与内核 syscall/types.rs 对应
// ============================================================

// 文件 I/O
pub const SYS_read: u64 = 0;
pub const SYS_write: u64 = 1;
pub const SYS_open: u64 = 2;
pub const SYS_close: u64 = 3;
pub const SYS_stat: u64 = 4;
pub const SYS_fstat: u64 = 5;
pub const SYS_lseek: u64 = 8;
pub const SYS_access: u64 = 21;
pub const SYS_pipe: u64 = 22;
pub const SYS_dup: u64 = 32;
pub const SYS_dup2: u64 = 33;
pub const SYS_getpid: u64 = 39;
pub const SYS_fork: u64 = 57;
pub const SYS_execve: u64 = 59;
pub const SYS_exit: u64 = 60;
pub const SYS_wait4: u64 = 61;
pub const SYS_uname: u64 = 63;
pub const SYS_getdents: u64 = 78;
pub const SYS_getcwd: u64 = 79;
pub const SYS_chdir: u64 = 80;
pub const SYS_rename: u64 = 82;
pub const SYS_mkdir: u64 = 83;
pub const SYS_rmdir: u64 = 84;
pub const SYS_unlink: u64 = 87;
pub const SYS_getuid: u64 = 102;
pub const SYS_getgid: u64 = 104;
pub const SYS_geteuid: u64 = 107;
pub const SYS_getegid: u64 = 108;
pub const SYS_getppid: u64 = 110;
pub const SYS_sync: u64 = 162;
pub const SYS_mount: u64 = 165;

// QueenX 私有 syscall (400+)
pub const SYS_QX_LOGIN: u64 = 400;
pub const SYS_QX_LOGOUT: u64 = 401;
pub const SYS_QX_CREATE_IDENTITY: u64 = 402;
pub const SYS_QX_CHANGE_PASSWORD: u64 = 405;
pub const SYS_QX_CREATE_FIRST: u64 = 407;
pub const SYS_QX_DISK_LIST: u64 = 420;
pub const SYS_QX_DISK_INFO: u64 = 421;
pub const SYS_QX_DISK_FORMAT: u64 = 422;
pub const SYS_QX_DISK_PARTITION: u64 = 423;
pub const SYS_QX_DISK_INSTALL: u64 = 424;
pub const SYS_QX_FAT_FORMAT: u64 = 425;
pub const SYS_QX_PROC_LIST: u64 = 430;
pub const SYS_QX_GETHOSTNAME: u64 = 433;
pub const SYS_QX_SETHOSTNAME: u64 = 434;
pub const SYS_QX_REBOOT: u64 = 436;

// POSIX open flags
pub const O_RDONLY: i32 = 0;
pub const O_WRONLY: i32 = 1;
pub const O_RDWR: i32 = 2;
pub const O_CREAT: i32 = 0o100;
pub const O_TRUNC: i32 = 0o1000;

pub const FT_FILE: u8 = 0;
pub const FT_DIR: u8 = 1;
pub const FT_DEV: u8 = 2;

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

// ============================================================
// POSIX 标准 syscall wrapper
// ============================================================

pub fn read(fd: i32, buf: &mut [u8]) -> i32               { unsafe { sys3(SYS_read, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
pub fn write(fd: i32, buf: &[u8]) -> i32                  { unsafe { sys3(SYS_write, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as i32 } }
pub fn open(path: &[u8], flags: i32, mode: i32) -> i32   { unsafe { sys3(SYS_open, path.as_ptr() as u64, flags as u64, mode as u64) as i32 } }
pub fn close(fd: i32) -> i32                              { unsafe { sys1(SYS_close, fd as u64) as i32 } }
pub fn getpid() -> u64                                    { unsafe { sys0(SYS_getpid) as u64 } }
pub fn fork() -> u64                                      { unsafe { sys0(SYS_fork) as u64 } }
pub fn getuid() -> u32                                    { unsafe { sys0(SYS_getuid) as u32 } }
pub fn geteuid() -> u32                                   { unsafe { sys0(SYS_geteuid) as u32 } }
pub fn sched_yield()                                      { unsafe { sys0(24); } }
pub fn sync()                                             { unsafe { sys0(SYS_sync); } }

// ============================================================
// QueenX 兼容 wrapper — 保留原有语义，内部映射到 POSIX/PWM 编号
// ============================================================

pub fn proc_exec(path: &[u8], argv: &[*const u8]) -> i64   { unsafe { sys3(SYS_execve, path.as_ptr() as u64, argv.as_ptr() as u64, 0) } }
pub fn proc_exit(code: i32) -> ! {
    unsafe { sys1(SYS_exit, code as u64); }
    loop {
        if cfg!(target_arch = "x86_64") { unsafe { asm!("hlt", options(nomem, nostack)); } }
        else { unsafe { asm!("wfi", options(nomem, nostack)); } }
    }
}
pub fn proc_get_pwm() -> u64                               { unsafe { sys0(415) as u64 } }
pub fn proc_yield()                                        { unsafe { sys0(24); } }

pub fn fs_open(path: &[u8], flags: i32, m: i32) -> i32    { unsafe { sys3(SYS_open, path.as_ptr() as u64, flags as u64, m as u64) as i32 } }
pub fn fs_close(fd: i32) -> i32                            { unsafe { sys1(SYS_close, fd as u64) as i32 } }
pub fn fs_read(fd: i32, buf: &mut [u8]) -> i32             { unsafe { sys3(SYS_read, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
pub fn fs_write(fd: i32, buf: &[u8]) -> i32                { unsafe { sys3(SYS_write, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as i32 } }

pub fn fs_mkdir(path: &[u8]) -> i32                        { unsafe { sys2(SYS_mkdir, path.as_ptr() as u64, 0o755) as i32 } }
pub fn fs_rmdir(path: &[u8]) -> i32                        { unsafe { sys1(SYS_rmdir, path.as_ptr() as u64) as i32 } }
pub fn fs_unlink(path: &[u8]) -> i32                       { unsafe { sys1(SYS_unlink, path.as_ptr() as u64) as i32 } }
pub fn fs_rename(old: &[u8], new: &[u8]) -> i32            { unsafe { sys2(SYS_rename, old.as_ptr() as u64, new.as_ptr() as u64) as i32 } }
pub fn fs_readdir(fd: i32, entry: &mut UserDirEntry) -> i32 { unsafe { sys2(SYS_getdents, fd as u64, entry as *mut UserDirEntry as u64) as i32 } }
pub fn fs_sync()                                           { unsafe { sys0(SYS_sync); } }

pub fn fs_mount(src: &[u8], tgt: &[u8], typ: &[u8], opt: &[u8]) -> i32 {
    unsafe { sys4(SYS_mount, src.as_ptr() as u64, tgt.as_ptr() as u64, typ.as_ptr() as u64, opt.as_ptr() as u64) as i32 }
}
pub fn fs_unmount(t: &[u8]) -> i32 { unsafe { sys1(166, t.as_ptr() as u64) as i32 } }

pub fn auth_login(n: &[u8], p: &[u8]) -> i64               { unsafe { sys2(SYS_QX_LOGIN, n.as_ptr() as u64, p.as_ptr() as u64) } }
pub fn auth_logout()                                       { unsafe { sys0(SYS_QX_LOGOUT); } }
pub fn auth_create_first(pw: &[u8]) -> i32                 { unsafe { sys1(SYS_QX_CREATE_FIRST, pw.as_ptr() as u64) as i32 } }
pub fn auth_change_password(o: &[u8], n: &[u8]) -> i32     { unsafe { sys2(SYS_QX_CHANGE_PASSWORD, o.as_ptr() as u64, n.as_ptr() as u64) as i32 } }

pub fn env_getcwd(buf: &mut [u8]) -> i32                   { unsafe { sys2(SYS_getcwd, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
pub fn env_chdir(path: &[u8]) -> i32                       { unsafe { sys1(SYS_chdir, path.as_ptr() as u64) as i32 } }

pub fn gethostname(buf: &mut [u8]) -> i32                  { unsafe { sys2(SYS_QX_GETHOSTNAME, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
pub fn sethostname(name: &[u8]) -> i32                     { unsafe { sys2(SYS_QX_SETHOSTNAME, name.as_ptr() as u64, name.len() as u64) as i32 } }
pub fn reboot(cmd: i32) -> i64                             { unsafe { sys1(SYS_QX_REBOOT, cmd as u64) } }
pub fn proc_list(buf: &mut [u8], max_entries: u32) -> i32  { unsafe { sys2(SYS_QX_PROC_LIST, buf.as_mut_ptr() as u64, max_entries as u64) as i32 } }

pub fn disk_list(disks: &mut [u64]) -> i32                 { unsafe { sys2(SYS_QX_DISK_LIST, disks.as_mut_ptr() as u64, disks.len() as u64) as i32 } }
pub fn disk_info(id: u32, info: &mut UserDiskInfo) -> i32  { unsafe { sys2(SYS_QX_DISK_INFO, id as u64, info as *mut UserDiskInfo as u64) as i32 } }
pub fn disk_format(id: u32) -> i32                         { unsafe { sys2(SYS_QX_DISK_FORMAT, id as u64, b"hvfs\0".as_ptr() as u64) as i32 } }
pub fn disk_partition(id: u32, sectors: u64) -> i64        { unsafe { sys2(SYS_QX_DISK_PARTITION, id as u64, sectors) } }
pub fn boot_install(id: u32) -> i64                        { unsafe { sys1(SYS_QX_DISK_INSTALL, id as u64) } }
pub fn fat_format(id: u32) -> i64                          { unsafe { sys1(SYS_QX_FAT_FORMAT, id as u64) } }

// ============================================================
// 新增 POSIX syscall wrapper
// ============================================================

pub const SYS_truncate: u64 = 76;
pub const SYS_ftruncate: u64 = 77;
pub const SYS_fsync: u64 = 170;
pub const SYS_getpgid: u64 = 111;
pub const SYS_setsid: u64 = 112;
pub const SYS_gettid: u64 = 186;
pub const SYS_nanosleep: u64 = 35;
pub const SYS_tgkill: u64 = 234;
pub const SYS_rt_sigaction: u64 = 13;
pub const SYS_rt_sigprocmask: u64 = 14;
pub const SYS_umask: u64 = 95;

#[repr(C)] pub struct Timespec { pub tv_sec: i64, pub tv_nsec: i64 }

pub fn truncate(path: &[u8], length: i64) -> i32           { unsafe { sys2(SYS_truncate, path.as_ptr() as u64, length as u64) as i32 } }
pub fn ftruncate(fd: i32, length: i64) -> i32              { unsafe { sys2(SYS_ftruncate, fd as u64, length as u64) as i32 } }
pub fn fsync(fd: i32) -> i32                               { unsafe { sys1(SYS_fsync, fd as u64) as i32 } }
pub fn getpgid(pid: i32) -> i64                            { unsafe { sys1(SYS_getpgid, pid as u64) } }
pub fn setsid() -> i64                                     { unsafe { sys0(SYS_setsid) } }
pub fn gettid() -> i64                                     { unsafe { sys0(SYS_gettid) } }
pub fn umask(mask: u32) -> u32                             { unsafe { sys1(SYS_umask, mask as u64) as u32 } }
pub fn nanosleep(req: &Timespec) -> i32                    { unsafe { sys2(SYS_nanosleep, req as *const Timespec as u64, 0) as i32 } }
pub fn kill(pid: i32, sig: i32) -> i32                     { unsafe { sys2(62, pid as u64, sig as u64) as i32 } }
pub fn tgkill(tgid: i32, tid: i32, sig: i32) -> i32        { unsafe { sys3(SYS_tgkill, tgid as u64, tid as u64, sig as u64) as i32 } }

// ============================================================
// 热插拔状态查询
// ============================================================

pub const SYS_QX_HOTPLUG_STATUS: u64 = 437;

pub fn hotplug_status(buf: &mut [u8]) -> i32               { unsafe { sys2(SYS_QX_HOTPLUG_STATUS, buf.as_mut_ptr() as u64, buf.len() as u64) as i32 } }
