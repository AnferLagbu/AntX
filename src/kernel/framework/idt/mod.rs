//! # Interrupt Descriptor Table (IDT) - Rust 安全重写
//!
//! AntX 操作系统的中断描述符表管理模块。
//!
//! ## 架构概览
//!
//! ```text
//! Hardware Interrupt/Exception
//!   ↓
//! [isr.asm] (汇编 stub)
//!   ↓
//! [exception_handler() / irq_handler()]  ← FFI 入口
//!   ↓
//! [IdtManager] (Rust 核心逻辑)
//!   ├── ExceptionDispatcher
//!   │   ├── DivisionByZeroHandler
//!   │   ├── PageFaultHandler
//!   │   ├── GeneralProtectionFaultHandler
//!   │   └── DoubleFaultHandler
//!   └── IrqManager
//!       ├── register_irq()
//!       ├── unregister_irq()
//!       └── dispatch_irq()
//! ```
//!
//! ## 安全性增强 (相比 C 版本)
//!
//! - ✅ **内存安全**: Ownership + Borrow Checker 消除缓冲区溢出
//! - ✅ **并发安全**: AtomicU64 / Mutex 保护全局状态
//! - ✅ **类型安全**: Trait 系统替代 void* 函数指针
//! - ✅ **空指针安全**: Option<T> 编译期排除 null deref
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! // 注册自定义 IRQ handler
//! idt::register_irq(1, my_keyboard_handler, "keyboard", 0);
//!
//! // 在异常处理中使用
//! fn handle_page_fault(frame: &mut InterruptFrame) -> RecoveryAction {
//!     if frame.is_user_mode() {
//!         RecoveryAction::TerminateProcess(1)
//!     } else {
//!         RecoveryAction::DomainRecovery
//!     }
//! }
//! ```

// 子模块声明
pub mod handlers; // Phase 3: 异常处理器实现
pub mod idt; // Phase 2: 核心管理器
/// T-04: 中断处理决策 trait
pub mod irq_trait;
pub mod safety;
pub mod statistics;
pub mod types; // Phase 3: 统计与 JSON 导出

// 重新导出核心类型 (方便外部使用)
pub use types::{
    // 辅助函数
    get_exception_name,
    get_irq_name,
    ErrorFlags,
    IdtEntry,
    IdtPtr,
    InterruptFrame,
    InterruptStatistics,
    IrqDescriptor,
    GDT_KERNEL_CODE,
    IDT_DPL_USER,
    // 常量
    IDT_ENTRIES,
    IDT_TYPE_INTERRUPT,
    IDT_TYPE_TRAP,
    IRQ_BASE,
    MODULE_INIT_FAILURE,
    MODULE_INIT_SUCCESS,
};

pub use safety::{
    disable_interrupts, enable_interrupts, halt_loop, is_null_or_invalid,
    is_valid_kernel_address, is_valid_user_address, rdtsc, read_cr2, CpuFeatures,
};

pub use idt::IdtManager;

// Phase 3: 异常处理器导出
pub use handlers::{
    create_handler, get_collector, DefaultHandler, DivisionByZeroHandler, DoubleFaultHandler,
    ExceptionCategory, ExceptionHandler, GeneralProtectionFaultHandler, PageFaultHandler,
    PanicInfo, RecoveryAction, Severity,
};

// Phase 3: 统计模块导出
pub use statistics::{get_detailed_statistics, DetailedStatistics, InterruptEvent};

// irq_trait 公共接口 re-export — T-04 策略-机制分离
pub use irq_trait::{IrqDecision, FallbackIrqDecision, IrqContext, SoftirqContext, register_irq_decision, current_irq_decision};

/// 全局 IDT 管理器实例 (Phase 2 已实现)
pub static IDT_MANAGER: () = ();

// ============================================================================
// FFI 接口层 (C ↔ Rust 桥接) - Phase 2 完整实现
// ============================================================================

/// x86_64 中断 wrapper 函数指针类型
/// 入口 stub (asm) → wrapper (本函数) → 业务 handler (Rust 普通调用)
/// 使用 `extern "C"` (x86_64 Linux 上等同 `sysv64`),因为 wrapper 内部需
/// 正常调用业务 handler,不能用 `x86-interrupt` (后者禁止普通函数调用)
pub type CExceptionHandler = extern "C" fn(*mut InterruptFrame);

