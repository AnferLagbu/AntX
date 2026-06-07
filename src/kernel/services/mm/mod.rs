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
pub mod brk;
pub mod mprotect;
