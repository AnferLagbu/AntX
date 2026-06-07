//! Host-side test utilities — re-implements services layer validation
//! logic for pure-scalar (no kernel-state) testing on host std environment.
//!
//! The actual services layer uses #[deny(unsafe_code)] and depends on
//! the no_std kernel crate. For host testing, we extract the parameter
//! validation rules into equivalent pure functions.

#![allow(dead_code)]

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Errno {
    EINVAL,
    EFAULT,
    EBADF,
    ESRCH,
    ENOMEM,
    ECHILD,
    ENOSYS,
    EPERM,
    ENOENT,
    EAGAIN,
}

impl Errno {
    pub fn from_ret(ret: i64) -> Self {
        let errno = (-ret) as u64;
        match errno {
            1 => Self::EPERM,
            2 => Self::ENOENT,
            9 => Self::EBADF,
            12 => Self::ENOMEM,
            14 => Self::EFAULT,
            22 => Self::EINVAL,
            3 => Self::ESRCH,
            10 => Self::ECHILD,
            11 => Self::EAGAIN,
            38 => Self::ENOSYS,
            _ => Self::EINVAL,
        }
    }
}

// =============== mprotect validation ===============

pub const PROT_NONE: i32 = 0x0;
pub const PROT_READ: i32 = 0x1;
pub const PROT_WRITE: i32 = 0x2;
pub const PROT_EXEC: i32 = 0x4;

