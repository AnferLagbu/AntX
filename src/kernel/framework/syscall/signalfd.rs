//! signalfd — 信号接收文件描述符 (TCB)
//!
//! 实现 Linux signalfd API: signalfd / signalfd4.
//!
//! ## 架构
//!
//! ```text
//! SignalFdTable (全局, IrqSpinLock 保护)
//!   └── [SignalFdSlot; SFD_MAX_SLOTS]
//!         ├── sigmask: u128     (信号掩码, bit N = 信号 N+1)
//!         ├── pid: u32          (绑定进程 PID)
//!         └── used: bool
//!
//! FD 空间: [1120, 1120 + SFD_MAX_SLOTS)
//!
//! read(fd): 检查当前进程 pending & sigmask, 取最低编号信号,
//!           构造 signalfd_siginfo 写入用户空间, 消费该信号
//!           无信号 → EAGAIN
//!
//! epoll 集成: pending & sigmask != 0 → EPOLLIN
//! ```
//!
//! # Safety
//!
//! - SignalFdTable 由 IrqSpinLock 保护
//! - 信号消费操作与 signal 投递路径共享进程 pending 位图,
//!   通过 IrqSpinLock 保证互斥
//! - signalfd_siginfo 写入用户空间前由 services 层校验指针

use core::sync::atomic::{AtomicU32, Ordering};

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock as Mutex;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 常量
// ============================================================================

/// signalfd 最大实例数
pub const SFD_MAX_SLOTS: usize = 16;
/// FD 空间起始
/// TD-02: 基址来源已迁移至 `framework::proc::fd_alloc::FdPlan::SIGNAL_FD` 单一来源, 不再硬编码.
pub const SFD_FD_BASE: i32 = crate::kernel::framework::proc::fd_alloc::FdPlan::SIGNAL_FD.base;
/// SFD_CLOEXEC
pub const SFD_CLOEXEC: i32 = 0o2000000;
/// SFD_NONBLOCK
pub const SFD_NONBLOCK: i32 = 0o4000;

/// signalfd_siginfo 大小 (128 字节, 与 Linux 一致)
pub const SIGNALFD_SIGINFO_SIZE: usize = 128;

// ============================================================================
// signalfd_siginfo 布局 (与 Linux 兼容)
// ============================================================================

/// signalfd_siginfo — 传递给用户空间的信号信息
///
/// 布局与 Linux `struct signalfd_siginfo` 兼容 (128 字节)
#[repr(C)]
pub struct SignalFdSigInfo {
    pub ssi_signo: u32,       // 0: 信号编号
    pub ssi_errno: i32,       // 4: 错误码 (通常 0)
    pub ssi_code: i32,        // 8: 信号来源码
    pub ssi_pid: u32,         // 12: 发送者 PID
    pub ssi_uid: u32,         // 16: 发送者 UID
    pub ssi_fd: i32,          // 20: 文件描述符 (SIGIO)
    pub ssi_band: u32,        // 24: 带宽事件 (SIGIO)
    pub ssi_tid: u32,         // 28: 定时器 ID (SIGEV_THREAD_ID)
    pub ssi_overrun: u32,     // 32: 定时器溢出计数
    pub ssi_trapno: u32,      // 36: 陷阱号
    pub ssi_status: i32,      // 40: 退出状态 / 信号码
    pub ssi_int: i32,         // 44: POSIX.1b 信号值 (int)
    pub ssi_ptr: u64,         // 48: POSIX.1b 信号值 (ptr)
    pub ssi_utime: u64,       // 56: 用户 CPU 时间
    pub ssi_stime: u64,       // 64: 系统 CPU 时间
    pub ssi_addr: u64,        // 72: 触发地址
    pub _pad: [u8; 48],       // 80-127: 填充到 128 字节
}

