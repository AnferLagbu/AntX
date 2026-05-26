# AntX 图形子系统工程书

> 从零到 Hymenoptera → 内核图形化的完整工程路线图
> 最后更新: 2026-07-16

---

## 一、现状评估

### 1.1 已有资产

| 组件 | 文件 | 完成度 | 说明 |
|------|------|--------|------|
| **VGA 文本模式** | `driver/char/vga.rs` | ✅ 100% | 80×25, 16 色, 光标, 滚动 |
| **Framebuffer 软件栈** | `driver/display/framebuffer.rs` | ✅ 85% | 像素格式/Color/Alpha混合/图形原语(Bresenham/中点圆)/双缓冲 |
| **显示控制器抽象** | `driver/display/controller.rs` | ✅ 70% | DisplayController trait, DisplayManager, DisplayMode, MonitorInfo, 多显示器 |
| **HDMI 骨架** | `driver/display/hdmi.rs` | ⚠️ 30% | EDID 结构体, VideoMode 枚举, 端口常量 |
| **DP 骨架** | `driver/display/dp.rs` | ⚠️ 20% | DPCD, LinkRate, LaneCount, TrainingState |
| **Hymenoptera 设计** | `docs/design/hymenoptera-display-server.md` | 📝 100% | 多用户多会话架构, Hymenoptera 协议, 会话/窗口/合成/输入管理器 |
| **显示驱动文档** | `docs/drivers/display-drivers.md` | 📝 100% | Framebuffer API 参考, HDMI/DP 使用示例 |

### 1.2 核心缺口

```
软件层 (Framebuffer + Color + 图形原语)     ✅ 已完成
       ↕  ❌ 连接缺失
硬件层 (LFB 物理地址 → 内核虚拟地址)         ❌ 完全缺失
       ↕  ❌ 连接缺失  
用户态 (Hymenoptera 显示服务器)              ❌ 无法访问帧缓冲
```

**根本原因**：Framebuffer 是纯软件对象——它持有一个 `*mut u8` 指针，但这个指针**从未被赋值**为真实的显存地址。Multiboot2 传递的帧缓冲信息没有被解析和传递到 display 子系统。

### 1.3 相关基础设施成熟度

| 子系统 | 成熟度 | 对图形化的意义 |
|--------|--------|---------------|
| VMM (页表/映射) | ⭐⭐⭐⭐ | 身份映射 LFB 物理地址 → 内核虚拟地址 |
| Multiboot2 | ⭐⭐⭐ | 已传递 FRAMEBUFFER_INFO tag，未解析 |
| PWM 能力系统 | ⭐⭐⭐⭐ | 显示客户端的访问控制 |
| IPC (管道/SHM) | ⭐⭐⭐⭐ | 显示服务器 ↔ 客户端通信 |
| VMA | ⭐⭐⭐⭐ | 用户态 mmap 帧缓冲 |
| Barrier 栏栈 | ⭐⭐⭐⭐ | 显示服务器崩溃 → 域级恢复 |
| Chitin 设备树 | ⭐⭐⭐ | `user_mapped` 字段预留用户态设备映射 |

---

## 二、架构决策

### 2.1 核心决策：用户态显示服务器

经过完整分析 [详见讨论]，决定采用**用户态 Hymenoptera 显示服务器**架构。

**决策理由**:

1. **PWM 隔离已就绪** — 每个 GUI 客户端有独立 PWID，内核无需重新实现访问控制
2. **Barrier 栏栈消除恐惧** — 显示服务器崩溃 → BBR 1μs 恢复，不会黑屏
3. **Chitin 设备树天然适配** — `user_mapped` 字段可直接映射 LFB 到显示服务器进程
4. **避免内核态臃肿** — freetype/pixman/cairo 在用户态直接使用，无需 no_std 移植
5. **Hymenoptera 设计已假设用户态** — 架构文档本来就是用户态的

### 2.2 内核最小责任

内核**只**提供三个新接口：

| 接口 | 语义 | 复杂性 |
|------|------|--------|
| `map_framebuffer(node_id) → *mut u8` | 将 LFB 物理地址映射到调用进程 VMA | 低 — 本质是 `ioremap` |
| `input_register(node_id) → event_fd` | 注册输入事件通道 | 中 — 需要 IRQ → 进程转发 |
| `display_get_info(node_id) → DisplayInfo` | 返回分辨率/格式/EDID | 低 — 只读元数据 |

