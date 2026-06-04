#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! 存储设备驱动 — services 层 (Phase 2.1.3 + 2.1.4)
//!
//! 包含块设备控制器的 100% safe API,
//! 为内核块设备栈 (BlockDevice trait) 提供安全抽象。
//!
//! ## 模块结构
//!
//! - [nvme]  — NVMe 控制器 (Phase 2.1.3 演示级), 0 unsafe
//! - [ahci]  — AHCI SATA 控制器 (Phase 2.1.4 演示级), 0 unsafe
//!
//! ## 后续添加
//!
//! - `ata.rs` — 传统 ATA PIO 驱动 (Phase 2.1.4 后续)
//!
//! 评估日期: 2026-06-04

pub mod nvme;
pub mod ahci;