/// x86_64 IRQ wrapper 函数指针类型
pub type CIrqHandler = extern "C" fn(*mut InterruptFrame);

/// 初始化 IDT 子系统 (FFI 导出函数)
///
/// # Safety
/// 此函数必须在内核启动早期调用，且只能调用一次
///
/// # Returns
/// - `MODULE_INIT_SUCCESS` (0): 成功
/// - `MODULE_INIT_FAILURE` (-1): 失败
#[no_mangle]
#[cfg(target_arch = "x86_64")]
pub extern "C" fn idt_init() -> i32 {
    let manager = IdtManager::instance();

    // 获取 ISR 地址表 (从 isr.asm 导出的符号, 使用 fn 指针)
    extern "C" {
        fn isr0();
        fn isr1();
        fn isr2();
        fn isr3();
        fn isr4();
        fn isr5();
        fn isr6();
        fn isr7();
        fn isr8();
        fn isr9();
        fn isr10();
        fn isr11();
        fn isr12();
        fn isr13();
        fn isr14();
        fn isr15();
        fn isr16();
        fn isr17();
        fn isr18();
        fn isr19();
        fn isr20();
        fn isr21();
        fn isr22();
        fn isr23();
        fn isr24();
        fn isr25();
        fn isr26();
        fn isr27();
        fn isr28();
        fn isr29();
        fn isr30();
        fn isr31();
        fn irq0();
        fn irq1();
        fn irq2();
        fn irq3();
        fn irq4();
        fn irq5();
        fn irq6();
        fn irq7();
        fn irq8();
        fn irq9();
        fn irq10();
        fn irq11();
        fn irq12();
        fn irq13();
        fn irq14();
        fn irq15();
        fn syscall_handler();
        fn isr0x82();
    }

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        macro_rules! addr {
            ($f:ident) => {
                ($f as *const ()) as usize as u64
            };
        }
        let isr_table: [u64; 32] = [
            addr!(isr0),
            addr!(isr1),
            addr!(isr2),
            addr!(isr3),
            addr!(isr4),
            addr!(isr5),
            addr!(isr6),
            addr!(isr7),
            addr!(isr8),
            addr!(isr9),
            addr!(isr10),
            addr!(isr11),
            addr!(isr12),
            addr!(isr13),
            addr!(isr14),
            addr!(isr15),
            addr!(isr16),
            addr!(isr17),
            addr!(isr18),
            addr!(isr19),
            addr!(isr20),
            addr!(isr21),
            addr!(isr22),
            addr!(isr23),
            addr!(isr24),
            addr!(isr25),
            addr!(isr26),
            addr!(isr27),
            addr!(isr28),
            addr!(isr29),
            addr!(isr30),
            addr!(isr31),
        ];

        let irq_table: [u64; 16] = [
            addr!(irq0),
            addr!(irq1),
            addr!(irq2),
            addr!(irq3),
            addr!(irq4),
            addr!(irq5),
            addr!(irq6),
            addr!(irq7),
            addr!(irq8),
            addr!(irq9),
            addr!(irq10),
            addr!(irq11),
            addr!(irq12),
            addr!(irq13),
            addr!(irq14),
            addr!(irq15),
        ];

        match manager.init(
            &isr_table,
            &irq_table,
            addr!(syscall_handler),
            addr!(isr0x82),
        ) {
            Ok(()) => MODULE_INIT_SUCCESS,
            Err(msg) => {
                // TODO(TRACK-2CED20): 使用 klog 记录错误 (Phase 3)
                let _ = msg;
                MODULE_INIT_FAILURE
            }
        }
    }
}

/// 异常处理主入口 (从 isr.asm 调用)
///
/// # Arguments
/// * `frame` - 中断帧指针 (由 isr.asm 构建)
///
/// # Safety
/// 此函数在中断上下文中调用，必须快速执行
#[no_mangle]
pub unsafe extern "C" fn exception_handler(frame: *mut InterruptFrame) {
    let manager = IdtManager::instance();
    manager.handle_exception(frame);
}

