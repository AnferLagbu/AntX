//! @SAFE: 本文件不含 unsafe 代码。
//!
//! VirtIO 设备驱动 — services 层 (Phase 2.1.2 + 2.1.3 演示)
//!
//! 包含 VirtIO MMIO Transport 的 services 层安全 API,
//! 为 virtio-blk (Phase 2.1.3) 和 virtio-net (Phase 2.1.2) 提供 100% safe 的设备发现路径。
//!
//! ## 模块结构
//!
//! - [transport] — VirtIO MMIO Transport 安全代理, 0 unsafe
//!
//! ## 迁移状态
//!
//! - [transport::VirtioDevice] — MMIO 读写 + 状态机 + 中断 + 队列配置全 100% safe
//! - virtio-blk 块设备 (Phase 2.1.3) — 后续在 `blk.rs` 添加
//! - virtio-net 网卡 (Phase 2.1.2) — 后续在 `net.rs` 添加
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.2/2.1.3 任务

pub mod transport;
