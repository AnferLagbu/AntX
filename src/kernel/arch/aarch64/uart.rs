//! PL011 UART 驱动 (ARM PrimeCell UART)
//!
//! QEMU virt 机器默认使用 PL011 @ 0x09000000。
//! 寄存器定义基于 ARM DDI 0183G。

use core::ptr::{read_volatile, write_volatile};

/// PL011 寄存器基地址 (QEMU virt)
pub const PL011_BASE: u64 = 0x09000000;

/// PL011 寄存器偏移
const UARTDR: u64 = 0x000; // Data Register
const UARTFR: u64 = 0x018; // Flag Register
const UARTIBRD: u64 = 0x024; // Integer Baud Rate Divisor
const UARTFBRD: u64 = 0x028; // Fractional Baud Rate Divisor
const UARTLCR_H: u64 = 0x02C; // Line Control Register
const UARTCR: u64 = 0x030; // Control Register
const UARTIMSC: u64 = 0x038; // Interrupt Mask Set/Clear

/// 标志位
const UARTFR_TXFF: u32 = 1 << 5; // Transmit FIFO Full
const UARTFR_RXFE: u32 = 1 << 4; // Receive FIFO Empty

/// 控制位
const UARTCR_UARTEN: u32 = 1 << 0; // UART Enable
const UARTCR_TXE: u32 = 1 << 8; // Transmit Enable
const UARTCR_RXE: u32 = 1 << 9; // Receive Enable

/// LCR 配置: 8N1 (8 data bits, no parity, 1 stop bit)
const UARTLCR_8N1: u32 = (0b11 << 5) | (0 << 4) | (0 << 1);

// ============================================================================
// 寄存器 I/O
// ============================================================================

#[inline(always)]
unsafe fn read(offset: u64) -> u32 {
    read_volatile((PL011_BASE + offset) as *const u32)
}

#[inline(always)]
unsafe fn write(offset: u64, val: u32) {
    write_volatile((PL011_BASE + offset) as *mut u32, val);
}

// ============================================================================
// UART 初始化
// ============================================================================

/// 初始化 PL011 UART (115200-8N1)
pub unsafe fn init() {
    // 1. 禁用 UART
    write(UARTCR, 0);

    // 2. 设置波特率 (115200 @ 24MHz 或 62.5MHz 时钟)
    // QEMU virt 的 PL011 时钟频率取决于具体配置, 默认 ~24MHz
    // 波特率除数 = UARTCLK / (16 * BaudRate)
    // 以 24MHz 为例: 24000000 / (16 * 115200) ≈ 13.02 → IBRD=13, FBRD≈0
    write(UARTIBRD, 13); // 整数部分
    write(UARTFBRD, 0); // 小数部分 (0.02 * 64 ≈ 1, 但 QEMU 不严格要求)

    // 3. 设置数据格式: 8N1, FIFO enable
    write(UARTLCR_H, UARTLCR_8N1 | (1 << 4)); // 8N1 + FIFO enable

    // 4. 禁用中断 (Polling 模式)
    write(UARTIMSC, 0);

    // 5. 启用 UART: TX + RX + UART
    write(UARTCR, UARTCR_UARTEN | UARTCR_TXE | UARTCR_RXE);
}

// ============================================================================
// 数据收发
// ============================================================================

/// 发送单字节 (阻塞)
#[inline(always)]
pub unsafe fn putc(c: u8) {
    // Wait for TX FIFO not full
    while read(UARTFR) & UARTFR_TXFF != 0 {
        core::hint::spin_loop();
    }
    write(UARTDR, c as u32);
}

/// 接收单字节 (阻塞)
#[inline(always)]
pub unsafe fn getc() -> u8 {
    // Wait for RX FIFO not empty
    while read(UARTFR) & UARTFR_RXFE != 0 {
        core::hint::spin_loop();
    }
    read(UARTDR) as u8
}

/// 发送字符串
pub fn puts(s: &str) {
    for c in s.bytes() {
        unsafe {
            putc(c);
        }
    }
    unsafe {
        putc(b'\r');
    }
    unsafe {
        putc(b'\n');
    }
}