---

## 三、分阶段实施计划

```
Phase G1: 打通第一个像素        (2-3 天)  ← 当前焦点
Phase G2: 测试图案自检           (1 天)
Phase G3: PSF 字体渲染            (2 天)
Phase G4: 图形控制台              (2 天)
Phase G5: Hymenoptera 骨架        (3-5 天)
Phase G6: virtio-gpu 驱动         (3 天)
Phase G7: 合成器 + 输入路由       (5 天)
Phase G8: 完整窗口系统            (7 天)
```

---

### Phase G1: 打通第一个像素 🔴 P0

**目标**：在 QEMU GTK 窗口中显示一个蓝色像素。

#### 1.1 解析 Multiboot2 帧缓冲信息

```rust
// kernel/boot/multiboot2_fb.rs — 新建

#[repr(C, packed)]
pub struct Multiboot2FramebufferInfo {
    pub addr: u64,       // 物理地址
    pub pitch: u32,      // 每行字节数
    pub width: u32,
    pub height: u32,
    pub bpp: u8,         // 位深
    pub fb_type: u8,     // 0=indexed, 1=RGB, 2=text
    pub _reserved: u16,
    // 如果 fb_type == 0 (indexed):
    //   pub palette_len: u32
    //   pub palette: [Color; palette_len]
    // 如果 fb_type == 1 (RGB):
    pub red_field_position: u8,
    pub red_mask_size: u8,
    pub green_field_position: u8,
    pub green_mask_size: u8,
    pub blue_field_position: u8,
    pub blue_mask_size: u8,
}

// 从 Multiboot2 tag type=8 填充此结构
pub fn fb_from_multiboot2_tag(tag_addr: *const u8) -> Option<Multiboot2FramebufferInfo>
```

**关键细节**：Multiboot2 的 `framebuffer_addr` 是**物理地址**，需要映射到内核虚拟地址空间。

#### 1.2 映射 LFB 到内核虚拟地址

```rust
// kernel/mm/vmm.rs — 新增函数

/// 将设备 MMIO 帧缓冲物理地址映射到内核虚拟地址空间
///
/// 使用身份映射 (phys + KERNEL_BASE)，因为内核已映射所有物理内存。
/// 返回的虚拟地址可以直接读写，效果等同于 MMIO 访问。
///
/// # Safety
/// phys_addr 必须是有效的帧缓冲物理地址（由 Multiboot2 保证）
pub fn map_framebuffer(phys_addr: u64, size: u64) -> *mut u8 {
    let virt = phys_addr + KERNEL_BASE;  // 身份映射：phys → virt
    // 标记页面为 Write-Combining (PAT) 以优化帧缓冲写入性能
    // 参见: Intel SDM Vol 3, 11.12 PAT
    #[cfg(target_arch = "x86_64")]
    set_page_cache_mode(phys_addr, size, CacheMode::WriteCombining);
    virt as *mut u8
}
```

#### 1.3 连接 Framebuffer

```rust
// kernel/driver/display/mod.rs — display_init() 改造

pub fn display_init() -> framework::Result<()> {
    // 从 Multiboot2 获取帧缓冲信息
    let fb_info = crate::kernel::boot::multiboot2_fb::get_framebuffer_info()
        .ok_or(DriverError::NotFound)?;

    // 映射 LFB
    let fb_size = fb_info.pitch as u64 * fb_info.height as u64;
    let virt_addr = crate::kernel::mm::vmm::map_framebuffer(fb_info.addr, fb_size);

    // 推断像素格式
    let format = infer_pixel_format(fb_info.bpp,
        fb_info.red_field_position, fb_info.green_field_position,
        fb_info.blue_field_position);

    // 创建 Framebuffer 实例 ← 连接软件层和硬件层
    let mut fb = Framebuffer {
        base: virt_addr,
        width: fb_info.width,
        height: fb_info.height,
        pitch: fb_info.pitch,
        format,
        ..Default::default()
    };

    // Phase G1 最终目标: 画一个蓝色像素
    fb.set_pixel(100, 100, colors::BLUE);

    // 自检
    let pixel = fb.get_pixel(100, 100);
    assert!(pixel.b > 200, "FB self-test failed: pixel not blue");

    serial_println!("[DISPLAY] OK: {}x{}x{} @ 0x{:X}",
        fb_info.width, fb_info.height, fb_info.bpp, fb_info.addr);

    Ok(())
}
```

