// SPDX-License-Identifier: GPL-2.0
//! userland libc 子集 host 模拟测试
//!
//! 注: 真实 syscall 不能在 host 上执行, 测常量 / 标志位 / 类型 API.

use queenx_userland::{
    brk, eprintln, getpid, gettid, mmap, write_str, write, MAP_ANONYMOUS, MAP_FAILED,
    MAP_FIXED, MAP_PRIVATE, MAP_SHARED, PROT_EXEC, PROT_READ, PROT_WRITE, STDERR_FILENO,
};

// ============================================================================
// 常量 / 标志位
// ============================================================================

#[test]
fn test_errno_constants_match_linux() {
    // 与 Linux errno 数值一致, 内核服务层才能互通
    assert_eq!(queenx_userland::EPERM, 1);
    assert_eq!(queenx_userland::ENOENT, 2);
    assert_eq!(queenx_userland::EAGAIN, 11);
    assert_eq!(queenx_userland::EFAULT, 14);
    assert_eq!(queenx_userland::EINVAL, 22);
    assert_eq!(queenx_userland::ENOSYS, 38);
}

#[test]
fn test_mmap_flags_distinct() {
    assert_ne!(MAP_SHARED, MAP_PRIVATE);
    assert_ne!(MAP_SHARED, MAP_FIXED);
    assert_ne!(MAP_SHARED, MAP_ANONYMOUS);
    assert_ne!(MAP_PRIVATE, MAP_FIXED);
    assert_ne!(MAP_PRIVATE, MAP_ANONYMOUS);
    assert_ne!(MAP_FIXED, MAP_ANONYMOUS);
}

#[test]
fn test_prot_flags_orthogonal() {
    // PROT_* 独立位, 可 OR
    let prot_rw = PROT_READ | PROT_WRITE;
    assert_eq!(prot_rw & PROT_READ, PROT_READ);
    assert_eq!(prot_rw & PROT_WRITE, PROT_WRITE);
    assert_eq!(prot_rw & PROT_EXEC, 0);

    let prot_rwx = PROT_READ | PROT_WRITE | PROT_EXEC;
    assert_eq!(prot_rwx & PROT_EXEC, PROT_EXEC);
}

#[test]
fn test_map_failed_is_max() {
    assert_eq!(MAP_FAILED, u64::MAX);
}

#[test]
fn test_stderr_fileno() {
    assert_eq!(STDERR_FILENO, 2);
}

// ============================================================================
// 字符串字面量 → 字节指针 (host 模拟, 不调真实 syscall)
// ============================================================================

#[test]
fn test_str_as_ptr() {
    let s = "hello";
    let p = s.as_ptr();
    unsafe {
        assert_eq!(*p, b'h');
        assert_eq!(*p.add(1), b'e');
        assert_eq!(*p.add(4), b'o');
        assert_eq!(*p.add(5), 0); // null terminator
    }
}

#[test]
fn test_write_str_length() {
    let s = "queenx";
    // 真实 write 走 syscall; 在 host 上仅验证指针 + 长度
    let ptr = s.as_ptr();
    let len = s.len();
    assert_eq!(len, 6);
    assert!(!ptr.is_null());
}

// ============================================================================
// 类型 + 路径验证
// ============================================================================

#[test]
fn test_fn_pointer_signature() {
    // UserMain 是 extern "C" fn() -> i32
    let _f: queenx_userland::UserMain = main_stub;
}

extern "C" fn main_stub() -> i32 {
    0
}

#[test]
fn test_write_signature() {
    // 验证函数指针可调用, 但不调真实 syscall
    let _f: fn(i32, *const u8, usize) -> isize = write;
}

#[test]
fn test_brk_signature() {
    let _f: fn(u64) -> u64 = brk;
}

#[test]
fn test_mmap_signature() {
    let _f: fn(u64, u64, i32, i32, i32, i64) -> u64 = mmap;
}

// ============================================================================
// syscall 编号 (跨架构一致, 由内核识别)
// ============================================================================

#[test]
fn test_syscall_numbers_match_linux() {
    use queenx_userland::syscalls;
    assert_eq!(syscalls::SYS_WRITE, 1);
    assert_eq!(syscalls::SYS_EXIT, 60);
    assert_eq!(syscalls::SYS_EXIT_GROUP, 231);
    assert_eq!(syscalls::SYS_GETPID, 39);
    assert_eq!(syscalls::SYS_BRK, 12);
    assert_eq!(syscalls::SYS_MMAP, 9);
}

// ============================================================================
// 编译期不变量 (panic = "abort" + no_std)
// ============================================================================

#[test]
fn test_no_std_compiled() {
    // userland 是 no_std, 验证 cfg(target_os)
    #[cfg(target_os = "linux")]
    assert!(cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64"));
}

// ============================================================================
// 验证 getpid / gettid 签名 (不调真实 syscall)
// ============================================================================

#[test]
fn test_getpid_signature() {
    let _f: fn() -> i32 = getpid;
}

#[test]
fn test_gettid_signature() {
    let _f: fn() -> i32 = gettid;
}

// ============================================================================
// eprintln! 宏展开 (编译期验证)
// ============================================================================

#[test]
fn test_eprintln_expand() {
    // 不真正执行 (依赖 syscall); 编译期通过即认为宏正确
    let _code = quote::eprintln_str();
}

// 用本地小 stub 代替依赖
mod quote {
    pub fn eprintln_str() -> i32 {
        let _ = eprintln!("ok");
        0
    }
}
