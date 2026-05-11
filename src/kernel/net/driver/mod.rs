//! 网络驱动模块 (Network Driver Module)
//!
//! 提供网卡驱动的统一接口和实现：
//! - **E1000**: Intel 82540EM 千兆网卡驱动
//! - **Driver Trait**: 统一的网络设备抽象
//!
//! ## 架构设计
//!
//! ```text
//! Network Driver Subsystem
//! ├── driver/
//! │   ├── mod.rs        # 模块入口
//! │   └── e1000.rs      # Intel E1000 驱动 (Rust)
//! │
//! ├── netif.rs          # 网络接口管理 (lwIP 对接)
//! ```

/// Intel E1000 网卡驱动
pub mod e1000;

// 导出主要类型
pub use e1000::E1000Device;
