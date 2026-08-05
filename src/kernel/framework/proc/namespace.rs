//! Linux 兼容 Namespace 框架 (D1) — framework 层 re-export
//!
//! ## T1-3 迁移记录
//!
//! 策略代码 (命名空间数据结构 + 隔离规则 + 注册表 + syscall 接口)
//! 已于 2026-06-16 迁移到 services::proc::namespace.
//! 本文件仅 re-export 保持调用方兼容.

// Re-export services 层的策略主体 — 保持调用方路径兼容
pub use crate::kernel::services::proc::namespace::{
    CLONE_NEW_ALL, CLONE_NEWCGROUP, CLONE_NEWIPC, CLONE_NEWNET, CLONE_NEWNS, CLONE_NEWPID,
    CLONE_NEWUSER, CLONE_NEWUTS, CgroupNamespace, IpcNamespace, MountNamespace, NamespaceSet,
    NetNamespace, NsRegistry, NsRegistryEntry, NsType, PidNamespace, UserNamespace, UtsNamespace,
    ns_register, sys_setns, sys_unshare,
};
