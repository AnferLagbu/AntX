//! 跨架构 syscall 编号 (与 QueenX kernel 一致)
//!
//! x86_64 Linux ABI:  rax=syscall#, rdi,rsi,rdx,r10,r8,r9 → rax
//! aarch64 Linux:    x8=syscall#, x0..x5 → x0

#![allow(dead_code)]

// 与 kernel/framework/syscall/types.rs 同步
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_EXIT: u64 = 60;
pub const SYS_EXIT_GROUP: u64 = 231;
pub const SYS_BRK: u64 = 12;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_GETPID: u64 = 39;
pub const SYS_GETTID: u64 = 186;
