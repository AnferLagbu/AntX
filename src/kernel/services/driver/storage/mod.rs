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
//! - [nvme]  — NVMe 控制器 (Phase 2.1.3), 0 unsafe, 完整驱动逻辑
//! - [ahci]  — AHCI SATA 控制器 (Phase 2.1.4), 0 unsafe, 完整驱动逻辑
//! - [ata]   — 传统 ATA PIO 驱动 (Phase 2.1.4), 0 unsafe, 桩模块
//!
//! ## 架构
//!
//! - 所有 MMIO 通过 `framework::IoMem` 安全代理
//! - 所有 DMA 通过 framework safe wrapper (nvme_alloc_* / ahci_alloc_*)
//! - 命令构造在 services 层 (safe), 提交通过 framework safe function
//! - 零 unsafe: services 层严格遵守 `#![deny(unsafe_code)]`
//!
//! 评估日期: 2026-06-04

pub mod nvme;
pub mod ahci;
/// 传统 ATA PIO 驱动桩模块
pub mod ata;
