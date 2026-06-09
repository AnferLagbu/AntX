#![deny(unsafe_code)]
//! Namespace 安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::proc::namespace` 的安全 API.

// 重导出强类型
pub use crate::kernel::framework::proc::namespace::{
    NsType, NamespaceSet,
    UtsNamespace, IpcNamespace, PidNamespace, MountNamespace,
    UserNamespace, NetNamespace, CgroupNamespace,
    CLONE_NEWNS, CLONE_NEWUTS, CLONE_NEWIPC, CLONE_NEWUSER,
    CLONE_NEWPID, CLONE_NEWNET, CLONE_NEWCGROUP, CLONE_NEW_ALL,
};

use crate::kernel::framework::proc::namespace::{sys_unshare, sys_setns};

/// unshare — 取消共享指定 namespace (安全封装)
pub fn unshare(flags: u64) -> i64 {
    sys_unshare(flags)
}

/// setns — 切换到指定 namespace (安全封装)
pub fn setns(ns_type: u64, target_ns_id: u64) -> i64 {
    sys_setns(ns_type, target_ns_id)
}
