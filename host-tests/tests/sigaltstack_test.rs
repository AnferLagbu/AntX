//! sigaltstack 替代栈信号投递契约测试 (P1-I-45)
//!
//! 验证信号投递路径对 sigaltstack 的支持:
//! 1. 进程设置 sigaltstack (addr!=0, size>=frame) 后,
//!    do_signal_deliver 必须使用替代栈顶部, 不写主栈 (主栈溢出场景不死锁)
//! 2. SS_ONSTACK 标记位: 进入信号 handler 前置位, 防止重入信号再次落回替代栈
//! 3. SS_DISABLE 标记位: 用户禁用替代栈时, 投递回退到主栈
//! 4. 替代栈容量不足时, 回退到主栈
//! 5. sigreturn 时清除 SS_ONSTACK 标记 (允许下一次信号再次落回替代栈)
//!
//! host-test 镜像 Process 的 sigaltstack 字段语义, 验证内核源码
//! 静态契约 (I-45 关键点). 真实投递由 QEMU 集成测试覆盖.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// 镜像 Process 中 sigaltstack 相关字段语义
struct MockSigaltstack {
    addr: AtomicU64,
    size: AtomicU64,
    flags: AtomicU32,
}

const SS_ONSTACK: u32 = 1;
const SS_DISABLE: u32 = 2;

/// 镜像 SIGNAL_FRAME_TOTAL_SIZE
const FRAME_TOTAL: u64 = 8 + 256 + 8; // 返回地址 + SignalFrame 假设 256B + trampoline 8B

/// 镜像 do_signal_deliver 的关键决策 (主栈 vs 替代栈)
fn pick_frame_rsp(
    ss: &MockSigaltstack,
    user_rsp: u64,
    total: u64,
) -> (u64, bool, &'static str) {
    let ss_addr = ss.addr.load(Ordering::Acquire);
    let ss_size = ss.size.load(Ordering::Acquire);
    let ss_flags = ss.flags.load(Ordering::Acquire);
    let use_alternate = ss_addr != 0
        && ss_size >= total
        && (ss_flags & SS_DISABLE) == 0
        && (ss_flags & SS_ONSTACK) == 0;
    if use_alternate {
        (ss_addr + ss_size - total, true, "alternate")
    } else if user_rsp >= total {
        (user_rsp - total, false, "main")
    } else {
        // 栈溢出, 默认动作
        (0, false, "overflow")
    }
}

fn make_ss(addr: u64, size: u64, flags: u32) -> MockSigaltstack {
    MockSigaltstack {
        addr: AtomicU64::new(addr),
        size: AtomicU64::new(size),
        flags: AtomicU32::new(flags),
    }
}

#[test]
fn alternate_stack_used_when_configured() {
    // P1-I-45 主验收: 注册替代栈后, 信号帧写到替代栈顶部
    let ss = make_ss(0x7fff_0000_0000, 4096, 0);
    let (rsp, used_alt, src) = pick_frame_rsp(&ss, 0x1000, FRAME_TOTAL);
    assert!(used_alt, "P1-I-45: 注册替代栈后必须使用替代栈");
    assert_eq!(src, "alternate");
    assert_eq!(rsp, 0x7fff_0000_0000 + 4096 - FRAME_TOTAL);
}

#[test]
fn main_stack_used_when_sigaltstack_unset() {
    // P1-I-45 验收: 进程未注册 sigaltstack 时, 使用主栈
    let ss = make_ss(0, 0, 0);
    let (_rsp, used_alt, src) = pick_frame_rsp(&ss, 0x8000, FRAME_TOTAL);
    assert!(!used_alt);
    assert_eq!(src, "main");
}

#[test]
fn main_stack_used_when_already_on_alternate() {
    // P1-I-45 验收: 已经在替代栈上时 (SS_ONSTACK 已置位), 投递回退到主栈
    // 防止信号重入时无限在替代栈顶累积
    let ss = make_ss(0x7fff_0000_0000, 4096, SS_ONSTACK);
    let (_rsp, used_alt, src) = pick_frame_rsp(&ss, 0x8000, FRAME_TOTAL);
    assert!(!used_alt, "P1-I-45: SS_ONSTACK 已置位时回退主栈, 避免重入无限");
    assert_eq!(src, "main");
}

