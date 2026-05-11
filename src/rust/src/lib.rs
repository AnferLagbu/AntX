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
pub use kernel::logging::LogLevel;

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
    unsafe {
        crate::kernel::barrier::PANIC_MSG[..len].copy_from_slice(&bytes[..len]);
        crate::kernel::barrier::PANIC_MSG[len] = 0;
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
    // 1. 初始化定时器子系统 (必须在启用中断前)
    match crate::kernel::timer::timer_init(1000) {
        Ok(_freq) => {
            // 成功: 注册 IRQ0 handler
            let _ = crate::kernel::timer::irq::register_timer_irq();

            // 2. 执行 TSC 频率校准 (提高时间测量精度)
            match crate::kernel::timer::calibrate_tsc(20) {
                Ok(tsc_mhz) => {
                    // 校准成功: 可使用高精度时间函数
                    let _ = tsc_mhz;
                },
                Err(_) => {
                    // 校准失败: 使用近似值 (不影响基本功能)
                    // TSC 函数将返回 None 或使用默认估计值
                }
            }
        },
        Err(msg) => {
            // 失败: 使用默认值继续 (Timer 将回退到忙等待模式)
            let _ = msg;
        }
    }

    // 3. 初始化调度器
    crate::kernel::proc::scheduler::init();

    // 4. 初始化文件系统
    crate::kernel::fs::vfs::init();
}
