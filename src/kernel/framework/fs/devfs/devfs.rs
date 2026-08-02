//! `DevFS` — framework re-export 层
//!
//! 实现已迁移到 `services::fs::devfs`, 本文件仅 re-export 公共 API.
//! framework 内部代码通过本模块引用 `DevFS`, 保持路径兼容.

pub use crate::kernel::services::fs::devfs::{
    DevKind, DevFile, DevfsData, DevfsDevice,
    DEVFS_MAX_DEVICES, DEVFS_MAX_NAME,
    DEVFS_DATA,
    SafeDevFs,
    init, init_global, global,
    register, open, max_name_len, register_standard,
};
