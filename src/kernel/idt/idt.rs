//! # IDT 核心管理器
//!
//! 全局中断描述符表管理，提供线程安全的 IDT 操作接口。
//!
//! ## 架构设计
//!
//! ```text
//! IdtManager (全局单例)
//!   ├── inner: Mutex<IdtState>
//!   │     ├── entries[256]    (IDT 门描述符)
//!   │     ├── handlers[256]   (异常/IRQ 处理函数)
//!   │     └── irq_desc[16]    (IRQ 扩展信息)
//!   ├── stats: InterruptStatistics (无锁统计)  ← Phase 1
//!   ├── detailed_stats: DetailedStatistics      ← Phase 3
//!   └── exception_handlers: Trait objects       ← Phase 3
//! ```

use core::sync::atomic::{AtomicU64, Ordering};

use super::types::*;
use super::handlers::*;
use super::statistics::*;

// 内联硬件操作函数 (避免跨模块导入问题)
/// 从端口读字节
#[inline(always)]
unsafe fn port_inb(port: u16) -> u8 {
    crate::arch!(inb(port))
}

/// 向端口写字节
#[inline(always)]
unsafe fn port_outb(port: u16, value: u8) {
    crate::arch!(outb(port, value));
}

/// I/O 等待
#[inline(always)]
unsafe fn io_wait() { port_outb(0x80, 0); }

/// 重映射 8259A PIC: IRQ0-7→vec32-39, IRQ8-15→vec40-47
unsafe fn remap_pic() {
    let m = port_inb(0x21);
    let s = port_inb(0xA1);
    port_outb(0x20, 0x11); io_wait();
    port_outb(0xA0, 0x11); io_wait();
    port_outb(0x21, 0x20); io_wait();
    port_outb(0xA1, 0x28); io_wait();
    port_outb(0x21, 0x04); io_wait();
    port_outb(0xA1, 0x02); io_wait();
    port_outb(0x21, 0x01); io_wait();
    port_outb(0xA1, 0x01); io_wait();
    port_outb(0x21, 0xFF);
    port_outb(0xA1, 0xFF);
    let _ = (m, s);
}

/// 禁用中断
#[inline(always)]
unsafe fn cli() {
    let _ = crate::arch!(interrupt_disable());
}

/// Halt 循环 (永不返回)
fn halt_loop() -> ! {
    loop { crate::arch!(halt()); }
}

/// 检查指针是否为 null 或无效
fn is_null_or_invalid(ptr: u64) -> bool { ptr == 0 || ptr < 0x1000 }

/// 验证 user 地址
fn is_valid_user_address(addr: u64) -> bool { addr > 0xFFFF && addr < 0xFFFFFFFF80000000 }

/// 验证 kernel 地址
fn is_valid_kernel_address(addr: u64) -> bool { addr >= 0xFFFFFFFF80000000 }

/// IDT 状态 (受 Mutex 保护)
pub(crate) struct IdtState {
    /// IDT 条目表
    pub entries: [IdtEntry; IDT_ENTRIES],
    /// 异常处理函数指针表
    pub handlers: [Option<extern "C" fn(*mut InterruptFrame)>; IDT_ENTRIES],
    /// IRQ 描述符扩展信息
    pub irq_descriptors: [IrqDescriptor; 16],
}

impl Default for IdtState {
    fn default() -> Self {
        // 手动初始化 irq_descriptors 数组 (IrqDescriptor 不是 Copy)
        let irq_descs = [const { IrqDescriptor {
            handler: None,
            name: "",
            description: "",
            flags: 0,
            call_count: core::sync::atomic::AtomicU64::new(0),
            error_count: core::sync::atomic::AtomicU64::new(0),
        }}; 16];
        
        Self {
            entries: [IdtEntry::default(); IDT_ENTRIES],
            handlers: [None; IDT_ENTRIES],
            irq_descriptors: irq_descs,
        }
    }
}

/// 全局 IDT 管理器
pub struct IdtManager {
    /// 内部状态 (Mutex 保护)
    pub(crate) state: spin::Mutex<IdtState>,
    /// 基础统计 (Phase 1)
    pub stats: InterruptStatistics,
    /// 详细统计 (Phase 3)
    pub detailed_stats: DetailedStatistics,
    /// 嵌套中断计数
    pub nested_count: AtomicU64,
    /// 当前正在处理的中断向量
    current_vector: AtomicU64,
}

