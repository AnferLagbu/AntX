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

### 现状 (2026-07-20 审计更新)

- **描述**: 0/6 完成全量迁移。所有驱动仅完成 MMIO/PIO 寄存器包装层迁移 (~14%)，业务逻辑 (DMA/队列/命令提交/数据路径) 仍留在 framework。
- **方案**: 见下方进度表。实际审计显示 framework 仍保留 ~282 个 unsafe 块。
- **状态: []**

| Phase | 驱动 | 文档状态 | 实际迁移率 | 说明 |
|-------|------|---------|-----------|------|
| 2.1.1 | E1000 网卡 | ~~[X]~~ → [] | ~12% | 仅 MMIO 寄存器包装；DMA/描述符/收发/中断处理仍在 framework (38 unsafe) |
| 2.1.2 | VirtIO 传输层 | ~~[X]~~ → [] | ~28% | 传输层 MMIO 已迁移；queue.rs/blk.rs/net.rs 仍留在 framework (37 unsafe) |
| 2.1.3 | NVMe 存储 | [] | ~27% | 寄存器抽象已迁移；队列管理/DMA PRP/命令提交仍在 framework (~20 unsafe) |
| 2.1.4 | AHCI/ATA 存储 | [] | ~13% | AHCI HBA 寄存器已迁移；FIS/DMA/命令队列仍在 framework；ATA 0% 迁移 |
| 2.1.5 | 字符/显示设备 | [] | ~10% | Serial+VGA 文本模式已迁移；PL011/Display 0% (6,078 行未迁移) |
| 2.1.6 | USB/XHCI | [] | ~12% | xHCI MMIO 已迁移；传输环/枚举/HID/大容量存储 0% (3,700+ 行未迁移) |

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
