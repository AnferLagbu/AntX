//! POSIX 信号投递 — framework 层核心实现
//!
//! 提供信号发送、投递和默认动作执行的完整机制：
//! - **do_signal_send**: 向目标进程发送信号 (设置 pending 位, 唤醒)
//! - **do_signal_deliver**: 返回用户态前检查并投递信号
//! - **signal_default_action**: 执行信号的默认动作 (Term/Core/Stop/Ign)
//!
//! ## 信号投递流程
//!
//! ```text
//! sys_kill(pid, sig)
//!   └→ do_signal_send(pid, sig)
//!        ├→ 设置 pending_signals 位
//!        └→ 唤醒目标进程 (如果可中断)
//!
//! 中断/系统调用返回用户态前:
//!   └→ do_signal_deliver()
//!        ├→ 检查 pending & ~blocked
//!        ├→ 选择最高优先级信号
//!        ├→ 查找 sigaction_table[sig]
//!        │   ├→ SIG_DFL: 执行默认动作
//!        │   ├→ SIG_IGN: 忽略
//!        │   └→ handler: 修改用户态栈帧, 跳转到 handler
//!        └→ sigreturn: 恢复原始栈帧
//! ```
//!
//! # Safety
//!
//! - do_signal_send: 操作进程原子字段, 线程安全
//! - do_signal_deliver: 操作当前进程, 单 CPU 执行, 无竞争
//! - PROCESS_TABLE.get() 返回 *mut Process, 需 unsafe 解引用

use core::sync::atomic::Ordering;

use super::process::PROCESS_TABLE;
use super::types::{Pid, ProcessState};

// ============================================================================
// 常量
// ============================================================================

/// SIG_DFL: 默认动作
pub const SIG_DFL: u64 = 0;
/// SIG_IGN: 忽略
pub const SIG_IGN: u64 = 1;

/// SS_ONSTACK: 信号替换栈正在使用
pub const SS_ONSTACK: u32 = 1;
/// SS_DISABLE: 信号替换栈已禁用
pub const SS_DISABLE: u32 = 2;

/// 信号默认动作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalDefaultAction {
    /// 终止进程
    Term,
    /// 终止进程 + 核心转储
    Core,
    /// 忽略
    Ign,
    /// 停止进程
    Stop,
    /// 继续 (如果已停止)
    Cont,
}

/// 获取标准信号的默认动作
pub fn signal_default_action(sig: u8) -> SignalDefaultAction {
    match sig {
        // Ign: CHLD(17), URG(23)
        17 | 23 => SignalDefaultAction::Ign,
        // Stop: STOP(19), TSTP(20), TTIN(21), TTOU(22)
        19 | 20 | 21 | 22 => SignalDefaultAction::Stop,
        // Cont: CONT(18)
        18 => SignalDefaultAction::Cont,
        // Core: QUIT(3), ILL(4), ABRT(6), BUS(7), FPE(8), SEGV(11), SYS(31), XCPU(24), XFSZ(25)
        3 | 4 | 6 | 7 | 8 | 11 | 31 | 24 | 25 => SignalDefaultAction::Core,
        // Term: 其余所有信号
        _ => SignalDefaultAction::Term,
    }
}

/// 信号是否不可捕获/屏蔽
pub fn is_uncatchable(sig: u8) -> bool {
    sig == 9 || sig == 19 // SIGKILL, SIGSTOP
}

// ============================================================================
// do_signal_send — 发送信号
// ============================================================================

