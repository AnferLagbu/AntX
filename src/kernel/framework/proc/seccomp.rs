//! Seccomp — 系统调用过滤 (C7)
//!
//! 实现 Linux seccomp 子集: BPF 规则匹配 + 三种动作 (ALLOW/KILL/TRAP/ERRNO).
//!
//! ## 设计
//!
//! - **SeccompFilter**: per-process 过滤器链 (最多 4 层, 与 Linux 一致)
//! - **SeccompRule**: 单条规则 = (syscall_nr, arch_mask, arg_comparators) → action
//! - **匹配**: 从最外层到最内层, 首条匹配即终止; 无匹配则 ALLOW
//! - **动作**:
//!   - `Allow`: 放行
//!   - `KillThread` / `KillProcess`: 发 SIGSYS
//!   - `Trap`: 发 SIGSYS + siginfo
//!   - `Errno(e)`: 返回 -e
//!   - `Log`: 放行 + 审计日志
//! - **SeccompMode**: None / Strict (只允许 read/write/exit/sigreturn) / Filter
//!
//! ## 安全约束
//!
//! - `prctl(PR_SET_SECCOMP, ...)` 和 `seccomp(SECCOMP_SET_MODE_FILTER, ...)`
//!   需要 CAP_SYS_ADMIN 或设置 `no_new_privs` 位
//! - Strict 模式不可逆; Filter 模式只能追加不能删除
//! - fork 继承全部过滤器; execve 保留 (除非 no_new_privs 未设置)
//!
//! ## 与 Linux 差异
//!
//! - 不实现 cBPF 经典 BPF, 改用结构化规则 (更安全, 更易审计)
//! - 不实现 SECCOMP_GET_NOTIF_SIZES / SECCOMP_ADDFD (userfd 通知机制)
//! - SECCOMP_RET_TRACE 暂不支持 (ptrace 依赖)

use core::sync::atomic::{AtomicU8, Ordering};

use alloc::vec::Vec;

use crate::kernel::framework::proc::api::process_get_current_pid;
use crate::kernel::framework::proc::process::PROCESS_TABLE;
use crate::kernel::framework::proc::signal::do_signal_send;
use crate::kernel::framework::proc::types::Pid;
use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 常量
// ============================================================================

/// per-process 过滤器最大层数 (与 Linux SECCOMP_MAX_FILTERS 一致)
const MAX_FILTERS: usize = 4;

/// Strict 模式允许的 syscall 编号 (x86_64/aarch64 共用 QX 编号)
const STRICT_ALLOWED: &[u64] = &[
    502, // QX_READ
    503, // QX_WRITE
    501, // QX_EXIT
    525, // QX_EXIT_GROUP
    542, // QX_RT_SIGRETURN
];

// ============================================================================
// Seccomp 模式
// ============================================================================

/// Seccomp 模式 (对应 /proc/self/status Seccomp 字段)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SeccompMode {
    /// 未启用
    Disabled = 0,
    /// Strict: 只允许 read/write/exit/sigreturn
    Strict = 1,
    /// Filter: BPF 规则过滤
    Filter = 2,
}

// ============================================================================
// Seccomp 动作
// ============================================================================

/// Seccomp 过滤动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompAction {
    /// 放行
    Allow,
    /// 杀线程 (发 SIGSYS)
    KillThread,
    /// 杀进程 (发 SIGSYS)
    KillProcess,
    /// 陷阱 (发 SIGSYS + siginfo)
    Trap,
    /// 返回 -errno
    Errno(u32),
    /// 放行 + 审计日志
    Log,
}

impl SeccompAction {
    /// 从 Linux SECCOMP_RET_* 值解析
    pub fn from_linux(ret: u32) -> Self {
        match ret & 0xFFFF_0000 {
            0x7FFF_0000 => Self::Allow,
            0x0000_0000 => Self::KillThread,
            0x8000_0000 => Self::KillProcess,
            0x0003_0000 => Self::Trap,
            0x0005_0000 => Self::Errno(ret & 0xFFFF),
            0x7FFC_0000 => Self::Log,
            _ => Self::Allow,
        }
    }