impl SignalFdSigInfo {
    /// 创建零值 signalfd_siginfo
    pub const fn zeroed() -> Self {
        Self {
            ssi_signo: 0,
            ssi_errno: 0,
            ssi_code: 0,
            ssi_pid: 0,
            ssi_uid: 0,
            ssi_fd: 0,
            ssi_band: 0,
            ssi_tid: 0,
            ssi_overrun: 0,
            ssi_trapno: 0,
            ssi_status: 0,
            ssi_int: 0,
            ssi_ptr: 0,
            ssi_utime: 0,
            ssi_stime: 0,
            ssi_addr: 0,
            _pad: [0u8; 48],
        }
    }
}

// ============================================================================
// 数据结构
// ============================================================================

/// signalfd 槽位
struct SignalFdSlot {
    /// 信号掩码 (bit N 代表信号 N+1, 即 bit 0 = SIGHUP)
    sigmask: u128,
    /// 绑定进程 PID
    pid: u32,
    /// 是否已使用
    used: bool,
}

impl SignalFdSlot {
    const fn new() -> Self {
        Self {
            sigmask: 0,
            pid: 0,
            used: false,
        }
    }
}

/// signalfd 全局表
struct SignalFdTable {
    slots: [SignalFdSlot; SFD_MAX_SLOTS],
}

impl SignalFdTable {
    const fn new() -> Self {
        Self {
            slots: [const { SignalFdSlot::new() }; SFD_MAX_SLOTS],
        }
    }
}

/// 全局 signalfd 表
static SFD_TABLE: Mutex<SignalFdTable> = Mutex::new(SignalFdTable::new());

/// 已分配的 signalfd 数量
static SFD_COUNT: AtomicU32 = AtomicU32::new(0);

// ============================================================================
// 系统调用实现
// ============================================================================

/// signalfd — 创建/修改 signalfd 实例
///
/// `fd`: -1 创建新实例, ≥ SFD_FD_BASE 修改已有实例的掩码
/// `mask_ptr`: 指向 u128 信号掩码的用户空间指针
/// `flags`: SFD_CLOEXEC | SFD_NONBLOCK
/// 返回 fd (≥ 220), 或负 errno
pub fn sys_signalfd(fd: i32, mask_ptr: u64, flags: i32) -> i64 {
    // flags 校验
    let valid_flags = SFD_CLOEXEC | SFD_NONBLOCK;
    if flags & !valid_flags != 0 {
        return Errno::EINVAL.as_ret();
    }

    // 读取用户空间掩码
    if mask_ptr == 0 {
        return Errno::EFAULT.as_ret();
    }
    // SAFETY: mask_ptr 由 syscall 入口验证
    let sigmask = unsafe { core::ptr::read(mask_ptr as *const u128) };

    // 屏蔽 SIGKILL (9) 和 SIGSTOP (19): bit 8 和 bit 18
    let sigmask = sigmask & !((1u128 << 8) | (1u128 << 18));

    // 获取当前 PID
    let current_pid = crate::kernel::framework::proc::scheduler::SCHEDULER
        .current()
        .unwrap_or(0) as u32;
    if current_pid == 0 {
        return Errno::EINVAL.as_ret();
    }

    let mut table = SFD_TABLE.lock();

    if fd >= SFD_FD_BASE {
        // 修改已有实例
        let idx = (fd - SFD_FD_BASE) as usize;
        if idx >= SFD_MAX_SLOTS || !table.slots[idx].used {
            return Errno::EBADF.as_ret();
        }
        table.slots[idx].sigmask = sigmask;
        crate::klog_debug!(Sync, "[signalfd] Update fd={} mask=0x{:X}", fd, sigmask);
        return fd as i64;
    }

    if fd != -1 {
        return Errno::EINVAL.as_ret();
    }

    // 创建新实例
    for i in 0..SFD_MAX_SLOTS {
        if !table.slots[i].used {
            table.slots[i].used = true;
            table.slots[i].sigmask = sigmask;
            table.slots[i].pid = current_pid;
            SFD_COUNT.fetch_add(1, Ordering::Relaxed);

            // TD-02 V3: 通过 fd_alloc 集中计算 FD 编号
            let new_fd = crate::kernel::framework::proc::fd_alloc::fd_at(
                crate::kernel::framework::proc::fd_alloc::FdSubsystem::SignalFd,
                i,
            );
            crate::klog_debug!(Sync, "[signalfd] Created fd={} pid={}", new_fd, current_pid);
            return new_fd as i64;
        }
    }

    Errno::EMFILE.as_ret()
}

