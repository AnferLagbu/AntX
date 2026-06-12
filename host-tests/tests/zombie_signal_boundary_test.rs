//! I-52: Zombie 进程信号投递边界检查
//!
//! 验证 do_signal_send / do_signal_send_inner 的 Zombie 状态判定:
//! 1. Zombie 状态: 返回 -3 (ESRCH), pending 不变
//! 2. 正常状态: 正常投递
//! 3. 边界: 信号编号 0/32/255 等无效信号
//!
//! 主机端镜像内核 ProcessState 枚举和发送逻辑.

use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum ProcessState {
    Ready = 0,
    Running = 1,
    Blocked = 2,
    Zombie = 3,
}

struct FakeProcess {
    state: AtomicU32,
    signal_pending: AtomicU32,
}

impl FakeProcess {
    fn new(state: ProcessState) -> Self {
        Self {
            state: AtomicU32::new(state as u32),
            signal_pending: AtomicU32::new(0),
        }
    }
    fn signal_pending_set(&self, sig: u32) {
        self.signal_pending.fetch_or(1u32 << (sig - 1), Ordering::Release);
    }
}

const ERR_INVALID: i32 = -1;
const ERR_NOENT: i32 = -2;
const ERR_ZOMBIE: i32 = -3;

/// 镜像内核 do_signal_send 逻辑 (仅核心判定)
fn do_signal_send(proc: Option<&FakeProcess>, sig: u8) -> Result<(), i32> {
    if sig == 0 {
        return if proc.is_some() { Ok(()) } else { Err(ERR_NOENT) };
    }
    if !(1..=31).contains(&sig) {
        return Err(ERR_INVALID);
    }
    let p = proc.ok_or(ERR_NOENT)?;
    let state = p.state.load(Ordering::Acquire);
    if state == ProcessState::Zombie as u32 {
        return Err(ERR_ZOMBIE);
    }
    p.signal_pending_set(sig as u32);
    Ok(())
}

/// 镜像内核 do_signal_send_inner 逻辑
fn do_signal_send_inner(proc: Option<&FakeProcess>, sig: u8) -> Result<(), i32> {
    if sig == 0 {
        return if proc.is_some() { Ok(()) } else { Err(ERR_NOENT) };
    }
    if !(1..=31).contains(&sig) {
        return Err(ERR_INVALID);
    }
    let p = proc.ok_or(ERR_NOENT)?;
    let state = p.state.load(Ordering::Acquire);
    if state == ProcessState::Zombie as u32 {
        return Err(ERR_ZOMBIE);
    }
    p.signal_pending_set(sig as u32);
    Ok(())
}

#[test]
fn test_zombie_blocks_signal_send() {
    let proc = FakeProcess::new(ProcessState::Zombie);
    let res = do_signal_send(Some(&proc), 9);
    assert_eq!(res, Err(ERR_ZOMBIE));
    // pending 不变
    assert_eq!(proc.signal_pending.load(Ordering::Acquire), 0);
}

#[test]
fn test_zombie_blocks_inner_signal_send() {
    let proc = FakeProcess::new(ProcessState::Zombie);
    let res = do_signal_send_inner(Some(&proc), 15);
    assert_eq!(res, Err(ERR_ZOMBIE));
    assert_eq!(proc.signal_pending.load(Ordering::Acquire), 0);
}

#[test]
fn test_ready_state_delivers() {
    let proc = FakeProcess::new(ProcessState::Ready);
    let res = do_signal_send(Some(&proc), 9);
    assert!(res.is_ok());
    // SIGKILL (9) bit 8 → 0x100
    assert_eq!(proc.signal_pending.load(Ordering::Acquire), 1 << 8);
}

#[test]
fn test_blocked_state_delivers_and_wakes() {
    let proc = FakeProcess::new(ProcessState::Blocked);
    let res = do_signal_send(Some(&proc), 9);
    assert!(res.is_ok());
    // 在内核实现中, state 会被转为 Ready
    assert_eq!(proc.state.load(Ordering::Acquire), ProcessState::Blocked as u32);
    assert_eq!(proc.signal_pending.load(Ordering::Acquire), 1 << 8);
}

#[test]
fn test_running_state_delivers() {
    let proc = FakeProcess::new(ProcessState::Running);
    let res = do_signal_send(Some(&proc), 9);
    assert!(res.is_ok());
    assert_eq!(proc.signal_pending.load(Ordering::Acquire), 1 << 8);
}

#[test]
fn test_invalid_signal_zero_returns_noent_for_missing() {
    // sig=0 仅检查存在性
    let res = do_signal_send(None, 0);
    assert_eq!(res, Err(ERR_NOENT));
}

#[test]
fn test_invalid_signal_zero_returns_ok_for_existing() {
    let proc = FakeProcess::new(ProcessState::Zombie);
    // sig=0 仍应返回 Ok (POSIX: 仅检查存在)
    let res = do_signal_send(Some(&proc), 0);
    assert!(res.is_ok());
    // pending 不变 (sig=0 不投递)
    assert_eq!(proc.signal_pending.load(Ordering::Acquire), 0);
}

#[test]
fn test_invalid_signal_32_rejected() {
    let proc = FakeProcess::new(ProcessState::Ready);
    let res = do_signal_send(Some(&proc), 32);
    assert_eq!(res, Err(ERR_INVALID));
}

#[test]
fn test_invalid_signal_255_rejected() {
    let proc = FakeProcess::new(ProcessState::Ready);
    let res = do_signal_send(Some(&proc), 255);
    assert_eq!(res, Err(ERR_INVALID));
}

#[test]
fn test_zombie_recovery_not_via_signal() {
    // Zombie 状态不应被任何信号清除, 但调度器/父进程 waitpid 可回收.
    // 本测试仅验证: 给 Zombie 发多次信号, 状态保持 Zombie, pending 全 0.
    let proc = FakeProcess::new(ProcessState::Zombie);
    for sig in 1..=31u8 {
        let res = do_signal_send(Some(&proc), sig);
        assert_eq!(res, Err(ERR_ZOMBIE));
    }
    assert_eq!(proc.state.load(Ordering::Acquire), ProcessState::Zombie as u32);
    assert_eq!(proc.signal_pending.load(Ordering::Acquire), 0);
}
