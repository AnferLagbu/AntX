//! AArch64 启动入口 (Rust 侧)
//!
//! 从 start.S 跳转后的第一个 Rust 函数。负责:
//!   1. BSS 清零
//!   2. PL011 UART 初始化
//!   3. MMU 初始化 (identity mapping + TTBR1)
//!   4. 异常向量表设置
//!   5. GICv3 初始化
//!   6. Timer 初始化
//!   7. 跳转 kernel_init()

use crate::kernel::arch::aarch64::uart;

// ============================================================================
// 启动入口
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn entry() -> ! {
    // 1. BSS 清零
    clear_bss();

    // 2. 初始化 UART
    uart::init();
    uart::puts("[aarch64] QueenX starting...");

    // 3. 初始化 MMU (identity mapping + TTBR1)
    uart::puts("[aarch64] Initializing MMU...");
    crate::kernel::arch::aarch64::mmu::init();

    // 4. 初始化异常向量表
    uart::puts("[aarch64] Setting up exception vectors...");
    crate::kernel::arch::aarch64::exception::init();

    // 5. 初始化 GICv3
    uart::puts("[aarch64] Initializing GICv3...");
    crate::kernel::arch::aarch64::gic::init();

    // 6. 初始化定时器
    uart::puts("[aarch64] Initializing timer...");
    crate::kernel::arch::aarch64::timer::init();

    // 7. 跳转内核主循环
    uart::puts("[aarch64] Booting kernel...");
    crate::kernel_init();

    // kernel_init() returns, but entry must diverge
    loop {
        core::arch::asm!("wfi", options(nomem, nostack));
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

    if bss_start as usize >= bss_end {
        return;
    }

    let size = bss_end - bss_start as usize;
    core::ptr::write_bytes(bss_start, 0, size);
}