// 全局单例实例
static IDT_MANAGER_INSTANCE: spin::Once<IdtManager> = spin::Once::new();

impl IdtManager {
    /// 获取全局 IDT 管理器实例
    pub fn instance() -> &'static IdtManager {
        IDT_MANAGER_INSTANCE.call_once(|| {
            IdtManager {
                state: spin::Mutex::new(IdtState::default()),
                stats: InterruptStatistics::new(),
                detailed_stats: DetailedStatistics::new(),  // Phase 3
                nested_count: AtomicU64::new(0),
                current_vector: AtomicU64::new(0xFFFFFFFFFFFFFFFF),
            }
        })
    }

    /// 初始化 IDT 子系统
    ///
    /// # Arguments
    /// * `isr_table` - ISR handler 地址表 (32 个异常)
    /// * `irq_table` - IRQ handler 地址表 (16 个 IRQ)
    /// * `syscall_handler` - 系统调用门地址
    /// * `isr0x82` - 恢复中断 (int 0x82) 地址
    ///
    /// # Returns
    /// - `Ok(())`: 初始化成功
    /// - `Err(msg)`: 初始化失败
    pub fn init(
        &self,
        isr_table: &[u64; 32],
        irq_table: &[u64; 16],
        syscall_handler: u64,
        isr0x82: u64,
    ) -> Result<(), &'static str> {
        unsafe { remap_pic(); }

        let mut state = self.state.lock();

        // 1. 清空所有条目
        for i in 0..IDT_ENTRIES {
            state.entries[i] = IdtEntry::default();
            state.handlers[i] = None;
        }

        // 2. 设置异常门描述符 (向量 0-31)
        for i in 0..32u8 {
            self.set_gate_internal(&mut state, i, isr_table[i as usize], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT);
        }

        // 2a. 为关键异常设置 IST 专用栈
        // Double Fault (#DF, vector 8) → IST0
        state.entries[8] = IdtEntry::new_with_ist(
            isr_table[8], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT, 1  // IST1 (TSS ist[0])
        );
        // NMI (vector 2) → IST1
        state.entries[2] = IdtEntry::new_with_ist(
            isr_table[2], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT, 2  // IST2 (TSS ist[1])
        );

        // 3. 设置 IRQ 门描述符 (向量 32-47)
        for i in 0..16u8 {
            let vector = IRQ_BASE + i;
            self.set_gate_internal(&mut state, vector, irq_table[i as usize], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT);
        }

        // 4. 设置系统调用门 (int 0x80, DPL=3 允许 user 调用)
        self.set_gate_internal(&mut state, 0x80, syscall_handler, GDT_KERNEL_CODE, IDT_TYPE_TRAP | IDT_DPL_USER);

        // 5. 设置恢复中断 (int 0x82, barrier-stack) — 使用 IST2 专用栈
        state.entries[0x82] = IdtEntry::new_with_ist(
            isr0x82, GDT_KERNEL_CODE, IDT_TYPE_TRAP, 3  // IST3 (TSS ist[2])
        );

        drop(state); // 释放锁，准备加载 IDT

        // 6. 加载 IDT 到 CPU
        #[cfg(target_arch = "x86_64")]
        unsafe { self.load_idt(); }

        Ok(())
    }

    /// 内部函数: 设置门描述符 (需要 &mut state)
    fn set_gate_internal(&self, state: &mut IdtState, num: u8, handler: u64, selector: u16, type_attr: u8) {
        let entry = &mut state.entries[num as usize];
        *entry = IdtEntry::new(handler, selector, type_attr);
    }

    /// 加载 IDT 到 CPU (lidt 指令)
    ///
    /// # Safety
    /// 必须确保 IDT 表已正确初始化
    #[cfg(target_arch = "x86_64")]
    unsafe fn load_idt(&self) {
        let state = self.state.lock();
        let base_addr = state.entries.as_ptr() as u64;
        
        let idt_ptr = IdtPtr::new(base_addr);
        
        core::arch::asm!(
            "lidt [{0}]",
            in(reg) &idt_ptr,
            options(nostack, preserves_flags)
        );
    }

    /// 注册异常处理函数
    pub fn set_exception_handler(&self, vector: u8, handler: extern "C" fn(*mut InterruptFrame)) {
        if vector < IDT_ENTRIES as u8 {
            let mut state = self.state.lock();
            state.handlers[vector as usize] = Some(handler);
        }
    }

    /// 注册 IRQ 处理函数
    pub fn register_irq(
        &self,
        irq: u8,
        handler: extern "C" fn(*mut InterruptFrame),
        name: &'static str,
        flags: u32,
    ) -> Result<(), &'static str> {
        if irq >= 16 {
            return Err("Invalid IRQ number");
        }

        let mut state = self.state.lock();
        let vector = (IRQ_BASE + irq) as usize;

        // 检查是否已有 handler (非共享模式下警告)
        if (flags & IRQ_FLAG_SHARED) == 0 && state.handlers[vector].is_some() {
            // Log warning (Phase 3 实现)
        }

        // 更新 descriptor
        state.irq_descriptors[irq as usize] = IrqDescriptor {
            handler: Some(handler),
            name,
            description: "",
            flags,
            call_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
        };

        // 注册 handler
        state.handlers[vector] = Some(handler);

        Ok(())
    }

    /// 注销 IRQ 处理函数
    pub fn unregister_irq(&self, irq: u8, handler: extern "C" fn(*mut InterruptFrame)) -> Result<(), &'static str> {
        if irq >= 16 {
            return Err("Invalid IRQ number");
        }

        let mut state = self.state.lock();
        let vector = (IRQ_BASE + irq) as usize;

        // 匹配 handler 后注销
        if let Some(registered) = state.handlers[vector] {
            let registered_ptr = registered as *const ();
            let input_ptr = handler as *const ();
            
            if registered_ptr == input_ptr {
                state.handlers[vector] = None;
                state.irq_descriptors[irq as usize] = IrqDescriptor::empty();
                return Ok(());
            }
        }

        Err("Handler not found")
    }

    /// 启用指定 IRQ
    pub fn enable_irq(&self, irq: u8) {
        if irq >= 16 { return; }

        // TODO: 通过 IOAPIC 或 PIC 启用
        // 当前简化实现: 直接操作 PIC
        unsafe {
            if irq < 8 {
                let mask = port_inb(0x21) & !(1 << irq);
                port_outb(0x21, mask);
            } else {
                let slave_mask = port_inb(0xA1) & !(1 << (irq - 8));
                port_outb(0xA1, slave_mask);
                
                let master_mask = port_inb(0x21) & !(1 << 2);
                port_outb(0x21, master_mask);
            }
        }
    }

    /// 禁用指定 IRQ
    pub fn disable_irq(&self, irq: u8) {
        if irq >= 16 { return; }

        unsafe {
            if irq < 8 {
                let mask = port_inb(0x21) | (1 << irq);
                port_outb(0x21, mask);
            } else {
                let mask = port_inb(0xA1) | (1 << (irq - 8));
                port_outb(0xA1, mask);
            }
        }
    }

    /// 处理异常 (从 exception_handler FFI 调用)
    pub fn handle_exception(&self, frame: *mut InterruptFrame) {
        if frame.is_null() { return; }

        unsafe {
            let frame_ref = &*frame;
            let vector = frame_ref.int_no as u8;

            // 更新嵌套计数
            let nesting = self.nested_count.fetch_add(1, Ordering::SeqCst);
            self.current_vector.store(vector as u64, Ordering::SeqCst);

            // 记录统计 (Phase 1 + Phase 3)
            self.stats.record_exception(vector);
            self.detailed_stats.record_exception(vector, frame_ref);
            self.detailed_stats.record_nested(nesting + 1);

            // 分发到对应的 handler (使用 Phase 3 的 trait 系统)
            if vector < 32 {
                // 创建对应的异常处理器 (工厂模式)
                let exception_handler = create_handler(vector);
                
                // 执行处理并获取恢复动作
                let action = exception_handler.handle(frame_ref);
                
                // 记录恢复动作到详细统计
                self.detailed_stats.record_recovery_action(&action);
                
                // 执行恢复动作
                self.execute_recovery_action(&action, frame_ref);
                
                // 如果有 C 兼容的 handler，也调用它（向后兼容）
                if let Some(c_handler) = self.state.lock().handlers[vector as usize] {
                    c_handler(frame);  // C handler 可能会执行额外的副作用
                }
            } else if (vector as usize) >= IRQ_BASE as usize && (vector as usize) < IRQ_BASE as usize + 16 {
                // IRQ 处理
                self.handle_irq(frame, vector);
            } else {
                // 其他向量 (syscall, recovery 等)
                match vector {
                    0x80 => { /* System call - handled by syscall handler */ }
                    0x82 => { /* Recovery interrupt - handled by barrier-stack */ }
                    _ => {}  // 忽略未知向量
                }
            }

            // 恢复嵌套计数
            self.nested_count.fetch_sub(1, Ordering::SeqCst);
            self.current_vector.store(0xFFFFFFFFFFFFFFFF, Ordering::SeqCst);
        }
    }

    /// 执行恢复动作 (Phase 3 结构化错误处理)
    fn execute_recovery_action(&self, action: &RecoveryAction, _frame: &InterruptFrame) {
        match action {
            RecoveryAction::Recovered => {
                // 成功恢复，无需额外操作
            },
            
            RecoveryAction::TerminateProcess(exit_code) => {
                // User-mode 异常：终止进程
                extern "C" { fn process_exit(code: u32); }
                extern "C" { fn scheduler_yield(); }
                
                unsafe {
                    process_exit(*exit_code);
                    scheduler_yield();
                }
            },
            
            RecoveryAction::DomainRecovery => {
                // 尝试域级恢复 (barrier-stack)
                extern "C" { fn recovery_try_recover_from_idt() -> i32; }
                
                unsafe {
                    let result = recovery_try_recover_from_idt();
                    match result {
                        0 => {},  // Recovery 成功
                        -2 => {
                            // 已尝试过，拒绝循环 → panic
                            self.kernel_panic("Recovery already attempted");
                        }
                        _ => {
                            // Recovery 失败 → panic
                            self.kernel_panic("Domain recovery failed");
                        }
                    }
                }
            },
            
            RecoveryAction::Panic(info) => {
                // 无法恢复 → kernel panic
                self.kernel_panic(info.reason);
            },
        }
    }

    /// 默认异常处理 (临时实现，Phase 2.2 完善)
    fn default_exception_handler(&self, frame: &InterruptFrame) {
        let vector = frame.int_no as u8;
        
        // 打印基本信息 (Phase 3 使用结构化日志)
        let _ = (vector, frame);
        
        // TODO: 根据 vector 类型分发到专门的 handler
        match vector {
            0 => self.handle_division_by_zero(frame),
            13 => self.handle_gpf(frame),
            14 => self.handle_page_fault(frame),
            8 => self.handle_double_fault(frame),
            _ => {}  // 其他异常暂不处理
        }
    }

    /// Division By Zero 处理
    fn handle_division_by_zero(&self, frame: &InterruptFrame) {
        if frame.is_user_mode() {
            // User-mode #DE: 终止进程 (安全恢复)
            self.terminate_user_process(frame, 1);
        } else {
            // Kernel-mode #DE: 尝试 domain recovery
            self.attempt_domain_recovery(frame);
        }
    }

    /// Page Fault 处理
    fn handle_page_fault(&self, frame: &InterruptFrame) {
        let fault_addr = unsafe { frame.fault_address() };
        let error_flags = frame.error_code_flags();

        if frame.is_user_mode() {
            if !error_flags.contains(super::types::ErrorFlags::PRESENT) {
                if crate::kernel::proc::user_proc::try_expand_user_stack(fault_addr) {
                    return;
                }
            }
            self.terminate_user_process(frame, 1);
            return;
        }

        // Kernel-mode PF: 尝试恢复
        if is_null_or_invalid(fault_addr) {
            self.attempt_domain_recovery(frame);
            return;
        }

        if is_valid_user_address(frame.rip) && !is_valid_kernel_address(frame.rip) {
            self.attempt_domain_recovery(frame);
            return;
        }

        // 无法恢复: panic
        self.attempt_domain_recovery(frame);
    }

    /// General Protection Fault 处理
    fn handle_gpf(&self, frame: &InterruptFrame) {
        if frame.is_user_mode() {
            self.terminate_user_process(frame, 1);
        } else {
            self.print_stack_trace(frame);
            self.attempt_domain_recovery(frame);
        }
    }

    /// Double Fault 处理
    fn handle_double_fault(&self, frame: &InterruptFrame) {
        static DOUBLE_FAULT_COUNT: AtomicU64 = AtomicU64::new(0);
        let count = DOUBLE_FAULT_COUNT.fetch_add(1, Ordering::SeqCst);

        self.print_stack_trace(frame);

        if count <= 3 {
            // 尝试调度切换恢复
            extern "C" { fn scheduler_yield(); }
            unsafe { scheduler_yield(); }
        } else {
            // 多次 double fault: 系统不稳定
            self.kernel_panic("Multiple double faults - system unstable");
        }
    }

    /// 终止 user 进程
    fn terminate_user_process(&self, _frame: &InterruptFrame, exit_code: u32) {
        extern "C" { fn process_exit(code: u32); }
        extern "C" { fn scheduler_yield(); }
        
        unsafe {
            process_exit(exit_code);
            scheduler_yield();
        }
    }

    /// 尝试域级恢复
    fn attempt_domain_recovery(&self, _frame: &InterruptFrame) {
        extern "C" { fn recovery_try_recover_from_idt() -> i32; }
        
        unsafe {
            let result = recovery_try_recover_from_idt();
            match result {
                0 => {
                    // Recovery 成功
                    return;
                }
                -2 => {
                    // 已尝试过，拒绝循环
                    self.kernel_panic("Recovery already attempted");
                }
                _ => {
                    // Recovery 失败: panic
                    self.kernel_panic("Domain recovery failed");
                }
            }
        }
    }

    /// Kernel panic (停止系统)
    fn kernel_panic(&self, message: &str) {
        let _ = message;
        
        unsafe {
            cli();
            halt_loop();
        }
    }

    /// 打印堆栈回溯
    fn print_stack_trace(&self, frame: &InterruptFrame) {
        let rbp = frame.rbp;
        let mut rbp_ptr = rbp as *const u64;
        let mut frame_count = 0usize;
        const MAX_FRAMES: usize = 10;

        while !rbp_ptr.is_null() && frame_count < MAX_FRAMES {
            unsafe {
                let rip_val = *rbp_ptr.offset(1);
                if rip_val == 0 { break; }

                let mode = if rip_val < 0xFFFFFFFF80000000 && rip_val > 0xFFFF {
                    "user"
                } else {
                    "kernel"
                };

                // TODO: 使用 klog 替代 (Phase 3)
                let _ = (frame_count, rip_val, mode, rbp_ptr);

                rbp_ptr = *rbp_ptr as *const u64;
                frame_count += 1;
            }
        }
    }

    /// 处理 IRQ
    pub fn handle_irq(&self, frame: *mut InterruptFrame, vector: u8) {
        let irq = (vector - IRQ_BASE) as u8;

        if irq < 16 {
            self.stats.record_irq(irq);

            let handler_opt = {
                let state = self.state.lock();
                let handler = state.irq_descriptors[irq as usize].handler;
                if let Some(desc) = state.irq_descriptors.get(irq as usize) {
                    desc.call_count.fetch_add(1, Ordering::Relaxed);
                }
                handler
            };

            if let Some(handler) = handler_opt {
                unsafe { handler(frame); }
            }

            self.send_eoi(irq);
        }
    }

    /// 发送 EOI (End of Interrupt)
    fn send_eoi(&self, irq: u8) {
        // TODO: 通过 IOAPIC 发送 EOI
        // 当前简化: PIC EOI
        unsafe {
            if irq >= 8 {
                port_outb(0xA0, 0x20);
            }
            port_outb(0x20, 0x20);
        }
    }

    /// 导出状态信息 (用于调试)
    pub fn dump_state(&self) {
        let state = self.state.lock();

        // 打印嵌套中断计数
        let nesting = self.nested_count.load(Ordering::Relaxed);
        let current_vec = self.current_vector.load(Ordering::Relaxed);

        // TODO: 使用 klog 输出 (Phase 3)
        let _ = (nesting, current_vec, &state.irq_descriptors);
    }

    /// 获取中断计数
    pub fn get_interrupt_count(&self, vector: u8) -> u64 {
        self.stats.get_count(vector)
    }

    /// 打印统计信息
    pub fn print_statistics(&self) {
        // TODO: 格式化输出统计 (Phase 3)
        let _ = &self.stats;
    }
}