#### 1.4 QEMU 启动命令

```bash
qemu-system-x86_64 \
    -cdrom build/antx.iso \
    -m 256M \
    -display gtk \           # 图形窗口
    -serial stdio \          # 串口日志
    -vga std                 # 标准 VGA → 提供线性帧缓冲
```

**预期结果**：
- GTK 窗口显示黑屏 + 一个蓝色像素在 (100,100)
- 终端输出 `[DISPLAY] OK: 1024x768x32 @ 0xFD000000`

#### 涉及文件

| 文件 | 操作 |
|------|------|
| `kernel/boot/multiboot2_fb.rs` | **新建** — Multiboot2 FRAMEBUFFER_INFO tag 解析 |
| `kernel/mm/vmm.rs` | **修改** — 添加 `map_framebuffer()` |
| `kernel/driver/display/mod.rs` | **修改** — `display_init()` 连接真实 LFB |
| 编译脚本 | **修改** — 链接新的 multiboot2_fb 模块 |

---

### Phase G2: 测试图案自检 🟡 P1

**目标**：让内核自己验证帧缓冲功能正确，不依赖肉眼。

```
绘制顺序（从左到右，从上到下）:

┌──────────────────────────────────────────────┐
│  ████████  ████████  ████████  ████████      │  纯色条 (R/G/B/W)
│  RED       GREEN      BLUE       WHITE       │
├──────────────────────────────────────────────┤
│  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │  256 级灰度渐变
├──────────────────────────────────────────────┤
│  ╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱   │  对角 RGB 渐变
├──────────────────────────────────────────────┤
│  ┌──────┐    ╭──────╮    ●                  │  图形原语
│  │ rect │    ╰ line ╯    circle             │  (矩形/线/圆)
├──────────────────────────────────────────────┤
│  The quick brown fox jumps over the lazy dog │  ASCII 调试文本
└──────────────────────────────────────────────┘
```

**自检逻辑**：

```rust
fn framebuffer_self_test(fb: &Framebuffer) -> usize {
    let mut failures = 0;

    // 1. 纯色条验证
    fb.fill_rect(Rect::new(0, 0, 100, 50), colors::RED);
    fb.fill_rect(Rect::new(100, 0, 100, 50), colors::GREEN);
    fb.fill_rect(Rect::new(200, 0, 100, 50), colors::BLUE);
    fb.fill_rect(Rect::new(300, 0, 100, 50), colors::WHITE);

    let r = fb.get_pixel(50, 25);
    if r.r < 200 || r.g > 50 { failures += 1; }

    let g = fb.get_pixel(150, 25);
    if g.g < 200 || g.r > 50 { failures += 1; }

    let b = fb.get_pixel(250, 25);
    if b.b < 200 || b.r > 50 { failures += 1; }

    // 2. 灰度渐变扫描: 检查 0, 128, 255 三个采样点
    for x in 0..256 {
        let gray = x as u8;
        fb.set_pixel(x, 60, Color::new(gray, gray, gray));
    }

    // 3. 对角渐变: 验证相邻像素单调递增
    // ...

    // 4. 绘制调试文本 (ASCII art, 无字体)
    draw_debug_text(fb, 10, 180, "DISPLAY SELF-TEST");

    failures
}
```

---

### Phase G3: PSF 字体渲染 🟡 P1

**目标**：在帧缓冲上显示矢量文本，替代 VGA 文本模式的硬编码字符。

#### 3.1 PSF (PC Screen Font) 格式

PSF 是最简单的位图字体格式——每个字形是一个 `width × height` 的位图：

