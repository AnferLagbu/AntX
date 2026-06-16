//! NUMA (Non-Uniform Memory Access) — framework 层 re-export
//!
//! ## T2-6 迁移记录
//!
//! 策略代码 (拓扑 + 节点 + 策略 + syscall)
//! 已于 2026-06-16 迁移到 services::mm::numa.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::mm::numa::{
    NumaPolicy, NumaMempolicy, NumaNode, NumaTopology,
    MAX_NUMA_NODES, LOCAL_DISTANCE, REMOTE_DISTANCE,
    numa_init, numa_topology, numa_is_initialized,
    sys_get_mempolicy, sys_set_mempolicy, sys_migrate_pages, sys_getcpu,
};
