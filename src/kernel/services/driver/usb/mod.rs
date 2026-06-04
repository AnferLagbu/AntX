#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! USB 驱动 — services 层 (Phase 2.1.6)
//!
//! 包含 USB 主机控制器驱动的 100% safe API。
//!
//! ## 模块结构
//!
//! - [xhci] — xHCI 主机控制器 (USB 3.0), 0 unsafe
//!
//! ## 后续添加
//!
//! - `ehci.rs`  — EHCI (USB 2.0) (Phase 2.1.6 后续)
//! - `uhci.rs`  — UHCI (USB 1.1) (Phase 2.1.6 后续)
//! - `ohci.rs`  — OHCI (USB 1.1) (Phase 2.1.6 后续)
//!
//! 评估日期: 2026-06-04

pub mod xhci;