/// 向目标进程发送信号
///
/// 1. 验证信号编号有效性
/// 2. 设置目标进程的 pending_signals 位
/// 3. 如果目标进程在可中断睡眠状态, 唤醒它
///
/// # Returns
/// - `Ok(())`: 成功发送
/// - `Err(i32)`: 错误码 (-1: 无效信号, -2: 进程不存在)
pub fn do_signal_send(pid: Pid, sig: u8) -> Result<(), i32> {
    // 验证信号编号
    if sig == 0 {
        // POSIX: kill(pid, 0) 仅检查进程存在
        return if PROCESS_TABLE.get(pid).is_some() {
            Ok(())
        } else {
            Err(-2)
        };
    }
    if sig < 1 || sig > 31 {
        return Err(-1);
    }

    // 查找目标进程
    let proc_ptr = PROCESS_TABLE.get(pid).ok_or(-2)?;

    // SAFETY: PROCESS_TABLE.get() 返回有效指针, 进程在表中期间不会释放
    let proc = unsafe { &*proc_ptr };

    // SIGKILL/SIGSTOP 不可忽略, 直接设置 pending
    // 其他信号: 如果 handler 是 SIG_IGN 且不是不可捕获信号, 则忽略
    if !is_uncatchable(sig) {
        let actions = proc.sigaction_table.lock();
        if actions[(sig - 1) as usize] == SIG_IGN {
            return Ok(()); // 显式忽略, 不报错
        }
    }

    // 设置 pending 位
    proc.signal_pending_set(sig as u32);

    // 唤醒目标进程 (如果处于可中断睡眠)
    let state = proc.state.load(Ordering::Acquire);
    if state == ProcessState::Blocked as u32 {
        proc.state.store(ProcessState::Ready as u32, Ordering::Release);
    }

    Ok(())
}

// ============================================================================
// do_signal_deliver — 投递信号
// ============================================================================

/// 选择下一个待投递的信号
///
/// 从 pending & ~blocked 中选择编号最小的信号 (标准信号按编号优先)
///
/// # Returns
/// - `Some(sig)`: 待投递的信号编号 (1..=31)
/// - `None`: 无待投递信号
pub fn signal_pick_next(proc: &super::process::Process) -> Option<u8> {
    let pending = proc.signal_pending_get();
    let blocked = proc.blocked_mask.load(Ordering::Acquire);
    let deliverable = pending & !blocked;

    if deliverable == 0 {
        return None;
    }

    // 找最低位的 1 (信号编号 = bit position)
    // bit 0 = sig 1 (SIGHUP), bit 1 = sig 2 (SIGINT), ...
    let sig_bit = deliverable.trailing_zeros() as u8;
    if sig_bit == 0 || sig_bit > 31 {
        return None;
    }
    Some(sig_bit)
}

/// 执行信号的默认动作
///
/// 在 do_signal_deliver 中, 当 sigaction 为 SIG_DFL 时调用.
pub fn do_signal_default_action(pid: Pid, sig: u8) {
    match signal_default_action(sig) {
        SignalDefaultAction::Ign => {}
        SignalDefaultAction::Cont => {
            if let Some(proc_ptr) = PROCESS_TABLE.get(pid) {
                // SAFETY: PROCESS_TABLE 保证指针有效
                let proc = unsafe { &*proc_ptr };
                proc.state.store(ProcessState::Ready as u32, Ordering::Release);
            }
        }
        SignalDefaultAction::Stop => {
            if let Some(proc_ptr) = PROCESS_TABLE.get(pid) {
                let proc = unsafe { &*proc_ptr };
                proc.state.store(ProcessState::Blocked as u32, Ordering::Release);
            }
        }
        SignalDefaultAction::Term | SignalDefaultAction::Core => {
            if let Some(proc_ptr) = PROCESS_TABLE.get(pid) {
                let proc = unsafe { &*proc_ptr };
                proc.exit_code.store((sig as u32) << 8 | 0x7f, Ordering::Release);
                proc.state.store(ProcessState::Zombie as u32, Ordering::Release);
            }
        }
    }
}

