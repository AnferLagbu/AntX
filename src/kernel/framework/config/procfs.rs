//! /proc/sys/config 接口 — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码 (ConfigFormat + parse_format + read_sys_config + read_sys_config_json)
//! 已于 2026-06-17 迁移到 services::config::procfs.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::config::procfs::{
    ConfigFormat, parse_format, read_sys_config, read_sys_config_json,
};
