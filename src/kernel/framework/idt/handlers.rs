//! # 异常处理器实现
//!
//! 提供类型安全的异常处理机制，替代 C 版本的硬编码逻辑。
//!
//! ## 架构设计
//!
//! ```text
//! ExceptionHandler Trait (多态)
//! ├── DivisionByZeroHandler      (#DE)
//! ├── PageFaultHandler           (#PF)
//! ├── GeneralProtectionFaultHandler (#GPF)
//! ├── DoubleFaultHandler         (#DF)
//! └── DefaultHandler             (其他异常)
//!
//! RecoveryAction (结构化错误处理)
//! ├── Recovered                  // 成功恢复
//! ├── TerminateProcess(pid)      // 终止 user 进程
//! ├── DomainRecovery             // 域级恢复
//! └── Panic(info)                // 系统崩溃
//! ```

use core::sync::atomic::{AtomicU64, Ordering};

use super::idt::IdtManager;
use super::types::*;

/// 异常处理结果 (结构化错误处理)
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    /// 成功恢复，可以继续执行
    Recovered,

    /// 需要终止 user-mode 进程
    TerminateProcess(u32),

    /// 需要域级恢复 (barrier-stack)
    DomainRecovery,

    /// 无法恢复，触发 kernel panic
    Panic(PanicInfo),
}

/// Kernel panic 信息
#[derive(Debug, Clone, PartialEq)]
pub struct PanicInfo {
    pub reason: &'static str,
    pub vector: u8,
    pub rip: u64,
}

impl PanicInfo {
    pub fn new(reason: &'static str, vector: u8, rip: u64) -> Self {
        Self {
            reason,
            vector,
            rip,
        }
    }
}

/// 异常严重性分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 信息级别 (可忽略)
    Info,
    /// 警告级别 (需要关注)
    Warning,
    /// 错误级别 (需要恢复)
    Error,
    /// 致命级别 (可能导致系统不稳定)
    Fatal,
    /// 灾难级别 (必须立即停止)
    Catastrophic,
}

/// 异常分类 (用于统计和策略决策)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionCategory {
    /// 数学运算异常 (除零、溢出等)
    Arithmetic,
    /// 内存访问异常 (Page Fault、段不存在等)
    MemoryAccess,
    /// 保护异常 (GPF、段违规等)
    Protection,
    /// 调试相关 (断点、单步等)
    Debug,
    /// 系统内部 (Double Fault 等)
    SystemInternal,
    /// 未知/保留
    Unknown,
}

/// 异常处理器 trait (对象安全)
pub trait ExceptionHandler: Send + Sync {
    /// 处理异常并返回恢复动作
    fn handle(&self, frame: *mut InterruptFrame) -> RecoveryAction;

    /// 获取异常严重性
    fn severity(&self) -> Severity;

    /// 获取异常分类
    fn category(&self) -> ExceptionCategory;

    /// 获取异常名称
    fn name(&self) -> &'static str;
}

// ============================================================================
// 具体异常处理器实现
// ============================================================================

/// Division By Zero 处理器 (#DE, Vector 0)
pub struct DivisionByZeroHandler;

impl ExceptionHandler for DivisionByZeroHandler {
    fn handle(&self, frame: *mut InterruptFrame) -> RecoveryAction {
        if unsafe { (*frame).is_user_mode() } {
            // User-mode #DE: 安全终止进程
            RecoveryAction::TerminateProcess(1)
        } else {
            // Kernel-mode #DE: 尝试域级恢复
            RecoveryAction::DomainRecovery
        }
    }

    fn severity(&self) -> Severity {
        #[cfg(target_arch = "x86_64")]
        if is_currently_user_mode() {
            Severity::Error
        } else {
            Severity::Fatal
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Severity::Error
        }
    }

    fn category(&self) -> ExceptionCategory {
        ExceptionCategory::Arithmetic
    }

    fn name(&self) -> &'static str {
        "Division By Zero"
    }
}

/// Page Fault 处理器 (#PF, Vector 14)
pub struct PageFaultHandler;

impl PageFaultHandler {
    /// 分析 Page Fault 错误码
    pub fn analyze_error_code(error_code: u64) -> PageFaultAnalysis {
        let present = error_code & 0x01 != 0;
        let write = error_code & 0x02 != 0;
        let user = error_code & 0x04 != 0;
        let reserved = error_code & 0x08 != 0;
        let instruction = error_code & 0x10 != 0;

        let access_type = if write {
            AccessType::Write
        } else {
            AccessType::Read
        };
        let mode = if user { Mode::User } else { Mode::Kernel };

        let cause = match (!present, reserved) {
            (true, true) => FaultCause::ReservedBitSet,
            (true, false) => FaultCause::PageNotPresent,
            (false, _) => FaultCause::ProtectionViolation,
        };

        PageFaultAnalysis {
            present,
            access_type,
            mode,
            cause,
            instruction_fetch: instruction,
        }
    }
}

