//! POSIX Timer 系统调用 (TCB, 0 services-layer safe API)
//!
//! ## 编号 (740-745: POSIX Timer)
//!
//! - `QX_TIMER_CREATE`     (740): 创建 per-process 定时器
//! - `QX_TIMER_SETTIME`    (741): 启动 / 调整 / 停止定时器
//! - `QX_TIMER_GETTIME`    (742): 查询剩余时间和间隔
//! - `QX_TIMER_DELETE`     (743): 释放定时器
//! - `QX_TIMER_GETOVERRUN` (744): 返回上次 read 之后补打的次数
//! - `QX_CLOCK_GETRES`     (745): 时钟分辨率
//!
//! ## 用户态布局
//!
//! ```c
//! struct sigevent {
//!     union sigval sigev_value;     // 8 字节
//!     int sigev_signo;              // 4 字节
//!     int sigev_notify;             // 4 字节
//!     void (*sigev_notify_function)(union sigval);
//!     pthread_attr_t *sigev_notify_attributes;
//! };
//!
//! struct itimerspec {
//!     struct timespec it_interval;  // 16 字节 (sec, nsec)
//!     struct timespec it_value;     // 16 字节
//! };
//! ```
//!
//! ## 安全
//!
//! - 用户态指针经 `check_user_buf` 校验
//! - `timer_id` 用 `slot_index + 1` (1-based), 0 保留为无效
//! - 进程退出时由 `posix_timer_release_pid` 释放全部 timer

use crate::kernel::framework::proc as ptimer;

// ============================================================================
// sys_timer_create
// ============================================================================

/// `sys_timer_create(clockid, sigev_ptr, timer_id_ptr) -> 0/-errno`  // POSIX 函数签名
pub fn sys_timer_create(a0: u64, a1: u64, a2: u64) -> i64 {
    ptimer::sys_timer_create(a0 as i32, a1, a2)
}

// ============================================================================
// sys_timer_settime
// ============================================================================

/// `sys_timer_settime(timer_id, flags, new_value_ptr, old_value_ptr) -> 0/-errno`  // POSIX 函数签名
pub fn sys_timer_settime(a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    ptimer::sys_timer_settime(a0 as i32, a1 as i32, a2, a3)
}

// ============================================================================
// sys_timer_gettime
// ============================================================================

/// `sys_timer_gettime(timer_id, curr_value_ptr) -> 0/-errno`  // POSIX 函数签名
pub fn sys_timer_gettime(a0: u64, a1: u64) -> i64 {
    ptimer::sys_timer_gettime(a0 as i32, a1)
}

// ============================================================================
// sys_timer_delete
// ============================================================================

/// `sys_timer_delete(timer_id) -> 0/-errno`  // POSIX 函数签名
pub fn sys_timer_delete(a0: u64) -> i64 {
    ptimer::sys_timer_delete(a0 as i32)
}

// ============================================================================
// sys_timer_getoverrun
// ============================================================================

/// `sys_timer_getoverrun(timer_id) -> overrun / -errno`  // POSIX 函数签名
pub fn sys_timer_getoverrun(a0: u64) -> i64 {
    ptimer::sys_timer_getoverrun(a0 as i32)
}

// ============================================================================
// sys_clock_getres
// ============================================================================

/// `sys_clock_getres(clockid, res_ptr) -> 0/-errno`  // POSIX 函数签名
///
/// `res_ptr` 可为 NULL (仅做时钟存在性检查)。
pub fn sys_clock_getres(a0: u64, a1: u64) -> i64 {
    ptimer::sys_clock_getres(a0 as i32, a1)
}
