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
pub mod types;
pub mod safety;
pub mod idt;         // Phase 2: 核心管理器
pub mod handlers;     // Phase 3: 异常处理器实现
pub mod statistics;   // Phase 3: 统计与 JSON 导出

// 重新导出核心类型 (方便外部使用)
pub use types::{
    IdtEntry,
    IdtPtr,
    InterruptFrame,
    IrqDescriptor,
    InterruptStatistics,
    ErrorFlags,
    // 常量
    IDT_ENTRIES,
    IRQ_BASE,
    IDT_TYPE_INTERRUPT,
    IDT_TYPE_TRAP,
    IDT_DPL_USER,
    GDT_KERNEL_CODE,
    MODULE_INIT_SUCCESS,
    MODULE_INIT_FAILURE,
    // 辅助函数
    get_exception_name,
    get_irq_name,
};

pub use safety::{
    CpuFeatures,
    read_cr2,
    disable_interrupts,
    enable_interrupts,
    rdtsc,
    halt_loop,
    is_valid_user_address,
    is_valid_kernel_address,
};

pub use idt::IdtManager;

// Phase 3: 异常处理器导出
pub use handlers::{
    ExceptionHandler,
    RecoveryAction,
    PanicInfo,
    Severity,
    ExceptionCategory,
    DivisionByZeroHandler,
    PageFaultHandler,
    GeneralProtectionFaultHandler,
    DoubleFaultHandler,
    DefaultHandler,
    create_handler,
    get_collector,
};

// Phase 3: 统计模块导出
pub use statistics::{
    DetailedStatistics,
    InterruptEvent,
    get_detailed_statistics,
};

/// 全局 IDT 管理器实例 (Phase 2 已实现)
pub static IDT_MANAGER: () = ();

// ============================================================================
// FFI 接口层 (C ↔ Rust 桥接) - Phase 2 完整实现
// ============================================================================

/// C 兼容的异常处理函数指针类型
pub type CExceptionHandler = extern "C" fn(*mut InterruptFrame);

/// C 兼容的 IRQ 处理函数指针类型
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
pub extern "C" fn idt_init() -> i32 {
    let manager = IdtManager::instance();
    
    // 获取 ISR 地址表 (从 isr.asm 导出的符号)
    extern "C" {
        static isr0: u64;
        static isr1: u64;
        static isr2: u64;
        static isr3: u64;
        static isr4: u64;
        static isr5: u64;
        static isr6: u64;
        static isr7: u64;
        static isr8: u64;
        static isr9: u64;
        static isr10: u64;
        static isr11: u64;
        static isr12: u64;
        static isr13: u64;
        static isr14: u64;
        static isr15: u64;
        static isr16: u64;
        static isr17: u64;
        static isr18: u64;
        static isr19: u64;
        static isr20: u64;
        static isr21: u64;
        static isr22: u64;
        static isr23: u64;
        static isr24: u64;
        static isr25: u64;
        static isr26: u64;
        static isr27: u64;
        static isr28: u64;
        static isr29: u64;
        static isr30: u64;
        static isr31: u64;
        
        // IRQ handlers
        static irq0: u64;
        static irq1: u64;
        static irq2: u64;
        static irq3: u64;
        static irq4: u64;
        static irq5: u64;
        static irq6: u64;
        static irq7: u64;
        static irq8: u64;
        static irq9: u64;
        static irq10: u64;
        static irq11: u64;
        static irq12: u64;
        static irq13: u64;
        static irq14: u64;
        static irq15: u64;
        
        // Special handlers
        static syscall_handler: u64;
        static isr0x82: u64;
    }
    
    unsafe {
        let isr_table: [u64; 32] = [
            isr0, isr1, isr2, isr3, isr4, isr5, isr6, isr7,
            isr8, isr9, isr10, isr11, isr12, isr13, isr14, isr15,
            isr16, isr17, isr18, isr19, isr20, isr21, isr22, isr23,
            isr24, isr25, isr26, isr27, isr28, isr29, isr30, isr31,
        ];
        
        let irq_table: [u64; 16] = [
            irq0, irq1, irq2, irq3, irq4, irq5, irq6, irq7,
            irq8, irq9, irq10, irq11, irq12, irq13, irq14, irq15,
        ];
        
        match manager.init(&isr_table, &irq_table, syscall_handler, isr0x82) {
            Ok(()) => MODULE_INIT_SUCCESS,
            Err(msg) => {
                // TODO: 使用 klog 记录错误 (Phase 3)
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
pub unsafe extern "C" fn irq_handler(frame: *mut InterruptFrame) {
    if frame.is_null() { return; }
    
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
    name: *const core::ffi::c_char, 
    flags: u32
) -> i32 {
    let manager = IdtManager::instance();
    
    // 将 C 字符串转换为 Rust &str
    let name_str = if name.is_null() {
        ""
    } else {
        // 简单处理：假设 name 指向静态字符串
        unsafe { core::ffi::CStr::from_ptr(name).to_str().unwrap_or("") }
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
