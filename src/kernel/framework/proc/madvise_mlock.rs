//! madvise / mlock / mincore — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码已于 2026-06-17 迁移到 `services::mm::madvise_mlock`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::mm::madvise_mlock::{
    MADV_DONTNEED, MADV_FREE, MADV_NORMAL, MADV_PAGEOUT, MADV_RANDOM, MADV_REMOVE, MADV_SEQUENTIAL,
    MADV_WILLNEED, MCL_CURRENT, MCL_FUTURE, MCL_ONFAULT, sys_madvise, sys_mincore, sys_mlock,
    sys_mlockall, sys_munlock, sys_munlockall,
};
