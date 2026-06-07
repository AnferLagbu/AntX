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