/// 投递待处理信号 (在返回用户态前调用)
///
/// 遍历当前进程的 pending & ~blocked, 逐个投递:
/// - SIG_DFL: 执行默认动作
/// - SIG_IGN: 清除 pending 位, 忽略
/// - handler: 修改用户态栈帧跳转到 handler (当前实现暂执行默认动作)
///
/// # Returns
///
/// 返回 true 表示有信号被投递.
/// 返回 false 表示无待投递信号.
///
/// # Note
///
/// 当前实现仅处理 SIG_DFL 和 SIG_IGN.
/// handler 跳转需要架构相关的栈帧修改, 将在后续迭代中实现.
pub fn do_signal_deliver() -> bool {
    let pid = match super::scheduler::SCHEDULER.current() {
        Some(p) => p,
        None => return false,
    };

    let proc_ptr = match PROCESS_TABLE.get(pid) {
        Some(p) => p,
        None => return false,
    };

    // SAFETY: 当前进程一定有效
    let proc = unsafe { &*proc_ptr };

    let mut delivered = false;

    loop {
        let sig = match signal_pick_next(proc) {
            Some(s) => s,
            None => break,
        };

        // 清除 pending 位
        proc.signal_pending_clear(1u64 << sig as u64);

        // 查找 sigaction
        let action = {
            let actions = proc.sigaction_table.lock();
            actions[(sig - 1) as usize]
        };

        match action {
            SIG_DFL => {
                do_signal_default_action(pid, sig);
                delivered = true;
                // 如果进程被终止或停止, 不再投递更多信号
                let state = proc.state.load(Ordering::Acquire);
                if state == ProcessState::Zombie as u32
                    || state == ProcessState::Blocked as u32
                {
                    break;
                }
            }
            SIG_IGN => {
                delivered = true;
            }
            _handler_addr => {
                // TODO: 架构相关的栈帧修改 (x86_64: 修改 RIP/RSP, aarch64: 修改 ELR/SP)
                // 当前简化实现: 暂时执行默认动作
                do_signal_default_action(pid, sig);
                delivered = true;
            }
        }
    }

    delivered
}

// ============================================================================
// 便利函数
// ============================================================================

/// 检查当前进程是否有可投递信号
pub fn has_deliverable_signal(pid: Pid) -> bool {
    let proc_ptr = match PROCESS_TABLE.get(pid) {
        Some(p) => p,
        None => return false,
    };
    // SAFETY: PROCESS_TABLE 保证指针有效
    let proc = unsafe { &*proc_ptr };
    let pending = proc.signal_pending_get();
    let blocked = proc.blocked_mask.load(Ordering::Acquire);
    (pending & !blocked) != 0
}

/// 获取进程的信号屏蔽字
pub fn get_blocked_mask(pid: Pid) -> u64 {
    let proc_ptr = match PROCESS_TABLE.get(pid) {
        Some(p) => p,
        None => return 0,
    };
    // SAFETY: PROCESS_TABLE 保证指针有效
    let proc = unsafe { &*proc_ptr };
    proc.blocked_mask.load(Ordering::Acquire)
}

/// 设置进程的信号屏蔽字
pub fn set_blocked_mask(pid: Pid, mask: u64) {
    let proc_ptr = match PROCESS_TABLE.get(pid) {
        Some(p) => p,
        None => return,
    };
    // SAFETY: PROCESS_TABLE 保证指针有效
    let proc = unsafe { &*proc_ptr };
    proc.blocked_mask.store(mask, Ordering::Release);
}

/// 获取进程的 sigaction 表项
pub fn get_sigaction(pid: Pid, sig: u8) -> Option<u64> {
    if sig < 1 || sig > 31 {
        return None;
    }
    let proc_ptr = PROCESS_TABLE.get(pid)?;
    // SAFETY: PROCESS_TABLE 保证指针有效
    let proc = unsafe { &*proc_ptr };
    let actions = proc.sigaction_table.lock();
    Some(actions[(sig - 1) as usize])
}

