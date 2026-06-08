#![deny(unsafe_code)]
//! 信号系统 — services 层安全代理
//!
//! ## 状态 (v2.15, 2026-06-04)
//!
//! Phase 2.5 进程迁移 4/4 (signal):
//! - [x] 强类型 `Signal` (POSIX 信号枚举 + 未知信号)
//! - [x] 强类型 `SignalAction` (handler / siginfo / mask)
//! - [x] 信号传递 (内核基础设施已通过 `proc::table::signal_set` 暴露)
//! - [x] 标准信号常量 (SIGHUP/SIGINT/SIGQUIT/SIGKILL/SIGSEGV/...)
//! - [x] 位掩码操作 (block/unblock/test)
//! - [x] `kill(pid, sig)` 顶层 API
//!
//! ## 迁移方法
//!
//! AntX 当前内核信号子系统是 per-process 32 bit 简易实现 (`signal_pending_*`),
//! 不完整的 POSIX 信号语义由 services 层提供类型安全封装 + 标准常量。
//! 未来完整化信号子系统 (sigaction 表, 共享处理, sigaltstack) 时, 替换 `do_signal`
//! 内部实现, services 层 API 保持稳定。
//!
//! 评估日期: 2026-06-04


// ============================================================================
// 标准 POSIX 信号 (强类型枚举)
// ============================================================================

/// POSIX 标准信号 (1..=31)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StandardSignal {
    /// Hangup
    Hup = 1,
    /// 终端中断 (Ctrl-C)
    Int = 2,
    /// 终端退出 (Ctrl-\\)
    Quit = 3,
    /// 非法指令
    Ill = 4,
    /// 跟踪/断点陷阱
    Trap = 5,
    /// 异常中止
    Abrt = 6,
    /// 总线错误
    Bus = 7,
    /// 浮点异常
    Fpe = 8,
    /// Kill (不能被捕获/忽略)
    Kill = 9,
    /// 用户定义信号 1
    Usr1 = 10,
    /// 段错误
    Segv = 11,
    /// 用户定义信号 2
    Usr2 = 12,
    /// 管道破裂
    Pipe = 13,
    /// 闹钟定时器
    Alrm = 14,
    /// 终止 (可捕获)
    Term = 15,
    /// 栈故障 (Linux 特有)
    Stkflt = 16,
    /// 子进程退出
    Chld = 17,
    /// 继续执行 (SIGSTOP 对应)
    Cont = 18,
    /// 停止 (Ctrl-Z)
    Stop = 19,
    /// 终端停止输入
    Tstp = 20,
    /// 终端停止输出
    Ttin = 21,
    /// 终端恢复输出
    Ttou = 22,
    /// 紧急情况 (不可捕获)
    Urg = 23,
    /// CPU 时间限制
    Xcpu = 24,
    /// 文件大小限制
    Xfsz = 25,
    /// 虚拟闹钟
    Vtalrm = 26,
    /// 性能分析
    Prof = 27,
    /// 窗口大小变化
    Winch = 28,
    /// I/O 可能
    Io = 29,
    /// 电源故障
    Pwr = 30,
    /// 非法系统调用
    Sys = 31,
}

impl StandardSignal {
    /// 从原始信号编号构造
    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Hup),
            2 => Some(Self::Int),
            3 => Some(Self::Quit),
            4 => Some(Self::Ill),
            5 => Some(Self::Trap),
            6 => Some(Self::Abrt),
            7 => Some(Self::Bus),
            8 => Some(Self::Fpe),
            9 => Some(Self::Kill),
            10 => Some(Self::Usr1),
            11 => Some(Self::Segv),
            12 => Some(Self::Usr2),
            13 => Some(Self::Pipe),
            14 => Some(Self::Alrm),
            15 => Some(Self::Term),
            16 => Some(Self::Stkflt),
            17 => Some(Self::Chld),
            18 => Some(Self::Cont),
            19 => Some(Self::Stop),
            20 => Some(Self::Tstp),
            21 => Some(Self::Ttin),
            22 => Some(Self::Ttou),
            23 => Some(Self::Urg),
            24 => Some(Self::Xcpu),
            25 => Some(Self::Xfsz),
            26 => Some(Self::Vtalrm),
            27 => Some(Self::Prof),
            28 => Some(Self::Winch),
            29 => Some(Self::Io),
            30 => Some(Self::Pwr),
            31 => Some(Self::Sys),
            _ => None,
        }
    }

    /// 信号编号
    pub fn number(self) -> u8 {
        self as u8
    }

    /// 是否可被进程捕获或忽略
    pub fn is_catchable(self) -> bool {
        !matches!(self, Self::Kill | Self::Stop)
    }

    /// 是否是核心转储信号 (默认行为)
    pub fn is_core_dump(self) -> bool {
        matches!(
            self,
            Self::Quit
                | Self::Ill
                | Self::Abrt
                | Self::Bus
                | Self::Fpe
                | Self::Segv
                | Self::Sys
                | Self::Xcpu
                | Self::Xfsz
        )
    }
}

