//! DMA 引擎 API 层
//!
//! 一致性 DMA 内存管理、ioremap MMIO 映射、流式 DMA、散射聚集列表的统一入口。
//!
//! ## 调用方契约
//! - `driver::storage::nvme` —— NVMe 命令队列的 DMA 缓冲区
//! - `driver::storage::ahci` —— AHCI PRDT 表的 DMA 映射
//! - `driver::net::e1000` —— E1000 收发描述符的 DMA 映射
//! - `driver::virtio::blk` / `driver::virtio::net` —— VirtIO 队列的 DMA 映射
//! - `fs::hvfs` —— HvFS 页缓存直接 I/O (通过 DMA 绕过 CPU)
//!
//! ## 内部接口
//! - `mod.rs` —— `DmaMapping`, `DmaTransfer`, `DmaScatterList`, `DmaStats`, `virt_to_phys`
//! - `engine.rs` —— `DmaEngine` trait, `get_dma()` 全局单例
//!
//! ## 安全约束
//! - `DmaTransfer.callback` 函数指针在 ISR 上下文调用, 不得持有锁或 sleep
//! - `DmaMapping.cpu_addr` 和 `dma_addr` 必须保持一致性 (x86: 自动; aarch64: 需显式 cache flush)
//! - MMIO 映射的虚拟地址范围: [0xFFFF900000000000, ...)
//! - `DmaStats` 使用 lock-free AtomicU64, 可在中断上下文更新
//!
//! ## 性能特征
//! - 映射查找: O(1) 哈希 (DMA_MAX_MAPPINGS = 256)
//! - 统计更新: lock-free atomic, 无竞争开销
//! - 散射聚集: 最多 64 条目, O(N) 线性

pub use super::{
    DmaCachePolicy, DmaDirection,
    DmaMapping, DmaScatterEntry, DmaScatterList,
    DmaPoolStats, DmaStats, DmaTransfer,
    DmaCallback,
    DMA_MAX_MAPPINGS, DMA_MAX_SCATTER_ENTRIES, MMIO_VIRT_BASE,
    get_dma,
};
