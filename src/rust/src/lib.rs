#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(asm)]  // 内联汇编支持 (x86_64 特定指令)

// ============================================================================
// ✅ 全局警告抑制配置 (内核开发环境特有)
// ============================================================================

//! 允许的警告类别 (符合 OS 内核开发最佳实践)

// 0. 稳定特性使用 - asm 特性在 nightly 中稳定但标记为需要 feature
#![allow(stable_features)]            // 1个: asm 特性声明

// 1. 全局单例模式 - 内核中常见且必要
#![allow(static_mut_refs)]           // 32个: TRUST_CHAIN, TOKEN_MANAGER 等全局可变静态

// 2. 第三方库配置 - lwIP 的 feature flags
#![allow(unexpected_cfgs)]           // ~35个: snmp, mdns, ipv6, sntp 等

// 3. C 语言兼容性 - 与原有 C 代码保持一致的命名风格
#![allow(non_upper_case_globals)]   // 函数名: kfree, kmalloc 等
#![allow(non_camel_case_types)]     // 类型名: u8_t, u32_t 等
#![allow(non_snake_case)]            // 变量名: io_port 等

// 4. FFI 边界 - C/Rust 互操作不可避免
#![allow(improper_ctypes)]         // 3个: IrqSaveFlags, SysProt 等 FFI 类型
#![allow(dead_code)]                 // 多个: 未使用的函数/字段 (API 导出预留)

// 5. 安全相关 - 已通过代码审查确认安全
#![allow(unused_unsafe)]           // 16个: 过度保守的 unsafe 块

extern crate alloc;

mod memory_allocator;

// ============================================================================
// 内核模块 (统一从 kernel/ 入口)
// ============================================================================

/// AntX 内核 - 所有子系统的统一入口
///
/// ## 模块结构
///
/// ```text
/// kernel/
/// ├── arch/       # 架构相关 (x86_64)
/// ├── cpu/        # CPU 管理
/// ├── mm/         # 内存管理
/// ├── proc/       # 进程/线程
/// ├── fs/         # 文件系统
/// ├── net/        # 网络协议栈
/// ├── idt/        # 中断处理
/// ├── sync/       # 同步原语
/// ├── pwid/       # 安全框架
/// ├── dma/        # DMA 引擎
/// ├── barrier/    # 故障恢复
/// ├── pci/        # PCI 管理
/// ├── syscall/    # 系统调用
/// └── driver/     # 设备驱动
/// ```
#[path = "../../kernel/mod.rs"]
pub mod kernel;

// 重新导出常用类型 (方便直接使用 crate::xxx 而非 crate::kernel::xxx)
pub use kernel::cpu::CpuInfo;
pub use kernel::klog::LogLevel;

use core::panic::PanicInfo;
use core::sync::atomic::Ordering;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Signal to the scheduler/IDT that a recoverable panic occurred
    crate::kernel::barrier::PANIC_FLAG.store(true, Ordering::SeqCst);

    // Store panic message for recovery diagnostics
    let msg = alloc::format!("{}", info);
    let bytes = msg.as_bytes();
    let len = bytes.len().min(127);
    {
        let mut panic_msg = crate::kernel::barrier::PANIC_MSG.lock();
        panic_msg[..len].copy_from_slice(&bytes[..len]);
        panic_msg[len] = 0;
    }

    // Trigger int 0x82 — dedicated recovery interrupt.
    // The IDT handler will check PANIC_FLAG → attempt domain recovery → return.
    // If recovery fails, it falls through to kernel panic.
    unsafe {
        core::arch::asm!("int 0x82", options(noreturn));
    }
}

#[alloc_error_handler]
fn alloc_error(layout: alloc::alloc::Layout) -> ! {
    panic!("Allocation error: {:?}", layout);
}

