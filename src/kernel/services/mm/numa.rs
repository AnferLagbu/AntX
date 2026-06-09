#![deny(unsafe_code)]
//! NUMA 安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::mm::numa` 的安全 API.

// 重导出强类型
pub use crate::kernel::framework::mm::numa::{
    NumaPolicy, NumaMempolicy, NumaNode, NumaTopology,
    MAX_NUMA_NODES, LOCAL_DISTANCE, REMOTE_DISTANCE,
};

use crate::kernel::framework::mm::numa::{
    numa_init, numa_is_initialized, numa_topology,
    sys_get_mempolicy, sys_set_mempolicy, sys_migrate_pages, sys_getcpu,
};

/// 初始化 NUMA 子系统 (UMA 回退)
pub fn init(total_memory: u64, num_cpus: u32) {
    numa_init(total_memory, num_cpus);
}

/// NUMA 是否已初始化
pub fn is_initialized() -> bool {
    numa_is_initialized()
}

/// 获取全局 NUMA 拓扑
pub fn topology() -> &'static NumaTopology {
    numa_topology()
}

/// 获取当前 CPU 和 NUMA 节点 (安全封装)
pub fn getcpu() -> i64 {
    sys_getcpu()
}

/// 获取 NUMA 内存策略 (安全封装)
pub fn get_mempolicy(mode_ptr: u64, nodemask_ptr: u64) -> i64 {
    sys_get_mempolicy(mode_ptr, nodemask_ptr)
}

/// 设置 NUMA 内存策略 (安全封装)
pub fn set_mempolicy(mode: u64, nodemask: u64) -> i64 {
    sys_set_mempolicy(mode, nodemask)
}

/// 迁移进程页面 (安全封装)
pub fn migrate_pages(target_nodemask: u64) -> i64 {
    sys_migrate_pages(target_nodemask)
}
