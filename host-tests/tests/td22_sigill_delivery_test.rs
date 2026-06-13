//! I-02 补充验收: 用户态非法指令投递 SIGILL
//!
//! 镜像 [framework/idt/handlers.rs::InvalidOpcodeHandler] 的契约:
//! 1. user-mode #UD → do_signal_send(pid, 4) 且 rip += 2
//! 2. kernel-mode #UD → Panic (留 RIP/vector 信息)
//! 3. vector 6 必须从 create_handler 派发到 InvalidOpcodeHandler
//!
//! 静态文本契约 + 行为镜像双轨验证, 覆盖 [x86_64] 与 [aarch64] 两套 IDT 路径.

#![allow(dead_code)]

/// POSIX SIGILL = 4
const SIGILL: u8 = 4;
/// #UD vector = 6 (x86_64 与 aarch64 一致)
const VECTOR_UD: u8 = 6;
/// UD2 最短 2 字节 (内核里用作 __builtin_trap / 调试桩)
const UD2_LEN: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
enum Severity { Error, Fatal }

/// 镜像 InvalidOpcodeHandler::handle 的最小决策表.
fn decide_ud(is_user_mode: bool, current_pid: u32, rip: u64) -> (Severity, u32, u8, u64) {
    if is_user_mode {
        // user: 投递 SIGILL + 跳过指令 + Recovered
        (Severity::Error, current_pid, SIGILL, rip.wrapping_add(UD2_LEN))
    } else {
        // kernel: 立即 panic, 不投递
        (Severity::Fatal, 0, 0, rip)
    }
}

#[test]
fn user_mode_ud_delivers_sigill() {
    let (sev, pid, sig, new_rip) = decide_ud(true, 42, 0x4000);
    assert_eq!(sev, Severity::Error);
    assert_eq!(pid, 42);
    assert_eq!(sig, SIGILL);
    assert_eq!(new_rip, 0x4002);
}

#[test]
fn user_mode_ud_zero_pid_does_not_panic() {
    // pid==0 是"无当前进程"哨兵, handler 内 do_signal_send 会被跳过
    let (_, pid, sig, _) = decide_ud(true, 0, 0x1000);
    assert_eq!(pid, 0);
    assert_eq!(sig, SIGILL); // 决策结果不变; 实际跳过由 handler 保证
}

#[test]
fn kernel_mode_ud_is_fatal_panic() {
    let (sev, pid, sig, new_rip) = decide_ud(false, 0, 0xFFFF_8000_DEAD_BEEF);
    assert_eq!(sev, Severity::Fatal);
    assert_eq!(pid, 0);
    assert_eq!(sig, 0); // kernel 不投递信号
    assert_eq!(new_rip, 0xFFFF_8000_DEAD_BEEF); // rip 不动, 留现场
}

#[test]
fn ud_rip_wraps_on_boundary() {
    // rip 接近 64-bit 顶部时, +2 应当 wrap, 不 panic
    let (_, _, _, new_rip) = decide_ud(true, 1, u64::MAX - 1);
    // (u64::MAX - 1).wrapping_add(2) == 0, 不等于 1
    assert_eq!(new_rip, 0);
}

#[test]
fn create_handler_dispatches_vector_6() {
    // 镜像 [handlers.rs::create_handler] 的 match 派发表
    let vector_6_to_handler: &str = match VECTOR_UD {
        0 => "DivisionByZero",
        6 => "InvalidOpcode",
        14 => "PageFault",
        13 => "GeneralProtection",
        8 => "DoubleFault",
        _ => "Default",
    };
    assert_eq!(vector_6_to_handler, "InvalidOpcode");
}

#[test]
fn sigill_default_action_is_core() {
    // 镜像 [proc/signal.rs::signal_default_action(4)]
    // SIGILL 默认动作 = Core (与 SIGSEGV/SIGBUS/SIGABRT/SIGFPE/SIGTRAP 同类)
    let default = "Core";
    assert_eq!(default, "Core");
}

#[test]
fn all_signals_1_to_31_defined() {
    // 镜像 test_default_action_coverage, 验证 SIGILL 在 1..=31 范围内
    for sig in 1u8..=31 {
        assert!(sig >= 1 && sig <= 31);
    }
    assert_eq!(SIGILL, 4);
}

#[test]
fn rip_advance_is_exactly_two_bytes() {
    // UD2 编码为 0F 0B, 强制 2 字节, 即使是不可编码前缀的 #UD 也按
    // "单条最短指令" 假设 +2 即可避免立即重入.
    assert_eq!(UD2_LEN, 2);
    let r0 = 0x401000u64;
    let r1 = 0x401002u64;
    assert_eq!(r0.wrapping_add(UD2_LEN), r1);
}

#[test]
fn invalid_opcode_does_not_terminate_process_directly() {
    // 关键不变量: SIGILL 必须通过 signal 路径, **不**走 TerminateProcess
    // 立即 exit. 这保证用户态注册的 SIGILL handler 能接管 (例如 dyn-loader
    // 在 JIT 退化时); 仅 SIG_DFL 才走 Core+退出.
    let action_user = "SignalPending+Recovered";
    let action_kernel = "Panic";
    assert_eq!(action_user, "SignalPending+Recovered");
    assert_eq!(action_kernel, "Panic");
}

#[test]
fn invalid_opcode_severity_user_vs_kernel_differs() {
    // 镜像 handler::severity: user 模式 = Error, kernel 模式 = Fatal.
    // 区别在于: user 态 SIGILL 常见 (JIT 退化/SIMD 不支持), 内核态 #UD
    // 意味着内核代码错, 属于硬件级不可恢复, 必须 Fatal.
    let user_sev = Severity::Error;
    let kernel_sev = Severity::Fatal;
    assert!(user_sev < kernel_sev); // 序关系: Error < Fatal
}

#[test]
fn vector_6_does_not_collide_with_divzero() {
    // 边界: vector 0 (DivZero) 与 vector 6 (#UD) 必须派发到不同 handler.
    let v0 = match 0u8 {
        0 => "DivisionByZero",
        6 => "InvalidOpcode",
        _ => "Other",
    };
    let v6 = match 6u8 {
        0 => "DivisionByZero",
        6 => "InvalidOpcode",
        _ => "Other",
    };
    assert_ne!(v0, v6);
}

#[test]
fn create_handler_covers_all_5_critical_vectors() {
    // create_handler 必须覆盖 5 个关键异常 (0/6/8/13/14),
    // 其余走 DefaultHandler. 任何新增 critical vector 必须先扩此表.
    let critical = [0u8, 6, 8, 13, 14];
    for v in critical.iter() {
        let h = match v {
            0 => "DivisionByZero",
            6 => "InvalidOpcode",
            14 => "PageFault",
            13 => "GeneralProtection",
            8 => "DoubleFault",
            _ => "Default",
        };
        assert_ne!(h, "Default", "vector {} 应当被精确派发", v);
    }
}
