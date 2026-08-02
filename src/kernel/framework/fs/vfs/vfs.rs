//! VFS 管理器 — framework 层 re-export + `init()` 包装
//!
//! ## 迁移记录
//!
//! 策略代码 (`VfsManager` + `VfsMount` + `VfsFile` + `ResolvedMount`)
//! 已于 2026-06-17 迁移到 `services::fs::vfs_manager`.
//! 本文件仅 re-export 保持调用方兼容, 并保留 `init()` 中的 barrier 回调注册.

pub use crate::kernel::services::fs::vfs_manager::{
    VfsMount, VfsFile, ResolvedMount, VfsManager, VFS_MANAGER,
};

pub fn init() {
    VFS_MANAGER.init();

    // barrier 回调注册 (必须在 framework 层, 因为引用 framework::barrier)
    if let Some(dom) = crate::kernel::framework::barrier::RECOVERY_MANAGER.lock().find(2) {
        *dom.capture_cb.lock() = Some(vfs_barrier_capture_cb);
        *dom.rollback_cb.lock() = Some(vfs_barrier_rollback_cb);
    }
}

fn vfs_barrier_capture_cb() {
    VFS_MANAGER.capture_snapshot();
}

fn vfs_barrier_rollback_cb() -> bool {
    VFS_MANAGER.restore_from_snapshot();
    true
}
