# AntX 高级硬件驱动开发指南

## 📋 概述

本文档介绍 AntX 内核的高级硬件驱动，包括 USB、HDMI、DisplayPort 等。

## 🔧 已实现的驱动

### 1. USB 子系统

#### USB 核心框架 (`usb/usb_core.rs`)

**功能**：
- 设备枚举和配置
- URB (USB Request Block) 管理
- 设备类驱动支持
- 热插拔检测

**架构**：
```text
USB Core
├── 设备枚举
├── 描述符解析
├── URB调度
└── 类驱动注册
```

**使用示例**：
```rust
use crate::kernel::driver::usb::{UsbCore, DeviceClass};

let mut usb_core = UsbCore::new();
usb_core.init().unwrap();

// 查找HID设备
if let Some(device) = usb_core.find_device_by_class(DeviceClass::Hid) {
    // 使用HID设备
}

// 根据VID/PID查找设备
if let Some(device) = usb_core.find_device_by_vid_pid(0x1234, 0x5678) {
    // 使用特定设备
}
```

**关键数据结构**：
- `DeviceDescriptor`: 设备描述符 (18字节)
- `ConfigurationDescriptor`: 配置描述符
- `InterfaceDescriptor`: 接口描述符
- `EndpointDescriptor`: 端点描述符
- `Urb`: USB请求块

---

#### xHCI 主机控制器 (`usb/xhci.rs`)

**功能**：
- USB 3.0 SuperSpeed (5 Gbps)
- USB 2.0 兼容 (480 Mbps)
- 多端口支持 (最多256个)
- DMA传输优化

**硬件寄存器**：
```text
xHCI Registers:
├── Capability Registers
│   ├── CAPLENGTH (0x00)
│   ├── HCSPARAMS1-3
│   └── HCCPARAMS1-2
├── Operational Registers
│   ├── USBCMD (0x00)
│   ├── USBSTS (0x04)
│   └── PORTSC (0x400+)
└── Runtime Registers
    ├── Interrupter Management
    └── Event Ring
```

**使用示例**：
```rust
use crate::kernel::driver::usb::xhci::XhciController;

let mut xhci = XhciController::new(0xFE000000);  // MMIO基地址
xhci.init().unwrap();

// 检查端口
for port in 0..xhci.num_ports() {
    if xhci.port_has_device(port) {
        let speed = xhci.get_port_speed(port);
        // 处理设备
    }
}
```

**TRB (Transfer Request Block)**：
- Normal TRB: 普通数据传输
- Setup Stage TRB: 控制传输设置
- Data Stage TRB: 控制传输数据
- Status Stage TRB: 控制传输状态

---

### 2. 显示子系统

#### HDMI 驱动 (`display/hdmi.rs`)

**功能**：
- EDID自动读取和解析
- 视频模式自动配置
- 热插拔检测
- 音频支持

**EDID解析**：
```rust
use crate::kernel::driver::display::{HdmiController, Edid};

let mut hdmi = HdmiController::new(0xFE000000);
hdmi.init().unwrap();

// 读取EDID
if let Some(edid) = hdmi.get_edid() {
    println!("Manufacturer: {:?}", edid.manufacturer);
    println!("Product: 0x{:04X}", edid.product_code);
    
    // 获取首选分辨率
    if let Some((width, height)) = edid.preferred_resolution() {
        println!("Preferred: {}x{}", width, height);
    }
}
```

**视频模式**：
```rust
use crate::kernel::driver::display::{VideoMode, STANDARD_VIDEO_MODES};

// 查找合适的视频模式
for mode in STANDARD_VIDEO_MODES {
    if mode.width == 1920 && mode.height == 1080 {
        hdmi.set_video_mode(*mode).unwrap();
        break;
    }
}
```

**标准视频模式**：
- 640x480 @ 60Hz
- 800x600 @ 60Hz
- 1024x768 @ 60Hz
- 1280x720 @ 60Hz (720p)
- 1920x1080 @ 60Hz (1080p)
- 2560x1440 @ 60Hz (2K)
- 3840x2160 @ 60Hz (4K)

---

#### DisplayPort 驱动 (`display/dp.rs`)

**功能**：
- AUX通道通信
- DPCD (DisplayPort Configuration Data) 读取
- 链路训练
- MST (Multi-Stream Transport) 支持

**链路速率**：
- RBR: 1.62 Gbps/lane
- HBR: 2.7 Gbps/lane
- HBR2: 5.4 Gbps/lane
- HBR3: 8.1 Gbps/lane

**使用示例**：
```rust
use crate::kernel::driver::display::{DpController, LinkRate, LaneCount};

let mut dp = DpController::new(0xFE000000);
dp.init().unwrap();

// 读取DPCD
if let Some(dpcd) = dp.dpcd.as_ref() {
    println!("Max link rate: {:?}", dpcd.max_link_rate);
    println!("Max lane count: {:?}", dpcd.max_lane_count);
}

// 检查链路状态
if dp.is_link_trained() {
    if let Some(bw) = dp.get_bandwidth_gbps() {
        println!("Bandwidth: {} Gbps", bw);
    }
}
```

**链路训练流程**：
```text
1. 检测热插拔 (HPD)
2. 读取DPCD
3. 选择链路速率和通道数
4. 链路训练阶段1 (Clock Recovery)
5. 链路训练阶段2 (Channel Equalization)
6. 验证链路状态
```

---

## 🚀 QEMU 测试

