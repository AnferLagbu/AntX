//! Linux 兼容 Namespace 框架 (D1) — framework 层 re-export
//!
//! ## T1-3 迁移记录
//!
//! 策略代码 (命名空间数据结构 + 隔离规则 + 注册表 + syscall 接口)
//! 已于 2026-06-16 迁移到 services::proc::namespace.
//! 本文件仅 re-export 保持调用方兼容.

// Re-export services 层的策略主体 — 保持调用方路径兼容
pub use crate::kernel::services::proc::namespace::{
    NsType, NamespaceSet,
    UtsNamespace, IpcNamespace, PidNamespace, MountNamespace,
    UserNamespace, NetNamespace, CgroupNamespace,
    NsRegistryEntry, NsRegistry,
    CLONE_NEWNS, CLONE_NEWUTS, CLONE_NEWIPC, CLONE_NEWUSER,
    CLONE_NEWPID, CLONE_NEWNET, CLONE_NEWCGROUP, CLONE_NEW_ALL,
    ns_register,
    sys_unshare, sys_setns,
};
