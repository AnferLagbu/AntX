//! Seccomp — framework 层 re-export
//!
//! ## T1-5 迁移记录
//!
//! 策略代码 (过滤器 + 规则匹配 + syscall)
//! 已于 2026-06-16 迁移到 services::proc::seccomp.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::proc::seccomp::{
    SeccompMode, SeccompAction, SeccompRule, SeccompFilter,
    ArgComparator, CmpOp, SeccompState,
    seccomp_check,
    sys_seccomp, sys_prctl_prctl,
    add_rule,
};