#[no_mangle]
pub extern "C" fn kernel_init() {
    // 0. KLog — 自举串口驱动, 必须先于所有子系统
    unsafe { crate::kernel::klog::klog_init(); }
    crate::klog_boot_info!("AntX kernel starting {} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    // Test mode: skip normal init, run unit tests
    #[cfg(feature = "kernel_test")]
    {
        // Minimal init: boot + PMM + VMM + kmalloc + IDT (for Mutex)
        let boot_info = crate::kernel::boot::init();
        crate::kernel::mm::pmm::pmm_init(boot_info.mem_size, boot_info.kernel_end);
        crate::kernel::mm::vmm::vmm_init();
        const KMALLOC_HEAP_SIZE: u64 = 16 * 1024 * 1024;
        let heap_start = crate::kernel::mm::VirtAddr(
            crate::kernel::mm::KERNEL_BASE + boot_info.kernel_end + 0x200000
        );
        unsafe {
            crate::kernel::mm::kmalloc::get_kmalloc_mut().init(heap_start, KMALLOC_HEAP_SIZE);
        }
        // Initialize IDT and interrupts (needed for spin::Mutex)
        crate::kernel::idt::idt_init();
        crate::klog_boot_info!("Test mode: IDT initialized");
        crate::kernel::tests::test_runner_init();
        crate::klog_boot_info!("Tests complete, halting");
        // Halt after tests
        loop {
            unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
        }
    }

    // 1. Boot Info — 解析Multiboot信息获取内存布局
    #[cfg(not(feature = "kernel_test"))]
    {
    let boot_info = crate::kernel::boot::init();
    crate::klog_boot_info!("Boot info: mem={} MB, kernel_end=0x{:X}", 
        boot_info.mem_size / (1024 * 1024), boot_info.kernel_end);

    // 2. PMM — 物理内存管理器初始化
    crate::kernel::mm::pmm::pmm_init(boot_info.mem_size, boot_info.kernel_end);
    crate::klog_boot_info!("PMM initialized");

    // 3. VMM — 虚拟内存管理器初始化 (必须在PMM之后)
    crate::kernel::mm::vmm::vmm_init();
    crate::klog_boot_info!("VMM initialized");

    // 4. kmalloc — 内核堆初始化
    const KMALLOC_HEAP_SIZE: u64 = 16 * 1024 * 1024; // 16 MB
    let heap_start = crate::kernel::mm::VirtAddr(
        crate::kernel::mm::KERNEL_BASE + boot_info.kernel_end + 0x200000 // 在内核结束后的2MB处
    );
    unsafe {
        crate::kernel::mm::kmalloc::get_kmalloc_mut().init(heap_start, KMALLOC_HEAP_SIZE);
    }
    crate::klog_boot_info!("kmalloc initialized at 0x{:X}, size={} MB", 
        heap_start.0, KMALLOC_HEAP_SIZE / (1024 * 1024));

    // 5. PMM Bitmap — 初始化位图分配器
    crate::kernel::mm::pmm::pmm_init_bitmap(KMALLOC_HEAP_SIZE);
    crate::klog_boot_info!("PMM bitmap initialized");

    // 6. IDT + PIC
    crate::kernel::idt::idt_init();
    crate::klog_boot_info!("IDT+PIC ready");

    // 7. Timer + IRQ0
    match crate::kernel::timer::timer_init(1000) {
        Ok(_freq) => {
            crate::klog_boot_info!("PIT timer configured");
            let _ = crate::kernel::timer::irq::register_timer_irq();
            crate::klog_boot_info!("IRQ0 handler registered");
            crate::klog_boot_info!("Interrupts enabled");

            unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
        },
        Err(_msg) => { let _ = _msg; }
    }

    // 8. Scheduler
    crate::kernel::proc::scheduler::init();
    crate::klog_boot_info!("Scheduler ready");

    // 9. VFS
    crate::kernel::fs::vfs::init();
    crate::klog_boot_info!("VFS ready");

    // 10. Network (lwIP + E1000)
    #[cfg(not(feature = "kernel_test"))]
    crate::kernel::net::init::qx_net_init();

    // 11. Barrier-Stack recovery domains
    crate::kernel::mm::pmm::pmm_register_barrier_domain();
    crate::kernel::proc::process::proc_register_barrier_domain();
    #[cfg(not(feature = "kernel_test"))]
    crate::kernel::net::netif::net_register_barrier_domain();
    crate::klog_boot_info!("Barrier-stack recovery domains registered (PMM=3, PROC=4, NET=5)");

    crate::klog_boot_info!("AntX kernel initialized, entering main loop...");

    // 主循环 — 轮询网络数据包
    loop {
        extern "C" {
            fn e1000_poll_rx();
        }
        unsafe { e1000_poll_rx(); }

        unsafe {
            core::arch::asm!(
                "sti",
                "hlt",
                "cli",
                options(nomem, nostack)
            );
        }
    }
    } // end #[cfg(not(feature = "kernel_test"))]
}