```rust
// kernel/driver/display/font.rs — 新建

#[repr(C, packed)]
pub struct Psf2Header {
    pub magic: [u8; 4],      // 0x72 0xB5 0x4A 0x86
    pub version: u32,        // 0
    pub header_size: u32,    // 32
    pub flags: u32,
    pub glyph_count: u32,    // 字形数量
    pub glyph_size: u32,     // 每个字形字节数
    pub glyph_height: u32,
    pub glyph_width: u32,
}

pub struct PsfFont {
    pub glyphs: &'static [u8],   // 字模数据
    pub glyph_count: u32,
    pub glyph_width: u32,
    pub glyph_height: u32,
    pub glyph_size: u32,
}

impl PsfFont {
    /// 从编译时嵌入的 PSF 字模初始化
    pub fn from_embedded(data: &'static [u8]) -> Option<Self> { ... }

    /// 在帧缓冲上渲染一个字符
    pub fn render_char(&self, fb: &mut Framebuffer, x: u32, y: u32,
                        ch: char, fg: Color, bg: Color) { ... }

    /// 渲染文本字符串（自动换行）
    pub fn render_text(&self, fb: &mut Framebuffer, x: u32, y: u32,
                        text: &str, fg: Color, bg: Color) { ... }
}
```

#### 3.2 字体嵌入

```toml
# 编译脚本: 将 PSF 文件编译为 .o 嵌入内核
# 使用默认的 GNU Unifont 8x16 (覆盖所有 Unicode BMP)
# 或 Lat2-Terminus16 (轻量, 仅 ASCII + Latin)
```

```rust
// 静态链接嵌入的 PSF 字模
extern "C" {
    static _binary_font_psf_start: u8;
    static _binary_font_psf_end: u8;
}

let font = PsfFont::from_embedded(unsafe {
    core::slice::from_raw_parts(
        &_binary_font_psf_start as *const u8,
        &_binary_font_psf_end as *const u8 as usize
            - &_binary_font_psf_start as *const u8 as usize,
    )
}).expect("embedded PSF font corrupted");
```

---

### Phase G4: 图形控制台 🟡 P1

**目标**：用帧缓冲 + PSF 字体替代 VGA 文本模式，作为新的内核控制台。

```rust
// kernel/console/gfx_console.rs — 新建

pub struct GfxConsole {
    fb: Framebuffer,
    font: PsfFont,
    cursor_x: u32,
    cursor_y: u32,
    cols: u32,             // 屏幕列数 (像素/字形宽)
    rows: u32,             // 屏幕行数 (像素/字形高)
    fg_color: Color,
    bg_color: Color,
    scrollback: [u8; 4096 * 256],  // 滚动缓冲区
}

impl GfxConsole {
    pub fn new(fb: Framebuffer, font: PsfFont) -> Self { ... }

    pub fn putchar(&mut self, ch: char) {
        match ch {
            '\n' => { self.cursor_x = 0; self.cursor_y += 1; }
            '\r' => { self.cursor_x = 0; }
            '\t' => { self.cursor_x = (self.cursor_x + 8) & !7; }
            '\x08' => { if self.cursor_x > 0 { self.cursor_x -= 1; } }
            _ => {
                self.font.render_char(&mut self.fb,
                    self.cursor_x * self.font.glyph_width,
                    self.cursor_y * self.font.glyph_height,
                    ch, self.fg_color, self.bg_color);
                self.cursor_x += 1;
            }
        }
        // 换行 / 滚动
        if self.cursor_x >= self.cols {
            self.cursor_x = 0;
            self.cursor_y += 1;
        }
        if self.cursor_y >= self.rows {
            self.scroll_up(1);
        }
    }
}

impl core::fmt::Write for GfxConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for ch in s.chars() { self.putchar(ch); }
        Ok(())
    }
}
```

**与 VGA 文本模式的共存**:

```
启动早期 → VGA 文本模式 (bootstrap, 不依赖任何映射)
    ↓
Multiboot2 FB 解析成功 → 映射 LFB → 创建 GfxConsole
    ↓
所有后续 printk! / klog → 同时写串口 + GfxConsole
    ↓
如果 LFB 映射失败 → 回退 VGA 文本模式
```

**panic_handler 三路输出**:

```
panic! 发生时:

1. 串口 — 永远可用，最可靠
2. GfxConsole — 帧缓冲可用时，红色背景 + 白色文本
3. VGA 文本模式 — 最后的 fallback (0xB8000)
```

---

### Phase G5: Hymenoptera 显示服务器骨架 🟢 P2

