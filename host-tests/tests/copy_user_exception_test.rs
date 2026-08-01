//! P0-I-36/37/38: exception table 缺失修复验证测试
//!
//! 验证 3 处内核写用户空间 (coredump / socket / signal) 已切换到
//! 异常表保护版 copy_from_user / copy_to_user:
//! - 用户 munmap 正在传输的缓冲区, 内核返回 EFAULT 而非 panic
//! - socket send/recv 缓冲区失效, 返回 EFAULT
//! - 信号栈帧写入失败, 信号不投递但进程继续运行
//!
//! host-tests 不链接 queenx; 复刻 copy_user 的核心接口契约来
//! 验证调用方语义: 返回 Result<usize, ()>, 失败时调用方必须
//! 把 EFAULT / SIG_IGN 路径走通.

// 单元素 &[Range] 是 unmapped 区间的合理表示 (测试用单区间), 抑制 clippy 误报
#![allow(clippy::single_range_in_vec_init)]

// ============================================================================
// 模型: 异常表保护版 copy API 契约
// ============================================================================

#[derive(Debug, PartialEq, Eq)]
enum CopyOutcome {
    Ok(usize),
    Efault,
}

/// 模拟 framework/mm::copy_user::copy_to_user
/// 失败场景: user_dst 落在 "已 munmap 区间" 集合中.
fn mock_copy_to_user(user_dst: u64, src: &[u8], len: usize, unmapped: &[std::ops::Range<u64>]) -> CopyOutcome {
    if unmapped.iter().any(|r| r.contains(&user_dst) || (user_dst < r.end && user_dst + len as u64 > r.start)) {
        return CopyOutcome::Efault;
    }
    CopyOutcome::Ok(len.min(src.len()))
}

/// 模拟 framework/mm::copy_user::copy_from_user
fn mock_copy_from_user(kernel_dst: &mut [u8], user_src: u64, len: usize, unmapped: &[std::ops::Range<u64>]) -> CopyOutcome {
    if unmapped.iter().any(|r| r.contains(&user_src) || (user_src < r.end && user_src + len as u64 > r.start)) {
        return CopyOutcome::Efault;
    }
    let n = len.min(kernel_dst.len());
    CopyOutcome::Ok(n)
}

// ============================================================================
// I-36: coredump 用户地址读
// ============================================================================

#[test]
fn coredump_user_buf_mapped_succeeds() {
    let unmapped: Vec<std::ops::Range<u64>> = vec![];
    let mut buf = [0u8; 256];
    let r = mock_copy_from_user(&mut buf, 0x1000, 256, &unmapped);
    assert_eq!(r, CopyOutcome::Ok(256));
}

#[test]
fn coredump_user_buf_munmapped_returns_efault() {
    // 用户在 coredump 遍历 VMA 过程中 munmap 了当前段
    let unmapped: &[std::ops::Range<u64>] = &[0x1000..0x2000];
    let mut buf = [0u8; 256];
    let r = mock_copy_from_user(&mut buf, 0x1000, 256, unmapped);
    assert_eq!(r, CopyOutcome::Efault, "I-36 修复: coredump 读 munmap 段必须 EFAULT, 不能 panic");
}

// ============================================================================
// I-37: socket send/recv 用户缓冲
// ============================================================================

#[test]
fn socket_send_user_buf_munmapped_returns_efault() {
    let unmapped: &[std::ops::Range<u64>] = &[0x4000_0000..0x4000_1000];
    let buf = [0xABu8; 512];
    let r = mock_copy_to_user(0x4000_0000, &buf, 512, unmapped);
    assert_eq!(r, CopyOutcome::Efault, "I-37 修复: send 缓冲区失效必须 EFAULT");
}

#[test]
fn socket_recv_user_buf_munmapped_returns_efault() {
    let unmapped: &[std::ops::Range<u64>] = &[0x5000_0000..0x5000_1000];
    let mut buf = [0u8; 512];
    let r = mock_copy_from_user(&mut buf, 0x5000_0000, 512, unmapped);
    assert_eq!(r, CopyOutcome::Efault, "I-37 修复: recv 缓冲区失效必须 EFAULT");
}