/// IRQ 处理主入口 (从 isr.asm 调用)
///
/// # Arguments
/// * `frame` - 中断帧指针
///
/// # Safety
/// 此函数在中断上下文中调用，需要发送 EOI
#[no_mangle]
#[cfg(target_arch = "x86_64")]
pub unsafe extern "C" fn irq_handler(frame: *mut InterruptFrame) {
    if frame.is_null() {
        return;
    }

    let manager = IdtManager::instance();
    let frame_ref = &*frame;
    let vector = frame_ref.int_no as u8;

    manager.handle_irq(frame, vector);
}

/// 设置 IDT 门描述符 (FFI 兼容接口)
///
/// # Arguments
/// * `num` - 向量号 (0-255)
/// * `handler` - handler 地址
/// * `selector` - 代码段选择子
/// * `type_attr` - 类型属性标志
#[no_mangle]
pub extern "C" fn idt_set_gate(num: u8, handler: u64, selector: u16, type_attr: u8) {
    let manager = IdtManager::instance();

    // 直接修改 entries 数组 (需要 Mutex 保护)
    let mut state = manager.state.lock();
    if num < IDT_ENTRIES as u8 {
        state.entries[num as usize] = IdtEntry::new(handler, selector, type_attr);
    }
}

/// 注册 IRQ handler (FFI 兼容接口)
///
/// # Arguments
/// * `irq` - IRQ 号 (0-15)
/// * `handler` - C 函数指针
/// * `name` - handler 名称 (用于日志)
/// * `flags` - 标志位
///
/// # Returns
/// - `0`: 成功
/// - `-1`: 参数无效
#[no_mangle]
pub extern "C" fn idt_register_irq(
    irq: u8,
    handler: CIrqHandler,
    name: *const u8,
    flags: u32,
) -> i32 {
    let manager = IdtManager::instance();

    // 将 C 字符串转换为 Rust &str
    let name_str = if name.is_null() {
        ""
    } else {
        // 简单处理：假设 name 指向静态字符串
        // SAFETY: `const` 由调用方保证为有效指针; 只读访问
        unsafe { core::ffi::CStr::from_ptr(name as *const core::ffi::c_char).to_str().unwrap_or("") }
    };

    match manager.register_irq(irq, handler, name_str, flags) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 注销 IRQ handler (FFI 兼容接口)
#[no_mangle]
pub extern "C" fn idt_unregister_irq(irq: u8, handler: CIrqHandler) -> i32 {
    let manager = IdtManager::instance();

    match manager.unregister_irq(irq, handler) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 启用指定 IRQ
#[no_mangle]
pub extern "C" fn idt_enable_irq(irq: u8) {
    let manager = IdtManager::instance();
    manager.enable_irq(irq);
}

/// 禁用指定 IRQ
#[no_mangle]
pub extern "C" fn idt_disable_irq(irq: u8) {
    let manager = IdtManager::instance();
    manager.disable_irq(irq);
}

/// 导出 IDT 状态 (用于调试)
#[no_mangle]
pub extern "C" fn idt_dump_state() {
    let manager = IdtManager::instance();
    manager.dump_state();
}

/// 获取中断计数统计
#[no_mangle]
pub extern "C" fn idt_get_interrupt_count(vector: u8) -> u64 {
    let manager = IdtManager::instance();
    manager.get_interrupt_count(vector)
}

/// 打印详细的中断统计信息
#[no_mangle]
pub extern "C" fn idt_print_interrupt_stats() {
    let manager = IdtManager::instance();
    manager.print_statistics();
}

// ============================================================================
// 测试模块
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_ffi_interface_compiles() {
        // 验证所有 FFI 函数可以正常编译链接
        assert_eq!(idt_init(), MODULE_INIT_SUCCESS);

        // 测试 dump 函数不会 panic
        idt_dump_state();
        idt_print_interrupt_stats();
    }

    #[test]
    fn test_exception_names_coverage() {
        // 确保所有标准异常都有名称
        for i in 0..32u8 {
            let name = get_exception_name(i);
            assert!(!name.is_empty(), "Exception {} should have a name", i);
            assert_ne!(name, "Unknown", "Exception {} should be known", i);
        }
    }

    #[test]
    fn test_constants_sanity() {
        assert_eq!(IDT_ENTRIES, 256);
        assert_eq!(IRQ_BASE, 32);
        assert!(MODULE_INIT_SUCCESS != MODULE_INIT_FAILURE);
    }
}
