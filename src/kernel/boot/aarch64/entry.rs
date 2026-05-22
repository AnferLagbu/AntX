//! AArch64 启动入口 (Rust 侧)
//!
//! 从 start.S 跳转后的第一个 Rust 函数。负责:
//!   1. BSS 清零 (Rust 期望未初始化静态变量为 0)
//!   2. MMU 初始化 (identity mapping + 启用)
//!   3. GICv3 初始化
//!   4. 异常向量表设置
//!   5. 跳转 kernel_init()

use core::ptr;

// PL011 UART 基地址 (QEMU virt)
const UART_BASE: u64 = 0x09000000;
const UART_FR: u64 = 0x18;   // Flag register
const UART_DR: u64 = 0x00;   // Data register
const UART_FR_TXFF: u32 = 1 << 5; // Transmit FIFO full

/// PL011 发送单字节
unsafe fn uart_putc(c: u8) {
    // Wait for TX FIFO not full
    while ptr::read_volatile((UART_BASE + UART_FR) as *const u32) & UART_FR_TXFF != 0 {
        core::hint::spin_loop();
    }
    ptr::write_volatile((UART_BASE + UART_DR) as *mut u32, c as u32);
}

/// UART 发送字符串
fn uart_puts(s: &str) {
    for c in s.bytes() {
        unsafe { uart_putc(c); }
    }
    // CRLF
    unsafe { uart_putc(b'\r'); }
    unsafe { uart_putc(b'\n'); }
}

// ============================================================================
// 启动入口
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn entry() -> ! {
    // 1. BSS 清零
    clear_bss();

    // 2. 早期串口输出
    uart_puts("[aarch64] QueenX starting...");

    // 3. 初始化 MMU (identity mapping)
    uart_puts("[aarch64] Initializing MMU...");
    crate::kernel::arch::aarch64::mmu::init();

    // 4. 初始化异常向量表
    uart_puts("[aarch64] Setting up exception vectors...");
    crate::kernel::arch::aarch64::exception::init();

    // 5. 初始化 GICv3
    uart_puts("[aarch64] Initializing GICv3...");
    crate::kernel::arch::aarch64::gic::init();

    // 6. 跳转内核主循环
    uart_puts("[aarch64] Booting kernel...");
    crate::kernel_init();

    // kernel_init() returns, but entry must diverge
    loop {
        unsafe { core::arch::asm!("wfi", options(nomem, nostack)); }
    }
}

// ============================================================================
// BSS 清零
// ============================================================================

extern "C" {
    static mut __bss_start: u8;
    static _kernel_end: u8;
}

unsafe fn clear_bss() {
    let bss_start = &mut __bss_start as *mut u8;
    let bss_end = &_kernel_end as *const u8 as usize;

    // 仅当 BSS 区域合法时清零
    if bss_start as usize >= bss_end {
        return;
    }

    let size = bss_end - bss_start as usize;
    ptr::write_bytes(bss_start, 0, size);
}