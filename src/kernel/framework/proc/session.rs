//! 会话 / 进程组 / 控制终端 — framework 层 re-export
//!
//! ## T1-6 迁移记录
//!
//! 策略代码 (会话管理 + 进程组 + 控制终端规则)
//! 已于 2026-06-16 迁移到 `services::proc::session`.
//! 本文件仅 re-export 保持调用方兼容.

// Re-export services 层的策略主体 — 保持调用方路径兼容
pub use crate::kernel::services::proc::session::{
    SESSION_MANAGER, Session, SessionManager, SessionState, get_controlling_terminal,
    get_foreground_pgid, init, proc_getpgid, proc_getsid, proc_init_pgid, proc_setpgid,
    proc_setsid, session_leader_exit, signal_foreground_pgid, sys_tcgetpgrp, sys_tcsetpgrp,
    sys_tiocsctty,
};
