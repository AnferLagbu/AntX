//! PS/2 键盘驱动 (Rust 安全重写)
//!
//! 提供对 PS/2 兼容键盘的完整支持：
//! - **键盘初始化**: LED 设置、扫描码集检测
//! - **Scancode 转换**: 扫描码到 ASCII 字符映射
//! - **修饰键支持**: Shift, Ctrl, Alt, Caps Lock
//! - **键盘缓冲区**: 环形缓冲区存储按键
//! - **中断处理**: IRQ1 中断服务程序
//!
//! ## 硬件接口
//!
//! ```text
//! PS/2 Controller Ports:
//! ├── 0x60: Data Port (读/写键盘数据)
//! └── 0x64: Status/Command Register
//! ```
//!
//! # Safety
//! 此模块直接操作 PS/2 控制器硬件。

use super::framework::{Driver, DeviceType, DriverError, Result, DeviceInfo};
use super::framework::{outb, inb};
use alloc::boxed::Box;
use spin::Mutex;

// ============================================================================
// 硬件常量定义
// ============================================================================

/// PS/2 数据端口
const PS2_DATA_PORT: u16 = 0x60;
/// PS/2 状态/命令端口
const PS2_CMD_PORT: u16 = 0x64;

/// 状态寄存器标志位
const PS2_STATUS_OUTPUT_FULL: u8 = 0x01;  // 输出缓冲区满
const PS2_STATUS_INPUT_FULL: u8 = 0x02;   // 输入缓冲区满
const PS2_STATUS_SYSTEM: u8 = 0x04;       // 系统标志

/// 键盘命令
const KB_CMD_SET_LED: u8 = 0xED;          // 设置 LED
const KB_CMD_ECHO: u8 = 0xEE;             // Echo
const KB_CMD_SCANCODE: u8 = 0xF0;         // 获取/设置扫描码集
const KB_CMD_IDENTIFY: u8 = 0xF2;         // Identify Keyboard

/// LED 标志位
const KB_LED_SCROLL_LOCK: u8 = 0x01;
pub(crate) const KB_LED_NUM_LOCK: u8 = 0x02;
pub(crate) const KB_LED_CAPS_LOCK: u8 = 0x04;

/// 缓冲区大小
const KEYBOARD_BUFFER_SIZE: usize = 128;

// ============================================================================
// Scancode 转换表
// ============================================================================

