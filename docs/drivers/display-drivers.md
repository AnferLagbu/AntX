# AntX 屏幕显示器驱动开发指南

## 📋 概述

本文档介绍 AntX 内核的屏幕显示器驱动系统，包括 Framebuffer、图形绘制、多显示器支持等。

## 🔧 已实现的组件

### 1. Framebuffer 驱动 (`display/framebuffer.rs`)

#### 功能特性
- **多种像素格式**：RGB565、RGB888、ARGB8888、BGR888、BGRA8888
- **图形绘制原语**：点、线、矩形、圆形
- **颜色转换**：不同格式间的转换
- **Alpha混合**：支持半透明效果

#### 使用示例

```rust
use crate::kernel::driver::display::{
    Framebuffer,
    PixelFormat,
    Color,
    Rect,
    colors,
};

// 创建Framebuffer
let mut fb = Framebuffer::new(
    0xE0000000 as *mut u8,  // 帧缓冲地址
    1920,                    // 宽度
    1080,                    // 高度
    1920 * 4,                // pitch (每行字节数)
    PixelFormat::Argb8888,   // 像素格式
);

fb.init().unwrap();

// 清屏
fb.clear();

// 绘制红色矩形
fb.fill_rect(Rect::new(100, 100, 200, 150), colors::RED);

// 绘制绿色圆形
fb.draw_circle(400, 300, 50, colors::GREEN);

// 绘制蓝色线条
fb.draw_line(0, 0, 1920, 1080, colors::BLUE);

// 设置单个像素
fb.set_pixel(500, 500, Color::new(255, 128, 0));
```

#### 像素格式

| 格式 | 位深 | 字节数 | 说明 |
|------|------|--------|------|
| RGB565 | 16位 | 2 | 5-6-5分布 |
| RGB888 | 24位 | 3 | R-G-B顺序 |
| ARGB8888 | 32位 | 4 | 带Alpha通道 |
| BGR888 | 24位 | 3 | B-G-R顺序 |
| BGRA8888 | 32位 | 4 | 带Alpha通道 |

#### 图形绘制函数

```rust
// 绘制点
fb.set_pixel(x, y, color);

// 绘制水平线
fb.draw_hline(x, y, length, color);

// 绘制垂直线
fb.draw_vline(x, y, length, color);

// 绘制线条 (Bresenham算法)
fb.draw_line(x0, y0, x1, y1, color);

// 绘制矩形边框
fb.draw_rect(rect, color);

// 填充矩形
fb.fill_rect(rect, color);

// 绘制圆形 (中点圆算法)
fb.draw_circle(cx, cy, radius, color);

// 填充整个屏幕
fb.fill(color);

// 清屏
fb.clear();
```

---

### 2. 显示控制器抽象 (`display/controller.rs`)

#### 显示管理器

```rust
use crate::kernel::driver::display::{
    DisplayManager,
    MonitorInfo,
    DisplayMode,
    DisplayOutput,
};

// 创建显示管理器
let mut manager = DisplayManager::new();

// 注册显示器
let monitor = MonitorInfo::new(1, DisplayOutput::Hdmi);
let index = manager.register_monitor(monitor);

// 检测所有显示器
manager.detect_all();

// 启用显示器
manager.enable_monitor(index)?;

// 设置显示模式
let mode = DisplayMode::new(1920, 1080, 60, PixelFormat::Argb8888);
manager.set_display_mode(index, mode)?;

// 获取主显示器
let primary = manager.get_primary_monitor();

// 获取活动显示器
let active = manager.get_active_monitor();
```

#### 显示器信息

```rust
pub struct MonitorInfo {
    pub id: u32,                    // 显示器ID
    pub name: [u8; 32],             // 显示器名称
    pub output: DisplayOutput,      // 输出类型
    pub connected: bool,            // 是否已连接
    pub enabled: bool,              // 是否启用
    pub current_mode: DisplayMode,  // 当前模式
    pub supported_modes: Vec<DisplayMode>, // 支持的模式
    pub physical_width: u32,        // 物理宽度(mm)
    pub physical_height: u32,       // 物理高度(mm)
}
```

#### 显示模式

```rust
pub struct DisplayMode {
    pub width: u32,              // 宽度(像素)
    pub height: u32,             // 高度(像素)
    pub refresh_rate: u32,       // 刷新率(Hz)
    pub pixel_format: PixelFormat, // 像素格式
    pub preferred: bool,         // 是否首选
}

// 计算像素时钟
let pixel_clock = mode.pixel_clock_khz();

// 计算带宽
let bandwidth = mode.bandwidth_mbps();
```

---

### 3. HDMI 驱动 (`display/hdmi.rs`)

#### EDID读取

```rust
use crate::kernel::driver::display::HdmiController;

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

#### 视频模式配置

```rust
// 获取支持的视频模式
let modes = hdmi.get_supported_modes();

// 设置视频模式
for mode in &modes {
    if mode.width == 1920 && mode.height == 1080 {
        hdmi.set_video_mode(*mode)?;
        break;
    }
}
```

---

### 4. DisplayPort 驱动 (`display/dp.rs`)

#### 链路训练

```rust
use crate::kernel::driver::display::DpController;

let mut dp = DpController::new(0xFE000000);
dp.init().unwrap();

// 检查链路状态
if dp.is_link_trained() {
    if let Some(bw) = dp.get_bandwidth_gbps() {
        println!("Bandwidth: {} Gbps", bw);
    }
}