    /// 转为 Linux SECCOMP_RET_* 值
    pub fn to_linux(self) -> u32 {
        match self {
            Self::Allow => 0x7FFF_0000,
            Self::KillThread => 0x0000_0000,
            Self::KillProcess => 0x8000_0000,
            Self::Trap => 0x0003_0000,
            Self::Errno(e) => 0x0005_0000 | (e & 0xFFFF),
            Self::Log => 0x7FFC_0000,
        }
    }
}

/// 默认动作 (无规则匹配时)
const DEFAULT_ACTION: SeccompAction = SeccompAction::Allow;

// ============================================================================
// 参数比较器
// ============================================================================

/// 参数比较运算符
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CmpOp {
    /// arg == value
    Equal = 0,
    /// arg != value
    NotEqual = 1,
    /// arg > value
    GreaterThan = 2,
    /// arg >= value
    GreaterEqual = 3,
    /// arg < value
    LessThan = 4,
    /// arg <= value
    LessEqual = 5,
    /// (arg & mask) == value
    MaskedEqual = 6,
}

/// 单个参数比较条件
#[derive(Debug, Clone, Copy)]
pub struct ArgComparator {
    /// 参数索引 (0-5)
    pub index: u8,
    /// 比较运算符
    pub op: CmpOp,
    /// 比较值
    pub value: u64,
    /// 掩码 (仅 MaskedEqual 使用)
    pub mask: u64,
}

impl ArgComparator {
    /// 评估比较条件
    pub fn evaluate(&self, arg: u64) -> bool {
        match self.op {
            CmpOp::Equal => arg == self.value,
            CmpOp::NotEqual => arg != self.value,
            CmpOp::GreaterThan => arg > self.value,
            CmpOp::GreaterEqual => arg >= self.value,
            CmpOp::LessThan => arg < self.value,
            CmpOp::LessEqual => arg <= self.value,
            CmpOp::MaskedEqual => (arg & self.mask) == self.value,
        }
    }
}

// ============================================================================
// Seccomp 规则
// ============================================================================

/// 单条 Seccomp 规则
///
/// 匹配条件: syscall_nr 相同 且 所有 arg_comparators 满足
#[derive(Debug, Clone)]
pub struct SeccompRule {
    /// 目标 syscall 编号 (QX_* 原生编号)
    pub syscall_nr: u64,
    /// 参数比较条件 (AND 语义)
    pub arg_comparators: Vec<ArgComparator>,
    /// 匹配时的动作
    pub action: SeccompAction,
}

impl SeccompRule {
    /// 评估规则是否匹配
    pub fn matches(&self, syscall_nr: u64, args: &[u64; 6]) -> bool {
        if self.syscall_nr != syscall_nr {
            return false;
        }
        self.arg_comparators.iter().all(|cmp| {
            let idx = cmp.index as usize;
            if idx >= 6 {
                return false;
            }
            cmp.evaluate(args[idx])
        })
    }
}

// ============================================================================
// Seccomp 过滤器
// ============================================================================

/// 一层过滤器 (包含多条规则)
#[derive(Debug, Clone)]
pub struct SeccompFilter {
    /// 规则列表
    pub rules: Vec<SeccompRule>,
    /// 默认动作 (本层无匹配时)
    pub default_action: SeccompAction,
}

impl SeccompFilter {
    /// 创建新过滤器
    pub fn new(rules: Vec<SeccompRule>, default_action: SeccompAction) -> Self {
        Self { rules, default_action }
    }

    /// 匹配 syscall, 返回动作
    pub fn check(&self, syscall_nr: u64, args: &[u64; 6]) -> SeccompAction {
        for rule in &self.rules {
            if rule.matches(syscall_nr, args) {
                return rule.action;
            }
        }
        self.default_action
    }
}

// ============================================================================
// Per-process Seccomp 状态
// ============================================================================

/// Per-process Seccomp 状态
pub struct SeccompState {
    /// 当前模式
    pub mode: AtomicU8,
    /// 过滤器链 (最多 MAX_FILTERS 层)
    pub filters: spin::Mutex<Vec<SeccompFilter>>,
    /// no_new_privs 位 (一旦设置不可清除)
    pub no_new_privs: AtomicU8,
}

