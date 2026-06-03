//! 设备驱动 — 网卡/存储/显示/输入 (services 层)
//!
//! @SAFE: 所有 MMIO/PIO/DMA 操作通过 framework::IoMem/IoPort/DmaStream 进行。
