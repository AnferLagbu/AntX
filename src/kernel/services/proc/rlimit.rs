#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 资源限制系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
//! - 封装 framework::proc::rlimit 的 per-process 资源限制
//!
//! ## POSIX 资源类型
//!
//! ```text
//! RLIMIT_CPU      = 0   // CPU 时间 (秒)
//! RLIMIT_FSIZE    = 1   // 文件大小
//! RLIMIT_DATA     = 2   // 数据段
//! RLIMIT_STACK    = 3   // 栈
//! RLIMIT_CORE     = 4   // core 文件
//! RLIMIT_RSS      = 5   // 驻留集
//! RLIMIT_NPROC    = 6   // 进程数
//! RLIMIT_NOFILE   = 7   // 打开文件数
//! RLIMIT_MEMLOCK  = 8   // 锁定内存
//! RLIMIT_AS       = 9   // 地址空间
//! ```

// Re-export framework 层的类型和常量
pub use crate::kernel::framework::proc::rlimit::{
    Rlimit, RlimitTable,
    RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK, RLIMIT_CORE,
    RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE, RLIMIT_MEMLOCK, RLIMIT_AS,
    RLIMIT_LOCKS, RLIMIT_SIGPENDING, RLIMIT_MSGQUEUE, RLIMIT_NICE,
    RLIMIT_RTPRIO, RLIMIT_RTTIME, RLIMIT_NLIMITS,
    RLIM_INFINITY,
    check_nofile_exceeded, check_as_exceeded, check_nproc_exceeded,
    get_stack_limit, get_nofile_limit,
};

/// getrlimit — 获取资源限制 (委托 framework)
pub fn getrlimit_syscall(resource: i32, rlim_ptr: u64) -> Result<usize, crate::kernel::framework::syscall::types::Errno> {
    let ret = crate::kernel::framework::proc::rlimit::sys_getrlimit(resource, rlim_ptr);
    if ret >= 0 {
        Ok(0)
    } else {
        Err(crate::kernel::framework::syscall::types::Errno::from_ret(ret))
    }
}

/// setrlimit — 设置资源限制 (委托 framework)
pub fn setrlimit_syscall(resource: i32, rlim_ptr: u64) -> Result<usize, crate::kernel::framework::syscall::types::Errno> {
    let ret = crate::kernel::framework::proc::rlimit::sys_setrlimit(resource, rlim_ptr);
    if ret >= 0 {
        Ok(0)
    } else {
        Err(crate::kernel::framework::syscall::types::Errno::from_ret(ret))
    }
}