/// 验证 mprotect 参数 (等价于 services::mm::mprotect::mprotect_syscall 的验证部分)
pub fn mprotect_validate(addr: u64, len: u64, prot: i32) -> Result<(), Errno> {
    if addr & 0xFFF != 0 {
        return Err(Errno::EINVAL);
    }
    if len == 0 {
        return Err(Errno::EINVAL);
    }
    let valid_prot = PROT_NONE | PROT_READ | PROT_WRITE | PROT_EXEC;
    if prot & !valid_prot != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// =============== clone flags ===============

pub const CLONE_VM: u64 = 0x00000100;
pub const CLONE_FS: u64 = 0x00000200;
pub const CLONE_FILES: u64 = 0x00000400;
pub const CLONE_SIGHAND: u64 = 0x00000800;
pub const CLONE_THREAD: u64 = 0x00010000;
pub const CLONE_PARENT_SETTID: u64 = 0x00100000;

/// 验证 clone 参数 (等价于 services::proc::clone::clone_syscall 的验证部分)
pub fn clone_validate(
    flags: u64,
    child_stack: u64,
) -> Result<(), Errno> {
    if (flags & CLONE_VM != 0 || flags & CLONE_THREAD != 0) && flags & CLONE_SIGHAND == 0 {
        return Err(Errno::EINVAL);
    }
    if child_stack != 0 && child_stack % 16 != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// =============== Errno::from_ret 测试 ===============

pub fn errno_from_ret_value(ret: i64) -> Errno {
    Errno::from_ret(ret)
}

// =============== wait4 validation ===============

pub const WNOHANG: i32 = 0x1;
pub const WUNTRACED: i32 = 0x2;
pub const WCONTINUED: i32 = 0x8;

/// 验证 wait4 参数 (等价于 services::proc::wait4::wait4_syscall 的验证部分)
pub fn wait4_validate(pid: i32, options: i32) -> Result<(), Errno> {
    // pid 范围: -32768..=32767
    const PID_MAX: i32 = 0x7FFF;
    const PID_MIN: i32 = -0x8000;
    if pid < PID_MIN || pid > PID_MAX {
        return Err(Errno::EINVAL);
    }

    let valid_opts = WNOHANG | WUNTRACED | WCONTINUED;
    if options & !valid_opts != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// =============== info validation ===============

/// 验证 getpgid 参数 (等价于 services::proc::info::getpgid_syscall 的验证部分)
pub fn getpgid_validate(pid: i32) -> Result<(), Errno> {
    if pid < 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// 验证 uname 参数 (等价于 services::proc::info::uname_syscall 的验证部分)
pub fn uname_validate(buf: u64) -> Result<(), Errno> {
    if buf == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 gettimeofday 参数 (等价于 services::proc::info::gettimeofday_syscall 的验证部分)
pub fn gettimeofday_validate(tv: u64) -> Result<(), Errno> {
    if tv == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

// =============== io validation ===============

/// 验证 pipe 参数 (等价于 services::fs::io::pipe_syscall 的验证部分)
pub fn pipe_validate(fds: u64) -> Result<(), Errno> {
    if fds == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 dup 参数 (等价于 services::fs::io::dup_syscall 的验证部分)
pub fn dup_validate(oldfd: i32) -> Result<(), Errno> {
    if oldfd < 0 {
        return Err(Errno::EBADF);
    }
    Ok(())
}

/// 验证 dup2 参数 (等价于 services::fs::io::dup2_syscall 的验证部分)
pub fn dup2_validate(oldfd: i32, newfd: i32) -> Result<(), Errno> {
    if oldfd < 0 || newfd < 0 {
        return Err(Errno::EBADF);
    }
    Ok(())
}

/// 验证 fcntl 参数 (等价于 services::fs::io::fcntl_syscall 的验证部分)
pub fn fcntl_validate(fd: i32, _cmd: i32, _arg: u64) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    Ok(())
}

// =============== net socket validation ===============

/// POSIX AF_INET
pub const AF_INET: i32 = 2;
/// POSIX SOCK_STREAM / SOCK_DGRAM
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
/// SOL_SOCKET
pub const SOL_SOCKET: i32 = 1;
/// SO_REUSEADDR
pub const SO_REUSEADDR: i32 = 2;

/// 验证 socket 参数 (等价于 services::net::syscall::socket_syscall 的验证部分)
pub fn socket_validate(domain: i32, sock_type: i32, protocol: i32) -> Result<(), Errno> {
    if domain != AF_INET {
        return Err(Errno::EINVAL);
    }
    if sock_type != SOCK_STREAM && sock_type != SOCK_DGRAM {
        return Err(Errno::EINVAL);
    }
    if protocol != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// 验证 bind 参数 (等价于 services::net::syscall::bind_syscall 的验证部分)
pub fn bind_validate(fd: i32, addr_ptr: u64, _addrlen: u32) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if addr_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 listen 参数
pub fn listen_validate(fd: i32, backlog: i32) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if backlog < 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// 验证 accept 参数
pub fn accept_validate(fd: i32, addr_ptr: u64) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    let _ = addr_ptr; // 0 允许, 非 0 也允许
    Ok(())
}

/// 验证 connect 参数
pub fn connect_validate(fd: i32, addr_ptr: u64, _addrlen: u32) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if addr_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 sendto 参数
pub fn sendto_validate(fd: i32, buf_ptr: u64, len: u32, dest_ptr: u64) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if len > 0 && buf_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    let _ = dest_ptr;
    Ok(())
}

/// 验证 recvfrom 参数
pub fn recvfrom_validate(fd: i32, buf_ptr: u64, len: u32) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if buf_ptr == 0 || len == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 setsockopt 参数
pub fn setsockopt_validate(fd: i32, _level: i32, _optname: i32, val_ptr: u64) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if val_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 getsockopt 参数
pub fn getsockopt_validate(fd: i32, _level: i32, _optname: i32, val_ptr: u64) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if val_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 shutdown 参数
pub fn shutdown_validate(fd: i32, _how: i32) -> Result<(), Errno> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    Ok(())
}

// =============== session validation ===============

/// setsid — 无参数, 总是尝试创建
pub fn setsid_validate() -> Result<(), Errno> {
    Ok(())
}

/// getsid(pid) — 验证 pid 参数
pub fn getsid_validate(pid: i32) -> Result<(), Errno> {
    if pid < 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// setpgid(pid, pgid) — 验证两个参数
pub fn setpgid_validate(pid: i32, pgid: i32) -> Result<(), Errno> {
    if pid < 0 || pgid < 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// =============== mmap validation ===============

/// POSIX mmap flags
pub const MAP_SHARED: i32 = 0x01;
pub const MAP_PRIVATE: i32 = 0x02;
pub const MAP_ANONYMOUS: i32 = 0x20;
pub const MAP_FIXED: i32 = 0x10;
pub const MAP_FAILED: i64 = -1; // (void*)-1

/// 验证 mmap 参数 (等价于 services::mm::mmap::mmap_syscall 的验证部分)
pub fn mmap_validate(
    _addr: u64,
    size: u64,
    prot: i32,
    flags: i32,
    _fd: i32,
    _offset: u64,
) -> Result<(), Errno> {
    if size == 0 {
        return Err(Errno::EINVAL);
    }
    // 校验 prot
    let valid_prot = 0x0 | 0x1 | 0x2 | 0x4;
    if prot & !valid_prot != 0 {
        return Err(Errno::EINVAL);
    }
    // MAP_SHARED / MAP_PRIVATE 必选其一
    if flags & (MAP_SHARED | MAP_PRIVATE) == 0 {
        return Err(Errno::EINVAL);
    }
    if flags & MAP_SHARED != 0 && flags & MAP_PRIVATE != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// =============== brk validation ===============

/// 验证 brk 参数 (等价于 services::mm::brk::brk_syscall 的验证部分)
pub fn brk_validate(addr: u64) -> Result<(), Errno> {
    // POSIX 允许 0 (取当前) 或非 0 (请求新 brk)
    if addr == 0 {
        return Ok(()); // 0 表示查询当前
    }
    // 用户地址范围: 不应超过用户空间上界
    const USER_ADDR_MAX: u64 = 0x0000_7FFF_FFFF_FFFF;
    if addr > USER_ADDR_MAX {
        return Err(Errno::ENOMEM);
    }
    Ok(())
}

// =============== path validation ===============

/// 验证 chdir 参数 (等价于 services::fs::path::chdir_syscall 的验证部分)
pub fn chdir_validate(path_ptr: u64) -> Result<(), Errno> {
    if path_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 验证 getcwd 参数 (等价于 services::fs::path::getcwd_syscall 的验证部分)
pub fn getcwd_validate(buf_ptr: u64, size: u64) -> Result<(), Errno> {
    if buf_ptr == 0 || size == 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// =============== uid/gid validation ===============

/// 验证 setuid 参数范围 (简化: u32 范围不溢出,无需校验)
pub fn setuid_validate(_uid: u32) -> Result<(), Errno> {
    Ok(())
}

/// 验证 setreuid 参数 (任一为 (uid_t)-1 = 0xFFFFFFFF 表示不变)
pub fn setreuid_validate(ruid: u32, euid: u32) -> Result<(), Errno> {
    let _ = ruid;
    let _ = euid;
    Ok(())
}
