//! `ConfigSummary` 启动期编码 — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码于 2026-06-17 从 `framework::config::boot_image` 迁移至此。
//! framework 层仅 re-export 保持调用方兼容.

pub use crate::kernel::services::config::boot_image::{
    BOOT_IMAGE, encode_boot_image, encoded_len, read_boot_image,
};