### USB 测试

QEMU支持USB控制器模拟：

```bash
# 启动QEMU with USB 3.0 (xHCI)
qemu-system-x86_64 \
    -device qemu-xhci \
    -device usb-kbd \
    -device usb-mouse \
    -device usb-storage,drive=usbdrive \
    -drive if=none,id=usbdrive,file=usb.img

# 启动QEMU with USB 2.0 (EHCI)
qemu-system-x86_64 \
    -device usb-ehci \
    -device usb-kbd
```

### 显示测试

QEMU支持多种显示输出：

```bash
# HDMI输出
qemu-system-x86_64 \
    -device qxl-vga,vgamem_mb=64 \
    -display gtk

# DisplayPort输出 (使用virtio-gpu)
qemu-system-x86_64 \
    -device virtio-gpu-pci \
    -display gtk
```

---

## 📊 驱动架构

### USB 子系统架构

```text
┌─────────────────────────────────────┐
│         USB 类驱动                   │
│  ┌─────┐ ┌─────┐ ┌─────┐          │
│  │ HID │ │ MSC │ │ UVC │ ...      │
│  └─────┘ └─────┘ └─────┘          │
└─────────────────────────────────────┘
              ↕
┌─────────────────────────────────────┐
│         USB 核心                     │
│  ┌──────────────────────────────┐  │
│  │ 设备枚举 │ URB管理 │ 热插拔  │  │
│  └──────────────────────────────┘  │
└─────────────────────────────────────┘
              ↕
┌─────────────────────────────────────┐
│       主机控制器驱动                 │
│  ┌──────┐ ┌──────┐ ┌──────┐       │
│  │ xHCI │ │ EHCI │ │ UHCI │       │
│  └──────┘ └──────┘ └──────┘       │
└─────────────────────────────────────┘
              ↕
┌─────────────────────────────────────┐
│          硬件层                      │
│  USB 3.0 │ USB 2.0 │ USB 1.1       │
└─────────────────────────────────────┘
```

### 显示子系统架构

```text
┌─────────────────────────────────────┐
│         显示控制器                   │
│  ┌──────┐ ┌────┐ ┌─────┐          │
│  │ HDMI │ │ DP │ │ DVI │          │
│  └──────┘ └────┘ └─────┘          │
└─────────────────────────────────────┘
              ↕
┌─────────────────────────────────────┐
│       显示接口抽象层                 │
│  ┌──────────────────────────────┐  │
│  │ EDID │ 视频模式 │ 链路训练  │  │
│  └──────────────────────────────┘  │
└─────────────────────────────────────┘
              ↕
┌─────────────────────────────────────┐
│          硬件层                      │
│  TMDS │ Main Link │ AUX            │
└─────────────────────────────────────┘
```

---

## 📁 文件结构

```
src/kernel/driver/
├── usb/
│   ├── mod.rs           # USB模块注册
│   ├── usb_core.rs      # USB核心框架
│   └── xhci.rs          # xHCI控制器
├── display/
│   ├── mod.rs           # 显示模块注册
│   ├── hdmi.rs          # HDMI驱动
│   └── dp.rs            # DisplayPort驱动
└── vga.rs               # VGA驱动
```

---

## 🎯 开发建议

### 添加新的USB类驱动

1. 在 `src/kernel/driver/usb/` 下创建新文件
2. 实现类驱动接口
3. 在 `usb_core.rs` 中注册类驱动
4. 编写测试代码

### 添加新的显示接口驱动

1. 在 `src/kernel/driver/display/` 下创建新文件
2. 实现显示控制器接口
3. 支持EDID读取和视频模式配置
4. 编写测试代码

### 调试技巧

**USB调试**：
```bash
# QEMU USB调试
qemu-system-x86_64 -device qemu-xhci -d usb

# 查看USB设备
lsusb -v
```

**显示调试**：
```bash
# QEMU显示调试
qemu-system-x86_64 -device qxl-vga -d guest_errors

# 查看EDID
xrandr --props
```

---

## 🔍 性能优化

### USB性能

- **批量传输优化**：使用大缓冲区减少URB数量
- **DMA优化**：使用连续物理内存
- **中断聚合**：减少中断频率

### 显示性能

- **链路训练优化**：选择最高链路速率
- **多通道优化**：使用所有可用通道
- **压缩支持**：DSC (Display Stream Compression)

---

## 📚 参考资料

### USB规范
- [USB 3.0 Specification](https://www.usb.org/documents)
- [xHCI Specification 1.1](https://www.intel.com/content/www/us/en/products/docs/io/xhci-spec.html)

### 显示规范
- [HDMI Specification 2.1](https://www.hdmi.org/spec/hdmi2_1)
- [DisplayPort Specification 2.0](https://www.vesa.org/vesa-standards/)
- [EDID Standard](https://www.vesa.org/vesa-standards/)

### QEMU文档
- [QEMU USB Emulation](https://qemu.org/docs/master/system/devices/usb.html)
- [QEMU Display](https://qemu.org/docs/master/system/devices/display.html)

---

## 📝 更新日志

**2026-05-18**：
- ✅ 实现USB核心框架
- ✅ 实现xHCI主机控制器驱动
- ✅ 实现HDMI驱动和EDID解析
- ✅ 实现DisplayPort驱动和链路训练
- ✅ 添加标准视频模式支持
- ✅ 创建驱动文档

---

**最后更新**：2026-05-18  
**维护者**：AntX Team
