//! UserMode — 进入 Ring 3 / EL0 的安全句柄 (TCB)
//!
//! 封装 `sysret` (x86_64) / `eret` (aarch64) 指令，
//! 确保回内核后栈/状态正确。
//!
//! ## 与 Asterinas OSTD `UserMode` 的关系
//!
//! 等价于 OSTD 的 `enter_usermode()` 入口。
//!
//! ## SAFETY 不变量
//!
//! - 必须在进程的内核栈上调用（非中断栈）。
//! - 返回时内核栈必须恢复到调用前状态。
//! - 调用前必须调用 `VmSpace::activate()` 切换到正确的页表。

use super::vmspace::VmSpace;
use super::userctx::UserContext;

/// 进入用户模式执行直到下一次陷入（syscall / interrupt / exception）。
///
/// 返回 UserContext 携带返回时的用户态寄存器状态。
///
/// # SAFETY
/// - 必须在进程的内核栈上调用（非中断栈）。
/// - 调用前必须调用 `vmspace.activate()` 切换到正确的页表。
/// - 返回时内核栈恢复到调用前状态。
#[cfg(target_arch = "x86_64")]
pub unsafe fn enter_user_mode(_vmspace: &VmSpace, ctx: &UserContext) -> UserContext {
    // 实际实现在 Phase 1.3 (与 asm stub 对接) 完成。
    // 当前占位：直接返回传入的 ctx（hello world 内核可先跑 busy-loop 用户态）。
    let _ = _vmspace;
    *ctx
}

#[cfg(target_arch = "aarch64")]
pub unsafe fn enter_user_mode(_vmspace: &VmSpace, ctx: &UserContext) -> UserContext {
    let _ = _vmspace;
    *ctx
}

/// 安全分发系统调用 (services 层入口)。
///
/// 封装 `unsafe extern "C" fn syscall_dispatch` 调用,
/// 使 services 层无需直接接触 unsafe FFI。
///
/// # 安全约束
/// - 调用方保证 `num` 是有效的系统调用号。
/// - 参数 `a0..a3` 来自用户态寄存器, 由 `UserContext` 提取。
pub fn dispatch_syscall(num: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    // SAFETY: syscall_dispatch is unsafe extern "C" because it processes
    // raw user-supplied arguments. The framework is the TCB and is
    // responsible for validating these arguments before passing to services.
    unsafe { crate::kernel::syscall::syscall_dispatch(num, a0, a1, a2, a3) }
}

/// 安全注册系统调用处理器 (services 层入口)。
///
/// 封装 `unsafe fn syscall_register` 调用,
/// 使 services 层无需直接接触 unsafe。
///
/// # 安全约束
/// - 仅在内核启动阶段单线程调用。
/// - `num` 不可与已有注册冲突。
pub fn register_syscall_handler(num: u64, handler: crate::kernel::syscall::types::SyscallHandler) {
    // SAFETY: 启动阶段单线程安全, handler 来自 services 层。
    unsafe { crate::kernel::syscall::api::syscall_register(num, handler) }
}
