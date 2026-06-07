//! 跨架构内联 syscall 桩
//!
//! x86_64: `syscall` + `ret` (rcx/rip 自动保存, rax=ret)
//! aarch64: `svc #0` + `ret` (x8=syscall#, x0=ret)

#![allow(dead_code)]
#![allow(unused_imports)]

use core::arch::asm;

use super::syscalls::*;

// ============================================================================
// 通用 raw 模块
// ============================================================================

pub mod raw {
    use super::*;

    // ------------------------------------------------------------------
    // exit / exit_group
    // ------------------------------------------------------------------

    /// exit(status) — 终止当前线程
    pub unsafe fn exit(status: i32) -> ! {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "syscall",
                in("rax") SYS_EXIT,
                in("rdi") status as i64,
                options(noreturn),
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            asm!(
                "svc #0",
                in("x8") SYS_EXIT,
                in("x0") status as i64,
                options(noreturn),
            );
        }
    }

    /// exit_group(status) — 终止整个进程
    pub unsafe fn exit_group(status: i32) -> ! {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "syscall",
                in("rax") SYS_EXIT_GROUP,
                in("rdi") status as i64,
                options(noreturn),
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            asm!(
                "svc #0",
                in("x8") SYS_EXIT_GROUP,
                in("x0") status as i64,
                options(noreturn),
            );
        }
    }

    // ------------------------------------------------------------------
    // I/O
    // ------------------------------------------------------------------

    /// write(fd, buf, count) → 写入字节数, 失败 -1
    pub unsafe fn write(fd: i64, buf: u64, count: u64) -> i64 {
        let ret: i64;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "syscall",
                lateout("rax") ret,
                in("rax") SYS_WRITE,
                in("rdi") fd,
                in("rsi") buf,
                in("rdx") count,
                out("rcx") _,
                out("r11") _,
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            asm!(
                "svc #0",
                lateout("x0") ret,
                in("x8") SYS_WRITE,
                in("x0") fd,
                in("x1") buf,
                in("x2") count,
            );
        }
        ret
    }

    // ------------------------------------------------------------------
    // 进程信息
    // ------------------------------------------------------------------

    pub unsafe fn getpid() -> u64 {
        let ret: i64;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "syscall",
                lateout("rax") ret,
                in("rax") SYS_GETPID,
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            asm!(
                "svc #0",
                lateout("x0") ret,
                in("x8") SYS_GETPID,
            );
        }
        ret as u64
    }

    pub unsafe fn gettid() -> u64 {
        let ret: i64;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "syscall",
                lateout("rax") ret,
                in("rax") SYS_GETTID,
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            asm!(
                "svc #0",
                lateout("x0") ret,
                in("x8") SYS_GETTID,
            );
        }
        ret as u64
    }

    // ------------------------------------------------------------------
    // 内存
    // ------------------------------------------------------------------

    pub unsafe fn brk(addr: u64) -> u64 {
        let ret: i64;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "syscall",
                lateout("rax") ret,
                in("rax") SYS_BRK,
                in("rdi") addr,
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            asm!(
                "svc #0",
                lateout("x0") ret,
                in("x8") SYS_BRK,
                in("x0") addr as i64,
            );
        }
        ret as u64
    }

    pub unsafe fn mmap(
        addr: u64,
        length: u64,
        prot: u64,
        flags: u64,
        fd: i64,
        offset: u64,
    ) -> u64 {
        let ret: i64;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!(
                "syscall",
                lateout("rax") ret,
                in("rax") SYS_MMAP,
                in("rdi") addr,
                in("rsi") length,
                in("rdx") prot,
                in("r10") flags,
                in("r8") fd,
                in("r9") offset,
                out("rcx") _,
                out("r11") _,
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            asm!(
                "svc #0",
                lateout("x0") ret,
                in("x8") SYS_MMAP,
                in("x0") addr,
                in("x1") length,
                in("x2") prot,
                in("x3") flags,
                in("x4") fd,
                in("x5") offset,
            );
        }
        ret as u64
    }
}