impl SeccompState {
    /// 创建默认状态 (Disabled)
    pub fn new() -> Self {
        Self {
            mode: AtomicU8::new(SeccompMode::Disabled as u8),
            filters: spin::Mutex::new(Vec::new()),
            no_new_privs: AtomicU8::new(0),
        }
    }

    /// 读取当前模式
    pub fn get_mode(&self) -> SeccompMode {
        match self.mode.load(Ordering::Acquire) {
            1 => SeccompMode::Strict,
            2 => SeccompMode::Filter,
            _ => SeccompMode::Disabled,
        }
    }

    /// 设置 no_new_privs
    pub fn set_no_new_privs(&self) {
        self.no_new_privs.store(1, Ordering::Release);
    }

    /// 检查 no_new_privs
    pub fn is_no_new_privs(&self) -> bool {
        self.no_new_privs.load(Ordering::Acquire) != 0
    }
}

// ============================================================================
// Seccomp 检查入口
// ============================================================================

/// 检查当前进程的 Seccomp 过滤
///
/// 在 `syscall_dispatch` 入口处调用. 返回 `None` 表示放行,
/// 返回 `Some(result)` 表示拦截, result 为应返回给用户态的值.
#[inline(never)]
pub fn seccomp_check(syscall_nr: u64, args: &[u64; 6]) -> Option<i64> {
    let pid = process_get_current_pid();
    let mode = PROCESS_TABLE
        .with_process(pid, |p| p.seccomp.get_mode())
        .unwrap_or(SeccompMode::Disabled);

    match mode {
        SeccompMode::Disabled => None,
        SeccompMode::Strict => {
            if STRICT_ALLOWED.contains(&syscall_nr) {
                None
            } else {
                // Strict 模式: 非法 syscall → SIGKILL
                let _ = do_signal_send(pid as Pid, 9); // SIGKILL
                Some(-(Errno::EPERM as i64))
            }
        }
        SeccompMode::Filter => {
            let action = PROCESS_TABLE
                .with_process(pid, |p| {
                    let filters = p.seccomp.filters.lock();
                    // 从最外层到最内层, 首条匹配即终止
                    let mut result = DEFAULT_ACTION;
                    for filter in filters.iter() {
                        result = filter.check(syscall_nr, args);
                        if result != SeccompAction::Allow {
                            break;
                        }
                    }
                    result
                })
                .unwrap_or(SeccompAction::Allow);

            match action {
                SeccompAction::Allow => None,
                SeccompAction::Log => None, // TODO: 审计日志
                SeccompAction::KillThread => {
                    let _ = do_signal_send(pid as Pid, 31); // SIGSYS
                    Some(-(Errno::EPERM as i64))
                }
                SeccompAction::KillProcess => {
                    let _ = do_signal_send(pid as Pid, 31); // SIGSYS
                    Some(-(Errno::EPERM as i64))
                }
                SeccompAction::Trap => {
                    let _ = do_signal_send(pid as Pid, 31); // SIGSYS
                    Some(-(Errno::EPERM as i64))
                }
                SeccompAction::Errno(e) => Some(-(e as i64)),
            }
        }
    }
}

// ============================================================================
// Syscall 入口
// ============================================================================

