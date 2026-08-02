//! 会话 / 进程组 / 控制终端 — framework 层 re-export
//!
//! ## T1-6 迁移记录
//!
//! 策略代码 (会话管理 + 进程组 + 控制终端规则)
//! 已于 2026-06-16 迁移到 `services::proc::session`.
//! 本文件仅 re-export 保持调用方兼容.

// Re-export services 层的策略主体 — 保持调用方路径兼容
pub use crate::kernel::services::proc::session::{
    SessionState, Session, SessionManager, SESSION_MANAGER,
    init,
    proc_setsid, proc_getsid, proc_setpgid, proc_getpgid, proc_init_pgid,
    sys_tiocsctty, sys_tcsetpgrp, sys_tcgetpgrp,
    get_controlling_terminal, get_foreground_pgid,
    signal_foreground_pgid, session_leader_exit,
};
