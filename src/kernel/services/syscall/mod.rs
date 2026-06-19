#![deny(unsafe_code)]
//! 系统调用 (Syscall) — services 层安全代理
//!
//! ## 状态 (v2.18, 2026-06-04)
//!
//! Phase 2.5 syscall 迁移:
//! - [x] 强类型 `SyscallNumber` 替代裸 u64
//! - [x] 强类型 `SyscallArgs` 替代 4 个独立 u64 参数
//! - [x] 强类型 `SyscallResult<T>` (基于 `Errno`)
//! - [x] 用户态指针/缓冲区验证
//! - [x] `UserContext` 入口安全分发
//! - [x] 处理器注册 API
//!
//! ## 迁移方法
//!
//! 1. 内部把 4 个 u64 包装为 `SyscallArgs`, 委托 `framework::usermode::dispatch_syscall`
//! 2. services 层 0 unsafe — 所有 unsafe 在 framework TCB
//! 3. 强类型 `SyscallResult<T>` 替代 `i64` 返回码 (POSIX 风格: 负数 = -errno)
//!
//! 评估日期: 2026-06-04
//!
//! ## T5-2 迁移记录
//!
//! linuxulator (编号翻译表 + 参数转换) 已于 2026-06-16 从
//! framework/syscall/linuxulator.rs 迁移到本目录 linuxulator.rs.

pub mod dispatch;
pub mod linuxulator;
pub mod types;

use crate::kernel::framework::syscall_init as fw_syscall_init;
use crate::kernel::framework::userctx::UserContext;
use crate::kernel::framework::usermode;
use crate::kernel::framework::syscall;

// ============================================================================
// 强类型 re-export
// ============================================================================

/// POSIX errno 错误码
pub use types::Errno;

/// Syscall 编号 (新类型, 替代裸 u64)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct SyscallNumber(pub u64);

impl SyscallNumber {
    /// 构造
    #[inline]
    pub const fn new(n: u64) -> Self {
        Self(n)
    }

    /// 原始值
    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl From<u64> for SyscallNumber {
    #[inline]
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<SyscallNumber> for u64 {
    #[inline]
    fn from(s: SyscallNumber) -> u64 {
        s.0
    }
}

// ============================================================================
// Syscall 参数 (替代 4 个独立 u64)
// ============================================================================

/// Syscall 参数 (6 个 u64)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyscallArgs {
    /// 第 1 参数 (rdi / x1)
    pub a0: u64,
    /// 第 2 参数 (rsi / x2)
    pub a1: u64,
    /// 第 3 参数 (rdx / x3)
    pub a2: u64,
    /// 第 4 参数 (r10 / x4)
    pub a3: u64,
    /// 第 5 参数 (r8 / x5)
    pub a4: u64,
    /// 第 6 参数 (r9 / x6)
    pub a5: u64,
}

impl SyscallArgs {
    /// 零参数
    pub const NONE: Self = Self {
        a0: 0,
        a1: 0,
        a2: 0,
        a3: 0,
        a4: 0,
        a5: 0,
    };