/// sys_seccomp — 安装 Seccomp 过滤器
///
/// # 参数
/// - `operation`: SECCOMP_SET_MODE_STRICT(0) / SECCOMP_SET_MODE_FILTER(1)
/// - `flags`: SECCOMP_FILTER_FLAG_* 位集
/// - `args_ptr`: 用户态 struct sock_fprog 指针 (仅 SET_MODE_FILTER)
///
/// # 返回
/// 0 成功, 负数 errno 失败
pub fn sys_seccomp(operation: u32, _flags: u32, _args_ptr: u64) -> i64 {
    let pid = process_get_current_pid();

    match operation {
        0 => {
            // SECCOMP_SET_MODE_STRICT
            match PROCESS_TABLE
                .with_process(pid, |p| {
                    let mode = p.seccomp.get_mode();
                    if mode != SeccompMode::Disabled {
                        return Err(Errno::EINVAL);
                    }
                    p.seccomp
                        .mode
                        .store(SeccompMode::Strict as u8, Ordering::Release);
                    Ok(())
                })
                .unwrap_or(Err(Errno::ESRCH))
            {
                Ok(()) => 0,
                Err(e) => -(e as i64),
            }
        }
        1 => {
            // SECCOMP_SET_MODE_FILTER
            // 权限检查: 需要 CAP_SYS_ADMIN 或 no_new_privs
            let has_priv = PROCESS_TABLE
                .with_process(pid, |p| p.seccomp.is_no_new_privs())
                .unwrap_or(false);

            if !has_priv {
                // TODO: 检查 CAP_SYS_ADMIN (当前无 capability 完整实现,
                // PID 1 视为有特权)
                if pid != 1 {
                    return -(Errno::EACCES as i64);
                }
            }

            match PROCESS_TABLE
                .with_process(pid, |p| {
                    let mode = p.seccomp.get_mode();
                    if mode == SeccompMode::Strict {
                        return Err(Errno::EINVAL);
                    }
                    let mut filters = p.seccomp.filters.lock();
                    if filters.len() >= MAX_FILTERS {
                        return Err(Errno::ENOMEM);
                    }
                    // TODO: 从 args_ptr 解析 sock_fprog → SeccompFilter
                    // 当前接受空过滤器 (默认 ALLOW), 后续实现 BPF 解析
                    let filter = SeccompFilter::new(Vec::new(), DEFAULT_ACTION);
                    filters.push(filter);
                    p.seccomp
                        .mode
                        .store(SeccompMode::Filter as u8, Ordering::Release);
                    Ok(())
                })
                .unwrap_or(Err(Errno::ESRCH))
            {
                Ok(()) => 0,
                Err(e) => -(e as i64),
            }
        }
        _ => -(Errno::EINVAL as i64),
    }
}

/// sys_prctl — 进程控制 (Seccomp 相关子集)
///
/// 仅处理 PR_SET_SECCOMP(22) / PR_GET_SECCOMP(21) / PR_SET_NO_NEW_PRIVS(38) /
/// PR_GET_NO_NEW_PRIVS(39)
pub fn sys_prctl_prctl(option: i64, arg2: u64, _arg3: u64, _arg4: u64, _arg5: u64) -> i64 {
    let pid = process_get_current_pid();

    match option {
        22 => {
            // PR_SET_SECCOMP
            match arg2 {
                1 => sys_seccomp(0, 0, 0), // SECCOMP_MODE_STRICT
                2 => sys_seccomp(1, 0, 0), // SECCOMP_MODE_FILTER
                _ => -(Errno::EINVAL as i64),
            }
        }
        21 => {
            // PR_GET_SECCOMP
            let mode = PROCESS_TABLE
                .with_process(pid, |p| p.seccomp.get_mode())
                .unwrap_or(SeccompMode::Disabled);
            mode as i64
        }
        38 => {
            // PR_SET_NO_NEW_PRIVS
            if arg2 != 1 {
                return -(Errno::EINVAL as i64);
            }
            PROCESS_TABLE
                .with_process(pid, |p| p.seccomp.set_no_new_privs())
                .unwrap_or(());
            0
        }
        39 => {
            // PR_GET_NO_NEW_PRIVS
            PROCESS_TABLE
                .with_process(pid, |p| p.seccomp.is_no_new_privs() as i64)
                .unwrap_or(0)
        }
        _ => -(Errno::ENOSYS as i64),
    }
}

/// 向进程添加一条结构化规则 (内核内部 API, 非 syscall)
///
/// 用于测试或内核策略注入.
pub fn add_rule(pid: u64, rule: SeccompRule) -> Result<(), Errno> {
    PROCESS_TABLE
        .with_process(pid as u32, |p| {
            let mode = p.seccomp.get_mode();
            if mode == SeccompMode::Disabled {
                // 自动进入 Filter 模式
                p.seccomp
                    .mode
                    .store(SeccompMode::Filter as u8, Ordering::Release);
            }
            let mut filters = p.seccomp.filters.lock();
            if filters.is_empty() {
                filters.push(SeccompFilter::new(
                    alloc::vec![rule],
                    DEFAULT_ACTION,
                ));
            } else {
                filters[0].rules.push(rule);
            }
            Ok(())
        })
        .unwrap_or(Err(Errno::ESRCH))
}
