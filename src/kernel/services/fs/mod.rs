#![deny(unsafe_code)]
//! 文件系统 — services 层策略主体
//!
//! VFS Manager + Inode trait 抽象. 7 个原生 FS (ramfs/devfs/procfs/ext2/
//! exfat/tmpfs/overlayfs) + HvFS 在 services::fs::inode 实现 Plan B 契约.
//! 0 unsafe, 全部块设备/页缓存底层走 framework.
//!
//! 历史: 2026-06 之前 v2.5 状态评估已过时, 当前已远超当时范围. 详细
//! 进度见 docs/plan/progress-active-tasks.md.

pub mod ramfs;
pub mod ramfs_core;
/// T6-9: VFS 公共类型 (原 framework/fs/vfs/types.rs)
pub mod vfs_types;
/// Plan B: Inode trait — 文件级操作抽象
pub mod inode;
pub mod vfs_poll_policy;
pub mod devfs;
pub mod procfs;
pub mod procfs_core;
/// 扩展属性 (xattr) 系统调用处理器
pub mod xattr;
/// 快照 (snapshot) 系统调用处理器
pub mod snapshot;
pub mod hvfs;
pub mod ext2;
pub mod exfat;
pub mod sysfs;
/// G8: 容器辅助文件系统
pub mod devpts;
pub mod cgroupfs;
pub mod configfs;
pub mod virtiofs;
/// G9: 动态系统树
pub mod systree;
/// 文件句柄系统 (name_to_handle_at / open_by_handle_at)
pub mod file_handle;
/// Per-process FD 表
pub mod process_fd_table;
pub mod io;
pub mod path;
pub mod mode;
pub mod stat;
pub mod access;
pub mod open;
pub mod link;
pub mod mount;
pub mod misc;
pub mod dcache;
pub mod flock;
pub mod inotify;
pub mod sendfile;
/// 文件操作策略 — ioctl / clock_gettime / poll / chown / truncate / flock
pub mod file_ops;
/// 目录与定位操作策略 — lseek / getdents
pub mod dir_ops;
/// VFS 管理器 (挂载表 + FD 表 + 路径解析)
pub mod vfs_manager;
/// 全局 OpenFile 表 (POSIX 打开文件描述)
pub mod open_file_table;
/// 匿名文件系统 (memfd 基础)
pub mod anonymous;

// ============================================================================
// T-05: VFS 后端决策策略
// ============================================================================

use crate::kernel::framework::fs::vfs::backend_trait::{FsBackend, register_fs_backend};
use crate::kernel::services::fs::vfs_types::KernelError;
use crate::kernel::framework::fs::vfs::api as vfs_api;

/// services 层 VFS 后端决策策略
///
/// 维护文件系统注册表, 根据 `fs_type` 名称选择挂载方式.
/// 挂载权限: 当前允许所有挂载请求 (未来可扩展为权限检查).
pub struct ServicesFsBackend;

impl FsBackend for ServicesFsBackend {
    fn mount_fs(&self, fs_name: &str, path: &str) -> Result<(), KernelError> {
        // services 根据 fs_name 选择挂载方式并调用 framework safe API
        let rc = vfs_api::vfs_mount_safe(path, fs_name);
        if rc == 0 {
            Ok(())
        } else {
            // framework 层返回负 errno，转换为精确的 KernelError
            Err(KernelError::from_i32(-rc))
        }
    }

    fn allow_mount(&self, _path: &str, _fs_name: &str) -> bool {
        // 当前允许所有挂载; 未来可按路径/fs_type 做权限检查
        true
    }
}

/// `services::fs` 初始化 — 注册策略到 framework
pub fn init() {
    static POLICY: ServicesFsBackend = ServicesFsBackend;
    let _ = register_fs_backend(&POLICY);
}
