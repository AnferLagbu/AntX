#![deny(unsafe_code)]
//! 电源管理安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::driver::power` 的安全 API.

// 重导出强类型
pub use crate::kernel::framework::driver::power::{
    CpuIdleState, CpuIdleStats, CpuIdleDriver,
    FreqGovernor, FreqLevel, CpuFreqDriver,
    SystemPowerState, SuspendNotifier,
    PmSubsystem,
    MAX_CSTATES, MAX_FREQ_LEVELS, MAX_PM_CPUS,
};

use crate::kernel::framework::driver::power::{
    pm_init, pm_is_initialized, pm_subsystem, sys_pm,
};

/// 初始化电源管理子系统
pub fn init(num_cpus: u32) {
    pm_init(num_cpus);
}

/// 电源管理是否已初始化
pub fn is_initialized() -> bool {
    pm_is_initialized()
}

/// 获取全局 PM 子系统
pub fn subsystem() -> &'static PmSubsystem {
    pm_subsystem()
}

/// PM 系统调用 (安全封装)
pub fn pm_syscall(cmd: u64, a1: u64, a2: u64) -> i64 {
    sys_pm(cmd, a1, a2)
}
