#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
// I-09: 移除 `#![feature(asm)]`. nightly 1.97 中 `core::arch::asm!` 已稳定,
// 源码中所有 asm 调用已走 `core::arch::asm!`, 顶层 feature gate 不再需要.
// ============================================================================
// ✅ 全局警告抑制配置 (内核开发环境特有)
// ============================================================================

//! 允许的警告类别 (符合 OS 内核开发最佳实践)

// I-09: 移除 `#![allow(stable_features)]` — 该 allow 仅用于 asm feature
// 声明, 已一并移除, 不再有 unstable 特性走 stable 路径.

// 1. 全局单例模式 - 内核中常见且必要
#![allow(static_mut_refs)]
// 32个: TRUST_CHAIN, TOKEN_MANAGER 等全局可变静态

// 2. C 语言兼容性 - 与原有 C 代码保持一致的命名风格
#![allow(non_upper_case_globals)] // 函数名: kfree, kmalloc 等
#![allow(non_camel_case_types)] // 类型名: u8_t, u32_t 等
#![allow(non_snake_case)] // 变量名: io_port 等

// 4. FFI 边界 - C/Rust 互操作不可避免
#![allow(improper_ctypes)]
// 3个: IrqSaveFlags, FFI 类型

// 5. 安全相关 - 已通过代码审查确认安全
#![allow(unused_unsafe)]
// 16个: 过度保守的 unsafe 块

// 6. Clippy: 内核代码中原始指针解引用是固有操作，由调用者保证安全性
#![allow(clippy::not_unsafe_ptr_arg_deref)]
// 7. Clippy: 内核内部 &self → &mut T 模式（如 Mutex::get_mut、UnsafeCell 包装）
#![allow(clippy::mut_from_ref)]
// 8. Clippy: 内核 C 字符串字面量 — 多用于 FFI，接收方类型多样（*const u8/i8/c_char）
#![allow(clippy::manual_c_str_literals)]
// 9. Clippy: 内核文档注释风格 — 模块级 doc comment 后空行是既存惯例
#![allow(clippy::empty_line_after_doc_comments)]
// 10. Clippy: Result<_, ()> — 内核错误路径使用 () 作为错误值是有意设计
#![allow(clippy::result_unit_err)]
// 11. Clippy: module_inception — 内核模块命名（如 fs/hvfs/hvfs.rs）是架构惯例
#![allow(clippy::module_inception)]
// 12. Clippy: new_without_default — 内核对象通常不应有无参默认构造
#![allow(clippy::new_without_default)]
// 13. Clippy: collapsible_if — 内核路径中的 if 嵌套有时是为了可读性
#![allow(clippy::collapsible_if)]
// 14. Clippy: single_match — match 单分支有时比 if-let 更清晰地表明穷尽性
#![allow(clippy::single_match)]
// 15. Clippy: too_many_arguments — 内核 API 参数数量由协议决定
#![allow(clippy::too_many_arguments)]
// 16. Clippy: type_complexity — 内核类型天然复杂（Box<dyn Fn> 等）
#![allow(clippy::type_complexity)]
// 17. Clippy: transmute_ptr_to_ptr — 内核 FFI 中的指针转换是显式约定
#![allow(clippy::transmute_ptr_to_ptr)]
// 18. Clippy: missing_transmute_annotations
#![allow(clippy::missing_transmute_annotations)]
// 19. Clippy: let_and_return — 内核错误路径中中间变量有助于可读性
#![allow(clippy::let_and_return)]
// 20. Clippy: wrong_self_convention — 内核 to_*/as_* 的 self 约定与 std 不同
#![allow(clippy::wrong_self_convention)]
// 21. Clippy: needless_range_loop — 内核中部分循环显式索引是有意可读性选择
#![allow(clippy::needless_range_loop)]
// 22. Clippy: manual_find — 显式循环比 .find() 在某些场景更清晰
#![allow(clippy::manual_find)]
// 23. Clippy: unnecessary_cast
#![allow(clippy::unnecessary_cast)]
// 24. Clippy: double_parens
#![allow(clippy::double_parens)]
// 25. Clippy: unnecessary_lazy_evaluations
#![allow(clippy::unnecessary_lazy_evaluations)]
// 26. Clippy: manual_div_ceil
#![allow(clippy::manual_div_ceil)]
// 27. Clippy: match_like_matches_macro
#![allow(clippy::match_like_matches_macro)]
// 28. Clippy: manual_unwrap_or_default / manual_unwrap_or — 显式 match 更清晰
#![allow(clippy::manual_unwrap_or_default)]
#![allow(clippy::manual_unwrap_or)]
// 29. Clippy: unnecessary_map_or — 显式 map_or 可读性更好
#![allow(clippy::unnecessary_map_or)]
// 30. Clippy: derivable_impls — 部分内核 impl 有文档注释需要保留
#![allow(clippy::derivable_impls)]
// 31. Clippy: manual_checked_ops — 内核显式检查除法更直观
#![allow(clippy::manual_checked_ops)]
// 32. Clippy: question_mark — 内核错误路径保留显式 match 更清晰
#![allow(clippy::question_mark)]
// 33. Clippy: manual_range_patterns — 显式范围 vs range pattern 可读性各有优劣
#![allow(clippy::manual_range_patterns)]
// 34. Clippy: manual_flatten — 显式 if-let 比 .flatten() 更清晰
#![allow(clippy::manual_flatten)]
// 35. Clippy: collapsible_match — 保留嵌套 match 结构
#![allow(clippy::collapsible_match)]
// 36. Clippy: let_unit_value — 含副作用的 let 绑定是合理的
#![allow(clippy::let_unit_value)]
// 37. Clippy: empty_loop — 内核自旋等待
#![allow(clippy::empty_loop)]
// 38. Clippy: explicit_counter_loop — 显式计数器在测试代码中更直观
#![allow(clippy::explicit_counter_loop)]
// 39. Clippy: pointers_in_nomem_asm_block — 内核 ASM 代码必须传指针
#![allow(clippy::pointers_in_nomem_asm_block)]
// 40. Clippy: empty_line_after_outer_attr — 内核 attr 风格
#![allow(clippy::empty_line_after_outer_attr)]
// 41. Clippy: doc_lazy_continuation / doc_overindented_list_items — 内核文档风格
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::doc_overindented_list_items)]

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
/// ├── pwm/       # 安全框架
/// ├── dma/        # DMA 引擎
/// ├── barrier/    # 故障恢复
/// ├── pci/        # PCI 管理
/// ├── syscall/    # 系统调用
/// └── driver/     # 设备驱动
/// ```
#[path = "../../kernel/mod.rs"]
pub mod kernel;

