#![deny(unsafe_code)]
//! cgroup 安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::proc::cgroup` 的安全 API.

// 重导出强类型
pub use crate::kernel::framework::proc::cgroup::{
    CpuController, MemoryController, PidsController, IoController,
    CgroupRq, CgroupSubsystem,
    CPU_CFS_PERIOD_DEFAULT_US, CPU_CFS_QUOTA_MAX,
    MEMORY_LIMIT_MAX, PIDS_MAX_DEFAULT,
    CGROUP_MAX_PROCS,
};

use crate::kernel::framework::proc::cgroup::{
    cgroup_init, cgroup_is_initialized,
    sys_cgroup_create, sys_cgroup_destroy, sys_cgroup_attach,
    sys_cgroup_set_limit, sys_cgroup_get_stat,
};

/// 初始化 cgroup 子系统
pub fn init() {
    cgroup_init();
}

/// cgroup 是否已初始化
pub fn is_initialized() -> bool {
    cgroup_is_initialized()
}

/// 创建子 cgroup (安全封装)
pub fn create(parent_id: u64, name_ptr: u64, name_len: u64) -> i64 {
    sys_cgroup_create(parent_id, name_ptr, name_len)
}

/// 删除 cgroup (安全封装)
pub fn destroy(cg_id: u64) -> i64 {
    sys_cgroup_destroy(cg_id)
}

/// 将进程迁移到 cgroup (安全封装)
pub fn attach(cg_id: u64, pid: u64) -> i64 {
    sys_cgroup_attach(cg_id, pid)
}

/// 设置 cgroup 资源限制 (安全封装)
pub fn set_limit(cg_id: u64, controller: u64, value: u64) -> i64 {
    sys_cgroup_set_limit(cg_id, controller, value)
}

/// 获取 cgroup 统计信息 (安全封装)
pub fn get_stat(cg_id: u64, stat_type: u64) -> i64 {
    sys_cgroup_get_stat(cg_id, stat_type)
}
