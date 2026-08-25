//! E1000 网卡驱动 (framework 层)
//!
//! 子模块:
//! - `dma_ring`: 描述符结构 + 状态/命令常量 (B04-19 从 services 上移)
//! - `e1000_io`: E1000Io MMIO 安全访问器 + 寄存器偏移常量 (B04-AUDIT-005 #4 修复: 从 services 迁回)
//! - `e1000`: TxRing/RxRing 安全包装 + E1000Device 设备

pub mod dma_ring;
pub mod e1000;
pub mod e1000_io;