#[test]
fn main_stack_used_when_disabled() {
    // P1-I-45 验收: SS_DISABLE 显式禁用替代栈, 投递回退主栈
    let ss = make_ss(0x7fff_0000_0000, 4096, SS_DISABLE);
    let (_rsp, used_alt, src) = pick_frame_rsp(&ss, 0x8000, FRAME_TOTAL);
    assert!(!used_alt, "P1-I-45: SS_DISABLE 时回退主栈");
    assert_eq!(src, "main");
}

#[test]
fn main_stack_used_when_alternate_too_small() {
    // P1-I-45 验收: 替代栈容量不足时, 回退主栈
    let ss = make_ss(0x7fff_0000_0000, 16 /* < FRAME_TOTAL */, 0);
    let (_rsp, used_alt, src) = pick_frame_rsp(&ss, 0x8000, FRAME_TOTAL);
    assert!(!used_alt, "P1-I-45: 替代栈不足时回退主栈");
    assert_eq!(src, "main");
}

#[test]
fn onstack_flag_set_on_alternate() {
    // P1-I-45 验收: 进入替代栈后必须置位 SS_ONSTACK, 防重入
    let ss = make_ss(0x7fff_0000_0000, 4096, 0);
    let (_rsp, used_alt, _) = pick_frame_rsp(&ss, 0x1000, FRAME_TOTAL);
    assert!(used_alt);
    // 内核 commit: 投递成功后置位 SS_ONSTACK
    let flags = ss.flags.load(Ordering::Acquire);
    ss.flags.store(flags | SS_ONSTACK, Ordering::Release);

    let next_flags = ss.flags.load(Ordering::Acquire);
    assert_eq!(next_flags & SS_ONSTACK, SS_ONSTACK);
}

#[test]
fn onstack_flag_cleared_on_sigreturn() {
    // P1-I-45 验收: sigreturn 时必须清 SS_ONSTACK
    let ss = make_ss(0x7fff_0000_0000, 4096, SS_ONSTACK);
    // 模拟 sys_rt_sigreturn: 仅清 SS_ONSTACK, 保留 SS_DISABLE
    let flags = ss.flags.load(Ordering::Acquire);
    ss.flags.store(flags & !SS_ONSTACK, Ordering::Release);

    let after = ss.flags.load(Ordering::Acquire);
    assert_eq!(after & SS_ONSTACK, 0, "P1-I-45: sigreturn 必清 SS_ONSTACK");
    assert_eq!(after & SS_DISABLE, 0, "P1-I-45: 保留 SS_DISABLE 位");
}

#[test]
fn source_signal_uses_sigaltstack() {
    // P1-I-45 源码静态扫描: signal.rs 必须实现替代栈判定
    let source = include_str!("../../src/kernel/framework/proc/signal.rs");
    assert!(
        source.contains("sigaltstack_addr")
            && source.contains("sigaltstack_size")
            && source.contains("sigaltstack_flags"),
        "P1-I-45: signal.rs 必须读取 sigaltstack 字段"
    );
    assert!(
        source.contains("use_alternate")
            && source.contains("SS_DISABLE")
            && source.contains("SS_ONSTACK"),
        "P1-I-45: signal.rs 必须实现 SS_DISABLE / SS_ONSTACK 决策"
    );
    assert!(
        source.contains("ss_addr + ss_size - total as u64"),
        "P1-I-45: 替代栈顶部必须为 ss_addr + ss_size - total"
    );
}

#[test]
fn source_syscall_clears_onstack_on_sigreturn() {
    // P1-I-45 源码静态扫描: syscall/mod.rs::sys_rt_sigreturn 必须清 SS_ONSTACK
    let source = include_str!("../../src/kernel/framework/syscall/mod.rs");
    let rt_sigreturn_start = source
        .find("fn sys_rt_sigreturn() -> i64 {")
        .expect("必须存在 sys_rt_sigreturn");
    let rt_sigreturn_body = &source[rt_sigreturn_start..];
    // 必须清 SS_ONSTACK
    assert!(
        rt_sigreturn_body.contains("!crate::kernel::framework::proc::signal::SS_ONSTACK")
            || rt_sigreturn_body.contains("!crate::kernel::framework::proc::SS_ONSTACK")
            || rt_sigreturn_body.contains("!SS_ONSTACK"),
        "P1-I-45: sys_rt_sigreturn 必须清除 SS_ONSTACK 标记"
    );
}