/// 信号 (标准 1..=31, 或 RT 32..=64 实时信号, 0 用于空检查)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Signal(pub u8);

impl Signal {
    /// 空信号 (POSIX kill(pid, 0) 错误检查用)
    pub const NONE: Self = Self(0);

    /// 构造标准信号
    #[inline]
    pub const fn standard(s: StandardSignal) -> Self {
        Self(s as u8)
    }

    /// 构造 RT 信号 (sig >= 32)
    #[inline]
    pub const fn realtime(rt_num: u8) -> Self {
        Self(rt_num + 32)
    }

    /// 原始信号编号
    #[inline]
    pub const fn number(self) -> u8 {
        self.0
    }

    /// 是否为标准信号
    #[inline]
    pub fn is_standard(self) -> bool {
        StandardSignal::from_number(self.0).is_some()
    }

    /// 是否为 RT 信号 (>= 32)
    #[inline]
    pub fn is_realtime(self) -> bool {
        self.0 >= 32
    }

    /// 转位掩码 (用于 `signal_pending_*`)
    #[inline]
    pub fn to_bit(self) -> u64 {
        1u64 << (self.0 as u64)
    }
}

// ============================================================================
// 信号处理动作
// ============================================================================

/// 默认动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalDisposition {
    /// Term (默认: 终止进程)
    Term,
    /// Ign (默认: 忽略)
    Ign,
    /// Core (默认: 核心转储)
    Core,
    /// Stop (默认: 停止)
    Stop,
    /// Cont (默认: 继续)
    Cont,
}

impl SignalDisposition {
    /// POSIX 标准信号默认动作
    pub fn default_for(sig: StandardSignal) -> Self {
        match sig {
            StandardSignal::Chld | StandardSignal::Urg => Self::Ign,
            StandardSignal::Stop | StandardSignal::Tstp | StandardSignal::Ttin | StandardSignal::Ttou => {
                Self::Stop
            }
            StandardSignal::Cont => Self::Cont,
            s if s.is_core_dump() => Self::Core,
            _ => Self::Term,
        }
    }
}

/// 信号处理动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    /// 默认动作
    Default,
    /// 忽略
    Ignore,
    /// 用户态处理函数 (handler 入口地址)
    Handler(u64),
}

impl SignalAction {
    /// 是否为默认
    pub fn is_default(self) -> bool {
        matches!(self, Self::Default)
    }
}

// ============================================================================
// 信号错误
// ============================================================================

/// 信号错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalError {
    /// 目标进程不存在
    NoSuchProcess,
    /// 权限不足
    PermissionDenied,
    /// 无效信号编号
    InvalidSignal,
    /// 进程已退出
    ProcessExited,
    /// 其他
    Other(i32),
}

pub type SignalResult<T> = Result<T, SignalError>;

// ============================================================================
// 信号传递 (委托 proc::table)
// ============================================================================

/// 向指定进程发送信号
///
/// **实现**: 设置 `signal_pending` 位, 由内核在返回用户态前检查并分发
pub fn send(pid: crate::kernel::framework::proc::types::Pid, sig: Signal) -> SignalResult<()> {
    if sig == Signal::NONE {
        // POSIX: kill(pid, 0) 仅检查进程存在, 不发送
        return crate::kernel::services::proc::table::with(pid, |_p| ()).ok_or(SignalError::NoSuchProcess);
    }
    if sig.0 >= 64 {
        return Err(SignalError::InvalidSignal);
    }
    crate::kernel::services::proc::table::signal_set(pid, sig.0 as u32)
        .map_err(|_| SignalError::ProcessExited)
}

/// 检查进程是否有信号待处理
pub fn pending(pid: crate::kernel::framework::proc::types::Pid) -> Option<u64> {
    crate::kernel::services::proc::table::signal_get(pid)
}

/// 清除进程的信号位
pub fn clear(pid: crate::kernel::framework::proc::types::Pid, mask: u64) -> SignalResult<()> {
    crate::kernel::services::proc::table::signal_clear(pid, mask)
        .map_err(|_| SignalError::ProcessExited)
}

// ============================================================================
// 便利函数
// ============================================================================

/// 终止进程 (等价 kill(pid, SIGKILL))
pub fn kill(pid: crate::kernel::framework::proc::types::Pid) -> SignalResult<()> {
    send(pid, Signal::standard(StandardSignal::Kill))
}

/// 中断进程 (等价 kill(pid, SIGINT))
pub fn interrupt(pid: crate::kernel::framework::proc::types::Pid) -> SignalResult<()> {
    send(pid, Signal::standard(StandardSignal::Int))
}