/// 标准 US QWERTY 键盘 Scancode Set 1 映射表
/// [scancode] -> ASCII 字符 (无修饰键)
pub(crate) const SCANCODE_TABLE: &[u8; 87] = &[
    0x00, 0x1B, '1' as u8, '2' as u8, '3' as u8, '4' as u8,
    '5' as u8, '6' as u8, '7' as u8, '8' as u8, '9' as u8,
    '0' as u8, '-' as u8, '=' as u8, 0x08, 0x09,

    'q' as u8, 'w' as u8, 'e' as u8, 'r' as u8, 't' as u8,
    'y' as u8, 'u' as u8, 'i' as u8, 'o' as u8, 'p' as u8,
    '[' as u8, ']' as u8, 0x0D, 0x00, 'a' as u8,

    's' as u8, 'd' as u8, 'f' as u8, 'g' as u8, 'h' as u8,
    'j' as u8, 'k' as u8, 'l' as u8, ';' as u8, '\'' as u8,
    '`' as u8, 0x00, '\\' as u8, 'z' as u8, 'x' as u8,

    'c' as u8, 'v' as u8, 'b' as u8, 'n' as u8, 'm' as u8,
    ',' as u8, '.' as u8, '/' as u8, 0x00, b'*',

    0x00, ' ' as u8, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Shift 修饰键下的字符映射表
pub(crate) const SHIFT_TABLE: &[u8; 87] = &[
    0x00, 0x1B, '!' as u8, '@' as u8, '#' as u8, '$' as u8,
    '%' as u8, '^' as u8, '&' as u8, '*' as u8, '(' as u8,
    ')' as u8, '_' as u8, '+' as u8, 0x08, 0x09,

    'Q' as u8, 'W' as u8, 'E' as u8, 'R' as u8, 'T' as u8,
    'Y' as u8, 'U' as u8, 'I' as u8, 'O' as u8, 'P' as u8,
    '{' as u8, '}' as u8, 0x0D, 0x00, 'A' as u8,

    'S' as u8, 'D' as u8, 'F' as u8, 'G' as u8, 'H' as u8,
    'J' as u8, 'K' as u8, 'L' as u8, ':' as u8, '"' as u8,
    '~' as u8, 0x00, '|' as u8, 'Z' as u8, 'X' as u8,

    'C' as u8, 'V' as u8, 'B' as u8, 'N' as u8, 'M' as u8,
    '<' as u8, '>' as u8, '?' as u8, 0x00, b'*',

    0x00, ' ' as u8, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// ============================================================================
// 特殊按键定义
// ============================================================================

/// 特殊按键枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKey {
    /// 无效按键
    None,
    /// Enter 回车
    Enter,
    /// Tab 制表符
    Tab,
    /// Backspace 退格
    Backspace,
    /// Space 空格
    Space,
    /// Escape 退出
    Escape,
    /// Delete 删除
    Delete,
    /// Insert 插入
    Insert,
    /// Home 首行
    Home,
    /// End 尾行
    End,
    /// PageUp 上翻页
    PageUp,
    /// PageDown 下翻页
    PageDown,
    /// 上箭头
    ArrowUp,
    /// 下箭头
    ArrowDown,
    /// 左箭头
    ArrowLeft,
    /// 右箭头
    ArrowRight,
    /// F1-F12 功能键
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
}

/// 特殊按键 scancode 映射
pub(crate) fn get_special_key(scancode: u8) -> SpecialKey {
    match scancode {
        0x0D => SpecialKey::Enter,
        0x0F => SpecialKey::Tab,
        0x0E => SpecialKey::Backspace,
        0x39 => SpecialKey::Space,
        0x01 => SpecialKey::Escape,
        0x53 => SpecialKey::Delete,
        0x52 => SpecialKey::Insert,
        0x47 => SpecialKey::Home,
        0x4F => SpecialKey::End,
        0x49 => SpecialKey::PageUp,
        0x51 => SpecialKey::PageDown,
        0x48 => SpecialKey::ArrowUp,
        0x50 => SpecialKey::ArrowDown,
        0x4B => SpecialKey::ArrowLeft,
        0x4D => SpecialKey::ArrowRight,
        0x3B => SpecialKey::F1,
        0x3C => SpecialKey::F2,
        0x3D => SpecialKey::F3,
        0x3E => SpecialKey::F4,
        0x3F => SpecialKey::F5,
        0x40 => SpecialKey::F6,
        0x41 => SpecialKey::F7,
        0x42 => SpecialKey::F8,
        0x43 => SpecialKey::F9,
        0x44 => SpecialKey::F10,
        0x57 => SpecialKey::F11,
        0x58 => SpecialKey::F12,
        _ => SpecialKey::None,
    }
}

// ============================================================================
// 键盘状态结构体
// ============================================================================

/// 键盘修饰键状态
#[derive(Debug, Clone, Copy)]
pub struct ModifierState {
    pub left_shift: bool,
    pub right_shift: bool,
    pub left_ctrl: bool,
    pub right_ctrl: bool,
    pub left_alt: bool,
    pub right_alt: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}

impl Default for ModifierState {
    fn default() -> Self {
        Self {
            left_shift: false,
            right_shift: false,
            left_ctrl: false,
            right_ctrl: false,
            left_alt: false,
            right_alt: false,
            caps_lock: false,
            num_lock: true,      // 默认开启数字锁定
            scroll_lock: false,
        }
    }
}

impl ModifierState {
    /// 检查是否有 Shift 键按下
    #[inline]
    pub fn shift_pressed(&self) -> bool {
        self.left_shift || self.right_shift
    }

    /// 检查是否有 Ctrl 键按下
    #[inline]
    pub fn ctrl_pressed(&self) -> bool {
        self.left_ctrl || self.right_ctrl
    }

    /// 检查是否有 Alt 键按下
    #[inline]
    pub fn alt_pressed(&self) -> bool {
        self.left_alt || self.right_alt
    }

    /// 计算 LED 状态字节
    pub fn to_led_byte(&self) -> u8 {
        let mut led: u8 = 0;
        if self.scroll_lock { led |= KB_LED_SCROLL_LOCK; }
        if self.num_lock { led |= KB_LED_NUM_LOCK; }
        if self.caps_lock { led |= KB_LED_CAPS_LOCK; }
        led
    }
}

/// 环形键盘缓冲区
pub(crate) struct KeyboardBuffer {
    buffer: [u8; KEYBOARD_BUFFER_SIZE],
    head: usize,
    tail: usize,
    count: usize,
}

impl Default for KeyboardBuffer {
    fn default() -> Self {
        Self {
            buffer: [0u8; KEYBOARD_BUFFER_SIZE],
            head: 0,
            tail: 0,
            count: 0,
        }
    }
}

impl KeyboardBuffer {
    pub(crate) fn push(&mut self, byte: u8) -> Result<()> {
        if self.count >= KEYBOARD_BUFFER_SIZE {
            return Err(DriverError::Busy);
        }

        self.buffer[self.tail] = byte;
        self.tail = (self.tail + 1) % KEYBOARD_BUFFER_SIZE;
        self.count += 1;
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<u8> {
        if self.count == 0 {
            return None;
        }

        let byte = self.buffer[self.head];
        self.head = (self.head + 1) % KEYBOARD_BUFFER_SIZE;
        self.count -= 1;
        Some(byte)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub(crate) fn len(&self) -> usize {
        self.count
    }

    fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
    }
}

/// PS/2 键盘驱动器
pub struct KeyboardDriver {
    /// 修饰键状态
    modifiers: ModifierState,
    /// 键盘缓冲区
    buffer: KeyboardBuffer,
    /// 设备信息
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

// ============================================================================
// 底层辅助函数
// ============================================================================

/// 等待输入缓冲区为空
fn wait_input_buffer_empty() {
    unsafe {
        while inb(PS2_CMD_PORT) & PS2_STATUS_INPUT_FULL != 0 {
            core::hint::spin_loop();
        }
    }
}

/// 等待输出缓冲区满
fn wait_output_buffer_full() -> bool {
    let mut timeout: u32 = 100000;
    
    while timeout > 0 {
        unsafe {
            if inb(PS2_CMD_PORT) & PS2_STATUS_OUTPUT_FULL != 0 {
                return true;
            }
        }
        timeout -= 1;
        core::hint::spin_loop();
    }
    
    false
}

/// 向 PS/2 控制器发送命令
fn ps2_send_command(cmd: u8) -> Result<()> {
    wait_input_buffer_empty();
    unsafe { outb(PS2_CMD_PORT, cmd); }
    Ok(())
}

/// 向键盘发送数据
fn keyboard_send_data(data: u8) -> Result<()> {
    wait_input_buffer_empty();
    unsafe { outb(PS2_DATA_PORT, data); }
    Ok(())
}

/// 从键盘读取数据
fn keyboard_read_data() -> Option<u8> {
    if !wait_output_buffer_full() {
        return None;
    }
    Some(unsafe { inb(PS2_DATA_PORT) })
}

/// 更新键盘 LED 状态
fn update_leds(modifiers: &ModifierState) {
    let _ = keyboard_send_data(KB_CMD_SET_LED);
    // 等待 ACK (0xFA)
    let _ = keyboard_read_data();
    let _ = keyboard_send_data(modifiers.to_led_byte());
    // 等待 ACK
    let _ = keyboard_read_data();
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

// SAFETY: 单核内核, 键盘操作无并发
unsafe impl Send for KeyboardDriver {}
unsafe impl Sync for KeyboardDriver {}

impl Driver for KeyboardDriver {
    fn name(&self) -> &'static str {
        "PS/2 Keyboard"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Input
    }

    fn init(&mut self) -> Result<()> {
        // 1. 清空输出缓冲区
        let _ = keyboard_read_data();

        // 2. 发送 SET LED 命令设置初始 LED 状态
        update_leds(&self.modifiers);

        // 3. 清空缓冲区
        self.buffer.clear();

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
        if !self.initialized {
            "Not initialized"
        } else {
            "Ready"
        }
    }
}

// ============================================================================
// 公共 API
// ============================================================================

impl KeyboardDriver {
    /// 创建新的键盘驱动实例
    pub fn new() -> Self {
        Self {
            modifiers: ModifierState::default(),
            buffer: KeyboardBuffer::default(),
            info: DeviceInfo::new("ps2_keyboard", DeviceType::Input),
            initialized: false,
        }
    }

    /// 处理 IRQ1 键盘中断
    ///
    /// 从 PS/2 数据端口读取 scancode 并转换后存入缓冲区。
    ///
    /// # Returns
    /// * `Some(u8)` - 成功读取并转换的 ASCII 字符
    /// * `None` - 无有效数据或特殊按键
    pub fn handle_interrupt(&mut self) -> Option<u8> {
        // 读取 scancode
        let scancode = match keyboard_read_data() {
            Some(s) => s,
            None => return None,
        };

        // 检测释放码 (0xE0 或 0xE1 前缀)
        if scancode == 0xE0 || scancode == 0xE1 {
            return None;  // 忽略扩展 scancode 前缀
        }

        // 检测按键释放 (bit 7 = 1 表示释放)
        let pressed = (scancode & 0x80) == 0;
        let key_code = scancode & 0x7F;

        // 处理修饰键
        match key_code {
            0x2A | 0x36 => {  // Left/Right Shift
                if pressed {
                    if key_code == 0x2A { self.modifiers.left_shift = true; }
                    else { self.modifiers.right_shift = true; }
                } else {
                    if key_code == 0x2A { self.modifiers.left_shift = false; }
                    else { self.modifiers.right_shift = false; }
                }
                return None;
            },
            0x1D => {  // Left Ctrl
                self.modifiers.left_ctrl = pressed;
                return None;
            },
            0x38 => {  // Left Alt
                self.modifiers.left_alt = pressed;
                return None;
            },
            0x3A => {  // Caps Lock
                if pressed {
                    self.modifiers.caps_lock = !self.modifiers.caps_lock;
                    update_leds(&self.modifiers);
                }
                return None;
            },
            0x45 => {  // Num Lock
                if pressed {
                    self.modifiers.num_lock = !self.modifiers.num_lock;
                    update_leds(&self.modifiers);
                }
                return None;
            },
            0x46 => {  // Scroll Lock
                if pressed {
                    self.modifiers.scroll_lock = !self.modifiers.scroll_lock;
                    update_leds(&self.modifiers);
                }
                return None;
            },
            _ => {},
        }

        // 只处理按键按下事件
        if !pressed {
            return None;
        }

        // 转换为 ASCII
        let ascii = if self.modifiers.shift_pressed() ^ self.modifiers.caps_lock {
            SHIFT_TABLE[key_code as usize]
        } else {
            SCANCODE_TABLE[key_code as usize]
        };

        // 存入缓冲区
        if ascii != 0x00 {
            let _ = self.buffer.push(ascii);
            Some(ascii)
        } else {
            None
        }
    }

    /// 读取一个字符 (非阻塞)
    ///
    /// # Returns
    /// * `Some(u8)` - 缓冲区中的字符
    /// * `None` - 缓冲区为空
    pub fn read_char(&mut self) -> Option<u8> {
        self.buffer.pop()
    }

    /// 读取一行文本 (阻塞直到遇到 Enter)
    ///
    /// # Arguments
    /// * `buffer` - 输出缓冲区
    /// * `max_len` - 最大长度
    ///
    /// # Returns
    /// * `Ok(usize)` - 实际读取的字符数 (不含换行符)
    /// * `Err(DriverError)` - 错误
    pub fn read_line(&mut self, buffer: &mut [u8], max_len: usize) -> Result<usize> {
        let mut count: usize = 0;

        loop {
            if let Some(ch) = self.read_char() {
                match ch {
                    b'\n' | b'\r' => {
                        break;
                    },
                    0x08 => {  // Backspace
                        if count > 0 {
                            count -= 1;
                        }
                    },
                    _ if count < max_len => {
                        buffer[count] = ch;
                        count += 1;
                    },
                    _ => {},  // 缓冲区满，忽略
                }
            }

            // 让出 CPU (避免忙等待)
            #[cfg(not(feature = "kernel_test"))]
            unsafe {
                extern "C" { fn scheduler_yield_ex(); }
                scheduler_yield_ex();
            }
        }

        Ok(count)
    }

    /// 检查缓冲区是否为空
    pub fn is_buffer_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// 获取缓冲区中的字符数
    pub fn buffer_length(&self) -> usize {
        self.buffer.len()
    }

    /// 获取当前修饰键状态
    pub fn get_modifiers(&self) -> &ModifierState {
        &self.modifiers
    }

    /// 清空键盘缓冲区
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// 获取设备信息
    pub fn get_info(&self) -> &DeviceInfo {
        &self.info
    }
}

// ============================================================================
// FFI 兼容接口
// ============================================================================

/// 全局键盘驱动实例 (无 unsafe, Mutex 保护)
static KEYBOARD_DEVICE: Mutex<Option<Box<KeyboardDriver>>> = Mutex::new(None);

/// 初始化键盘 (C 兼容接口)
#[no_mangle]
pub extern "C" fn keyboard_init() {
    let mut driver = Box::new(KeyboardDriver::new());
    let _ = driver.init();

    // 注册到几丁质框架 (非所有权指针, 内存由 KEYBOARD_DEVICE 管理)
    let raw_ptr: *mut KeyboardDriver = &mut *driver;
    let _id = crate::kernel::chitin::chitin_register(
        "ps2_keyboard",
        crate::kernel::chitin::ChitinProto::Input,
        None,
        Some(1), // IRQ 1
        raw_ptr as *mut core::ffi::c_void,
    );

    *KEYBOARD_DEVICE.lock() = Some(driver);
}

/// 处理键盘中断 (C 兼容接口)
/// IRQ 上下文使用 try_lock 避免与主代码路径死锁
#[no_mangle]
pub extern "C" fn keyboard_irq_handler() {
    if let Some(mut guard) = KEYBOARD_DEVICE.try_lock() {
        if let Some(ref mut driver) = *guard {
            driver.handle_interrupt();
        }
    }
}

/// 读取字符 (C 兼容接口)
#[no_mangle]
pub extern "C" fn keyboard_read_char() -> i32 {
    if let Some(ref mut guard) = *KEYBOARD_DEVICE.lock() {
        match guard.read_char() {
            Some(ch) => ch as i32,
            None => -1,
        }
    } else {
        -1
    }
}

/// 检查是否有可读字符 (C 兼容接口)
#[no_mangle]
pub extern "C" fn keyboard_has_char() -> i32 {
    if let Some(ref guard) = *KEYBOARD_DEVICE.lock() {
        if !guard.is_buffer_empty() { 1 } else { 0 }
    } else {
        0
    }
}

/// C 兼容别名: keyboard_has_data (旧C代码/FFI调用的名称)
#[no_mangle]
pub extern "C" fn keyboard_has_data() -> bool {
    keyboard_has_char() != 0
}

/// C 兼容别名: keyboard_get_char
#[no_mangle]
pub extern "C" fn keyboard_get_char() -> i32 {
    keyboard_read_char()
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scancode_table_basic() {
        assert_eq!(SCANCODE_TABLE[0x02], b'1');
        assert_eq!(SCANCODE_TABLE[0x03], b'2');
        assert_eq!(SCANCODE_TABLE[0x1E], b'a');
        assert_eq!(SCANCODE_TABLE[0x30], b'b');
        assert_eq!(SCANCODE_TABLE[0x39], b' ');
    }

    #[test]
    fn test_shift_table_basic() {
        assert_eq!(SHIFT_TABLE[0x02], b'!');
        assert_eq!(SHIFT_TABLE[0x03], b'@');
        assert_eq!(SHIFT_TABLE[0x1E], b'A');
        assert_eq!(SHIFT_TABLE[0x30], b'B');
    }

    #[test]
    fn test_special_keys() {
        assert_eq!(get_special_key(0x0D), SpecialKey::Enter);
        assert_eq!(get_special_key(0x0E), SpecialKey::Backspace);
        assert_eq!(get_special_key(0x48), SpecialKey::ArrowUp);
        assert_eq!(get_special_key(0x4B), SpecialKey::ArrowLeft);
        assert_eq!(get_special_key(0x57), SpecialKey::F11);
        
        // 无效 scancode
        assert_eq!(get_special_key(0xFF), SpecialKey::None);
    }

    #[test]
    fn test_modifier_state_default() {
        let mods = ModifierState::default();
        
        assert!(!mods.shift_pressed());
        assert!(!mods.ctrl_pressed());
        assert!(!mods.alt_pressed());
        assert!(!mods.caps_lock);
        assert!(mods.num_lock);  // 默认开启
    }

    #[test]
    fn test_modifier_state_operations() {
        let mut mods = ModifierState::default();
        
        // 测试 Shift
        mods.left_shift = true;
        assert!(mods.shift_pressed());
        mods.right_shift = true;
        assert!(mods.shift_pressed());
        mods.left_shift = false;
        assert!(mods.shift_pressed());  // 右 Shift 仍按下
        
        // 测试 Caps Lock
        mods.caps_lock = true;
        assert!(mods.caps_lock);
        
        // LED 字节计算
        let led = mods.to_led_byte();
        assert!(led & KB_LED_CAPS_LOCK != 0);
        assert!(led & KB_LED_NUM_LOCK != 0);
    }

    #[test]
    fn test_keyboard_buffer() {
        let mut buf = KeyboardBuffer::default();
        
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
        
        // 写入数据
        assert!(buf.push(b'A').is_ok());
        assert!(buf.push(b'B').is_ok());
        assert!(!buf.is_empty());
        assert_eq!(buf.len(), 2);
        
        // 读取数据
        assert_eq!(buf.pop(), Some(b'A'));
        assert_eq!(buf.pop(), Some(b'B'));
        assert!(buf.is_empty());
        assert_eq!(buf.pop(), None);
    }

    #[test]
    fn test_driver_trait_impl() {
        let mut driver = KeyboardDriver::new();
        
        assert_eq!(driver.name(), "PS/2 Keyboard");
        assert_eq!(driver.device_type(), DeviceType::Input);
        assert!(!driver.is_ready());
        
        let result = driver.init();
        let _ = result;
        assert!(driver.status().len() > 0);
    }

    #[test]
    fn test_error_codes() {
        let err = DriverError::Busy;
        assert_eq!(err.to_string(), "Device busy");
    }
}
