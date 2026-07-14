# 驱动层服务迁移计划

> Phase 2.1: 将 framework/driver 中的设备驱动迁移到 services/driver，实现 100% safe Rust 驱动 API.

## 工程计划: 驱动服务迁移 (Phase 2.1)

### 背景

- **描述**: 框内核要求 services 层 0 unsafe。当前 framework/driver 包含大量设备驱动实现 (含 unsafe)，需迁移到 services/driver 通过 framework 安全代理 (IoMem/IoPort/DmaStream/IrqLine) 访问硬件。
- **方案**: 每个驱动按统一路径迁移：MMIO → `IoMem`，PIO → `IoPort`，DMA → `DmaStream`，IRQ → `IrqLine`。services/driver 暴露纯 safe API。
- **状态: [X]**

### 目标

- **描述**: 所有设备驱动的业务逻辑迁移到 services/driver，framework/driver 仅保留 TCB 级硬件抽象。
- **方案**: 逐驱动迁移，每完成一个驱动即验证编译 + 审计 + 测试。
- **状态: []**

### 现状 (2026-07-14)

- **描述**: 2/6 已完成，4/6 待迁移。
- **方案**: 见下方进度表。
- **状态: []**

| Phase | 驱动 | 状态 | framework 代码量 | services 文件 |
|-------|------|------|-----------------|--------------|
| 2.1.1 | E1000 网卡 | [X] | 1,126 行 (net/) | `net/e1000.rs` (138 行) |
| 2.1.2 | VirtIO 传输层 | [X] | 1,672 行 (virtio/) | `virtio/transport.rs` (460 行) |
| 2.1.3 | NVMe 存储 | [] | 3,555 行 (storage/) | `storage/nvme.rs` (骨架 357 行) |
| 2.1.4 | AHCI/ATA 存储 | [] | 同上 | `storage/ahci.rs` (骨架 495 行) |
| 2.1.5 | 字符/显示设备 | [] | char 1,748 行 + display 6,036 行 | 待建 |
| 2.1.6 | USB/XHCI | [] | 3,980 行 (usb/) | 待建 |

### 方案

- **迁移路径**
  - 描述: 统一 5 步迁移模式
  - 方案: (1) MMIO 走 `framework::iomem::IoMem` (2) PIO 走 `framework::ioport::IoPort` (3) DMA 走 `framework::dma_buf::DmaStream` (4) IRQ 走 `framework::irqline::IrqLine` (5) 在 services/driver 暴露纯 safe 驱动 API
  - 状态: [X]

- **Phase 2.1.3: NVMe 存储迁移**
  - 描述: 将 framework/driver/storage/nvme.rs + nvme_block.rs 迁移到 services/driver/storage/nvme.rs
  - 方案: NVMe 骨架已存在 (357 行, 零 unsafe), 需补全队列管理/DMA 提交/中断处理逻辑
  - 状态: []

- **Phase 2.1.4: AHCI/ATA 存储迁移**
  - 描述: 将 framework/driver/storage/ahci.rs + ahci_block.rs + ata.rs + ata_block.rs 迁移到 services/driver/storage/ahci.rs
  - 方案: AHCI 骨架已存在 (495 行, 零 unsafe), 需补全 HBA 命令队列/FIS 传输/DMA 描述符逻辑
  - 状态: []

- **Phase 2.1.5: 字符/显示设备迁移**
  - 描述: 将 framework/driver/char/ (serial.rs, vga.rs, pl011.rs) + display/ (framebuffer, HDMI, DP) 迁移到 services/driver/char/ + services/driver/display/
  - 方案: 字符设备 1,748 行 + 显示设备 6,036 行, 为最大迁移量。建议先迁移字符设备 (串口/VGA), 再迁移显示设备。
  - 状态: []

- **Phase 2.1.6: USB/XHCI 迁移**
  - 描述: 将 framework/driver/usb/ (xhci.rs, usb_core.rs, enumerate.rs, ring.rs, hid.rs, mass_storage.rs) 迁移到 services/driver/usb/
  - 方案: USB 子系统 3,980 行, 含 xHCI 控制器枚举/传输环/设备类驱动。建议分阶段: 先 xHCI 核心, 再 HID, 再 mass storage。
  - 状态: []

### 待办

- **NVMe 核心逻辑补全**
  - 描述: 在 services/driver/storage/nvme.rs 中补全 Admin/IO 提交队列管理、DMA 描述符、中断合并
  - 方案: 参考 framework/driver/storage/nvme.rs 的 unsafe 实现, 通过 IoMem/DmaStream/IrqLine 重写
  - 状态: []

- **AHCI 核心逻辑补全**
  - 描述: 在 services/driver/storage/ahci.rs 中补全 Command List/FIS 传输/DMA 引擎
  - 方案: 参考 framework/driver/storage/ahci.rs 的 unsafe 实现, 通过 IoMem/DmaStream 重写
  - 状态: []

- **字符设备迁移**
  - 描述: 迁移 serial (UART 16550) + VGA (文本模式) + pl011 (ARM 串口)
  - 方案: 串口迁移较简单 (PIO → IoPort), VGA 需 framebuffer MMIO → IoMem, pl011 走 MMIO → IoMem
  - 状态: []

- **显示设备迁移**
  - 描述: 迁移 framebuffer + HDMI + DisplayPort
  - 方案: framebuffer 迁移较直接 (MMIO → IoMem), HDMI/DP 含 DDC/I2C 和时序控制, 复杂度高
  - 状态: []

- **USB/XHCI 迁移**
  - 描述: 迁移 xHCI 控制器 + USB 核心 + 设备枚举 + HID + mass storage
  - 方案: xHCI 含 MMIO 寄存器 + DMA 环 + 中断, 复杂度最高。建议参考 Linux xHCI 驱动分层
  - 状态: []

### 决策记录

- **DECISION-001**
  - 描述: 迁移顺序: 网卡 → VirtIO → 存储 → 字符/显示 → USB
  - 方案: 按复杂度递增, 网卡/VirtIO 已验证迁移模式可行, 存储次之, USB 最复杂 (2026-06-04)
  - 状态: [X]
