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
pub use inotify::{sys_inotify_init1, sys_inotify_add_watch, sys_inotify_rm_watch, sys_inotify_read, is_inotify_fd, inotify_release};
pub use flock::{flock_release_fd, flock_release_pid, posix_lock_release_pid, sys_flock, FlockResult, sys_posix_lock, PosixLockResult, PosixLockConflict, F_GETLK, F_SETLK, F_SETLKW};
pub use api::{vfs_open, vfs_read, vfs_write, vfs_close, vfs_stat, vfs_fstat, vfs_seek, vfs_readdir, vfs_dup, vfs_dup2, vfs_read_internal, vfs_write_internal, vfs_mount, vfs_mount_safe, vfs_umount, vfs_get_cwd, vfs_set_cwd, vfs_mkdir, vfs_rmdir, vfs_unlink, vfs_rename, vfs_sync, vfs_stat_internal, vfs_chmod, vfs_chown, vfs_chown_ext, vfs_fchmod, vfs_fchown, vfs_link, vfs_symlink, vfs_readlink, vfs_truncate_internal, vfs_pread_inode};
// T-05: 后端决策策略 re-export
pub use backend_trait::{FsBackend, FallbackFsBackend, register_fs_backend, current_fs_backend};