//! madvise / mlock / mincore — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码已于 2026-06-17 迁移到 services::mm::madvise_mlock.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::mm::madvise_mlock::{
    sys_madvise, sys_mlock, sys_munlock, sys_mlockall, sys_munlockall, sys_mincore,
    MADV_NORMAL, MADV_RANDOM, MADV_SEQUENTIAL, MADV_WILLNEED, MADV_DONTNEED,
    MADV_FREE, MADV_REMOVE, MADV_PAGEOUT,
    MCL_CURRENT, MCL_FUTURE, MCL_ONFAULT,
};
