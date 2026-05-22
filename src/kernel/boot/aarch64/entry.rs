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
    // 0. 启用 FP/SIMD (编译器会生成 NEON 指令如 movi v0.2d)
    //    CPACR_EL1.FPEN[21:20] = 0b11 → 不 trap FP/SIMD
    core::arch::asm!("mrs x0, cpacr_el1", "orr x0, x0, #(0x3 << 20)", "msr cpacr_el1, x0", out("x0") _);

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
    kernel_init_aarch64();

    // 不应该到达这里
    loop {
        crate::arch!(halt());
    }
}

/// AArch64 内核初始化 (最小化, 替代 kernel_init)
#[no_mangle]
pub unsafe extern "C" fn kernel_init_aarch64() {
    crate::kernel::klog::klog_init();
    crate::klog_boot_info!("[aarch64] KLog initialized");
    crate::klog_boot_info!("[aarch64] QueenX aarch64 boot complete");

    // TODO: 后续实现:
    // - 物理内存管理 (PMM)
    // - 虚拟内存管理 (VMM)
    // - 内核堆 (kmalloc)
    // - 调度器
    // - 文件系统
    // - 用户进程
    crate::klog_boot_info!("[aarch64] Halting — init not yet implemented");

    loop {
        crate::arch!(halt());
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