/// 设置进程的 sigaction 表项, 返回旧值
pub fn set_sigaction(pid: Pid, sig: u8, action: u64) -> Option<u64> {
    if sig < 1 || sig > 31 {
        return None;
    }
    if is_uncatchable(sig) {
        return None; // SIGKILL/SIGSTOP 不可捕获
    }
    let proc_ptr = PROCESS_TABLE.get(pid)?;
    // SAFETY: PROCESS_TABLE 保证指针有效
    let proc = unsafe { &*proc_ptr };
    let mut actions = proc.sigaction_table.lock();
    let old = actions[(sig - 1) as usize];
    actions[(sig - 1) as usize] = action;
    Some(old)
}

// ============================================================================
// 内核测试
// ============================================================================

#[cfg(feature = "kernel_test")]
fn test_signal_default_action() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{assert_eq_test, TestResult};
    assert_eq_test!(signal_default_action(9), SignalDefaultAction::Term, "SIGKILL=Term");
    assert_eq_test!(signal_default_action(11), SignalDefaultAction::Core, "SIGSEGV=Core");
    assert_eq_test!(signal_default_action(17), SignalDefaultAction::Ign, "SIGCHLD=Ign");
    assert_eq_test!(signal_default_action(19), SignalDefaultAction::Stop, "SIGSTOP=Stop");
    assert_eq_test!(signal_default_action(18), SignalDefaultAction::Cont, "SIGCONT=Cont");
    assert_eq_test!(signal_default_action(15), SignalDefaultAction::Term, "SIGTERM=Term");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_uncatchable() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};
    check!(is_uncatchable(9), "SIGKILL uncatchable");
    check!(is_uncatchable(19), "SIGSTOP uncatchable");
    check!(!is_uncatchable(15), "SIGTERM catchable");
    check!(!is_uncatchable(2), "SIGINT catchable");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_signal_pick_next_logic() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{assert_eq_test, TestResult};
    // 模拟 pending = 0b1010 (bit 1=SIGINT, bit 3=SIGQUIT)
    // blocked = 0b10 (bit 1=SIGINT)
    // deliverable = 0b1000, 应选择 bit 3 = SIGQUIT (sig=3)
    let pending: u64 = 0b1010;
    let blocked: u64 = 0b0010;
    let deliverable = pending & !blocked;
    assert_eq_test!(deliverable, 0b1000, "deliverable");
    let sig_bit = deliverable.trailing_zeros() as u8;
    assert_eq_test!(sig_bit, 3, "lowest bit is 3");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_set_get_sigaction() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};
    // SIGKILL (9) 不可设置
    let result = set_sigaction(1, 9, 0xDEAD);
    check!(result.is_none(), "SIGKILL cannot be caught");
    // SIGSTOP (19) 不可设置
    let result = set_sigaction(1, 19, 0xDEAD);
    check!(result.is_none(), "SIGSTOP cannot be caught");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_default_action_coverage() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{assert_eq_test, TestResult};
    // 验证所有 31 个标准信号都有定义
    for sig in 1u8..=31 {
        let _action = signal_default_action(sig);
    }
    // 抽查关键信号
    assert_eq_test!(signal_default_action(1), SignalDefaultAction::Term, "SIGHUP=Term");
    assert_eq_test!(signal_default_action(2), SignalDefaultAction::Term, "SIGINT=Term");
    assert_eq_test!(signal_default_action(13), SignalDefaultAction::Term, "SIGPIPE=Term");
    assert_eq_test!(signal_default_action(14), SignalDefaultAction::Term, "SIGALRM=Term");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
pub fn register_signal_tests() {
    use crate::kernel::framework::tests::runner;
    let r = runner();
    r.register("signal", "default_action", test_signal_default_action);
    r.register("signal", "uncatchable", test_uncatchable);
    r.register("signal", "pick_next_logic", test_signal_pick_next_logic);
    r.register("signal", "set_get_sigaction", test_set_get_sigaction);
    r.register("signal", "default_action_coverage", test_default_action_coverage);
}