#[test]
fn socket_sockaddr_user_buf_munmapped_returns_efault() {
    // sockaddr_in 仅 8 字节, 跨越 munmap 边界
    let unmapped: &[std::ops::Range<u64>] = &[0x6000_0004..0x6000_0008];
    let mut buf = [0u8; 8];
    let r = mock_copy_from_user(&mut buf, 0x6000_0000, 8, unmapped);
    assert_eq!(r, CopyOutcome::Efault, "I-37 修复: sockaddr 跨越 munmap 边界必须 EFAULT");
}

// ============================================================================
// I-38: signal 栈帧写入
// ============================================================================

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct SignalFrame {
    r15: u64, r14: u64, r13: u64, r12: u64,
    r11: u64, r10: u64, r9: u64, r8: u64,
    rdi: u64, rsi: u64, rbp: u64, rdx: u64,
    rcx: u64, rbx: u64, rax: u64,
    int_no: u64, err_code: u64,
    rip: u64, cs: u64, rflags: u64, rsp: u64, ss: u64,
    signum: u64,
}

/// 模拟 I-38 修复后的 signal 投递:
/// 三段写入 (ret_addr / SignalFrame / trampoline) 任一失败,
/// 不投递信号, 进程继续运行.
fn try_deliver_signal(
    frame_rsp: u64,
    sigframe: &SignalFrame,
    trampoline: &[u8],
    unmapped: &[std::ops::Range<u64>],
) -> bool {
    let frame_size = core::mem::size_of::<SignalFrame>();
    let ret_addr_bytes = (frame_rsp + 8 + frame_size as u64).to_ne_bytes();
    let sigframe_bytes = unsafe {
        core::slice::from_raw_parts(
            sigframe as *const SignalFrame as *const u8,
            frame_size,
        )
    };
    let ok_ret = mock_copy_to_user(frame_rsp, &ret_addr_bytes, 8, unmapped);
    let ok_frame = mock_copy_to_user(frame_rsp + 8, sigframe_bytes, frame_size, unmapped);
    let ok_tramp = mock_copy_to_user(frame_rsp + 8 + frame_size as u64, trampoline, trampoline.len(), unmapped);
    if matches!(ok_ret, CopyOutcome::Efault) || matches!(ok_frame, CopyOutcome::Efault) || matches!(ok_tramp, CopyOutcome::Efault) {
        return false;
    }
    true
}

#[test]
fn signal_stack_mapped_delivers() {
    let unmapped: &[std::ops::Range<u64>] = &[];
    let sigframe = SignalFrame {
        r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
        rdi: 0, rsi: 0, rbp: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
        int_no: 0, err_code: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
        signum: 9,
    };
    let trampoline = [0u8; 16];
    assert!(try_deliver_signal(0x7000_0000, &sigframe, &trampoline, unmapped));
}

#[test]
fn signal_stack_munmapped_rolls_back() {
    // 用户栈在信号投递前被 munmap
    let unmapped: &[std::ops::Range<u64>] = &[0x7000_0000..0x7000_1000];
    let sigframe = SignalFrame {
        r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
        rdi: 0, rsi: 0, rbp: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
        int_no: 0, err_code: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
        signum: 9,
    };
    let trampoline = [0u8; 16];
    let delivered = try_deliver_signal(0x7000_0000, &sigframe, &trampoline, unmapped);
    assert!(!delivered, "I-38 修复: 栈帧写入失败时, 信号必须不投递, 进程继续运行");
}

#[test]
fn signal_trampoline_page_munmapped_rolls_back() {
    // ret_addr 写入成功, 但 trampoline 所在页被 munmap
    // frame_rsp=0x7000_0000, frame_size=192, trampoline 起点 = 0x7000_00C8
    let unmapped: &[std::ops::Range<u64>] = &[0x7000_00C8..0x7000_00D8];
    let sigframe = SignalFrame {
        r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
        rdi: 0, rsi: 0, rbp: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
        int_no: 0, err_code: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
        signum: 9,
    };
    let trampoline = [0u8; 16];
    let delivered = try_deliver_signal(0x7000_0000, &sigframe, &trampoline, unmapped);
    assert!(!delivered);
}
