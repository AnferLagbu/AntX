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
//! ## x86_64 信号栈帧布局
//!
//! ```text
//! 用户栈 (低地址 → 高地址):
//! ┌────────────────────┐ ← 新 RSP
//! │ ucontext / siginfo │  (未来扩展)
//! ├────────────────────┤
//! │ SignalFrame         │  保存原始寄存器
//! │   rip              │
//! │   cs               │
//! │   rflags           │
//! │   rsp              │
//! │   ss               │
//! │   rax..r15         │
//! │   signum           │
//! ├────────────────────┤
//! │ sigreturn trampoline│  __sigreturn 代码 (syscall 15)
//! └────────────────────┘ ← handler 返回地址
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
// 信号栈帧 (x86_64)
// ============================================================================

/// 信号栈帧 — 保存在用户栈上, sigreturn 时恢复
///
/// 布局与 InterruptFrame 兼容, 便于直接拷贝寄存器状态.
/// signum 字段放在最后, handler 通过第一个参数 (rdi) 获取.
#[repr(C)]
pub struct SignalFrame {
    // 通用寄存器 (与 InterruptFrame 顺序一致)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    // 中断元数据
    pub int_no: u64,
    pub err_code: u64,

    // 返回地址信息
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,

    // 信号编号 (handler 参数)
    pub signum: u64,
}

/// sigreturn trampoline 代码
///
/// x86_64 机器码:
///   mov eax, 15       ; SYS_rt_sigreturn = 15 (x86_64)
///   syscall            ; 系统调用
///
/// 共 8 字节: B8 0F 00 00 00 0F 05
pub const SIGRETURN_TRAMPOLINE: [u8; 7] = [0xB8, 0x0F, 0x00, 0x00, 0x00, 0x0F, 0x05];

/// sigreturn trampoline 大小
pub const SIGRETURN_TRAMPOLINE_SIZE: usize = SIGRETURN_TRAMPOLINE.len();

/// 信号栈帧总大小 (含 trampoline)
pub const SIGNAL_FRAME_TOTAL_SIZE: usize = core::mem::size_of::<SignalFrame>() + SIGRETURN_TRAMPOLINE_SIZE;

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
/// - handler: 修改用户态栈帧跳转到 handler
///
/// # Arguments
/// * `frame` - 当前中断帧 (用户态寄存器状态), 仅当 handler 投递时修改
///
/// # Returns
///
/// 返回 true 表示有信号被投递 (handler 已设置, 需要返回用户态执行).
/// 返回 false 表示无待投递信号.
///
/// # Safety
///
/// - frame 指针必须有效且指向当前 CPU 的中断帧
/// - 仅在返回用户态前调用 (中断上下文或系统调用出口)
pub fn do_signal_deliver(frame: *mut crate::kernel::framework::idt::types::InterruptFrame) -> bool {
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
            handler_addr => {
                // 构建信号栈帧并修改 InterruptFrame
                // SAFETY: frame 由调用方保证有效
                let f = unsafe { &mut *frame };

                let user_rsp = f.rsp;

                // 栈帧布局:
                //   frame_rsp+0: 返回地址 (指向 trampoline, handler ret 时弹出)
                //   frame_rsp+8: SignalFrame (保存原始寄存器)
                //   frame_rsp+8+sizeof(SignalFrame): trampoline code (rt_sigreturn)
                let total = 8 + core::mem::size_of::<SignalFrame>() + SIGRETURN_TRAMPOLINE_SIZE;
                let frame_rsp = if user_rsp >= total as u64 {
                    user_rsp - total as u64
                } else {
                    // 栈溢出, 执行默认动作
                    do_signal_default_action(pid, sig);
                    delivered = true;
                    break;
                };

                // 构建 SignalFrame (保存原始寄存器)
                let sigframe = SignalFrame {
                    r15: f.r15, r14: f.r14, r13: f.r13, r12: f.r12,
                    r11: f.r11, r10: f.r10, r9: f.r9, r8: f.r8,
                    rdi: f.rdi, rsi: f.rsi, rbp: f.rbp, rdx: f.rdx,
                    rcx: f.rcx, rbx: f.rbx, rax: f.rax,
                    int_no: f.int_no, err_code: f.err_code,
                    rip: f.rip, cs: f.cs, rflags: f.rflags, rsp: f.rsp, ss: f.ss,
                    signum: sig as u64,
                };

                unsafe {
                    let trampoline_start = frame_rsp + 8 + core::mem::size_of::<SignalFrame>() as u64;

                    // 写入返回地址 (指向 trampoline)
                    core::ptr::write_unaligned(frame_rsp as *mut u64, trampoline_start);

                    // 写入 SignalFrame
                    core::ptr::write_unaligned((frame_rsp + 8) as *mut SignalFrame, sigframe);

                    // 写入 trampoline 代码 (mov eax,15; syscall)
                    core::ptr::copy_nonoverlapping(
                        SIGRETURN_TRAMPOLINE.as_ptr(),
                        trampoline_start as *mut u8,
                        SIGRETURN_TRAMPOLINE_SIZE,
                    );
                }

                // 修改 InterruptFrame: 跳转到 handler
                f.rip = handler_addr;
                f.rdi = sig as u64;  // 参数1: signum
                f.rsi = 0;           // 参数2: siginfo (简化: NULL)
                f.rdx = 0;           // 参数3: ucontext (简化: NULL)
                f.rsp = frame_rsp;

                delivered = true;
                // 一次只投递一个 handler 信号 (sigreturn 后再投递下一个)
                break;
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
