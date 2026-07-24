//! DisplayPort 驱动 — framework 薄包装层
//!
//! 完整实现已迁移至 `services::driver::display::dp` (0 unsafe).
//! 本文件仅保留 re-export 以保持向后兼容.
//!
//! # Safety
//!
//! 本文件不含 unsafe 代码. DisplayPort 驱动安全逻辑在 services 层.

// 重新导出 services 层的 DisplayPort 驱动类型
pub use crate::kernel::services::driver::display::dp::{
    AuxCommand, AuxTransaction, DpController, DpError, Dpcd, DpIo, LaneCount, LinkRate,
    TrainingState, assert_iomem_size_at_least,
};
pub use crate::kernel::services::driver::display::dp::REQUIRED_IOMEM_SIZE;
