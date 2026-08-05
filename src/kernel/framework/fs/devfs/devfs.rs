//! `DevFS` — framework re-export 层
//!
//! 实现已迁移到 `services::fs::devfs`, 本文件仅 re-export 公共 API.
//! framework 内部代码通过本模块引用 `DevFS`, 保持路径兼容.

pub use crate::kernel::services::fs::devfs::{
    DEVFS_DATA, DEVFS_MAX_DEVICES, DEVFS_MAX_NAME, DevFile, DevKind, DevfsData, DevfsDevice,
    SafeDevFs, global, init, init_global, max_name_len, open, register, register_standard,
};
