//! 网络栈 — smoltcp + 驱动适配 (services 层占位)
//!
//! ## 当前状态: ⏳ 未迁移
//!
//! 实际实现仍在 `kernel/net/` 老位置:
//! - [kernel/net/init.rs](file:///home/anfer/Code/AntX/src/kernel/net/init.rs) — smoltcp 初始化 (42 unsafe)
//! - [kernel/net/smoltcp/](file:///home/anfer/Code/AntX/src/kernel/net/smoltcp/) — smoltcp vendored 协议栈
//! - [kernel/net/driver/e1000.rs](file:///home/anfer/Code/AntX/src/kernel/net/driver/e1000.rs) — Intel e1000
//!
//! ## 迁移路径
//!
//! 1. e1000 驱动走 `framework::iomem::IoMem` (services/driver/net/e1000.rs 已有演示)
//! 2. smoltcp FFI 调用走 `framework::dma::DmaStream`
//! 3. 在 services/net/ 暴露 `pub fn init`, `pub fn poll` 等纯 safe API
//!
//! ## 估算: 1 人月
//!
//! 评估日期: 2026-06-03
//! 关键依赖: framework::iomem / framework::dma / framework::irqline 必须先就绪