impl ExceptionHandler for PageFaultHandler {
    fn handle(&self, frame: *mut InterruptFrame) -> RecoveryAction {
        let fault_addr = unsafe { (*frame).fault_address() };
        let analysis = Self::analyze_error_code(unsafe { (*frame).err_code });

        if unsafe { (*frame).is_user_mode() } {
            if crate::kernel::framework::proc::user_proc::try_expand_user_stack(fault_addr) {
                return RecoveryAction::Recovered;
            }
            let pid = crate::kernel::framework::proc::api::process_get_current_pid();
            return RecoveryAction::TerminateProcess(pid);
        }

        match analysis.cause {
            FaultCause::PageNotPresent => {
                if fault_addr == 0 || fault_addr < 0x1000 {
                    unsafe {
                        (*frame).rip += 2;
                    }
                    return RecoveryAction::Recovered;
                }

                if fault_addr > 0xFFFF && fault_addr < 0xFFFFFFFF80000000 {
                    unsafe {
                        (*frame).rsp += 8;
                    }
                    return RecoveryAction::Recovered;
                }

                RecoveryAction::DomainRecovery
            }

            FaultCause::ProtectionViolation | FaultCause::ReservedBitSet => {
                RecoveryAction::Panic(PanicInfo::new(
                    "Page Fault: Protection violation or hardware error",
                    14,
                    unsafe { (*frame).rip },
                ))
            }
        }
    }

    fn severity(&self) -> Severity {
        #[cfg(target_arch = "x86_64")]
        if is_currently_user_mode() {
            Severity::Error
        } else {
            Severity::Fatal
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Severity::Error
        }
    }

    fn category(&self) -> ExceptionCategory {
        ExceptionCategory::MemoryAccess
    }

    fn name(&self) -> &'static str {
        "Page Fault"
    }
}

/// Page Fault 分析结果
#[derive(Debug, Clone, Copy)]
pub struct PageFaultAnalysis {
    pub present: bool,
    pub access_type: AccessType,
    pub mode: Mode,
    pub cause: FaultCause,
    pub instruction_fetch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Kernel,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultCause {
    PageNotPresent,
    ProtectionViolation,
    ReservedBitSet,
}

/// General Protection Fault 处理器 (#GPF, Vector 13)
pub struct GeneralProtectionFaultHandler;

impl ExceptionHandler for GeneralProtectionFaultHandler {
    fn handle(&self, frame: *mut InterruptFrame) -> RecoveryAction {
        if unsafe { (*frame).is_user_mode() } {
            // User GPF: 终止进程
            RecoveryAction::TerminateProcess(1)
        } else {
            // Kernel GPF: 打印栈回溯后尝试恢复
            self.print_detailed_gpf_info(unsafe { &*frame });
            RecoveryAction::DomainRecovery
        }
    }

    fn severity(&self) -> Severity {
        #[cfg(target_arch = "x86_64")]
        if is_currently_user_mode() {
            Severity::Error
        } else {
            Severity::Fatal
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Severity::Error
        }
    }

    fn category(&self) -> ExceptionCategory {
        ExceptionCategory::Protection
    }

    fn name(&self) -> &'static str {
        "General Protection Fault"
    }
}

impl GeneralProtectionFaultHandler {
    fn print_detailed_gpf_info(&self, frame: &InterruptFrame) {
        let selector = frame.err_code as u16;

        // 解析错误码位域
        let external = selector & 0x01 != 0;
        let idt_flag = selector & 0x02 != 0;
        let table = (selector >> 1) & 0x03;
        let index = (selector >> 3) & 0x1FFF;

        let table_name = match table {
            0 => "IDT",
            1 => "GDT",
            2 => "LDT",
            3 => "IDT",
            _ => "Unknown",
        };

        // TODO: 使用 klog 输出详细信息 (当前为简化版)
        let _ = (external, idt_flag, table_name, index);
    }
}

/// Double Fault 处理器 (#DF, Vector 8)
pub struct DoubleFaultHandler;

static DOUBLE_FAULT_COUNT: AtomicU64 = AtomicU64::new(0);

impl ExceptionHandler for DoubleFaultHandler {
    fn handle(&self, frame: *mut InterruptFrame) -> RecoveryAction {
        let count = DOUBLE_FAULT_COUNT.fetch_add(1, Ordering::SeqCst);

        self.print_double_fault_context(unsafe { &*frame });

        if count <= 3 {
            // 前 3 次 DF: 尝试调度切换恢复
            RecoveryAction::DomainRecovery
        } else {
            // 多次 DF: 系统严重不稳定
            RecoveryAction::Panic(PanicInfo::new(
                "Multiple Double Faults - system unstable",
                8,
                unsafe { (*frame).rip },
            ))
        }
    }

