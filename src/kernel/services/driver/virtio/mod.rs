#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! `VirtIO` 设备驱动 — services 层 (Phase 2.1.2 + 2.1.3)
//!
//! 包含 `VirtIO` MMIO Transport 的 services 层安全 API,
//! 为 virtio-blk (Phase 2.1.3) 和 virtio-net (Phase 2.1.2) 提供 100% safe 的设备驱动。
//!
//! ## 模块结构
//!
//! - [transport] — `VirtIO` MMIO Transport 安全代理, 0 unsafe
//! - [blk] — `VirtIO` 块设备安全驱动, 0 unsafe
//! - [net] — `VirtIO` 网络设备安全驱动, 0 unsafe
//!
//! ## 迁移状态
//!
//! - [`transport::VirtioDevice`] — MMIO 读写 + 状态机 + 中断 + 队列配置全 100% safe
//! - [`blk::VirtioBlkDriver`] — 块设备初始化 + 特性协商 + 配置读取全 100% safe
//! - [`net::VirtioNetDriver`] — 网卡初始化 + 特性协商 + MAC/链路读取全 100% safe
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.2/2.1.3 任务

pub mod blk;
pub mod net;
pub mod transport;
