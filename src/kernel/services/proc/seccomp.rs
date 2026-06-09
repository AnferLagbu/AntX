#![deny(unsafe_code)]
//! Seccomp 安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::proc::seccomp` 的安全 API, 供用户态程序和 syscall 层调用.
//!
//! ## 架构
//!
//! ```text
//! services/proc/seccomp.rs (本文件, 0 unsafe)
//!     │
//!     ▼
//! framework/proc/seccomp.rs (TCB, 含 unsafe)
//! ```

// 重导出强类型
pub use crate::kernel::framework::proc::seccomp::{
    SeccompMode, SeccompAction, SeccompRule, SeccompFilter,
    ArgComparator, CmpOp, SeccompState,
};

use crate::kernel::framework::proc::seccomp::{add_rule, sys_seccomp, sys_prctl_prctl};

/// 安装 Seccomp 过滤器
///
/// 封装 `sys_seccomp`, 返回强类型 Result.
pub fn seccomp(operation: u32, flags: u32, args_ptr: u64) -> Result<i64, i64> {
    let ret = sys_seccomp(operation, flags, args_ptr);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret)
    }
}

/// prctl (Seccomp 子集)
///
/// 封装 `sys_prctl_prctl`, 返回强类型 Result.
pub fn prctl(option: i64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> Result<i64, i64> {
    let ret = sys_prctl_prctl(option, arg2, arg3, arg4, arg5);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret)
    }
}

/// 添加结构化规则 (内核策略注入)
///
/// 安全封装 `framework::proc::seccomp::add_rule`.
pub fn add_seccomp_rule(pid: u64, rule: SeccompRule) -> Result<(), i64> {
    add_rule(pid, rule).map_err(|e| -(e as i64))
}
