#![deny(unsafe_code)]
//! 设备驱动 — 网卡/存储/显示/输入 (services 层)
//!
//! ## 当前状态: ⏳ 5/6 未迁移 (Phase 2.1 在途)
//!
//! 已迁移 (演示级 safe API, 0 unsafe):
//! - [net/e1000.rs](file:///home/anfer/Code/AntX/src/kernel/services/driver/net/e1000.rs) — E1000 网卡 (Phase 2.1.1)
//! - [virtio/transport.rs](file:///home/anfer/Code/AntX/src/kernel/services/driver/virtio/transport.rs) — VirtIO MMIO Transport (Phase 2.1.2/2.1.3 共享底层)
//!
//! 未迁移 (实际实现仍在 `kernel/driver/`):
//! - [kernel/driver/char/](file:///home/anfer/Code/AntX/src/kernel/driver/char/) — 字符设备 (vga/serial/pl011) → Phase 2.1.5
//! - [kernel/driver/display/](file:///home/anfer/Code/AntX/src/kernel/driver/display/) — 显示 (framebuffer/HDMI/DP) → Phase 2.1.5
//! - [kernel/driver/storage/](file:///home/anfer/Code/AntX/src/kernel/driver/storage/) — 存储 (ATA/AHCI/NVMe) → Phase 2.1.3/2.1.4
//! - [kernel/driver/usb/](file:///home/anfer/Code/AntX/src/kernel/driver/usb/) — USB (xHCI) → Phase 2.1.6
//!
//! ## 迁移路径
//!
//! 1. 所有 MMIO 走 `framework::iomem::IoMem`
//! 2. 所有 PIO 走 `framework::ioport::IoPort`
//! 3. 所有 DMA 走 `framework::dma_buf::DmaStream`
//! 4. 中断处理走 `framework::irqline::IrqLine`
//! 5. 在 services/driver/ 暴露纯 safe 驱动 API
//!
//! ## Phase 2.1 进度 (2026-06-04)
//!
//! | 子任务 | 驱动 | 状态 | safe wrapper |
//! |--------|------|------|--------------|
//! | 2.1.1  | E1000 网卡 | ✅ | `net/e1000.rs` (138 行) |
//! | 2.1.2  | VirtIO-Net | 🟡 (transport 通用层已就绪) | `virtio/transport.rs` |
//! | 2.1.3  | NVMe 存储 | ⏳ | 待 `storage/nvme.rs` |
//! | 2.1.4  | AHCI/ATA  | ⏳ | 待 `storage/ahci.rs` |
//! | 2.1.5  | VGA/串口/Framebuffer | ⏳ | 待 `char/vga.rs` 等 |
//! | 2.1.6  | USB/XHCI | ⏳ | 待 `usb/xhci.rs` |
//!
//! 进度: 1/6 → 2/6 (transport 为后续 2.1.2/2.1.3 共享底层, 视为部分完成)

pub mod virtio;
pub mod char;
pub mod storage;
pub mod usb;
