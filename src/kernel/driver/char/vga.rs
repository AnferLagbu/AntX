#![allow(dead_code)]
//! VGA 文本模式驱动 (VGA Text Mode Driver)
//!
//! 提供对 VGA 文本模式显示的完整支持：
//! - **80x25 文本模式**: 标准控制台输出
//! - **颜色支持**: 16种前景色和背景色
//! - **光标控制**: 光标位置和可见性
//! - **滚动支持**: 屏幕滚动和清屏
//! - **硬件缓冲**: 直接操作 VGA 显存
//!
//! ## 硬件接口
//!
//! ```text
//! VGA Text Mode Memory:
//! ├── 0xB8000: 起始地址 (物理内存映射)
//! ├── 80x25:  标准分辨率
//! └── 2 bytes/char: [char | attr]
//!
//! VGA I/O Ports:
//! ├── 0x3D4: CRT Controller Index
//! ├── 0x3D5: CRT Controller Data
//! └── 0x3DA: Input Status Register
//! ```
//!
//! # Safety
//! 此模块直接操作 VGA 显存和硬件端口。

use crate::kernel::driver::framework::{DeviceInfo, DeviceType, Driver, Result};

// ============================================================================
// 硬件常量定义
// ============================================================================

/// VGA 文本模式显存起始地址
pub const VGA_BUFFER_START: usize = 0xB8000;

/// 屏幕宽度 (列数)
pub const SCREEN_WIDTH: usize = 80;

/// 屏幕高度 (行数)
pub const SCREEN_HEIGHT: usize = 25;

/// VGA 显存大小 (字节)
pub const VGA_BUFFER_SIZE: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 2;

/// CRT 控制器端口
const VGA_CTRL_REGISTER: u16 = 0x3D4;
const VGA_DATA_REGISTER: u16 = 0x3D5;

/// 光标位置寄存器
const VGA_CURSOR_HIGH: u8 = 0x0E;
const VGA_CURSOR_LOW: u8 = 0x0F;

// ============================================================================
// 颜色定义
// ============================================================================

/// VGA 标准 16 色枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGrey = 7,
    DarkGrey = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    LightMagenta = 13,
    Yellow = 14,
    White = 15,
}

impl Default for Color {
    fn default() -> Self {
        Color::LightGrey
    }
}

/// VGA 文本属性字节
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextAttribute {
    pub foreground: Color,
    pub background: Color,
    pub blink: bool,
}

impl Default for TextAttribute {
    fn default() -> Self {
        Self {
            foreground: Color::White,
            background: Color::Black,
            blink: false,
        }
    }
}

impl TextAttribute {
    pub fn new(foreground: Color, background: Color) -> Self {
        Self {
            foreground,
            background,
            blink: false,
        }
    }

    pub fn as_u8(&self) -> u8 {
        let fg = self.foreground as u8;
        let bg = (self.background as u8) << 4;
        let blink = if self.blink { 0x80 } else { 0 };
        fg | bg | blink
    }
}

// ============================================================================
// VGA 字符结构
// ============================================================================

/// VGA 文本模式字符单元
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VgaChar {
    pub character: u8,
    pub attribute: u8,
}

impl VgaChar {
    pub fn new(ch: u8, attr: TextAttribute) -> Self {
        Self {
            character: ch,
            attribute: attr.as_u8(),
        }
    }
}

impl Default for VgaChar {
    fn default() -> Self {
        Self {
            character: b' ',
            attribute: TextAttribute::default().as_u8(),
        }
    }
}

// ============================================================================
// 底层 I/O 操作
// ============================================================================

/// 向指定端口写入字节
#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    crate::arch!(outb(port, value));
}

/// 从指定端口读入字节
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    crate::arch!(inb(port))
}

// ============================================================================
// VGA 驱动主结构
// ============================================================================

/// VGA 文本模式驱动
pub struct VgaDriver {
    /// 显存缓冲区指针
    buffer: *mut VgaChar,
    /// 当前光标位置 (列)
    cursor_x: usize,
    /// 当前光标位置 (行)
    cursor_y: usize,
    /// 当前文本属性
    attribute: TextAttribute,
    /// 设备信息
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for VgaDriver {
    fn name(&self) -> &'static str {
        "VGA Text Mode"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }

    fn init(&mut self) -> Result<()> {
        self.clear_screen();
        self.set_cursor(0, 0);
        #[cfg(target_arch = "x86_64")]
        self.enable_cursor(true);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn status(&self) -> &'static str {
        if self.initialized {
            "VGA ready @ 0xB8000 (80x25)"
        } else {
            "VGA not initialized"
        }
    }
}

// ============================================================================
// 公共 API
// ============================================================================

impl VgaDriver {
    /// 创建新的 VGA 驱动实例
    pub fn new() -> Self {
        Self {
            buffer: VGA_BUFFER_START as *mut VgaChar,
            cursor_x: 0,
            cursor_y: 0,
            attribute: TextAttribute::default(),
            info: DeviceInfo::new("vga", DeviceType::Char),
            initialized: false,
        }
    }

