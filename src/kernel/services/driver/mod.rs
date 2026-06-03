//! 设备驱动 — 网卡/存储/显示/输入 (services 层占位)
//!
//! ## 当前状态: ⏳ 未迁移 (除 E1000 演示)
//!
//! 实际实现仍在 `kernel/driver/` 老位置:
//! - [kernel/driver/bus/pci.rs](file:///home/anfer/Code/AntX/src/kernel/driver/bus/pci.rs) — PCI 枚举
//! - [kernel/driver/char/](file:///home/anfer/Code/AntX/src/kernel/driver/char/) — 字符设备 (vga/serial/pl011)
//! - [kernel/driver/display/](file:///home/anfer/Code/AntX/src/kernel/driver/display/) — 显示 (framebuffer/HDMI/DP)
//! - [kernel/driver/storage/](file:///home/anfer/Code/AntX/src/kernel/driver/storage/) — 存储 (ATA/AHCI/NVMe)
//! - [kernel/driver/usb/](file:///home/anfer/Code/AntX/src/kernel/driver/usb/) — USB (xHCI)
//! - [kernel/driver/virtio/](file:///home/anfer/Code/AntX/src/kernel/driver/virtio/) — virtio 设备
//!
//! ## 已迁移 (演示级)
//!
//! - [services/driver/net/e1000.rs](file:///home/anfer/Code/AntX/src/kernel/services/driver/net/e1000.rs) — E1000 网卡走 `framework::IoMem` (138 行, 0 unsafe)
//!
//! ## 迁移路径
//!
//! 1. 所有 MMIO 走 `framework::iomem::IoMem`
//! 2. 所有 PIO 走 `framework::ioport::IoPort`
//! 3. 所有 DMA 走 `framework::dma::DmaStream`
//! 4. 中断处理走 `framework::irqline::IrqLine`
//! 5. 在 services/driver/ 暴露纯 safe 驱动 API
//!
//! ## 估算: 2 人月 (最重的一块)
//!
//! 评估日期: 2026-06-03
//! Phase 2.1 任务书标记"全部 6/6 ✅"但实际只有 1/6 演示级实现
