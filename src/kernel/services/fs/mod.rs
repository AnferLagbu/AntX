#![deny(unsafe_code)]
//! 文件系统 — services 层 (Phase 2.2 完成 ✓)
//!
//! ## 真实状态 (v2.5, 2026-06-04)
//!
//! 已完成 4/4 子系统迁移:
//! - [ramfs]  — RamFS 内存文件系统安全代理 (100% safe API, 0 unsafe)
//! - [devfs]  — DevFS 设备文件系统安全代理 (100% safe API, 0 unsafe)
//! - [procfs] — ProcFS 进程文件系统安全代理 (100% safe API, 0 unsafe)
//! - [hvfs]   — HvFS 磁盘文件系统安全代理 (100% safe API, 0 unsafe)
//!
//! ## 迁移方法
//!
//! 1. 把内核 `i32` 错误码 → `Result<_, FsError>` (services 层类型化)
//! 2. 把 `*const u8`/`*mut u8` 用户指针 → `&[u8]`/`&mut [u8]` 切片
//! 3. 把硬编码路径/标志 → 引入 `VfsOpenFlags`/`VfsSeekWhence` 等强类型
//! 4. 0 unsafe 出现在 services 层
//!
//! 评估日期: 2026-06-04

pub mod ramfs;
pub mod ramfs_core;
/// T6-9: VFS 公共类型 (原 framework/fs/vfs/types.rs)
pub mod vfs_types;
pub mod vfs_poll_policy;
pub mod devfs;
pub mod procfs;
pub mod procfs_core;
pub mod hvfs;
pub mod sysfs;
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

// ============================================================================
// T-05: VFS 后端决策策略
// ============================================================================

use crate::kernel::framework::fs::vfs::backend_trait::{FsBackend, register_fs_backend};
use crate::kernel::services::fs::vfs_types::KernelError;
use crate::kernel::framework::fs::vfs::api as vfs_api;

/// services 层 VFS 后端决策策略
///
/// 维护文件系统注册表, 根据 fs_type 名称选择挂载方式.
/// 挂载权限: 当前允许所有挂载请求 (未来可扩展为权限检查).
pub struct ServicesFsBackend;

impl FsBackend for ServicesFsBackend {
    fn mount_fs(&self, fs_name: &str, path: &str) -> Result<(), KernelError> {
        // services 根据 fs_name 选择挂载方式并调用 framework safe API
        let rc = vfs_api::vfs_mount_safe(path, fs_name);
        if rc == 0 {
            Ok(())
        } else {
            Err(KernelError::IoError)
        }
    }

    fn allow_mount(&self, _path: &str, _fs_name: &str) -> bool {
        // 当前允许所有挂载; 未来可按路径/fs_type 做权限检查
        true
    }
}

/// services::fs 初始化 — 注册策略到 framework
pub fn init() {
    static POLICY: ServicesFsBackend = ServicesFsBackend;
    let _ = register_fs_backend(&POLICY);
}
