//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 `kernel::framework::usermode`。
//!
//! ## 调用方
//! - `kernel::mod` — 通过 `pub mod services` 暴露给内核
//! - `framework::usermode` — 用户态入口后调用 `dispatch_from_ctx`
//!
//! ## 内部实现
//! - 所有 `sys_*` 函数委托给 `kernel::syscall::mod` (原始模块, unsafe 已在底层)。
//! - `validate_user_ptr` / `validate_user_buf` → 已通过 `kernel::syscall::api` 暴露为 pub fn。
//! - `dispatch_syscall` / `register_syscall_handler` → 通过 `framework::usermode` 安全封装。
//! - 本模块的目标: 在 Phase 2 完成后, syscall 分发全部走 `framework::UserContext`,
//!   消除 raw `InterruptFrame` 指针操作。

use crate::kernel::framework::userctx::UserContext;
use crate::kernel::framework::usermode;
use crate::kernel::syscall::types::SyscallHandler;

use crate::kernel::syscall::api::{validate_user_ptr, validate_user_buf};

/// 通过 UserContext 分发 syscall (安全入口)。
///
/// 替代 `syscall_dispatch_from_frame(*mut InterruptFrame)` 的 raw pointer 路径。
/// services 层通过此函数处理系统调用, 不再直接操作中断帧。
pub fn dispatch_from_ctx(ctx: &UserContext) -> u64 {
    let num = ctx.syscall_number();
    let a0 = ctx.arg0();
    let a1 = ctx.arg1();
    let a2 = ctx.arg2();
    let a3 = ctx.arg3();

    // 委托给 framework 的安全 syscall 分发
    usermode::dispatch_syscall(num, a0, a1, a2, a3) as u64
}

/// 验证用户态指针 (安全包装)。
pub fn check_user_ptr(ptr: u64) -> bool {
    validate_user_ptr(ptr)
}

/// 验证用户态缓冲区 (安全包装)。
pub fn check_user_buf(ptr: u64, len: u64) -> bool {
    validate_user_buf(ptr, len)
}

/// 注册 syscall 处理器 (安全包装)。
///
/// # 安全约束
/// - 仅在启动阶段单线程调用
/// - num 不可与已有注册冲突
pub fn register_handler(num: u64, handler: SyscallHandler) {
    usermode::register_syscall_handler(num, handler);
}