**目标**：创建用户态 Hymenoptera 进程框架，验证 IPC + SHM 通道。

#### 5.1 Chitin 设备节点

```c
// 内核创建 Chitin 节点描述帧缓冲设备
ChitinNode framebuffer0 = {
    .name     = "framebuffer0",
    .compatible = "qx,framebuffer",
    .props    = {
        { "phys_addr", 0xFD000000 },
        { "width",     1024 },
        { "height",    768 },
        { "bpp",       32 },
        { "pitch",     4096 },
    },
    .user_mapped = PID_DISPLAY_SERVER,  // 映射到显示服务器进程
};
```

#### 5.2 内核接口

| syscall | 功能 |
|---------|------|
| `sys_chitin_map_device(pid, node_id) → *mut u8` | 将设备 LFB 映射到进程 VMA |
| `sys_chitin_get_device_info(node_id) → DeviceInfo` | 返回设备属性 (分辨率/格式) |
| `sys_display_flush()` | 可选: 硬件垂直同步 |
| `sys_input_create_channel(pid, irq) → fd` | 创建输入事件通道 |

#### 5.3 Hymenoptera 最小原型

```rust
// 用户态进程 (编译为独立的 ELF 可执行文件)

fn main() {
    // 1. 获取帧缓冲映射
    let fb_ptr = sys_chitin_map_device(getpid(), "framebuffer0").unwrap();
    let info = sys_chitin_get_device_info("framebuffer0").unwrap();

    // 2. 创建 Framebuffer (复用内核的 Framebuffer 结构体)
    let mut fb = Framebuffer::new(fb_ptr, info.width, info.height,
                                   info.pitch, info.format);
    fb.clear();

    // 3. 画一个验证图案
    fb.fill_rect(Rect::new(0, 0, info.width / 2, info.height), colors::BLUE);
    fb.fill_rect(Rect::new(info.width / 2, 0, info.width / 2, info.height), colors::YELLOW);

    // 4. 加载 PSF 字体, 写文本
    let font = PsfFont::from_file("/usr/share/fonts/unifont.psf").unwrap();
    font.render_text(&mut fb, 10, 10, "Hymenoptera Display Server v0.1",
                     colors::WHITE, colors::TRANSPARENT);

    // 5. 进入事件循环 (先跑一个死循环验证持续渲染)
    loop {
        sys_yield();
    }
}
```

---

### Phase G6: virtio-gpu 驱动 🟢 P2

**目标**：替代 VBE/VESA 的简单 LFB，使用 virtio-gpu 获得 edid、模式切换、硬件光标。

```
VBE/VESA                     virtio-gpu
─────────                     ──────────
LFB 固定地址                  virtio queues (动态)
无 edid                       edid 通过 controlq
无模式切换 (需 BIOS)           运行时 modeset
无硬件光标                     cursorq 硬件光标
QEMU: -vga std               QEMU: -device virtio-gpu-pci
```

#### 6.1 virtio-gpu 架构

```
virtio-gpu PCI 设备
  ├── controlq   → 命令: 创建资源/设置扫描输出/传输数据
  ├── cursorq    → 硬件光标位置和图像
  └── PCI BAR0   → MMIO virtio 通用配置 (device features, queue setup)
```

**关键 virtio-gpu 命令**:

| 命令 | 用途 |
|------|------|
| `VIRTIO_GPU_CMD_GET_DISPLAY_INFO` | 获取显示器 EDID + 支持的模式 |
| `VIRTIO_GPU_CMD_RESOURCE_CREATE_2D` | 创建 GPU 资源 (像素缓冲) |
| `VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING` | 附加物理内存到资源 |
| `VIRTIO_GPU_CMD_SET_SCANOUT` | 将资源绑定到显示器输出 |
| `VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D` | 将数据从 guest 内存传输到 GPU |
| `VIRTIO_GPU_CMD_RESOURCE_FLUSH` | 刷新资源 → 显示器可见 |