/// signalfd read — 读取一个待处理信号
///
/// 检查当前进程 pending & sigmask, 取最低编号信号,
/// 构造 signalfd_siginfo 写入用户空间, 并消费该信号.
///
/// `fd`: signalfd 文件描述符
/// `buf`: 用户空间缓冲区 (至少 128 字节)
/// 返回 128 (成功), 或负 errno
pub fn sys_signalfd_read(fd: i32, buf: u64) -> i64 {
    let idx = match fd_to_idx(fd) {
        Some(i) => i,
        None => return Errno::EBADF.as_ret(),
    };

    let current_pid = crate::kernel::framework::proc::scheduler::SCHEDULER
        .current()
        .unwrap_or(0) as u32;
    if current_pid == 0 {
        return Errno::EINVAL.as_ret();
    }

    // 获取 sigmask 并检查 pending
    let sigmask = {
        let table = SFD_TABLE.lock();
        let slot = &table.slots[idx];
        if !slot.used || slot.pid != current_pid {
            return Errno::EBADF.as_ret();
        }
        slot.sigmask
    };

    // 读取当前进程的 pending 信号
    let pending = get_process_pending(current_pid);
    let ready = pending & sigmask;

    if ready == 0 {
        return Errno::EAGAIN.as_ret();
    }

    // 取最低编号信号 (trailing zeros + 1)
    let signo = ready.trailing_zeros() + 1;

    // 消费该信号 (从 pending 中清除)
    clear_process_pending(current_pid, signo);

    // 构造 signalfd_siginfo
    let mut info = SignalFdSigInfo::zeroed();
    info.ssi_signo = signo;
    info.ssi_code = 0; // SI_KERNEL 简化
    info.ssi_pid = 0;  // 发送者 PID (简化, 暂不追踪)

    // 写入用户空间
    if buf == 0 {
        return Errno::EFAULT.as_ret();
    }
    // SAFETY: buf 由 syscall 入口验证, 大小足够
    unsafe {
        core::ptr::write(buf as *mut SignalFdSigInfo, info);
    }

    crate::klog_debug!(Sync, "[signalfd] Read fd={} signo={}", fd, signo);
    SIGNALFD_SIGINFO_SIZE as i64
}

/// signalfd close — 关闭 signalfd
pub fn sys_signalfd_close(fd: i32) -> i64 {
    let idx = match fd_to_idx(fd) {
        Some(i) => i,
        None => return Errno::EBADF.as_ret(),
    };

    let mut table = SFD_TABLE.lock();
    let slot = &mut table.slots[idx];

    if !slot.used {
        return Errno::EBADF.as_ret();
    }

    slot.used = false;
    slot.sigmask = 0;
    slot.pid = 0;
    SFD_COUNT.fetch_sub(1, Ordering::Relaxed);

    crate::klog_debug!(Sync, "[signalfd] Closed fd={}", fd);
    0
}

// ============================================================================
// epoll 集成
// ============================================================================

/// 检查 signalfd 是否就绪 (供 epoll check_fd_ready 调用)
///
/// 返回 EPOLLIN (有待处理信号) 或 0
pub fn signalfd_poll_events(fd: i32) -> u32 {
    use crate::kernel::framework::syscall::epoll::{EPOLLIN, EPOLLERR};

    let idx = match fd_to_idx(fd) {
        Some(i) => i,
        None => return EPOLLERR,
    };

    let current_pid = crate::kernel::framework::proc::scheduler::SCHEDULER
        .current()
        .unwrap_or(0) as u32;

    let table = SFD_TABLE.lock();
    let slot = &table.slots[idx];

    if !slot.used || slot.pid != current_pid {
        return EPOLLERR;
    }

    let pending = get_process_pending(current_pid);
    if pending & slot.sigmask != 0 {
        EPOLLIN
    } else {
        0
    }
}

