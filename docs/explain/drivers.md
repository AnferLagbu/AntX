# AntX 硬件驱动开发指南

## 📋 概述

本文档介绍 AntX 内核的基本硬件驱动及其在 QEMU 中的调试方法。

## 🔧 已实现的驱动

### 1. VGA 文本模式驱动 (`vga.rs`)

**功能**：
- 80x25 文本模式显示
- 16种前景色和背景色
- 光标控制
- 屏幕滚动
- 边框绘制

**使用示例**：
```rust
use crate::kernel::driver::vga::{VgaDriver, Color};

let mut vga = VgaDriver::new();
vga.init().unwrap();

// 设置颜色
vga.set_color(Color::White, Color::Blue);

// 输出文本
vga.print("Hello, AntX!\n");

// 绘制边框
vga.draw_border(10, 5, 60, 15);
```

**硬件接口**：
- 显存地址：`0xB8000`
- CRT 控制器端口：`0x3D4` / `0x3D5`

---

### 2. 串口驱动 (`serial.rs`)

**功能**：
- COM1-COM4 支持
- 可配置波特率（9600-115200）
- 环形缓冲区
- 中断支持

**使用示例**：
```rust
use crate::kernel::driver::serial::{SerialPort, BaudRate};

let mut serial = SerialPort::new(0).unwrap(); // COM1
serial.init().unwrap();

// 发送数据
serial.send_string(b"Debug output\n").unwrap();

// 接收数据
if let Some(byte) = serial.receive_byte() {
    // 处理接收到的字节
}
```

**硬件接口**：
- COM1: `0x3F8` (IRQ4)
- COM2: `0x2F8` (IRQ3)
- COM3: `0x3E8` (IRQ4)
- COM4: `0x2E8` (IRQ3)

---

### 3. PIT 定时器驱动 (`pit.rs`)

**功能**：
- 可编程中断频率
- 精确计时
- 微秒级延迟

**使用示例**：
```rust
use crate::kernel::timer::pit;

// 初始化为 1000 Hz (1ms 间隔)
let actual_freq = pit::pit_init(1000).unwrap();

// 读取当前计数
if let Some(count) = pit::pit_read_count() {
    // 使用计数值
}

// 获取微秒级精度
if let Some(us) = pit::pit_elapsed_since_tick_us() {
    // 使用微秒值
}
```

**硬件接口**：
- 通道 0 数据端口：`0x40`
- 命令寄存器：`0x43`
- 基础频率：`1.193182 MHz`

---

### 4. PS/2 键盘驱动 (`keyboard.rs`)

**功能**：
- Scancode 转换
- 修饰键支持（Shift, Ctrl, Alt, Caps Lock）
- 环形缓冲区
- 中断处理

**使用示例**：
```rust
use crate::kernel::driver::keyboard::KeyboardDriver;

let mut keyboard = KeyboardDriver::new();
keyboard.init().unwrap();

// 检查是否有按键
if keyboard.has_char() {
    if let Some(key) = keyboard.read_char() {
        // 处理按键
    }
}
```

**硬件接口**：
- 数据端口：`0x60`
- 状态/命令端口：`0x64`
- IRQ：`IRQ1`

---

## 🚀 QEMU 调试环境

### 基本命令

```bash
# 1. 正常启动（带 VGA 显示）
make qemu-debug

# 2. GDB 调试模式
make qemu-debug-gdb
# 在另一个终端：
gdb -x .gdbinit.antx

# 3. 无头模式（后台运行）
make qemu-headless

# 4. 网络模式
make qemu-network

# 5. 运行驱动测试
make driver-test
```

### QEMU 调试脚本

使用 `scripts/qemu_debug.sh` 脚本可以更灵活地配置 QEMU：

```bash
# 显示帮助
./scripts/qemu_debug.sh --help

# 自定义内存和 CPU
./scripts/qemu_debug.sh -m 1024 -c host

# 调试模式
./scripts/qemu_debug.sh -d

# 无头模式 + 网络
./scripts/qemu_debug.sh -D none -n
```

### GDB 调试