    /// 构造
    #[inline]
    pub const fn new(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> Self {
        Self { a0, a1, a2, a3, a4, a5 }
    }

    /// 1 参数
    #[inline]
    pub const fn one(a0: u64) -> Self {
        Self::new(a0, 0, 0, 0, 0, 0)
    }

    /// 2 参数
    #[inline]
    pub const fn two(a0: u64, a1: u64) -> Self {
        Self::new(a0, a1, 0, 0, 0, 0)
    }

    /// 3 参数
    #[inline]
    pub const fn three(a0: u64, a1: u64, a2: u64) -> Self {
        Self::new(a0, a1, a2, 0, 0, 0)
    }

    /// 4 参数
    #[inline]
    pub const fn four(a0: u64, a1: u64, a2: u64, a3: u64) -> Self {
        Self::new(a0, a1, a2, a3, 0, 0)
    }
}

// ============================================================================
// Syscall 结果 (POSIX 风格, 强类型)
// ============================================================================

/// Syscall 结果 (成功值 或 errno 错误)
pub type SyscallResult<T> = Result<T, Errno>;

// ============================================================================
// 通用 Errno 转换
// ============================================================================

/// `Errno` 扩展: 提供 `try_from_i32` 反向查询 (Errno 没有 std::convert::From)
impl Errno {
    /// 从 i32 错误码构造 `Errno` (POSIX 反向: 整数值 = errno 编号)
    pub fn try_from_i32(code: i32) -> Option<Self> {
        // Errno 编号范围 1..=133 (POSIX 范围)
        // 我们通过 #[repr(i32)] 枚举, 直接 transmute 不安全;
        // 用 match 列举所有已知变体
        Some(match code {
            1 => Self::EPERM,
            2 => Self::ENOENT,
            3 => Self::ESRCH,
            4 => Self::EINTR,
            5 => Self::EIO,
            6 => Self::ENXIO,
            7 => Self::E2BIG,
            8 => Self::ENOEXEC,
            9 => Self::EBADF,
            10 => Self::ECHILD,
            11 => Self::EAGAIN,
            12 => Self::ENOMEM,
            13 => Self::EACCES,
            14 => Self::EFAULT,
            15 => Self::ENOTBLK,
            16 => Self::EBUSY,
            17 => Self::EEXIST,
            18 => Self::EXDEV,
            19 => Self::ENODEV,
            20 => Self::ENOTDIR,
            21 => Self::EISDIR,
            22 => Self::EINVAL,
            23 => Self::ENFILE,
            // 大于 23 的暂不识别, 返回 None
            _ => return None,
        })
    }
}

/// i64 → `Errno` (POSIX: 负数 = -errno)
pub fn errno_from_i64(rc: i64) -> Option<Errno> {
    if rc < 0 {
        Errno::try_from_i32(-rc as i32)
    } else {
        None
    }
}

/// `i64` 返回码 → `SyscallResult<u64>` (POSIX 约定)
pub fn parse_return(rc: i64) -> SyscallResult<u64> {
    if rc < 0 {
        Err(Errno::try_from_i32(-rc as i32).unwrap_or(Errno::EINVAL))
    } else {
        Ok(rc as u64)
    }
}

// ============================================================================
// 用户态指针验证
// ============================================================================

#[allow(dead_code)]
const USER_ADDR_MAX: u64 = 0x7FFFFFFFE000;

/// 验证用户态指针
pub fn check_user_ptr(ptr: u64) -> bool {
    syscall::api::validate_user_ptr(ptr)
}

/// 验证用户态缓冲区
pub fn check_user_buf(ptr: u64, len: u64) -> bool {
    syscall::api::validate_user_buf(ptr, len)
}

// ============================================================================
// 分发
// ============================================================================

/// 通过 `UserContext` 安全分发 syscall
///
/// **参数**:
/// - `ctx`: 用户态上下文 (由中断入口构造, 内核 TCB 保证有效)
///
/// **返回**:
/// - syscall 原始返回值 (POSIX: 0/-errno 约定)
pub fn dispatch_from_ctx(ctx: &UserContext) -> i64 {
    let num = ctx.syscall_number();
    let a0 = ctx.arg0();
    let a1 = ctx.arg1();
    let a2 = ctx.arg2();
    let a3 = ctx.arg3();
    let a4 = ctx.arg4();
    let a5 = ctx.arg5();
    usermode::dispatch_syscall(num, a0, a1, a2, a3, a4, a5)
}

/// 通过 `SyscallNumber` + `SyscallArgs` 强类型分发
pub fn dispatch(num: SyscallNumber, args: SyscallArgs) -> i64 {
    usermode::dispatch_syscall(num.0, args.a0, args.a1, args.a2, args.a3, args.a4, args.a5)
}

/// 通过 `UserContext` 分发并解析为 `SyscallResult<u64>`
pub fn dispatch_from_ctx_typed(ctx: &UserContext) -> SyscallResult<u64> {
    parse_return(dispatch_from_ctx(ctx))
}

// ============================================================================
// 处理器注册
// ============================================================================

/// Syscall 处理器类型 (C ABI: 4 参, 返回 i64)
pub type SyscallHandler = fn(u64, u64, u64, u64) -> i64;

// ============================================================================
// 初始化
// ============================================================================

/// 初始化 syscall 子系统
pub fn init() {
    fw_syscall_init::syscall_init();
    // T-03: 注册 services 层系统调用分发策略
    let _ = dispatch::register_services_dispatch();
}

// Re-export
pub use dispatch::ServicesSyscallDispatch;

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syscall_number_round_trip() {
        let n = SyscallNumber::new(42);
        assert_eq!(n.raw(), 42);
        assert_eq!(u64::from(n), 42u64);
        let m: SyscallNumber = 99u64.into();
        assert_eq!(m.raw(), 99);
    }

    #[test]
    fn syscall_args_constructors() {
        let a0 = SyscallArgs::one(7);
        assert_eq!(a0.a0, 7);
        assert_eq!(a0.a1, 0);
        assert_eq!(a0.a2, 0);
        assert_eq!(a0.a3, 0);

        let a2 = SyscallArgs::two(1, 2);
        assert_eq!(a2.a0, 1);
        assert_eq!(a2.a1, 2);

        let a3 = SyscallArgs::three(1, 2, 3);
        assert_eq!(a3.a0, 1);
        assert_eq!(a3.a1, 2);
        assert_eq!(a3.a2, 3);
        assert_eq!(a3.a3, 0);

        let a4 = SyscallArgs::new(1, 2, 3, 4);
        assert_eq!(a4.a3, 4);
    }

    #[test]
    fn errno_from_i64_works() {
        assert_eq!(errno_from_i64(-2), Some(Errno::ENOENT));
        assert_eq!(errno_from_i64(-14), Some(Errno::EFAULT));
        assert_eq!(errno_from_i64(0), None);
        assert_eq!(errno_from_i64(42), None);
    }

    #[test]
    fn parse_return_works() {
        assert_eq!(parse_return(0), Ok(0));
        assert_eq!(parse_return(42), Ok(42));
        assert_eq!(parse_return(-2), Err(Errno::ENOENT));
    }

    #[test]
    fn user_ptr_check() {
        // 零指针和超出范围的指针应无效
        assert!(!check_user_ptr(0));
        // 合法用户地址应通过
        assert!(check_user_ptr(0x1000));
        // 超过 USER_ADDR_MAX 应失败
        assert!(!check_user_ptr(USER_ADDR_MAX + 1));
    }

    #[test]
    fn user_buf_check() {
        // 零长度合法
        assert!(check_user_buf(0, 0));
        // 合法 buf
        assert!(check_user_buf(0x1000, 0x100));
        // ptr + len 溢出
        assert!(!check_user_buf(USER_ADDR_MAX - 1, 0x1000));
    }
}