    /// 清屏
    pub fn clear_screen(&mut self) {
        let blank = VgaChar::new(b' ', self.attribute);

        unsafe {
            for i in 0..(SCREEN_WIDTH * SCREEN_HEIGHT) {
                *self.buffer.add(i) = blank;
            }
        }

        self.cursor_x = 0;
        self.cursor_y = 0;
        #[cfg(target_arch = "x86_64")]
        self.update_hardware_cursor();
    }

    /// 设置当前颜色属性
    pub fn set_color(&mut self, foreground: Color, background: Color) {
        self.attribute = TextAttribute::new(foreground, background);
    }

    /// 设置文本属性
    pub fn set_attribute(&mut self, attr: TextAttribute) {
        self.attribute = attr;
    }

    /// 获取当前文本属性
    pub fn get_attribute(&self) -> &TextAttribute {
        &self.attribute
    }

    /// 写入单个字符
    pub fn putchar(&mut self, ch: u8) {
        match ch {
            b'\n' => {
                self.cursor_x = 0;
                self.cursor_y += 1;
            }
            b'\r' => {
                self.cursor_x = 0;
            }
            b'\t' => {
                self.cursor_x = (self.cursor_x + 8) & !7;
                if self.cursor_x >= SCREEN_WIDTH {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                }
            }
            0x08 => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            _ => {
                if self.cursor_y >= SCREEN_HEIGHT {
                    self.scroll_up();
                }

                let idx = self.cursor_y * SCREEN_WIDTH + self.cursor_x;
                let vga_char = VgaChar::new(ch, self.attribute);

                unsafe {
                    *self.buffer.add(idx) = vga_char;
                }

                self.cursor_x += 1;
                if self.cursor_x >= SCREEN_WIDTH {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                }
            }
        }

        #[cfg(target_arch = "x86_64")]
        self.update_hardware_cursor();
    }

    /// 写入字符串
    pub fn puts(&mut self, s: &[u8]) {
        for &ch in s {
            self.putchar(ch);
        }
    }

    /// 写入格式化字符串 (简化版)
    pub fn print(&mut self, s: &str) {
        self.puts(s.as_bytes());
    }

    /// 滚动屏幕 (向上滚动一行)
    pub fn scroll_up(&mut self) {
        unsafe {
            let src = self.buffer.add(SCREEN_WIDTH);
            let dst = self.buffer;
            let count = SCREEN_WIDTH * (SCREEN_HEIGHT - 1);

            core::ptr::copy(src, dst, count);

            let blank = VgaChar::new(b' ', self.attribute);
            for i in 0..SCREEN_WIDTH {
                *self.buffer.add(count + i) = blank;
            }
        }

        if self.cursor_y > 0 {
            self.cursor_y -= 1;
        }
    }

    /// 设置光标位置
    pub fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = x.min(SCREEN_WIDTH - 1);
        self.cursor_y = y.min(SCREEN_HEIGHT - 1);
        #[cfg(target_arch = "x86_64")]
        self.update_hardware_cursor();
    }

    /// 获取光标位置
    pub fn get_cursor(&self) -> (usize, usize) {
        (self.cursor_x, self.cursor_y)
    }

    /// 更新硬件光标位置
    #[cfg(target_arch = "x86_64")]
    fn update_hardware_cursor(&mut self) {
        let pos = (self.cursor_y * SCREEN_WIDTH + self.cursor_x) as u16;

        unsafe {
            outb(VGA_CTRL_REGISTER, VGA_CURSOR_LOW);
            outb(VGA_DATA_REGISTER, (pos & 0xFF) as u8);

            outb(VGA_CTRL_REGISTER, VGA_CURSOR_HIGH);
            outb(VGA_DATA_REGISTER, ((pos >> 8) & 0xFF) as u8);
        }
    }

    /// 启用/禁用光标
    #[cfg(target_arch = "x86_64")]
    pub fn enable_cursor(&mut self, enable: bool) {
        unsafe {
            outb(VGA_CTRL_REGISTER, 0x0A);
            let cursor_start = inb(VGA_DATA_REGISTER);

            if enable {
                outb(VGA_DATA_REGISTER, cursor_start & 0xC0);
            } else {
                outb(VGA_DATA_REGISTER, 0x20);
            }
        }
    }

    /// 在指定位置写入字符 (不移动光标)
    pub fn write_at(&mut self, x: usize, y: usize, ch: u8) {
        if x >= SCREEN_WIDTH || y >= SCREEN_HEIGHT {
            return;
        }

        let idx = y * SCREEN_WIDTH + x;
        let vga_char = VgaChar::new(ch, self.attribute);

        unsafe {
            *self.buffer.add(idx) = vga_char;
        }
    }

    /// 在指定位置写入字符串 (不移动光标)
    pub fn write_string_at(&mut self, x: usize, y: usize, s: &[u8]) {
        for (i, &ch) in s.iter().enumerate() {
            if x + i >= SCREEN_WIDTH {
                break;
            }
            self.write_at(x + i, y, ch);
        }
    }

    /// 获取指定位置的字符
    pub fn read_at(&self, x: usize, y: usize) -> Option<VgaChar> {
        if x >= SCREEN_WIDTH || y >= SCREEN_HEIGHT {
            return None;
        }

        let idx = y * SCREEN_WIDTH + x;
        unsafe { Some(*self.buffer.add(idx)) }
    }

    /// 填充矩形区域
    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, ch: u8) {
        for row in y..(y + height).min(SCREEN_HEIGHT) {
            for col in x..(x + width).min(SCREEN_WIDTH) {
                self.write_at(col, row, ch);
            }
        }
    }

    /// 绘制单线边框
    pub fn draw_border(&mut self, x: usize, y: usize, width: usize, height: usize) {
        let old_attr = self.attribute;
        self.attribute = TextAttribute::new(Color::White, Color::Blue);

        for col in x..x + width {
            self.write_at(col, y, 0xC4);
            self.write_at(col, y + height - 1, 0xC4);
        }

        for row in y..y + height {
            self.write_at(x, row, 0xB3);
            self.write_at(x + width - 1, row, 0xB3);
        }

        self.write_at(x, y, 0xDA);
        self.write_at(x + width - 1, y, 0xBF);
        self.write_at(x, y + height - 1, 0xC0);
        self.write_at(x + width - 1, y + height - 1, 0xD9);

        self.attribute = old_attr;
    }
}

