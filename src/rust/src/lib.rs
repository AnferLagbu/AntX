#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(asm)]

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
    crate::kernel::barrier::PANIC_FLAG.store(true, Ordering::SeqCst);

    let msg = alloc::format!("{}", info);
    let bytes = msg.as_bytes();
    let len = bytes.len().min(127);
    {
        let mut panic_msg = crate::kernel::barrier::PANIC_MSG.lock();
        panic_msg[..len].copy_from_slice(&bytes[..len]);
        panic_msg[len] = 0;
    }

    // Emit panic diagnostics to serial console before entering recovery
    if crate::kernel::klog::KLOG_INIT.load(Ordering::Acquire) {
        crate::kernel::klog::serial_write_bytes(b"\n========== KERNEL PANIC ==========\n");
        crate::kernel::klog::serial_write_bytes(msg.as_bytes());
        crate::kernel::klog::serial_write_bytes(b"\n");

        let mut regs: [u64; 16] = [0; 16];
        unsafe {
            core::arch::asm!(
                "mov {0}, rax", "mov {1}, rbx", "mov {2}, rcx",
                "mov {3}, rdx", "mov {4}, rsi", "mov {5}, rdi",
                out(reg) regs[0], out(reg) regs[1], out(reg) regs[2],
                out(reg) regs[3], out(reg) regs[4], out(reg) regs[5],
                options(nostack, preserves_flags)
            );
            core::arch::asm!(
                "mov {0}, rbp", "mov {1}, rsp", "mov {2}, r8",
                "mov {3}, r9",  "mov {4}, r10", "mov {5}, r11",
                out(reg) regs[6], out(reg) regs[7], out(reg) regs[8],
                out(reg) regs[9], out(reg) regs[10], out(reg) regs[11],
                options(nostack, preserves_flags)
            );
            core::arch::asm!(
                "mov {0}, r12", "mov {1}, r13", "mov {2}, r14", "mov {3}, r15",
                out(reg) regs[12], out(reg) regs[13], out(reg) regs[14], out(reg) regs[15],
                options(nostack, preserves_flags)
            );
        }
        let reg_names = [
            b"RAX", b"RBX", b"RCX", b"RDX",
            b"RSI", b"RDI", b"RBP", b"RSP",
            b"R8 ", b"R9 ", b"R10", b"R11",
            b"R12", b"R13", b"R14", b"R15",
        ];
        crate::kernel::klog::serial_write_bytes(b"--- Register Dump ---\n");
        for i in 0..16 {
            crate::kernel::klog::serial_write_bytes(b"  ");
            crate::kernel::klog::serial_write_bytes(reg_names[i]);
            crate::kernel::klog::serial_write_bytes(b"= 0x");
            let mut hex_buf = [0u8; 16];
            let v = regs[i];
            for d in 0..16 {
                let nibble = ((v >> (60 - d * 4)) & 0xF) as u8;
                hex_buf[d] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
            }
            crate::kernel::klog::serial_write_bytes(&hex_buf);
            if i % 4 == 3 {
                crate::kernel::klog::serial_write_bytes(b"\n");
            }
        }
        let mut cr2: u64 = 0;
        let mut cr3_val: u64 = 0;
        unsafe {
            core::arch::asm!("mov {}, cr2", out(reg) cr2);
            core::arch::asm!("mov {}, cr3", out(reg) cr3_val);
        }
        crate::kernel::klog::serial_write_bytes(b"  CR2= 0x");
        for d in 0..16 {
            let nibble = ((cr2 >> (60 - d * 4)) & 0xF) as u8;
            crate::kernel::klog::serial_write_bytes(&[if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 }]);
        }
        crate::kernel::klog::serial_write_bytes(b"  CR3= 0x");
        for d in 0..16 {
            let nibble = ((cr3_val >> (60 - d * 4)) & 0xF) as u8;
            crate::kernel::klog::serial_write_bytes(&[if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 }]);
        }
        crate::kernel::klog::serial_write_bytes(b"\n===================================\n");
    }

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
    crate::klog_boot_info!("QueenX starting {} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    // Test mode: skip normal init, run unit tests
    #[cfg(feature = "kernel_test")]
    {
        unsafe { core::arch::asm!("cli", options(nomem, nostack)); }

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
        crate::kernel::mm::pmm::pmm_init_bitmap(KMALLOC_HEAP_SIZE);
        crate::kernel::idt::idt_init();
        crate::klog_boot_info!("Test mode: IDT initialized");

        #[cfg(feature = "fault_injection")]
        {
            let rate = option_env!("FAULT_RATE").and_then(|s| s.parse::<u32>().ok()).unwrap_or(50);
            crate::kernel::barrier::fault_inject::FAULT_INJECTION_RATE.store(rate, core::sync::atomic::Ordering::Relaxed);
            crate::klog_boot_info!("[CHAOS] Fault injection enabled, rate={}/1000", rate);
        }

        crate::kernel::tests::test_runner_init();
        crate::klog_boot_info!("Tests complete");

        let r = crate::kernel::tests::runner();
        let failed = r.failed.load(Ordering::SeqCst);
        crate::kernel::tests::qemu_exit(failed == 0);
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
    crate::kernel::arch::x86_64::gdt::gdt_init();
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

    // 10. Network (lwIP + E1000) — now safe: cli/sti critical section protects lwip_init
    crate::kernel::net::init::qx_net_init();
    crate::klog_boot_info!("Network subsystem initialized");

    // 11. Barrier-Stack recovery domains
    crate::kernel::klog::serial_write_bytes(b"[BOOT] step 11 start\n");
    crate::kernel::mm::pmm::pmm_register_barrier_domain();
    crate::kernel::klog::serial_write_bytes(b"[BOOT] step 11 pmm done\n");
    crate::kernel::proc::process::proc_register_barrier_domain();
    crate::kernel::klog::serial_write_bytes(b"[BOOT] step 11 proc done\n");
    crate::klog_boot_info!("Barrier-stack recovery domains registered (PMM=3, PROC=4)");

    #[cfg(not(feature = "kernel_test"))]
    {
        extern "C" { fn ata_read_sector(disk: u8, sector: u32, buf: *mut u8) -> i32; }
        let mut cfg = [0u8; 512];
        if unsafe { ata_read_sector(0, 2046, cfg.as_mut_ptr()) } >= 0
            && cfg[0] == b'A' && cfg[1] == b'N' && cfg[2] == b'T' && cfg[3] == b'X'
        {
            let hvfs_lba = u32::from_le_bytes([cfg[4], cfg[5], cfg[6], cfg[7]]);
            crate::klog_boot_info!("Disk boot: HvFS at LBA {} → mounting root", hvfs_lba);
            crate::kernel::fs::hvfs::hvfs::get_hvfs().spa.disk_present.store(true, core::sync::atomic::Ordering::Release);
            crate::kernel::fs::hvfs::hvfs::get_hvfs().init();
            let r = crate::kernel::fs::vfs::ffi::vfs_mount_internal(
                b"/\0".as_ptr() as *const i8,
                b"hvfs\0".as_ptr() as *const i8,
            );
            if r == 0 { crate::klog_boot_info!("Root filesystem: HvFS (disk)"); }
            else { crate::klog_boot_info!("HvFS mount failed"); }
        }
    }

    crate::klog_boot_info!("QueenX initialized, entering user mode...");

    // 12. Launch first user process (Ring 3)
    unsafe {
        crate::kernel::proc::ffi::launch_first_user_process();
    }
    // unreachable: launch_first_user_process is noreturn
    } // end #[cfg(not(feature = "kernel_test"))]
}
