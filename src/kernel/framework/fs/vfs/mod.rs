pub mod api;
/// T-05: VFS 后端决策 trait
pub mod backend_trait;
pub mod dcache;
pub mod flock;
pub mod inotify;
pub mod types;
pub mod vfs;

pub use types::*;
pub use vfs::*;

// 公共接口 re-export — 避免跨子系统直接访问内部子模块
pub use api::{
    vfs_chmod, vfs_chown, vfs_chown_ext, vfs_close, vfs_dup, vfs_dup2, vfs_fchmod, vfs_fchown,
    vfs_fstat, vfs_get_cwd, vfs_link, vfs_mkdir, vfs_mount, vfs_mount_safe, vfs_open,
    vfs_pread_inode, vfs_read, vfs_read_internal, vfs_read_safe, vfs_readdir, vfs_readlink,
    vfs_rename, vfs_rmdir, vfs_seek, vfs_set_cwd, vfs_stat, vfs_stat_internal, vfs_symlink,
    vfs_sync, vfs_truncate_internal, vfs_umount, vfs_unlink, vfs_write, vfs_write_internal,
    vfs_write_safe,
};
pub use flock::{
    F_GETLK, F_SETLK, F_SETLKW, FlockResult, PosixLockConflict, PosixLockResult, flock_release_fd,
    flock_release_pid, posix_lock_release_pid, sys_flock, sys_posix_lock,
};
pub use inotify::{
    inotify_release, is_inotify_fd, sys_inotify_add_watch, sys_inotify_init1, sys_inotify_read,
    sys_inotify_rm_watch,
};
// T-05: 后端决策策略 re-export
pub use backend_trait::{FallbackFsBackend, FsBackend, current_fs_backend, register_fs_backend};
