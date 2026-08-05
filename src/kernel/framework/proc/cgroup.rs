//! cgroup (Control Group) — framework 层 re-export
//!
//! ## T1-4 迁移记录
//!
//! 策略代码 (控制器 + cgroup 实例 + 全局管理器 + syscall)
//! 已于 2026-06-16 迁移到 `services::proc::cgroup`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::proc::cgroup::{
    CGROUP_MAX_DEPTH, CGROUP_MAX_PROCS, CPU_CFS_PERIOD_DEFAULT_US, CPU_CFS_QUOTA_MAX, CgroupRq,
    CgroupSubsystem, CpuController, Errno, IoController, MEMORY_LIMIT_MAX, MemoryController,
    PIDS_MAX_DEFAULT, PidsController, cgroup_init, cgroup_is_initialized, cgroup_subsystem,
    sys_cgroup_attach, sys_cgroup_create, sys_cgroup_destroy, sys_cgroup_get_stat,
    sys_cgroup_set_limit,
};
