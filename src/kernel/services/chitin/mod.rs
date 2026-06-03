//! Chitin 设备驱动框架 (services 层占位)
//!
//! ## 当前状态: ⏳ 未迁移
//!
//! 实际实现仍在 `kernel/chitin/` 老位置:
//! - [kernel/chitin/devtree.rs](file:///home/anfer/Code/AntX/src/kernel/chitin/devtree.rs) — 设备树
//! - [kernel/chitin/composite.rs](file:///home/anfer/Code/AntX/src/kernel/chitin/composite.rs) — 复合设备
//! - [kernel/chitin/proto_*.rs](file:///home/anfer/Code/AntX/src/kernel/chitin/) — 6 个协议族 (block/char/input/net/...)
//! - [kernel/chitin/user_driver.rs](file:///home/anfer/Code/AntX/src/kernel/chitin/user_driver.rs) — 用户态驱动
//!
//! ## 迁移路径
//!
//! 1. 设备注册表走 `framework::sync::SpinLock` (55 unsafe 行集中在 mod.rs)
//! 2. 协议族 trait 走 framework 抽象
//! 3. 在 services/chitin/ 暴露 `pub fn register`, `pub fn lookup` 等纯 safe API
//!
//! ## 估算: 1 人月
//!
//! 评估日期: 2026-06-03
