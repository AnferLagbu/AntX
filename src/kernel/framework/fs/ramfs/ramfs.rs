//! `RamFS` — framework re-export 层 (E6-5 迁移)
//!
//! 实现已迁移到 `services::fs::ramfs_core`, 本文件仅 re-export 公共 API.
//! 保留向后兼容性, 使 framework 中引用 `RamFsData/RAMFS_DATA` 的代码无需修改.

pub use crate::kernel::services::fs::ramfs_core::{
    RAMFS_DATA, RamFsACE, RamFsData, RamFsDirEntry, RamFsNode, init,
};