```rust
// kernel/driver/virtio/gpu.rs — 新建

pub struct VirtioGpu {
    pub controlq: VirtQueue,
    pub cursorq:  VirtQueue,
    pub display_info: DisplayInfo,  // 从 GET_DISPLAY_INFO 获取
    pub scanout_id: u32,
}

impl VirtioGpu {
    pub fn probe(pci_device: &PciDevice) -> Option<Self> { ... }

    pub fn get_display_info(&mut self) -> DisplayInfo { ... }

    pub fn create_framebuffer(&mut self, width: u32, height: u32,
                              format: PixelFormat) -> Result<Framebuffer> { ... }

    pub fn modeset(&mut self, mode: DisplayMode) -> Result<()> { ... }

    pub fn flush(&mut self, x: u32, y: u32, w: u32, h: u32) -> Result<()> { ... }
}
```

---

### Phase G7: 合成器 + 输入路由 🟢 P2

**目标**：实现 Hymenoptera 的核心渲染流水线——多层窗口合成。

#### 7.1 合成流水线

```
Client 1 Surface  Client 2 Surface  Client 3 Surface
      │                 │                 │
      └─────────┬───────┴────────┬────────┘
                ▼                ▼
         ┌──────────────────────────┐
         │      Compositor          │
         │  ┌─────────────────────┐ │
         │  │ 1. 按 Z-order 排序  │ │
         │  │ 2. Damage rects 计算│ │
         │  │ 3. Alpha 混合合成   │ │
         │  │ 4. 绘制窗口装饰     │ │
         │  │ 5. 输出到 scanout   │ │
         │  └─────────────────────┘ │
         └──────────┬───────────────┘
                    ▼
              Framebuffer
```

```rust
impl Compositor {
    pub fn composite(&mut self, windows: &[Window], output: &mut Framebuffer) {
        // 只合成 dirty 区域 (damage tracking)
        let damage_rects = self.damage.compute_dirty(windows);

        for rect in &damage_rects {
            for window in windows.iter().filter(|w| w.visible) {
                // Alpha 混合: src OVER dst
                blend_rect(output, &window.surface, window.x, window.y, rect);
            }
        }

        self.damage.clear();
    }
}
```

#### 7.2 输入路由

```
硬件中断 (键盘/鼠标)
  → 内核 IRQ handler
  → 写入 event_fd
  → Hymenoptera InputManager::poll() 读取
  → 根据焦点窗口路由到正确的 Client
```

---

### Phase G8: 完整窗口系统 🟢 P3

**目标**：真正可用的桌面——窗口拖动、标题栏、最小化/最大化、多会话切换。

这一阶段已经超出工程规划范围。Hymenoptera 设计文档已覆盖其完整架构。本工程书的重点是 G1-G4（让像素可见），后续阶段在执行时细化。

---

## 四、调试体系

### 4.1 QEMU 五通道并行调试

```
                    ┌─────────────────────────────────┐
                    │           QEMU 虚拟机            │
                    │                                  │
  -s -S             │  ┌──────────┐  ┌─────────────┐  │
  ──────────────────│─→│  GDB stub│  │   内核       │  │
  :1234             │  │  :1234   │←─│  (你的代码)  │  │
                    │  └──────────┘  └──────┬──────┘  │
  -serial stdio     │  ┌──────────┐         │          │
  ──────────────────│─→│  串口    │←────────┘          │
  内核日志          │  └──────────┘                     │
                    │                       │          │
  -monitor tcp      │  ┌──────────┐  QMP/HMP│          │
  ──────────────────│─→│  Monitor │←────────┘          │
  :4444             │  │  :4444   │  screendump/       │
                    │  └──────────┘  info registers    │
                    │                       │          │
  -display gtk      │  ┌──────────┐  帧缓冲 │          │
  ──────────────────│─→│  GTK窗口 │←────────┘          │
  肉眼可见          │  └──────────┘                     │
                    └─────────────────────────────────┘
```

### 4.2 推荐全能调试命令

```bash
qemu-system-x86_64 \
    -cdrom build/antx.iso \
    -m 256M \
    -s -S \                              # GDB 挂起
    -display gtk \                       # 图形窗口
    -serial stdio \                      # 串口→终端
    -monitor tcp:127.0.0.1:4444,server,nowait \  # Monitor
    -vga std \                           # 线性帧缓冲
    -d guest_errors,cpu_reset \          # 异常日志
    -D /tmp/qemu.log                     # 日志文件
```

### 4.3 调试技巧速查

