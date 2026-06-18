#![deny(unsafe_code)]
//! 内存管理 — services 层安全代理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::mm。
//!
//! ## 职责
//!
//! - Page Cache: 文件内容缓存的安全 API
//! - Swap: 页面换出/换入的安全 API
//! - mmap: 文件映射的安全参数验证与 VFS 交互

pub mod pcache;
pub mod swap;
pub mod mmap;
pub mod mremap;
pub mod brk;
pub mod mprotect;
/// madvise / mlock / mincore 系统调用策略
pub mod madvise_mlock;
/// D3: NUMA 安全封装
pub mod numa;
/// D9: 内存压力策略 (阈值/分级/判定) — services 层
pub mod memory_pressure;

// memory_pressure 公共接口 re-export — T-02 策略-机制分离
pub use memory_pressure::{PressureAwareAllocPolicy, register_pressure_aware_policy};

/// services::mm 初始化 — 注册策略到 framework
///
/// 在 framework::mm 初始化完成后调用一次.
pub fn init() {
    // T-02: 注册 services 层分配决策策略
    let _ = memory_pressure::register_pressure_aware_policy();
}
