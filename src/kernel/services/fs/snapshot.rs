//! 快照 (snapshot) 系统调用处理器
//!
//! 提供 snapshot_create/snapshot_destroy/snapshot_rollback/snapshot_clone 系统调用的实现。
//! 调用 HvFS 层的快照方法。

use crate::kernel::services::syscall::types::Errno;

/// 创建快照
pub fn snapshot_create_syscall(name_ptr: u64) -> Result<usize, Errno> {
    if name_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    // 通过 framework 层获取快照名称
    let name = crate::kernel::framework::fs::vfs::api::snapshot_get_name(name_ptr);
    let result = crate::kernel::services::fs::hvfs::hvfs::get_hvfs().snapshot_create(&name);

    if result >= 0 { Ok(result as usize) } else { Err(Errno::from_ret(result as i64)) }
}

/// 销毁快照
pub fn snapshot_destroy_syscall(snap_id: u64) -> Result<usize, Errno> {
    let result = crate::kernel::services::fs::hvfs::hvfs::get_hvfs().snapshot_destroy(snap_id);

    if result >= 0 { Ok(result as usize) } else { Err(Errno::from_ret(result as i64)) }
}

/// 回滚快照
pub fn snapshot_rollback_syscall(snap_id: u64) -> Result<usize, Errno> {
    let result = crate::kernel::services::fs::hvfs::hvfs::get_hvfs().snapshot_rollback(snap_id);

    if result >= 0 { Ok(result as usize) } else { Err(Errno::from_ret(result as i64)) }
}

/// 从快照创建克隆
pub fn snapshot_clone_syscall(snap_id: u64, name_ptr: u64) -> Result<usize, Errno> {
    if name_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    // 通过 framework 层获取克隆名称
    let name = crate::kernel::framework::fs::vfs::api::snapshot_get_name(name_ptr);
    let result = crate::kernel::services::fs::hvfs::hvfs::get_hvfs().clone_create(snap_id, &name);

    if result >= 0 { Ok(result as usize) } else { Err(Errno::from_ret(result as i64)) }
}
