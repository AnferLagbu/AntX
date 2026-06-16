//! Unix Domain Socket (AF_UNIX) — framework 层 re-export
//!
//! ## T3-4 迁移记录
//!
//! 策略代码 (socket CRUD + 路径绑定 + STREAM/DGRAM 数据传输)
//! 已于 2026-06-16 迁移到 services::net::unix.
//! 本文件仅 re-export 保持调用方兼容.

// Unix socket 实现已迁移至 services 层, 保留文件级 allow: re-export 仅 pub use,
// 待调用方全部迁移后可移除.
#![allow(dead_code)]

pub use crate::kernel::services::net::unix::{
    UDS_FD_BASE, MAX_UDS_FD, UNIX_PATH_MAX, UNIX_MAX_BINDINGS,
    UNIX_STREAM_BUF, UNIX_DGRAM_MAX, UNIX_LISTEN_BACKLOG,
    UdsError, UnixSockType, UnixSockState,
    UnixSocket, UnixPathBinding, UdsState, UDS_STATE,
    uds_init, uds_create, uds_bind, uds_listen, uds_accept,
    uds_connect, uds_send, uds_recv, uds_sendto, uds_recvfrom,
    uds_close, uds_unlink, uds_reset_for_test,
};
