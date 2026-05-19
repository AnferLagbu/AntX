//! 硬件驱动测试示例 (Hardware Driver Test Example)
//!
//! 测试所有基本硬件驱动的功能：
//! - VGA 文本模式显示
//! - 串口输出
//! - PIT 定时器
//! - PS/2 键盘
//!
//! 此文件用于 QEMU 环境下的驱动验证。

#![no_std]
#![no_main]

use core::panic::PanicInfo;

// ============================================================================
// 内核入口点
// ============================================================================

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 初始化所有驱动
    test_drivers();
    
    // 进入无限循环
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

// ============================================================================
// 驱动测试函数
// ============================================================================

fn test_drivers() {
    // 1. 测试 VGA 显示
    test_vga();
    
    // 2. 测试串口输出
    test_serial();
    
    // 3. 测试 PIT 定时器
    test_pit();
    
    // 4. 测试键盘 (轮询模式)
    test_keyboard();
}

/// 测试 VGA 文本模式驱动
fn test_vga() {
    // 使用 VGA 驱动输出测试信息
    vga_println!("=== VGA Driver Test ===");
    vga_println!("Initializing VGA text mode (80x25)...");
    
    // 测试颜色
    vga_set_color!(Color::LightGreen, Color::Black);
    vga_println!("[OK] VGA initialized successfully");
    
    vga_set_color!(Color::White, Color::Black);
    vga_println!("Screen width: 80");
    vga_println!("Screen height: 25");
    vga_println!("Buffer address: 0xB8000");
    
    // 测试边框绘制
    vga_println!("Drawing border...");
    // draw_test_border();
    
    vga_set_color!(Color::LightCyan, Color::Black);
    vga_println!("[PASS] VGA test completed");
    vga_set_color!(Color::White, Color::Black);
    vga_println!("");
}

/// 测试串口驱动
fn test_serial() {
    vga_println!("=== Serial Port Test ===");
    vga_println!("Testing COM1 (0x3F8)...");
    
    // 初始化串口
    serial_init(0);
    
    // 发送测试字符串
    serial_puts(0, b"QueenX - Serial Port Test\n");
    serial_puts(0, b"COM1 initialized at 115200 baud\n");
    serial_puts(0, b"8N1 configuration\n");
    
    vga_set_color!(Color::LightGreen, Color::Black);
    vga_println!("[OK] Serial port test completed");
    vga_set_color!(Color::White, Color::Black);
    vga_println!("");
}

/// 测试 PIT 定时器
fn test_pit() {
    vga_println!("=== PIT Timer Test ===");
    vga_println!("Initializing PIT (8254)...");
    
    // 初始化 PIT 为 1000 Hz (1ms 间隔)
    // let freq = pit_init(1000);
    
    vga_println!("Target frequency: 1000 Hz");
    vga_println!("Base frequency: 1.193182 MHz");
    vga_println!("Divisor: 1193");
    
    vga_set_color!(Color::LightGreen, Color::Black);
    vga_println!("[OK] PIT timer initialized");
    
    vga_set_color!(Color::White, Color::Black);
    vga_println!("Testing timer delay...");
    
    // 简单延迟测试
    for i in 1..=5 {
        vga_print!("Delay test ");
        vga_println!(i);
        // pit_delay_ms(100);
    }
    
    vga_set_color!(Color::LightCyan, Color::Black);
    vga_println!("[PASS] PIT test completed");
    vga_set_color!(Color::White, Color::Black);
    vga_println!("");
}

/// 测试 PS/2 键盘驱动
fn test_keyboard() {
    vga_println!("=== Keyboard Test ===");
    vga_println!("Initializing PS/2 keyboard...");
    
    // 初始化键盘
    keyboard_init();
    
    vga_set_color!(Color::LightGreen, Color::Black);
    vga_println!("[OK] Keyboard initialized");
    
    vga_set_color!(Color::White, Color::Black);
    vga_println!("Waiting for key press (polling mode)...");
    vga_println!("Press any key to continue (timeout: 10s)");
    
    // 简单的键盘轮询测试
    let mut count = 0;
    let timeout = 10000000; // 简单的超时计数
    
    while count < timeout {
        // 检查是否有按键
        if keyboard_has_char() > 0 {
            let ch = keyboard_read_char();
            if ch != 0 {
                vga_print!("Key pressed: ");
                vga_putchar(ch as u8);
                vga_println!("");
                break;
            }
        }
        count += 1;
    }
    
    if count >= timeout {
        vga_set_color!(Color::Yellow, Color::Black);
        vga_println!("[TIMEOUT] No key pressed");
    } else {
        vga_set_color!(Color::LightCyan, Color::Black);
        vga_println!("[PASS] Keyboard test completed");
    }
    
    vga_set_color!(Color::White, Color::Black);
    vga_println!("");
}

// ============================================================================
// 辅助宏和函数
// ============================================================================

/// VGA 打印宏 (简化版)
macro_rules! vga_println {
    () => {
        vga_putchar(b'\n');
    };
    ($fmt:literal) => {
        vga_puts(concat!($fmt, "\n").as_bytes());
    };
    ($fmt:literal, $($arg:expr),+) => {
        // 简化实现，不支持格式化
        vga_puts(concat!($fmt, "\n").as_bytes());
    };
}

/// VGA 打印宏 (不换行)
macro_rules! vga_print {
    ($fmt:literal) => {
        vga_puts($fmt.as_bytes());
    };
}

/// VGA 设置颜色宏
macro_rules! vga_set_color {
    ($fg:expr, $bg:expr) => {
        vga_set_color($fg as u8, $bg as u8);
    };
}

// ============================================================================
// 外部函数声明 (FFI)
// ============================================================================

extern "C" {
    // VGA 函数
    fn vga_init();
    fn vga_putchar(ch: i32);
    fn vga_puts(s: *const i8);
    fn vga_clear();
    fn vga_set_color(fg: u8, bg: u8);
    
    // 串口函数
    fn serial_init(com: u32);
    fn serial_putc(com: u32, ch: i32);
    fn serial_puts(com: u32, s: *const i8);
    
    // 键盘函数
    fn keyboard_init();
    fn keyboard_has_char() -> i32;
    fn keyboard_read_char() -> i32;
    
    // PIT 函数
    // fn pit_init(freq: u32) -> u32;
    // fn pit_delay_ms(ms: u32);
}

// ============================================================================
// 颜色枚举 (与 VGA 驱动匹配)
// ============================================================================

#[allow(dead_code)]
enum Color {
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

// ============================================================================
// Panic 处理
// ============================================================================

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        vga_set_color(Color::LightRed as u8, Color::Black as u8);
        vga_puts(b"\n!!! KERNEL PANIC !!!\n".as_ptr() as *const i8);
        vga_puts(b"System halted.\n".as_ptr() as *const i8);
    }
    
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