    fn severity(&self) -> Severity {
        Severity::Catastrophic
    }

    fn category(&self) -> ExceptionCategory {
        ExceptionCategory::SystemInternal
    }

    fn name(&self) -> &'static str {
        "Double Fault"
    }
}

impl DoubleFaultHandler {
    fn print_double_fault_context(&self, _frame: &InterruptFrame) {
        let count = DOUBLE_FAULT_COUNT.load(Ordering::Relaxed);
        let nesting = IdtManager::instance().nested_count.load(Ordering::Relaxed);

        // TODO: 使用 klog 输出上下文信息
        let _ = (count, nesting);
    }
}

/// 默认异常处理器 (用于未专门处理的异常)
pub struct DefaultHandler {
    vector: u8,
}

impl DefaultHandler {
    pub fn new(vector: u8) -> Self {
        Self { vector }
    }
}

impl ExceptionHandler for DefaultHandler {
    fn handle(&self, _frame: *mut InterruptFrame) -> RecoveryAction {
        // 默认行为: 尝试域级恢复
        RecoveryAction::DomainRecovery
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> ExceptionCategory {
        ExceptionCategory::Unknown
    }

    fn name(&self) -> &'static str {
        get_exception_name(self.vector)
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 判断当前是否在 user-mode 执行
#[cfg(target_arch = "x86_64")]
fn is_currently_user_mode() -> bool {
    let cs: u16;
    unsafe { core::arch::asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack)) };
    (cs & 0x03) == 3
}

/// 异常分发器 (工厂模式 - 返回静态引用以避免 Box 分配)
pub fn create_handler(vector: u8) -> &'static dyn ExceptionHandler {
    static DIV_ZERO: DivisionByZeroHandler = DivisionByZeroHandler;
    static PAGE_FAULT: PageFaultHandler = PageFaultHandler;
    static GPF: GeneralProtectionFaultHandler = GeneralProtectionFaultHandler;
    static DOUBLE_FAULT: DoubleFaultHandler = DoubleFaultHandler;

    match vector {
        0 => &DIV_ZERO,
        14 => &PAGE_FAULT,
        13 => &GPF,
        8 => &DOUBLE_FAULT,
        _ => {
            // Default handler 需要动态创建，使用简化版
            &DEFAULT_HANDLER
        }
    }
}

/// 默认处理器实例 (用于未知异常)
static DEFAULT_HANDLER: DefaultHandler = DefaultHandler { vector: 0 };

/// 异常统计收集器
pub struct ExceptionStatisticsCollector {
    pub(crate) total_exceptions: AtomicU64,
    pub(crate) by_category: [AtomicU64; 6],
    by_severity: [AtomicU64; 5],
    recoveries: AtomicU64,
    pub(crate) process_terminations: AtomicU64,
    panics: AtomicU64,
}

impl ExceptionStatisticsCollector {
    pub const fn new() -> Self {
        Self {
            total_exceptions: AtomicU64::new(0),
            by_category: [const { AtomicU64::new(0) }; 6],
            by_severity: [const { AtomicU64::new(0) }; 5],
            recoveries: AtomicU64::new(0),
            process_terminations: AtomicU64::new(0),
            panics: AtomicU64::new(0),
        }
    }

    pub fn total(&self) -> u64 {
        self.total_exceptions.load(Ordering::Relaxed)
    }

    pub fn terminations(&self) -> u64 {
        self.process_terminations.load(Ordering::Relaxed)
    }

    pub fn category_count(&self, idx: usize) -> u64 {
        if idx < 6 {
            self.by_category[idx].load(Ordering::Relaxed)
        } else {
            0
        }
    }

    /// 记录一次异常处理
    pub fn record(&self, handler: &dyn ExceptionHandler, action: &RecoveryAction) {
        self.total_exceptions.fetch_add(1, Ordering::Relaxed);

        // 更新分类统计
        let cat_idx = handler.category() as usize;
        if cat_idx < 6 {
            self.by_category[cat_idx].fetch_add(1, Ordering::Relaxed);
        }

        // 更新严重性统计
        let sev_idx = handler.severity() as usize;
        if sev_idx < 5 {
            self.by_severity[sev_idx].fetch_add(1, Ordering::Relaxed);
        }

        // 更新动作统计
        let _ = match action {
            RecoveryAction::Recovered => self.recoveries.fetch_add(1, Ordering::Relaxed),
            RecoveryAction::TerminateProcess(_) => {
                self.process_terminations.fetch_add(1, Ordering::Relaxed)
            }
            RecoveryAction::DomainRecovery => {
                return;
            } // 不单独计数，提前返回
            RecoveryAction::Panic(_) => self.panics.fetch_add(1, Ordering::Relaxed),
        };
        // 使用 _ 抑制 unused warning
    }