// ============================================================================
// 进程信号状态访问 (桥接 proc::signal)
// ============================================================================

/// 获取进程 pending 信号位图
fn get_process_pending(pid: u32) -> u128 {
    use crate::kernel::framework::proc::process::PROCESS_TABLE;

    PROCESS_TABLE
        .with_process(pid as u32, |proc| proc.signal_pending_get() as u128)
        .unwrap_or(0)
}

/// 清除进程指定信号的 pending 位
fn clear_process_pending(pid: u32, signo: u32) {
    use crate::kernel::framework::proc::process::PROCESS_TABLE;

    let bit = 1u64 << (signo - 1);
    PROCESS_TABLE.with_process(pid as u32, |proc| {
        proc.signal_pending_clear(bit);
    });
}

// ============================================================================
// 辅助函数
// ============================================================================

/// fd → 槽位索引
fn fd_to_idx(fd: i32) -> Option<usize> {
    if fd < SFD_FD_BASE || fd >= SFD_FD_BASE + SFD_MAX_SLOTS as i32 {
        return None;
    }
    Some((fd - SFD_FD_BASE) as usize)
}

/// 检查 fd 是否属于 signalfd 空间
pub fn is_signalfd_fd(fd: i32) -> bool {
    fd >= SFD_FD_BASE && fd < SFD_FD_BASE + SFD_MAX_SLOTS as i32
}

// ============================================================================
// 内核测试
// ============================================================================

#[cfg(feature = "kernel_test")]
fn test_signalfd_create() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};

    // 创建 signalfd, 掩码 = SIGUSR1 (bit 9) | SIGUSR2 (bit 30)
    let mask: u128 = (1u128 << 9) | (1u128 << 30);
    let fd = sys_signalfd(-1, &mask as *const u128 as u64, 0);
    check!(fd >= 220, "signalfd returns fd >= 220");

    // 关闭
    let ret = sys_signalfd_close(fd as i32);
    check!(ret == 0, "signalfd close returns 0");

    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_signalfd_mask_update() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};

    let mask1: u128 = 1u128 << 9; // SIGUSR1
    let fd = sys_signalfd(-1, &mask1 as *const u128 as u64, 0);
    check!(fd >= 220, "signalfd create ok");

    // 更新掩码
    let mask2: u128 = 1u128 << 30; // SIGUSR2
    let ret = sys_signalfd(fd as i32, &mask2 as *const u128 as u64, 0);
    check!(ret == fd, "signalfd update returns same fd");

    sys_signalfd_close(fd as i32);
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_signalfd_sigkill_filtered() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};

    // 尝试注册 SIGKILL (bit 8) + SIGUSR1 (bit 9)
    let mask: u128 = (1u128 << 8) | (1u128 << 9);
    let fd = sys_signalfd(-1, &mask as *const u128 as u64, 0);
    check!(fd >= 220, "signalfd with SIGKILL creates ok");

    // 验证 SIGKILL 被过滤: 读取 slot 的 sigmask
    {
        let table = SFD_TABLE.lock();
        let idx = (fd as i32 - SFD_FD_BASE) as usize;
        let slot_mask = table.slots[idx].sigmask;
        check!(slot_mask & (1u128 << 8) == 0, "SIGKILL filtered from mask");
        check!(slot_mask & (1u128 << 9) != 0, "SIGUSR1 still in mask");
    }

    sys_signalfd_close(fd as i32);
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
pub fn register_signalfd_tests() {
    use crate::kernel::framework::tests::runner;
    let r = runner();
    r.register("signalfd", "create", test_signalfd_create);
    r.register("signalfd", "mask_update", test_signalfd_mask_update);
    r.register("signalfd", "sigkill_filtered", test_signalfd_sigkill_filtered);
}
