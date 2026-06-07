#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! 资源限制系统调用 — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全
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
//!
//! ## Framekernel 简化
//!
//! 所有资源限制硬编码为 RLIM_INFINITY (u64::MAX), 仅支持查询, 不支持设置。

use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::types::Errno;

/// POSIX RLIM_INFINITY
pub const RLIM_INFINITY: u64 = u64::MAX;

// ============================================================================
// getrlimit
// ============================================================================

/// getrlimit(resource, rlim) — 取资源限制到用户缓冲
///
/// rlim 指向 `struct rlimit { rlim_cur: u64, rlim_max: u64 }`。
pub fn getrlimit_syscall(resource: i32, rlim_ptr: u64) -> Result<usize, Errno> {
    if rlim_ptr == 0 {
        return Err(Errno::EINVAL);
    }
    // 资源类型范围: 0..=16 (POSIX RLIMIT_NLIMITS)
    if resource < 0 || resource > 16 {
        return Err(Errno::EINVAL);
    }
    // framework safe API: 写 rlim_cur/rlim_max 到 user buf, 内部已 check_user_buf
    if !raw::write_rlimit_to_user(rlim_ptr, RLIM_INFINITY, RLIM_INFINITY) {
        return Err(Errno::EFAULT);
    }
    Ok(0)
}