impl Default for VgaDriver {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 全局 VGA 实例 (用于内核早期输出)
// ============================================================================

/// 全局 VGA 驱动实例
static mut VGA_DRIVER: Option<VgaDriver> = None;

/// 初始化全局 VGA 驱动
#[no_mangle]
pub extern "C" fn vga_init() {
    unsafe {
        VGA_DRIVER = Some(VgaDriver::new());
        if let Some(ref mut vga) = VGA_DRIVER {
            let _ = vga.init();
        }
    }
}

/// 向 VGA 输出字符 (C 兼容接口)
#[no_mangle]
pub extern "C" fn vga_putchar(ch: i32) {
    unsafe {
        if let Some(ref mut vga) = VGA_DRIVER {
            vga.putchar(ch as u8);
        }
    }
}

/// 向 VGA 输出字符串 (C 兼容接口)
#[no_mangle]
pub extern "C" fn vga_puts(s: *const core::ffi::c_char) {
    if s.is_null() {
        return;
    }

    unsafe {
        if let Some(ref mut vga) = VGA_DRIVER {
            let mut ptr = s;
            while *ptr != 0 {
                vga.putchar(*ptr as u8);
                ptr = ptr.add(1);
            }
        }
    }
}

/// 清屏 (C 兼容接口)
#[no_mangle]
pub extern "C" fn vga_clear() {
    unsafe {
        if let Some(ref mut vga) = VGA_DRIVER {
            vga.clear_screen();
        }
    }
}

/// 设置颜色 (C 兼容接口)
#[no_mangle]
pub extern "C" fn vga_set_color(fg: u8, bg: u8) {
    unsafe {
        if let Some(ref mut vga) = VGA_DRIVER {
            let fg_color = match fg {
                0..=15 => unsafe { core::mem::transmute(fg) },
                _ => Color::White,
            };
            let bg_color = match bg {
                0..=15 => unsafe { core::mem::transmute(bg) },
                _ => Color::Black,
            };
            vga.set_color(fg_color, bg_color);
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_values() {
        assert_eq!(Color::Black as u8, 0);
        assert_eq!(Color::White as u8, 15);
        assert_eq!(Color::Blue as u8, 1);
    }

    #[test]
    fn test_text_attribute() {
        let attr = TextAttribute::new(Color::White, Color::Blue);
        assert_eq!(attr.foreground, Color::White);
        assert_eq!(attr.background, Color::Blue);
        assert!(!attr.blink);

        let byte = attr.as_u8();
        assert_eq!(byte & 0x0F, Color::White as u8);
        assert_eq!((byte >> 4) & 0x0F, Color::Blue as u8);
    }

    #[test]
    fn test_vga_char() {
        let vga = VgaChar::new(b'A', TextAttribute::default());
        assert_eq!(vga.character, b'A');
    }

    #[test]
    fn test_driver_creation() {
        let mut vga = VgaDriver::new();
        assert_eq!(vga.name(), "VGA Text Mode");
        assert_eq!(vga.device_type(), DeviceType::Char);
        assert!(!vga.is_ready());
    }

    #[test]
    fn test_screen_dimensions() {
        assert_eq!(SCREEN_WIDTH, 80);
        assert_eq!(SCREEN_HEIGHT, 25);
        assert_eq!(VGA_BUFFER_SIZE, 80 * 25 * 2);
    }

    #[test]
    fn test_buffer_address() {
        assert_eq!(VGA_BUFFER_START, 0xB8000);
    }
}