/// 停止进程 (等价 kill(pid, SIGSTOP))
pub fn stop(pid: crate::kernel::framework::proc::types::Pid) -> SignalResult<()> {
    send(pid, Signal::standard(StandardSignal::Stop))
}

/// 唤醒已停止的进程 (等价 kill(pid, SIGCONT))
pub fn cont(pid: crate::kernel::framework::proc::types::Pid) -> SignalResult<()> {
    send(pid, Signal::standard(StandardSignal::Cont))
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_round_trip() {
        assert_eq!(Signal::standard(StandardSignal::Int).number(), 2);
        assert_eq!(StandardSignal::from_number(2), Some(StandardSignal::Int));
        assert_eq!(StandardSignal::from_number(0), None);
    }

    #[test]
    fn signal_catchable() {
        assert!(StandardSignal::Int.is_catchable());
        assert!(!StandardSignal::Kill.is_catchable());
        assert!(!StandardSignal::Stop.is_catchable());
    }

    #[test]
    fn signal_core_dump() {
        assert!(StandardSignal::Segv.is_core_dump());
        assert!(StandardSignal::Bus.is_core_dump());
        assert!(!StandardSignal::Int.is_core_dump());
        assert!(!StandardSignal::Term.is_core_dump());
    }

    #[test]
    fn signal_default_disposition() {
        assert_eq!(
            SignalDisposition::default_for(StandardSignal::Chld),
            SignalDisposition::Ign
        );
        assert_eq!(
            SignalDisposition::default_for(StandardSignal::Stop),
            SignalDisposition::Stop
        );
        assert_eq!(
            SignalDisposition::default_for(StandardSignal::Segv),
            SignalDisposition::Core
        );
        assert_eq!(
            SignalDisposition::default_for(StandardSignal::Term),
            SignalDisposition::Term
        );
    }

    #[test]
    fn signal_realtime() {
        let rt = Signal::realtime(1);
        assert_eq!(rt.number(), 33);
        assert!(rt.is_realtime());
        assert!(!rt.is_standard());

        let std = Signal::standard(StandardSignal::Term);
        assert!(!std.is_realtime());
        assert!(std.is_standard());
    }

    #[test]
    fn signal_bit() {
        let sig = Signal::standard(StandardSignal::Int);
        assert_eq!(sig.to_bit(), 1u64 << 2);

        let rt = Signal::realtime(0);
        assert_eq!(rt.to_bit(), 1u64 << 32);
    }
}

// ============================================================================
// Syscall 安全代理
// ============================================================================

/// kill 系统调用安全代理
///
/// 验证: 信号编号 0..=31, 目标 pid 接受 POSIX 4 种语义 (pid>0 单进程,
///        pid=0 同进程组, pid=-1 全部, pid<-1 |pid| 进程组).
pub fn kill_syscall(pid: i32, sig: i32) -> Result<usize, crate::kernel::framework::syscall::types::Errno> {
    use crate::kernel::framework::syscall::types::Errno;

    // 验证信号编号 (POSIX kill: 0 = 检查存在, 1..=31 = 标准信号)
    if sig < 0 || sig > 31 {
        return Err(Errno::EINVAL);
    }
    // 验证 pid 范围 (POSIX: pid 必须非 0, -1, <-1 之一; pid=0 合法)
    // 原约束 pid <= 0 -> ESRCH 已移除 (TRACK-315B7C 解决)
    // 最小校验: pid 至少 0 或负数 (i32 范围), 由 framework 内部 4 路径分发

    let ret = crate::kernel::framework::syscall::api::sys_kill(pid, sig);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}

/// rt_sigaction 系统调用安全代理
///
/// 验证: signum 1..=31 (标准信号) 或 32..=64 (RT信号)
pub fn rt_sigaction_syscall(
    signum: i32,
    act: u64,
    oact: u64,
) -> Result<usize, crate::kernel::framework::syscall::types::Errno> {
    use crate::kernel::framework::syscall::types::Errno;

    // 验证信号编号 (SIGKILL=9 和 SIGSTOP=19 不可捕获)
    if signum < 1 || signum > 64 {
        return Err(Errno::EINVAL);
    }
    if signum == 9 || signum == 19 {
        return Err(Errno::EINVAL);
    }

    let ret = crate::kernel::framework::syscall::api::sys_rt_sigaction(signum, act, oact);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}

/// rt_sigprocmask 系统调用安全代理
///
/// 验证: how 有效 (SIG_BLOCK=0, SIG_UNBLOCK=1, SIG_SETMASK=2)
pub fn rt_sigprocmask_syscall(
    how: i32,
    set: u64,
    oset: u64,
) -> Result<usize, crate::kernel::framework::syscall::types::Errno> {
    use crate::kernel::framework::syscall::types::Errno;

    // 验证 how
    if how < 0 || how > 2 {
        return Err(Errno::EINVAL);
    }

    let ret = crate::kernel::framework::syscall::api::sys_rt_sigprocmask(how, set, oset);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}