// 重新导出常用类型 (方便直接使用 crate::xxx 而非 crate::kernel::xxx)
pub use kernel::framework::cpu::CpuInfo;
pub use kernel::framework::klog::LogLevel;

use core::panic::PanicInfo;
use core::sync::atomic::Ordering;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::kernel::framework::barrier::PANIC_FLAG.store(true, Ordering::SeqCst);

    let msg = alloc::format!("{}", info);
    let bytes = msg.as_bytes();
    let len = bytes.len().min(127);
    {
        let mut panic_msg = crate::kernel::framework::barrier::PANIC_MSG.lock();
        panic_msg[..len].copy_from_slice(&bytes[..len]);
        panic_msg[len] = 0;
    }

    // 先捕获寄存器状态 — 在所有架构都需要
    #[allow(unused_mut)]
    let mut regs: [u64; 16] = [0; 16];
    #[cfg(target_arch = "x86_64")]
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
    let reg_names: [[u8; 4]; 16] = [
        *b"RAX ", *b"RBX ", *b"RCX ", *b"RDX ", *b"RSI ", *b"RDI ", *b"RBP ", *b"RSP ", *b"R8  ",
        *b"R9  ", *b"R10 ", *b"R11 ", *b"R12 ", *b"R13 ", *b"R14 ", *b"R15 ",
    ];
    #[allow(unused_mut, unused_assignments)]
    let mut cr2: u64 = 0;
    #[allow(unused_mut, unused_assignments)]
    let mut cr3_val: u64 = 0;
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
        core::arch::asm!("mov {}, cr3", out(reg) cr3_val);
    }

    // 1. 串口输出崩溃信息
    if crate::kernel::framework::klog::KLOG_INIT.load(Ordering::Acquire) {
        crate::kernel::framework::klog::serial_write_bytes(b"\n========== KERNEL PANIC ==========\n");
        crate::kernel::framework::klog::serial_write_bytes(msg.as_bytes());
        crate::kernel::framework::klog::serial_write_bytes(b"\n--- Register Dump ---\n");
        for i in 0..16 {
            crate::kernel::framework::klog::serial_write_bytes(b"  ");
            crate::kernel::framework::klog::serial_write_bytes(&reg_names[i]);
            crate::kernel::framework::klog::serial_write_bytes(b"= 0x");
            let mut hex_buf = [0u8; 16];
            let v = regs[i];
            for (d, item) in hex_buf.iter_mut().enumerate() {
                let nibble = ((v >> (60 - d * 4)) & 0xF) as u8;
                *item = if nibble < 10 {
                    b'0' + nibble
                } else {
                    b'a' + nibble - 10
                };
            }
            crate::kernel::framework::klog::serial_write_bytes(&hex_buf);
            if i % 4 == 3 {
                crate::kernel::framework::klog::serial_write_bytes(b"\n");
            }
        }
        crate::kernel::framework::klog::serial_write_bytes(b"  CR2= 0x");
        for d in 0..16 {
            let nibble = ((cr2 >> (60 - d * 4)) & 0xF) as u8;
            crate::kernel::framework::klog::serial_write_bytes(&[if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            }]);
        }
        crate::kernel::framework::klog::serial_write_bytes(b"  CR3= 0x");
        for d in 0..16 {
            let nibble = ((cr3_val >> (60 - d * 4)) & 0xF) as u8;
            crate::kernel::framework::klog::serial_write_bytes(&[if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            }]);
        }
        crate::kernel::framework::klog::serial_write_bytes(b"\n===================================\n");
    }

    // 2. 图形控制台输出崩溃信息
    crate::kernel::framework::console::gfx_console_panic_reclaim(&msg);
    crate::kernel::framework::console::gfx_console_panic_write("\n--- Register Dump ---\n");
    for i in 0..16 {
        let mut buf = [0u8; 64];
        let mut cursor: usize = 0;
        write_hex_to_buf(&mut buf, &mut cursor, regs[i]);
        let label = alloc::format!(
            "  {} = {}\n",
            core::str::from_utf8(&reg_names[i]).unwrap_or("?? "),
            core::str::from_utf8(&buf[..cursor]).unwrap_or("?")
        );
        crate::kernel::framework::console::gfx_console_panic_write(&label);
    }
    {
        let mut cr2_str = [0u8; 32];
        let mut cur: usize = 0;
        write_hex_to_buf(&mut cr2_str, &mut cur, cr2);
        let cr2_line = alloc::format!(
            "  CR2= {}\n",
            core::str::from_utf8(&cr2_str[..cur]).unwrap_or("?")
        );
        crate::kernel::framework::console::gfx_console_panic_write(&cr2_line);
        let mut cr3_str = [0u8; 32];
        let mut c3: usize = 0;
        write_hex_to_buf(&mut cr3_str, &mut c3, cr3_val);
        let cr3_line = alloc::format!(
            "  CR3= {}\n",
            core::str::from_utf8(&cr3_str[..c3]).unwrap_or("?")
        );
        crate::kernel::framework::console::gfx_console_panic_write(&cr3_line);
    }

    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("int 0x82", options(noreturn));
    }
    #[cfg(target_arch = "aarch64")]
    {
        // AArch64 栏栈恢复: 直接调用恢复逻辑进行域回滚
        extern "C" {
            fn recovery_try_recover_from_idt() -> i32;
        }
        let result = unsafe { recovery_try_recover_from_idt() };
        if result >= 0 {
            // 域状态已回滚到一致快照, 记录恢复事件
            crate::kernel::framework::klog::serial_write_bytes(
                b"\n[RECOVERY] Barrier-stack: domain rolled back\n",
            );
            crate::kernel::framework::barrier::PANIC_FLAG.store(false, core::sync::atomic::Ordering::SeqCst);
        } else {
            crate::kernel::framework::klog::serial_write_bytes(
                b"\n[RECOVERY] Barrier-stack: recovery failed, halting\n",
            );
        }
        loop {
            unsafe {
                core::arch::asm!("wfi");
            }
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

fn write_hex_to_buf(buf: &mut [u8], cursor: &mut usize, value: u64) {
    for d in 0..16 {
        if *cursor >= buf.len() {
            break;
        }
        let nibble = ((value >> (60 - d * 4)) & 0xF) as u8;
        buf[*cursor] = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        *cursor += 1;
    }
}

#[alloc_error_handler]
fn alloc_error(layout: alloc::alloc::Layout) -> ! {
    panic!("Allocation error: {:?}", layout);
}

#[no_mangle]
pub extern "C" fn kernel_init() {
    // 0. KLog — 自举串口驱动, 必须先于所有子系统
    unsafe {
        crate::kernel::framework::klog::klog_init();
    }
    crate::klog_boot_info!("QueenX starting");

    // 0.1. Config validation — 验证系统配置一致性
    // Must be called after klog_init for error reporting
    crate::kernel::framework::config::init();
    crate::klog_boot_info!("Configuration validated");

    // Test mode: skip normal init, run unit tests
    #[cfg(feature = "kernel_test")]
    {
        // Validate configuration even in test mode
        crate::kernel::framework::config::init();
        
        <crate::kernel::framework::arch::CurrentArch as crate::kernel::framework::arch::InterruptArch>::interrupt_disable(
        );

        let boot_info = crate::kernel::framework::boot::init();
        crate::kernel::framework::mm::pmm::pmm_init(boot_info.mem_size, boot_info.kernel_end);
        crate::kernel::framework::mm::vmm::vmm_init();
        const KMALLOC_HEAP_SIZE: u64 = 16 * 1024 * 1024;
        let heap_start = crate::kernel::framework::mm::VirtAddr(
            crate::kernel::framework::mm::KERNEL_BASE + boot_info.kernel_end + 0x200000,
        );
        unsafe {
            crate::kernel::framework::mm::kmalloc::get_kmalloc_mut().init(heap_start, KMALLOC_HEAP_SIZE);
        }
        crate::kernel::framework::mm::pmm::pmm_init_bitmap(KMALLOC_HEAP_SIZE);
        <crate::kernel::framework::arch::CurrentArch as crate::kernel::framework::arch::Arch>::interrupt_early_init();
        crate::klog_boot_info!("Test mode: interrupt early init done");

        crate::kernel::framework::smp::init();
        crate::klog_boot_info!("Test mode: SMP BSP registered");
        crate::kernel::framework::proc::scheduler::init();
        crate::klog_boot_info!("Test mode: Scheduler ready");

        #[cfg(feature = "fault_injection")]
        {
            let rate = option_env!("FAULT_RATE")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(50);
            crate::kernel::framework::barrier::fault_inject::FAULT_INJECTION_RATE
                .store(rate, core::sync::atomic::Ordering::Relaxed);
            crate::klog_boot_info!("[CHAOS] Fault injection enabled, rate={}/1000", rate);
        }

        crate::kernel::framework::tests::test_runner_init();
        crate::klog_boot_info!("Tests complete");

        let r = crate::kernel::framework::tests::runner();
        let failed = r.failed.load(Ordering::SeqCst);
        crate::kernel::framework::tests::qemu_exit(failed == 0);
    }

    // 1. Boot Info — 获取内存布局
    #[cfg(not(feature = "kernel_test"))]
    {
        let boot_info = crate::kernel::framework::boot::init();
        crate::klog_boot_info!(
            "Boot info: mem={} MB, kernel_end=0x{:X}",
            boot_info.mem_size / (1024 * 1024),
            boot_info.kernel_end
        );

        // 2. PMM — 物理内存管理器初始化
        crate::kernel::framework::mm::pmm::pmm_init(boot_info.mem_size, boot_info.kernel_end);
        crate::klog_boot_info!("PMM initialized");

        // 3. VMM — 虚拟内存管理器初始化 (必须在PMM之后)
        crate::kernel::framework::mm::vmm::vmm_init();
        crate::klog_boot_info!("VMM initialized");

        // 4. kmalloc — 内核堆初始化
        const KMALLOC_HEAP_SIZE: u64 = 16 * 1024 * 1024; // 16 MB
        #[cfg(target_arch = "x86_64")]
        let heap_start = crate::kernel::framework::mm::VirtAddr(
            crate::kernel::framework::mm::KERNEL_BASE + boot_info.kernel_end + 0x200000,
        );
        #[cfg(target_arch = "aarch64")]
        let heap_start = crate::kernel::framework::mm::VirtAddr(boot_info.kernel_end + 0x200000);
        unsafe {
            crate::kernel::framework::mm::kmalloc::get_kmalloc_mut().init(heap_start, KMALLOC_HEAP_SIZE);
        }
        crate::klog_boot_info!(
            "kmalloc initialized at 0x{:X}, size={} MB",
            heap_start.0,
            KMALLOC_HEAP_SIZE / (1024 * 1024)
        );

        // 5. PMM Bitmap — 初始化位图分配器
        // Must include the 2MB gap between kernel_end and heap_start,
        // otherwise PMM bitmap will allocate from within the kmalloc heap
        // (pages 7165+), causing heap corruption when alloc_table() zeros
        // newly allocated page table pages.
        // GAP_SIZE + KMALLOC_HEAP_SIZE = 0x200000 + 16MB = 18MB total reserved after kernel.
        const GAP_SIZE: u64 = 0x200000;
        let reserved_after_kernel = GAP_SIZE + KMALLOC_HEAP_SIZE;
        crate::kernel::framework::mm::pmm::pmm_init_bitmap(reserved_after_kernel);
        crate::klog_boot_info!("PMM bitmap initialized");

        // --- Barrier-stack recovery domains (moved before interrupts to avoid race) ---
        // Register PMM + PROC domains before interrupts are enabled so timer IRQ
        // won't race with domain registration on the RECOVERY_MANAGER spinlock
        crate::kernel::framework::mm::pmm::pmm_register_barrier_domain();
        crate::kernel::framework::proc::process::proc_register_barrier_domain();
        #[cfg(target_arch = "aarch64")]
        unsafe {
            crate::kernel::framework::arch::aarch64::barrier::enable_barrier_sgi();
        }
        crate::klog_boot_info!("Barrier-stack recovery domains registered (PMM=3, PROC=4)");

        // 5.5. Swap — 物理内存回收/换出 (B3 完整实现)
        // 必须在 PMM + VMM + kmalloc 初始化之后 (使用 pmm/vmm 接口)
        // 必须在 interrupt_late_init 之前 (softirq 注册依赖 IRQ 子系统)
        // 实际上 softirq 是 static handler 表, 不强制 init 顺序, 但保持 init 流程清晰:
        //   swap_init (PMM 之后) → kswapd_init (interrupt_late_init 之后, scheduler tick 之前)
        if crate::kernel::services::mm::swap::swap_init() {
            crate::klog_boot_info!("Swap subsystem initialized");
        } else {
            crate::klog_boot_info!("Swap subsystem init FAILED (degraded mode)");
        }

        // 6. 中断/异常设置
        <crate::kernel::framework::arch::CurrentArch as crate::kernel::framework::arch::Arch>::interrupt_late_init();
        crate::klog_boot_info!("Interrupt subsystem ready");

        // 6.5. kswapd softirq 注册 (依赖 IRQ 子系统, scheduler tick 触发 wakeup)
        crate::kernel::services::mm::swap::kswapd_init();

        // 7. Timer 初始化 (中断延后到网络就绪后启用)
        match crate::kernel::framework::timer::timer_init(1000) {
            Ok(_freq) => {
                crate::klog_boot_info!("Timer configured");
                #[cfg(target_arch = "x86_64")]
                let _ = crate::kernel::framework::timer::irq::register_timer_irq();
            }
            Err(_msg) => {
                let _ = _msg;
            }
        }

        // 8. Scheduler
        crate::kernel::framework::proc::scheduler::init();
        crate::kernel::framework::proc::scheduler_ex::init();
        crate::klog_boot_info!("Scheduler ready");

        // 9. VFS
        crate::kernel::framework::fs::vfs::init();
        crate::klog_boot_info!("VFS ready");

        // 9-1. UDS (AF_UNIX) — Phase C.3
        crate::kernel::framework::net::unix::uds_init();
        crate::klog_boot_info!("UDS subsystem initialized");

        // 10. Network (smoltcp + 网卡驱动)
        // x86_64: E1000 PCI 网卡驱动
        // aarch64: virtio-net MMIO 网卡驱动
        {
            crate::kernel::framework::net::init::qx_net_init();
            crate::klog_boot_info!("Network subsystem initialized");
        }

        // 10-10.6. Driver subsystem init (VGA, serial, keyboard, PCI, storage, display, USB)
        crate::kernel::framework::driver::init_all();
        crate::klog_boot_info!("Driver subsystem initialized");
        {
            let chitin_count = crate::kernel::framework::chitin::chitin_count() as u64;
            let block_count = crate::kernel::framework::chitin::chitin_count_by_proto(
                crate::kernel::framework::chitin::ChitinProto::Block,
            ) as u64;
            let net_count = crate::kernel::framework::chitin::chitin_count_by_proto(
                crate::kernel::framework::chitin::ChitinProto::Net,
            ) as u64;
            let input_count = crate::kernel::framework::chitin::chitin_count_by_proto(
                crate::kernel::framework::chitin::ChitinProto::Input,
            ) as u64;
            crate::klog_boot_info!(
                "Chitin: {} device(s) [blk={} net={} input={}]",
                chitin_count,
                block_count,
                net_count,
                input_count
            );
        }

        // HvFS + 磁盘挂载 — BlockDevice 注册表自动发现多块磁盘 (支持 ATA/NVMe/virtio-blk)
        #[cfg(all(not(feature = "kernel_test"), target_arch = "x86_64"))]
        {
            let hvfs = crate::kernel::framework::fs::hvfs::hvfs::get_hvfs();
            // init() 会自动扫描所有块设备, 发现 ANTX 签名的磁盘并挂载
            hvfs.init();

            if hvfs.is_disk_mode() {
                crate::kernel::framework::fs::hvfs::hvfs::get_hvfs()
                    .spa
                    .disk_present
                    .store(true, core::sync::atomic::Ordering::Release);
                let r = crate::kernel::framework::fs::vfs::api::vfs_mount_internal(
                    b"/".as_ptr(),
                    b"hvfs".as_ptr(),
                );
                if r == 0 {
                    let n_drives = crate::kernel::framework::fs::hvfs::hvfs::get_hvfs()
                        .drives_discovered
                        .lock()
                        .len() as u64;
                    if n_drives > 1 {
                        crate::klog_boot_info!("Root filesystem: HvFS ({} drives)", n_drives);
                    } else {
                        crate::klog_boot_info!("Root filesystem: HvFS (disk)");
                    }
                } else {
                    crate::klog_boot_info!("HvFS mount failed");
                }
            } else {
                crate::klog_boot_info!("HvFS: running in memory mode (no disk)");
            }
        }

        // 启动定时器 (延迟到所有子系统初始化完成后)
        #[cfg(target_arch = "aarch64")]
        {
            let interval = crate::kernel::framework::arch::aarch64::exception::TIMER_INTERVAL_TICKS
                .load(core::sync::atomic::Ordering::Relaxed);
            crate::kernel::framework::arch::aarch64::timer::start_interval(interval);
        }

        crate::klog_boot_info!("QueenX initialized, entering user mode...");

        // 12. Launch first user process
        unsafe {
            crate::kernel::framework::proc::api::launch_first_user_process();
        }
        // unreachable: launch_first_user_process is noreturn
    } // end #[cfg(not(feature = "kernel_test"))]
}
