#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
//! queenx 用户态 libc-like 子集 (Phase C3)
//!
//! ## 范围
//!
//! - 内联 syscall 桩: x86_64 (syscall/ret) + aarch64 (svc #0 / ret)
//! - 极简 libc: write / exit / exit_group / brk / mmap_minimal / getpid
//! - 字符串: strlen / memcpy 编译期可优化
//!
//! ## 不变量
//!
//! 整个 crate 是 `#![no_std]`, 无堆分配 (除用户态分配 mmap 走 syscall).
//! 所有 unsafe 集中在 `syscall!` 宏 + `arch/` 两个内联汇编块.

#![allow(dead_code)]
#![allow(non_camel_case_types)]

// 用户态需要 panic handler (no_std 必填)
use core::panic::PanicInfo;
#[panic_handler]
fn _panic(_info: &PanicInfo) -> ! {
    // 写 "PANIC\n" 到 stderr 然后退出
    unsafe {
        let msg = b"PANIC\n";
        arch::raw::write(2, msg.as_ptr() as u64, msg.len() as u64);
        arch::raw::exit_group(127);
    }
}

pub mod arch;
pub mod syscalls;
pub mod string;

// ============================================================================
// 公共 errno (用户态视图, 与 Linux 数值一致)
// ============================================================================

pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;
pub const EAGAIN: i32 = 11;

// ============================================================================
// 进程退出 (无返回值, 编译器视为 noreturn)
// ============================================================================

/// _exit(status) — 立即终止当前线程
#[inline]
pub fn _exit(status: i32) -> ! {
    unsafe { arch::raw::exit(status) }
}

/// exit_group(status) — 终止整个进程 (所有线程)
#[inline]
pub fn _exit_group(status: i32) -> ! {
    unsafe { arch::raw::exit_group(status) }
}

// ============================================================================
// I/O
// ============================================================================

/// write(fd, buf, count) → 写入字节数, 失败 -1
#[inline]
pub fn write(fd: i32, buf: *const u8, count: usize) -> isize {
    unsafe { arch::raw::write(fd as i64, buf as u64, count as u64) as isize }
}

/// STDERR_FILENO
pub const STDERR_FILENO: i32 = 2;

/// println-lite: 写一行到 stderr
#[macro_export]
macro_rules! eprintln {
    ($s:expr) => {{
        let s = $s;
        $crate::write_str($crate::STDERR_FILENO, s);
    }};
}

/// 把字符串字面量或 &[u8] 写到 fd
#[inline]
pub fn write_str(fd: i32, s: &str) -> isize {
    write(fd, s.as_ptr(), s.len())
}

// ============================================================================
// 进程信息
// ============================================================================

/// getpid() → 当前进程 PID (Phase A4 后可用, 启动 PID=1)
#[inline]
pub fn getpid() -> i32 {
    unsafe { arch::raw::getpid() as i32 }
}

/// gettid() → 当前线程 TID
#[inline]
pub fn gettid() -> i32 {
    unsafe { arch::raw::gettid() as i32 }
}

// ============================================================================
// 内存
// ============================================================================

/// brk(addr) → 设置 program break, 返新 break (失败返旧 break)
#[inline]
pub fn brk(addr: u64) -> u64 {
    unsafe { arch::raw::brk(addr) }
}

/// mmap(addr, length, prot, flags, fd, offset) → 映射地址 (MAP_FAILED=!0)
pub const MAP_FAILED: u64 = u64::MAX;

pub const PROT_READ: i32 = 1;
pub const PROT_WRITE: i32 = 2;
pub const PROT_EXEC: i32 = 4;

pub const MAP_SHARED: i32 = 0x01;
pub const MAP_PRIVATE: i32 = 0x02;
pub const MAP_FIXED: i32 = 0x10;
pub const MAP_ANONYMOUS: i32 = 0x20;

#[inline]
pub fn mmap(
    addr: u64,
    length: u64,
    prot: i32,
    flags: i32,
    fd: i32,
    offset: i64,
) -> u64 {
    unsafe {
        arch::raw::mmap(
            addr,
            length,
            prot as u64,
            flags as u64,
            fd as i64,
            offset as u64,
        )
    }
}

// ============================================================================
// 用户态入口 (供 init / shell 链接)
// ============================================================================

/// 简易用户态程序入口类型: fn() -> i32 → 返 exit code
pub type UserMain = extern "C" fn() -> i32;

/// 启动用户态程序: 调 main() 并 exit_group
#[no_mangle]
pub extern "C" fn queenx_userland_start(main: UserMain) -> ! {
    let code = main();
    _exit_group(code)
}