// 读取DPCD
if let Some(dpcd) = dp.dpcd.as_ref() {
    println!("Max link rate: {:?}", dpcd.max_link_rate);
    println!("Max lane count: {:?}", dpcd.max_lane_count);
}
```

#### 链路速率

| 速率 | 带宽 (每通道) | 最大带宽 (4通道) |
|------|--------------|-----------------|
| RBR | 1.62 Gbps | 6.48 Gbps |
| HBR | 2.7 Gbps | 10.8 Gbps |
| HBR2 | 5.4 Gbps | 21.6 Gbps |
| HBR3 | 8.1 Gbps | 32.4 Gbps |

---

## 📊 显示系统架构

```
┌─────────────────────────────────────┐
│      应用层 (GUI/控制台)            │
└─────────────────────────────────────┘
                ↓
┌─────────────────────────────────────┐
│        显示管理器                    │
│  DisplayManager                     │
│  ├── 多显示器管理                   │
│  ├── 显示模式切换                   │
│  └── 热插拔支持                     │
└─────────────────────────────────────┘
                ↓
┌─────────────────────────────────────┐
│      显示控制器抽象层                │
│  DisplayController Trait            │
└─────────────────────────────────────┘
                ↓
┌─────────────────────────────────────┐
│        Framebuffer 驱动              │
│  ├── 像素格式转换                   │
│  ├── 图形绘制                       │
│  └── 双缓冲支持                     │
└─────────────────────────────────────┘
                ↓
┌─────────────────────────────────────┐
│       硬件接口层                     │
│  ├── HDMI控制器                     │
│  ├── DisplayPort控制器              │
│  └── VGA控制器                      │
└─────────────────────────────────────┘
```

---

## 🚀 QEMU 测试

### Framebuffer 测试

```bash
# 启动QEMU with VBE framebuffer
qemu-system-x86_64 \
    -device VGA,vgamem_mb=64 \
    -display gtk

# 启动QEMU with virtio-gpu
qemu-system-x86_64 \
    -device virtio-gpu-pci \
    -display gtk
```

### HDMI 测试

```bash
# QEMU模拟HDMI输出
qemu-system-x86_64 \
    -device qxl-vga,vgamem_mb=64 \
    -display gtk
```

---

## 📁 文件结构

```
src/kernel/driver/display/
├── mod.rs           # 显示模块注册
├── framebuffer.rs   # Framebuffer驱动
├── hdmi.rs          # HDMI驱动
├── dp.rs            # DisplayPort驱动
└── controller.rs    # 显示控制器抽象
```

---

## 🎯 使用场景

### 1. 控制台输出

```rust
// 初始化Framebuffer
let mut fb = Framebuffer::new(...);
fb.init()?;

// 清屏并设置背景
fb.fill(colors::BLACK);

// 绘制控制台边框
let border = Rect::new(10, 10, 800, 600);
fb.draw_rect(border, colors::WHITE);

// 填充控制台区域
let inner = Rect::new(11, 11, 798, 598);
fb.fill_rect(inner, colors::BLACK);
```

### 2. 多显示器配置

```rust
// 创建显示管理器
let mut manager = DisplayManager::new();

// 注册多个显示器
let hdmi_monitor = MonitorInfo::new(1, DisplayOutput::Hdmi);
let dp_monitor = MonitorInfo::new(2, DisplayOutput::DisplayPort);

manager.register_monitor(hdmi_monitor);
manager.register_monitor(dp_monitor);

// 设置主显示器
manager.set_primary_monitor(0)?;

// 设置扩展显示器
manager.enable_monitor(1)?;
```

### 3. 图形界面

```rust
// 绘制窗口
let window = Rect::new(100, 100, 400, 300);

// 窗口背景
fb.fill_rect(window, Color::new(240, 240, 240));

// 窗口标题栏
let title_bar = Rect::new(100, 100, 400, 30);
fb.fill_rect(title_bar, Color::new(50, 100, 200));

// 窗口边框
fb.draw_rect(window, colors::BLACK);
```

---

## 🔍 性能优化

### 1. 双缓冲

```rust
// 前缓冲和后缓冲
let front_buffer = Framebuffer::new(...);
let back_buffer = Framebuffer::new(...);

// 在后缓冲绘制
back_buffer.fill_rect(...);

// 交换缓冲
swap_buffers(&mut front_buffer, &mut back_buffer);
```

### 2. 损坏区域跟踪

```rust
// 只更新变化的区域
let damaged_regions: Vec<Rect> = get_damaged_regions();

for region in damaged_regions {
    update_region(&mut fb, region);
}
```

### 3. 硬件加速

- GPU加速绘制
- DMA传输
- 2D/3D加速引擎

---

## 📚 参考资料

- [VBE (VESA BIOS Extensions)](https://wiki.osdev.org/VBE)
- [HDMI Specification](https://www.hdmi.org/)
- [DisplayPort Specification](https://www.vesa.org/)
- [Framebuffer HOWTO](https://www.kernel.org/doc/Documentation/fb/)

---

## 📝 更新日志

**2026-05-18**：
- ✅ 实现Framebuffer驱动
- ✅ 实现多种像素格式支持
- ✅ 实现图形绘制原语
- ✅ 实现显示控制器抽象
- ✅ 实现多显示器管理
- ✅ 实现显示模式管理
- ✅ 创建显示器驱动文档

---

**最后更新**：2026-05-18  
**维护者**：AntX Team
