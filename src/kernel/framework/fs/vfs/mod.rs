pub mod api;
pub mod dcache;
pub mod flock;
pub mod inotify;
pub mod types;
pub mod vfs;

pub use types::*;
pub use vfs::*;

// 公共接口 re-export — 避免跨子系统直接访问内部子模块
pub use inotify::{sys_inotify_init1, sys_inotify_add_watch, sys_inotify_rm_watch, sys_inotify_read, is_inotify_fd, inotify_release};
pub use flock::{flock_release_fd, sys_flock, FlockResult};
