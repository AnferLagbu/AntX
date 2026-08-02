//! 文件锁 (flock + POSIX record locks) — framework re-export 层
//!
//! 实现已迁移到 `services::fs::flock`, 本文件仅 re-export 公共 API,
//! 保持 framework 内现有调用者路径不变。
//!
//! ## 迁移记录
//!
//! - 原实现: framework/fs/vfs/flock.rs (729 行, 0 unsafe)
//! - 迁移到: services/fs/flock.rs
//! - 原因: flock/POSIX record lock 是纯策略代码, 按框内核原则应放在 services 层

// Re-export 所有公共类型与函数
pub use crate::kernel::services::fs::flock::{
    FlockResult, PosixLockConflict, PosixLockResult,
    LOCK_SH, LOCK_EX, LOCK_UN, LOCK_NB,
    F_RDLCK, F_WRLCK, F_UNLCK,
    F_SETLK, F_SETLKW, F_GETLK,
    POSIX_LOCK_TO_EOF,
    sys_flock, sys_posix_lock,
    flock_release_fd, flock_release_pid,
    posix_lock_release_pid, posix_lock_release_inode,
    flock_ops, posix_lock_ops,
    flock_count, posix_lock_count,
    reset_stats,
};
