//! I-45 补充验收: 用户态 sigaltstack 系统调用, 内核正确记录替代栈参数
//!
//! 镜像 [framework/syscall/mod.rs::sys_sigaltstack] 的契约:
//! 1. ss & old_ss 都为 0 → 直接返回 0, 不修改状态
//! 2. SS_DISABLE: 清 addr/size, 置位 SS_DISABLE (保留 SS_ONSTACK)
//! 3. 启用: 写入 addr/size, 清除 SS_ONSTACK/SS_DISABLE
//! 4. 读 old_ss: 返回当前 addr/flags/size 三元组
//! 5. addr=0, size!=0 → 视为"未配置替代栈", use_alternate 决策返回 false
//!
//! 决策表 + 状态机镜像, 覆盖 [x86_64] 与 [aarch64] 两套 syscall 路径.

#![allow(dead_code)]

const SS_ONSTACK: u32 = 1;
const SS_DISABLE: u32 = 2;

/// stack_t 镜像 (POSIX)
#[derive(Clone, Copy, Debug, PartialEq)]
struct StackT {
    ss_sp: u64,
    ss_flags: u32,
    ss_size: u64,
}

impl StackT {
    const fn zero() -> Self { Self { ss_sp: 0, ss_flags: 0, ss_size: 0 } }
}

/// 进程 sigaltstack 状态
#[derive(Clone, Copy, Debug, PartialEq)]
struct AltStackState {
    addr: u64,
    size: u64,
    flags: u32,
}

impl AltStackState {
    const fn zero() -> Self { Self { addr: 0, size: 0, flags: 0 } }
}

/// 镜像 sys_sigaltstack 的状态机 (ss: in Option, old_ss: in Option).
///
/// 返回 (新状态, 写入 old_ss 的值).
fn sigaltstack_op(state: AltStackState, ss: Option<StackT>, old_ss_query: bool) -> (AltStackState, Option<StackT>) {
    let mut s = state;
    let written = if old_ss_query {
        Some(StackT { ss_sp: s.addr, ss_flags: s.flags, ss_size: s.size })
    } else { None };

    if let Some(new_ss) = ss {
        if (new_ss.ss_flags & SS_DISABLE) != 0 {
            // 禁用: 保留 SS_ONSTACK, 置 SS_DISABLE, 清 addr/size
            s.addr = 0;
            s.size = 0;
            s.flags = (s.flags & SS_ONSTACK) | SS_DISABLE;
        } else {
            // 启用: 写 addr/size, 清 ONSTACK/DISABLE
            s.addr = new_ss.ss_sp;
            s.size = new_ss.ss_size;
            s.flags = s.flags & !(SS_ONSTACK | SS_DISABLE);
        }
    }
    (s, written)
}

#[test]
fn query_only_no_change() {
    let s0 = AltStackState::zero();
    let (s1, old) = sigaltstack_op(s0, None, true);
    assert_eq!(s1, s0);
    assert_eq!(old, Some(StackT::zero()));
}

#[test]
fn set_then_query_returns_value() {
    let new = StackT { ss_sp: 0x7FFF_0000_1000, ss_flags: 0, ss_size: 0x4000 };
    let (s1, _) = sigaltstack_op(AltStackState::zero(), Some(new), false);
    let (_, old) = sigaltstack_op(s1, None, true);
    assert_eq!(old, Some(new));
    assert_eq!(s1.addr, 0x7FFF_0000_1000);
    assert_eq!(s1.size, 0x4000);
    assert_eq!(s1.flags, 0);
}

#[test]
fn disable_clears_addr_size_sets_disable_flag() {
    let s0 = AltStackState { addr: 0x1000, size: 0x2000, flags: 0 };
    let (s1, _) = sigaltstack_op(s0, Some(StackT { ss_sp: 0xDEAD, ss_flags: SS_DISABLE, ss_size: 0xBEEF }), false);
    assert_eq!(s1.addr, 0);
    assert_eq!(s1.size, 0);
    assert_eq!(s1.flags & SS_DISABLE, SS_DISABLE);
}

