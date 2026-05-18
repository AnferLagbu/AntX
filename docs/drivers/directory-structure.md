# AntX 驱动目录结构说明

## 📁 目录结构

AntX内核的驱动子系统采用模块化设计，按功能分类组织：

```
src/kernel/driver/
├── framework.rs       # 统一驱动框架 (Driver Trait, 设备管理)
├── mod.rs             # 顶层模块注册和初始化
│
├── bus/               # 总线驱动子系统
│   ├── mod.rs         # 总线模块注册
│   └── pci.rs         # PCI总线驱动
│
├── char/              # 字符设备驱动子系统
│   ├── mod.rs         # 字符设备模块注册
│   ├── serial.rs      # 串口驱动 (UART 16550)
│   └── vga.rs         # VGA文本模式驱动
│
├── input/             # 输入设备驱动子系统
│   ├── mod.rs         # 输入设备模块注册
│   └── keyboard.rs    # PS/2键盘驱动
│
├── storage/           # 存储设备驱动子系统
│   ├── mod.rs         # 存储模块注册
│   ├── nvme.rs        # NVMe驱动 (PCIe SSD)
│   ├── ahci.rs        # AHCI/SATA驱动
│   └── ata.rs         # ATA/IDE驱动
│
├── display/           # 显示设备驱动子系统
│   ├── mod.rs         # 显示模块注册
│   ├── hdmi.rs        # HDMI驱动
│   └── dp.rs          # DisplayPort驱动
│
└── usb/               # USB子系统
    ├── mod.rs         # USB模块注册
    ├── usb_core.rs    # USB核心框架
    └── xhci.rs        # xHCI主机控制器
```

## 🎯 模块说明

### 1. **framework.rs** - 统一驱动框架
- `Driver` trait：所有驱动的统一接口
- `DeviceType`：设备类型枚举
- `DeviceInfo`：设备信息结构
- `DriverError`：驱动错误类型
- IO操作封装：`inb`, `outb`, `inl`, `outl`等

### 2. **bus/** - 总线驱动
- **PCI**: PCI总线枚举和配置
- **PCIe**: PCI Express总线 (未来)

### 3. **char/** - 字符设备驱动
- **Serial**: UART 16550串口驱动 (COM1-COM4)
- **VGA**: VGA文本模式显示 (80x25)

### 4. **input/** - 输入设备驱动
- **Keyboard**: PS/2键盘驱动
- **Mouse**: PS/2鼠标驱动 (未来)

### 5. **storage/** - 存储设备驱动
- **NVMe**: PCIe SSD驱动 (高性能)
- **AHCI**: AHCI/SATA驱动 (传统SSD/HDD)
- **ATA**: ATA/IDE驱动 (传统硬盘)

### 6. **display/** - 显示设备驱动
- **HDMI**: HDMI显示接口
- **DisplayPort**: DisplayPort显示接口

### 7. **usb/** - USB子系统
- **USB Core**: USB核心框架和设备枚举
- **xHCI**: USB 3.0主机控制器

## 📊 模块依赖关系

```
framework.rs (基础)
    ↓
┌───┴───┬───────┬───────┬───────┬───────┐
│       │       │       │       │       │
bus    char   input  storage display  usb
│       │       │       │       │       │
PCI    Serial  Key    NVMe    HDMI   USB Core
       VGA            AHCI    DP     xHCI
                      ATA
```

## 🔧 使用方法

### 初始化所有驱动

```rust
use crate::kernel::driver;

// 初始化所有驱动
driver::init_all()?;
```

### 使用特定驱动

```rust
use crate::kernel::driver::{
    char,    // 字符设备
    input,   // 输入设备
    storage, // 存储设备
    display, // 显示设备
    usb,     // USB设备
};

// 使用VGA显示
char::vga::vga_puts(b"Hello, AntX!\n");

// 使用键盘
if input::keyboard::keyboard_has_char() > 0 {
    let ch = input::keyboard::keyboard_read_char();
}

// 使用存储设备
let buffer = vec![0u8; 512];
storage::ata::ata_read_sector(0, 0, buffer.as_mut_ptr());
```

### 直接导入驱动类型

```rust
use crate::kernel::driver::{
    VgaDriver,        // VGA驱动
    SerialPort,       // 串口驱动
    KeyboardDriver,   // 键盘驱动
    NvmeController,   // NVMe控制器
    AhciController,   // AHCI控制器
};

let vga = VgaDriver::new();
let serial = SerialPort::new(0)?;
let keyboard = KeyboardDriver::new();
let nvme = NvmeController::new(0xFE000000);
```

## 🚀 初始化顺序

驱动初始化按照依赖关系顺序执行：

1. **字符设备** (char_init)
   - VGA显示
   - 串口输出

2. **总线驱动** (bus_init)
   - PCI总线枚举

3. **存储设备** (storage_init)
   - NVMe控制器
   - AHCI控制器
   - ATA控制器

4. **输入设备** (input_init)
   - 键盘

5. **显示设备** (display_init)
   - HDMI
   - DisplayPort

6. **USB设备** (usb_init)
   - USB核心
   - xHCI控制器

## 📝 添加新驱动

### 步骤1: 选择合适的子目录

根据驱动类型选择目录：
- 总线驱动 → `bus/`
- 字符设备 → `char/`
- 输入设备 → `input/`
- 存储设备 → `storage/`
- 显示设备 → `display/`
- USB设备 → `usb/`

### 步骤2: 实现Driver trait

```rust
use super::framework::{Driver, DeviceType, Result};

pub struct MyDriver {
    // ...
}

impl Driver for MyDriver {
    fn name(&self) -> &'static str {
        "My Driver"
    }
    
    fn device_type(&self) -> DeviceType {
        DeviceType::Other
    }
    
    fn init(&mut self) -> Result<()> {
        // 初始化逻辑
        Ok(())
    }
    
    fn shutdown(&mut self) -> Result<()> {
        // 关闭逻辑
        Ok(())
    }
    
    fn is_ready(&self) -> bool {
        // 检查是否就绪
        true
    }
    
    fn status(&self) -> &'static str {
        // 返回状态信息
        "Ready"
    }
}
```

### 步骤3: 更新模块注册

在对应目录的`mod.rs`中：
```rust
pub mod my_driver;

pub use my_driver::MyDriver;

pub fn xxx_init() -> framework::Result<()> {
    my_driver::my_driver_init()?;
    Ok(())
}
```

### 步骤4: 更新顶层导出

在`driver/mod.rs`中：
```rust
pub use xxx::MyDriver;
```

## 🔍 模块化优势

1. **清晰的组织结构**：按功能分类，易于查找和维护
2. **降低耦合度**：各子系统相对独立
3. **便于扩展**：添加新驱动只需在对应目录添加文件
4. **提高可读性**：目录结构直观反映功能划分
5. **支持条件编译**：可按需启用/禁用子系统

## 📚 相关文档

- [基本驱动文档](./README.md)
- [高级驱动文档](./advanced-drivers.md)
- [SSD驱动文档](./ssd-drivers.md)

---

**最后更新**：2026-05-18  
**维护者**：AntX Team