    /// 导出为 JSON 格式 (用于测试框架)
    #[cfg(feature = "json_export")]
    pub fn export_json(&self) -> alloc::string::String {
        use alloc::format;

        format!(
            r#"{{
  "total_exceptions": {},
  "by_category": {{
    "arithmetic": {},
    "memory_access": {},
    "protection": {},
    "debug": {},
    "system_internal": {},
    "unknown": {}
  }},
  "by_severity": {{
    "info": {},
    "warning": {},
    "error": {},
    "fatal": {},
    "catastrophic": {}
  }},
  "actions": {{
    "recoveries": {},
    "process_terminations": {},
    "panics": {}
  }}
}}"#,
            self.total_exceptions.load(Ordering::Relaxed),
            self.by_category[0].load(Ordering::Relaxed), // Arithmetic
            self.by_category[1].load(Ordering::Relaxed), // MemoryAccess
            self.by_category[2].load(Ordering::Relaxed), // Protection
            self.by_category[3].load(Ordering::Relaxed), // Debug
            self.by_category[4].load(Ordering::Relaxed), // SystemInternal
            self.by_category[5].load(Ordering::Relaxed), // Unknown
            self.by_severity[0].load(Ordering::Relaxed), // Info
            self.by_severity[1].load(Ordering::Relaxed), // Warning
            self.by_severity[2].load(Ordering::Relaxed), // Error
            self.by_severity[3].load(Ordering::Relaxed), // Fatal
            self.by_severity[4].load(Ordering::Relaxed), // Catastrophic
            self.recoveries.load(Ordering::Relaxed),
            self.process_terminations.load(Ordering::Relaxed),
            self.panics.load(Ordering::Relaxed),
        )
    }
}

/// 全局统计收集器实例
static COLLECTOR: ExceptionStatisticsCollector = ExceptionStatisticsCollector::new();

/// 获取全局统计收集器
pub fn get_collector() -> &'static ExceptionStatisticsCollector {
    &COLLECTOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_action_variants() {
        assert_eq!(RecoveryAction::Recovered, RecoveryAction::Recovered);
        assert_ne!(
            RecoveryAction::TerminateProcess(1),
            RecoveryAction::Recovered
        );
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        assert!(Severity::Error < Severity::Fatal);
        assert!(Severity::Fatal < Severity::Catastrophic);
    }

    #[test]
    fn test_exception_categories() {
        let handler = DivisionByZeroHandler;
        assert_eq!(handler.category(), ExceptionCategory::Arithmetic);
        assert_eq!(handler.name(), "Division By Zero");
    }

    #[test]
    fn test_page_fault_analysis() {
        let analysis = PageFaultHandler::analyze_error_code(0x05); // Read + Not Present + User
        assert!(!analysis.present);
        assert_eq!(analysis.access_type, AccessType::Read);
        assert_eq!(analysis.mode, Mode::User);
        assert_eq!(analysis.cause, FaultCause::PageNotPresent);
    }

    #[test]
    fn test_default_handler() {
        let handler = DefaultHandler::new(7); // Device Not Available
        assert_eq!(handler.name(), "Device Not Available");
        assert_eq!(handler.severity(), Severity::Warning);
    }

    #[test]
    fn test_factory_pattern() {
        let handler0 = create_handler(0); // Division By Zero
        let handler13 = create_handler(13); // GPF
        let handler99 = create_handler(99); // Unknown

        assert_eq!(handler0.name(), "Division By Zero");
        assert_eq!(handler13.name(), "General Protection Fault");
        assert_eq!(handler99.name(), "Unknown");
    }

    #[test]
    fn test_statistics_collector() {
        let collector = ExceptionStatisticsCollector::new();

        let handler = DivisionByZeroHandler;
        let action = RecoveryAction::TerminateProcess(42);

        collector.record(&handler, &action);

        assert_eq!(collector.total_exceptions.load(Ordering::Relaxed), 1);
        assert_eq!(collector.process_terminations.load(Ordering::Relaxed), 1);
        assert_eq!(collector.by_category[0].load(Ordering::Relaxed), 1); // Arithmetic
    }

    #[test]
    fn test_panic_info_creation() {
        let info = PanicInfo::new("Test panic", 14, 0xDEADBEEF);
        assert_eq!(info.vector, 14);
        assert_eq!(info.rip, 0xDEADBEEF);
        assert_eq!(info.reason, "Test panic");
    }
}

#[cfg(feature = "kernel_test")]
pub fn register_idt_handlers_tests() {
    crate::kernel::framework::tests::idt::register_idt_handlers_tests();
}
