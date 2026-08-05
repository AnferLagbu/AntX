//! 异步 IPC 基础设施 — framework 层 re-export
//!
//! ## 迁移记录
//!
//! 策略代码 (AsyncPipeWriter/Reader, AsyncMsgSender/Receiver,
//! wait_for_condition) 于 2026-06-18 迁移到 services::io::async_ipc.
//! 本文件仅 re-export 保持调用方兼容.
//!
//! 注意: 原 framework 版本中 Message 字段名 (msg_type/sender_pid)
//! 与 services/types.rs 中实际字段名 (type_/sender) 不一致,
//! 迁移时已修正。

#[cfg(feature = "async")]
pub use crate::kernel::services::ipc::async_ipc::{
    AsyncMsgReceiver, AsyncMsgSender, AsyncPipeReader, AsyncPipeWriter, wait_for_condition,
};