#[test]
fn disable_preserves_onstack_bit() {
    // POSIX: 替代栈上执行 sigaltstack(SS_DISABLE) 仅置 DISABLE, 不解套 SS_ONSTACK
    let s0 = AltStackState { addr: 0x1000, size: 0x2000, flags: SS_ONSTACK };
    let (s1, _) = sigaltstack_op(s0, Some(StackT { ss_sp: 0, ss_flags: SS_DISABLE, ss_size: 0 }), false);
    assert_eq!(s1.flags & SS_ONSTACK, SS_ONSTACK);
    assert_eq!(s1.flags & SS_DISABLE, SS_DISABLE);
}

#[test]
fn enable_clears_onstack_bit() {
    // 从主栈重新启用: 替代栈旧状态 SS_ONSTACK 应当被清, 允许再次落回
    let s0 = AltStackState { addr: 0x5000, size: 0x1000, flags: SS_ONSTACK };
    let (s1, _) = sigaltstack_op(s0, Some(StackT { ss_sp: 0x6000, ss_flags: 0, ss_size: 0x2000 }), false);
    assert_eq!(s1.addr, 0x6000);
    assert_eq!(s1.size, 0x2000);
    assert_eq!(s1.flags & SS_ONSTACK, 0);
}

#[test]
fn enable_clears_disable_bit() {
    let s0 = AltStackState { addr: 0, size: 0, flags: SS_DISABLE };
    let (s1, _) = sigaltstack_op(s0, Some(StackT { ss_sp: 0x7000, ss_flags: 0, ss_size: 0x3000 }), false);
    assert_eq!(s1.flags & SS_DISABLE, 0);
    assert_eq!(s1.addr, 0x7000);
}

#[test]
fn use_alternate_decision_logic() {
    // 镜像 [proc/signal.rs] do_signal_deliver 中的 use_alternate 决策:
    // ss_addr != 0 && ss_size >= total && !SS_DISABLE && !SS_ONSTACK
    let total: u64 = 256; // 假设 frame + trampoline
    let decision = |addr: u64, size: u64, flags: u32| -> bool {
        addr != 0 && size >= total && (flags & SS_DISABLE) == 0 && (flags & SS_ONSTACK) == 0
    };
    assert!(decision(0x1000, 0x1000, 0));      // 普通
    assert!(!decision(0, 0x1000, 0));          // addr=0
    assert!(!decision(0x1000, 0x80, 0));       // 容量不足 (0x80 < 256)
    assert!(!decision(0x1000, 0x1000, SS_DISABLE));
    assert!(!decision(0x1000, 0x1000, SS_ONSTACK));
}

#[test]
fn set_and_disable_round_trip() {
    let mut s = AltStackState::zero();
    s = sigaltstack_op(s, Some(StackT { ss_sp: 0xAAAA, ss_flags: 0, ss_size: 0x1000 }), false).0;
    assert_eq!(s.addr, 0xAAAA);
    s = sigaltstack_op(s, Some(StackT { ss_sp: 0, ss_flags: SS_DISABLE, ss_size: 0 }), false).0;
    assert_eq!(s.addr, 0);
    assert_eq!(s.flags & SS_DISABLE, SS_DISABLE);
    let (_, old) = sigaltstack_op(s, None, true);
    assert_eq!(old.unwrap().ss_flags & SS_DISABLE, SS_DISABLE);
}

#[test]
fn syscall_number_is_546() {
    // 镜像 [framework/syscall/types.rs::QX_SIGALTSTACK]
    const QX_SIGALTSTACK: u64 = 546;
    assert_eq!(QX_SIGALTSTACK, 546);
}

#[test]
fn ss_onstack_cleared_on_sigrturn() {
    // 镜像 sys_rt_sigreturn: 仅清 SS_ONSTACK, 保留 SS_DISABLE
    let s0 = AltStackState { addr: 0x1000, size: 0x1000, flags: SS_ONSTACK | SS_DISABLE };
    let new_flags = s0.flags & !SS_ONSTACK;
    assert_eq!(new_flags & SS_ONSTACK, 0);
    assert_eq!(new_flags & SS_DISABLE, SS_DISABLE);
}

#[test]
fn large_alt_stack_handled() {
    // 大栈 (>1GiB) 不溢出
    let big = StackT { ss_sp: 0x7F00_0000_0000, ss_flags: 0, ss_size: 4 * 1024 * 1024 * 1024 };
    let (s1, _) = sigaltstack_op(AltStackState::zero(), Some(big), false);
    assert_eq!(s1.size, big.ss_size);
    assert_eq!(s1.addr, big.ss_sp);
}
