//! 信号 (Signal) 机制实现
//!
//! 提供异步进程间通知能力
//! 功能等价于 POSIX signals

use super::types::*;
use crate::kernel::proc::api::{
    process_get_by_pid, process_get_current_pwm, process_get_pwm_by_pid,
};

/// 发送信号到指定进程 (Rust 安全接口)
///
/// # Arguments
/// * `sig` - 信号编号 (1-32)
/// * `target_pid` - 目标进程 PID
///
/// # Returns
/// * Ok(()) - 成功发送
/// * Err(i32) - 错误码 (-1: 无效信号编号, -2: 进程不存在)
pub fn signal_send_safe(sig: u8, target_pid: u32) -> Result<(), i32> {
    if sig < 1 || sig > IPC_MAX_SIGNALS as u8 {
        return Err(-1);
    }

    unsafe {
        let proc_addr = process_get_by_pid(target_pid);
        if proc_addr == 0 {
            return Err(-2);
        }

        let sender_pwm = process_get_current_pwm();
        if sender_pwm != 0 {
            let sender_level = crate::kernel::credo::engine::get_privilege_level(sender_pwm);
            if sender_level != 0 {
                let target_pwm = process_get_pwm_by_pid(target_pid);
                if target_pwm != sender_pwm {
                    return Err(-3);
                }
            }
        }
    }

    Ok(())
}

/// 注册信号处理函数 (Rust 安全接口)
///
/// # Arguments
/// * `sig` - 信号编号
/// * `handler` - 处理函数指针 (C ABI)
/// * `flags` - 标志位
///
/// # Returns
/// * Ok(()) - 成功注册
/// * Err(i32) - 错误码 (-1: 无效信号)
pub fn signal_register_safe(
    sig: u8,
    _handler: Option<SignalHandlerFn>,
    _flags: u32,
) -> Result<(), i32> {
    if sig < 1 || sig > IPC_MAX_SIGNALS as u8 {
        return Err(-1);
    }

    // TODO: 将处理函数注册到当前进程的 SignalPending 结构中
    // 当前简化实现仅验证参数有效性

    Ok(())
}

/// 屏蔽信号 (Rust safe interface)
///
/// # Arguments
/// * `sig` - 信号编号
///
/// # Returns
/// * Ok(()) - 成功
/// * Err(i32) - 错误码 (-1: 无效信号)
pub fn signal_block_safe(sig: u8) -> Result<(), i32> {
    if sig < 1 || sig > IPC_MAX_SIGNALS as u8 {
        return Err(-1);
    }

    // TODO: 在当前进程的 blocked 位图中设置对应位
    Ok(())
}

/// 解除信号屏蔽 (Rust safe interface)
///
/// # Arguments
/// * `sig` - 信号编号
///
/// # Returns
/// * Ok(()) - 成功
/// * Err(i32) - 错误码 (-1: 无效信号)
pub fn signal_unblock_safe(sig: u8) -> Result<(), i32> {
    if sig < 1 || sig > IPC_MAX_SIGNALS as u8 {
        return Err(-1);
    }

    // TODO: 在当前进程的 blocked 位图中清除对应位
    Ok(())
}

/// 分发待处理的信号 (Rust safe interface)
///
/// 应在每次从系统调用返回或中断处理完成后调用。
/// 检查当前进程是否有待处理信号，如果有则执行对应的处理动作。
pub fn signal_dispatch_safe() {
    // TODO: 实现完整的信号分发逻辑:
    // 1. 检查 pending 位图是否非零
    // 2. 找出最高优先级的待处理信号
    // 3. 检查该信号是否被屏蔽
    // 4. 根据 action 执行相应操作:
    //    - Default: 执行默认行为 (终止/忽略/停止等)
    //    - Ignore: 忽略
    //    - Handler: 调用用户空间处理函数
    // 5. 清除 pending 位图中的对应位
}

// ============================================================================
// FFI 导出函数
// ============================================================================

/// FFI: 发送信号
#[no_mangle]
pub fn ipc_signal_send(pid: i32, sig: i32) -> i32 {
    match signal_send_safe(sig as u8, pid as u32) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// FFI: 注册信号处理函数
#[no_mangle]
pub fn ipc_signal_register(
    sig: i32,
    handler: Option<extern "C" fn(i32)>,
    flags: u32,
) -> i32 {
    match signal_register_safe(sig as u8, handler, flags) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// FFI: 屏蔽信号
#[no_mangle]
pub fn ipc_signal_block(sig: i32) -> i32 {
    match signal_block_safe(sig as u8) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// FFI: 解除屏蔽信号
#[no_mangle]
pub fn ipc_signal_unblock(sig: i32) -> i32 {
    match signal_unblock_safe(sig as u8) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// FFI: 分发信号
#[no_mangle]
pub fn ipc_signal_dispatch() {
    signal_dispatch_safe();
}