| 场景 | 对策 |
|------|------|
| 黑屏 | `screendump /tmp/fb.ppm`, 在宿主机打开 ppm 检查原始像素 |
| 颜色错乱 | GDB: `x/64wx fb->virt_addr`, 检查像素值字节序 |
| LFB 地址错误 | `info mtree` 查看真实地址映射 |
| 撕裂/画面不对齐 | 检查 `pitch` 值 (常见错误: =width 而非 =width*bpp) |
| 图形代码 panic 不可见 | panic_handler 三路输出 (串口+FB+VGA) |
| 像素验证 | `framebuffer_self_test()` 自检函数, 代码验证替代肉眼 |

### 4.4 GDB 断点列表

```gdb
# 图形子系统关键断点
b multiboot2_parse_framebuffer   # FB 信息解析
b vmm_map_framebuffer            # LFB 映射
b display_init                   # 显示子系统初始化
b framebuffer_fill_rect          # 绘制矩形
b framebuffer_set_pixel          # 单个像素写入 (最细粒度)
b gfx_console_putchar            # 控制台字符输出

# 检查映射结果
p/x fb_info.addr                 # 物理地址
p/x fb->base                     # 虚拟地址
x/64wx fb->base                  # 查看前64个像素的原始值
```

---

## 五、文件清单

### 新建文件

| 文件 | Phase | 说明 |
|------|-------|------|
| `kernel/boot/multiboot2_fb.rs` | G1 | Multiboot2 FRAMEBUFFER_INFO tag 解析 |
| `kernel/driver/display/font.rs` | G3 | PSF 位图字体渲染 |
| `kernel/console/gfx_console.rs` | G4 | 图形控制台 |
| `kernel/driver/virtio/gpu.rs` | G6 | virtio-gpu 驱动 |
| `usr/hymenoptera/main.rs` | G5 | Hymenoptera 显示服务器入口 |

### 修改文件

| 文件 | Phase | 变更 |
|------|-------|------|
| `kernel/mm/vmm.rs` | G1 | 添加 `map_framebuffer()` |
| `kernel/driver/display/mod.rs` | G1-G3 | `display_init()` 改造 |
| `kernel/driver/display/framebuffer.rs` | G2 | 添加 `self_test()` |
| `rust/src/lib.rs` | G4 | panic_handler 三路输出 |
| `kernel/boot/` (汇编入口) | G1 | 多引导信息传递 |
| `Cargo.toml` / Makefile | G5 | Hymenoptera 用户态程序编译 |

---

## 六、里程碑与验收指标

| 里程碑 | Phase | 验收标准 |
|--------|-------|---------|
| **M1: 第一个像素** | G1 | QEMU GTK 窗口显示 (100,100) 处蓝色像素, `screendump` 验证 |
| **M2: 自检通过** | G2 | `framebuffer_self_test()` 返回 0 failures |
| **M3: 字体渲染** | G3 | "Hello World" 文本以 PSF 字体显示在帧缓冲 |
| **M4: 图形控制台** | G4 | 所有 `klog!` 输出同时出现在 GTK 窗口和串口 |
| **M5: 用户态像素** | G5 | Hymenoptera 进程通过 SHM 映射 LFB 画蓝色背景 |
| **M6: virtio-gpu** | G6 | `-device virtio-gpu-pci` 启动, 获取 edid, 显示测试图案 |
| **M7: 合成器** | G7 | 两个客户端窗口 alpha 混合合成到同一帧缓冲 |
| **M8: 完整桌面** | G8 | 窗口拖动/标题栏/最小化/多会话切换 |

---

## 七、风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Multiboot2 未传 FB tag | 中 | 高 (无 LFB) | 回退 VGA 文本模式, 提示用户用 `-vga std` |
| LFB 映射地址冲突 | 低 | 高 | `info mtree` 验证映射; 如果冲突, 使用页表显式映射而非身份映射 |
| PSF 字体版权 | 低 | 低 | GNU Unifont 是 GPL 兼容; Lat2-Terminus 是 SIL OFL |
| virtio-gpu 规范兼容性 | 中 | 中 | QEMU virtio-gpu 是参考实现; 严格遵循 virtio 1.2 规范 |
| 用户态 SHM 安全 | 中 | 高 | PWM capability 验证每个 SHM 映射请求; Chitin 节点白名单 |
