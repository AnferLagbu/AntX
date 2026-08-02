//! OOMD — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码 (`OomDaemon` + 常量 + tick/stats/disable)
//! 已于 2026-06-17 迁移到 `services::proc::oomd`.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::proc::oomd::{OomDaemon, OOMD};
