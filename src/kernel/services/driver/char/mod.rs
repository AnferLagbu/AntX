#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! 字符设备驱动 — services 层 (Phase 2.1.5)
//!
//! 包含文本模式显示的 100% safe API,
//! 为内核早期控制台和字符设备提供统一接口。
//!
//! ## 模块结构
//!
//! - [vga] — VGA 文本模式 (0xB8000 MMIO + 0x3D4/0x3D5 PIO), 0 unsafe
//! - [serial] — 16550 UART 串口 (COM1-COM4 PIO), 0 unsafe
//!
//! ## 后续添加
//!
//! - `pl011.rs` — ARM PL011 UART (Phase 2.1.5 后续)
//!
//! 评估日期: 2026-06-04

pub mod vga;
pub mod serial;
