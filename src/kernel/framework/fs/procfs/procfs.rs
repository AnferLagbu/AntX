//! `ProcFS` — framework re-export 层 (E6-8 迁移)
//! 实现已迁移到 `services::fs::procfs_core`, 本文件仅 re-export 公共 API.

pub use crate::kernel::services::fs::procfs_core::{
    PROCFS_DATA, PROCFS_MAX_ENTRIES, PROCFS_MAX_NAME, ProcfsData, ProcfsEntry, init,
};