1. 启动 QEMU 调试模式：
```bash
make qemu-debug-gdb
```

2. 在另一个终端连接 GDB：
```bash
gdb -x .gdbinit.antx
```

3. 常用 GDB 命令：
```
(gdb) break kernel_main    # 设置断点
(gdb) continue             # 继续执行
(gdb) step                 # 单步执行
(gdb) info registers       # 查看寄存器
(gdb) x/10i $rip           # 查看指令
```

---

## 📊 驱动测试

### 运行测试

```bash
# 运行所有驱动测试
make driver-test

# 查看测试输出
cat tests/reports/driver_test_*.log
```

### 测试内容

驱动测试会验证：
1. **VGA 驱动**：初始化、颜色设置、文本输出
2. **串口驱动**：初始化、数据发送
3. **PIT 定时器**：初始化、频率配置、延迟测试
4. **键盘驱动**：初始化、按键检测

---

## 🎯 开发建议

### 添加新驱动

1. 在 `src/kernel/driver/` 下创建新文件
2. 实现 `Driver` trait
3. 在 `driver/mod.rs` 中注册模块
4. 在 `init_all()` 中添加初始化调用
5. 编写测试代码

### 驱动框架

所有驱动都实现统一的 `Driver` trait：

```rust
pub trait Driver {
    fn name(&self) -> &'static str;
    fn device_type(&self) -> DeviceType;
    fn init(&mut self) -> Result<()>;
    fn shutdown(&mut self) -> Result<()>;
    fn is_ready(&self) -> bool;
    fn status(&self) -> &'static str;
}
```

### IO 端口操作

使用框架提供的安全封装：

```rust
use crate::kernel::driver::framework::{outb, inb};

unsafe {
    outb(0x60, 0xAE);  // 写入端口
    let value = inb(0x60);  // 读取端口
}
```

---

## 📁 文件结构

```
src/kernel/driver/
├── mod.rs           # 模块注册和初始化
├── framework.rs     # 驱动框架和 IO 操作
├── vga.rs          # VGA 文本模式驱动
├── serial.rs       # 串口驱动
├── keyboard.rs     # PS/2 键盘驱动
├── ata.rs          # ATA/IDE 磁盘驱动
└── pci.rs          # PCI 总线驱动

src/kernel/timer/
└── pit.rs          # PIT 定时器驱动

scripts/
└── qemu_debug.sh   # QEMU 调试脚本

src/kernel/tests/
└── driver_test.rs  # 驱动测试代码
```

---

## 🔍 故障排查

### QEMU 无法启动

**问题**：`qemu-system-x86_64: command not found`

**解决**：
```bash
sudo apt install qemu-system-x86
```

### 内核镜像不存在

**问题**：`Kernel image not found: build/kernel.flat`

**解决**：
```bash
make build
```

### 串口无输出

**检查**：
1. QEMU 是否正确配置了串口重定向
2. 串口是否已初始化
3. 波特率配置是否正确

### VGA 显示异常

**检查**：
1. 显存地址是否正确（`0xB8000`）
2. 是否在 QEMU 中启用了显示
3. 光标位置是否有效

---

## 📚 参考资料

- [OSDev Wiki - VGA Hardware](https://wiki.osdev.org/VGA_Hardware)
- [OSDev Wiki - Serial Ports](https://wiki.osdev.org/Serial_Ports)
- [OSDev Wiki - PIT](https://wiki.osdev.org/Programmable_Interval_Timer)
- [OSDev Wiki - PS/2 Keyboard](https://wiki.osdev.org/PS/2_Keyboard)
- [QEMU Documentation](https://qemu.org/docs/master/)

---

## 📝 更新日志

**2026-05-18**：
- ✅ 实现 VGA 文本模式驱动
- ✅ 实现串口驱动（UART 16550）
- ✅ 实现 PIT 定时器驱动
- ✅ 实现 PS/2 键盘驱动
- ✅ 创建 QEMU 调试脚本
- ✅ 添加驱动测试代码
- ✅ 更新 Makefile 目标

---

**最后更新**：2026-05-18  
**维护者**：AntX Team
