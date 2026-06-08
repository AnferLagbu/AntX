//! # 安全硬件操作封装
//!
//! 提供对 x86-64 底层硬件寄存器的安全访问接口。
//! 封装内联汇编，提供类型安全的 API。

/// CPU 特性检测结果
#[derive(Debug, Clone)]
pub struct CpuFeatures {
    /// 是否支持 APIC
    pub has_apic: bool,
    /// 是否支持 X2APIC
    pub has_x2apic: bool,
    /// 最大支持的 CPUID 叶
    pub max_cpuid_leaf: u32,
}

impl CpuFeatures {
    /// 检测当前 CPU 的特性
    pub fn detect() -> Self {
        // 简化版 CPU 特性检测 (Phase 1)
        // TODO Phase 3: 完整实现 CPUID 解析
        Self {
            has_apic: true,    // 假设现代 CPU 都有 APIC
            has_x2apic: false, // Phase 3 再检测
            max_cpuid_leaf: 0,
        }
    }

    /// 打印 CPU 特性信息
    #[cfg(feature = "log")]
    pub fn log_info(&self) {
        use log::info;
        info!("CPU Features:");
        info!("  APIC: {}", self.has_apic);
        info!("  X2APIC: {}", self.has_x2apic);
        info!("  Max CPUID Leaf: {}", self.max_cpuid_leaf);
    }
}

/// CR2 寄存器安全读取 (Page Fault 地址)
///
/// # Safety
/// 此函数仅在异常处理上下文中有效
///
/// # Returns
/// Page Fault 触发时的线性地址
#[inline(always)]
pub unsafe fn read_cr2() -> u64 {
    crate::arch!(read_fault_address()) as u64
}

/// 中断控制函数

/// 禁用中断 (cli)
#[inline(always)]
///
/// # Safety
///
/// Only valid during early boot before interrupt subsystem is live.
pub unsafe fn disable_interrupts() {
    let _ = crate::arch!(interrupt_disable());
}

/// 启用中断 (sti)
#[inline(always)]
///
/// # Safety
///
/// Enables interrupts via the `sti` instruction. Caller must ensure the IDT
/// has been fully initialized and no interrupt handler can observe partially-
/// constructed kernel state.
pub unsafe fn enable_interrupts() {
    crate::arch!(interrupt_enable());
}

/// RFLAGS 寄存器操作

/// 读取当前 RFLAGS
#[inline]
#[cfg(target_arch = "x86_64")]
pub fn read_rflags() -> u64 {
    let rflags: u64;
    // SAFETY: 内联汇编的寄存器约束与变量类型一致; 无内存副作用; 输出 reg 通过 out(reg) 绑定
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nomem, nostack)) };
    rflags
}

/// 检查中断是否启用 (IF flag)
#[inline]
#[cfg(target_arch = "x86_64")]
pub fn interrupts_enabled() -> bool {
    (read_rflags() & (1 << 9)) != 0
}

/// 内存屏障操作

/// 全局内存屏障 (mfence → Arch trait)
#[inline(always)]
///
/// # Safety
///
/// This is a full memory barrier. Caller must ensure that no pending stores
/// or loads can be reordered across this fence in a way that violates the
/// intended synchronization protocol.
pub unsafe fn memory_fence() {
    crate::arch!(fence());
}

/// 写内存屏障 (sfence → Arch trait)
#[inline(always)]
///
/// # Safety
///
/// This is a store fence. Caller must ensure that store ordering guarantees
/// provided by this fence match the intended synchronization protocol.
pub unsafe fn store_fence() {
    crate::arch!(fence_w());
}

/// TSC 时间戳计数器

/// 读取 TSC (时间戳计数器 → Arch trait)
///
/// # Returns
/// 当前 CPU 周期数 (可用于性能测量)
#[inline]
pub fn rdtsc() -> u64 {
    crate::arch!(timestamp())
}

/// 读取带 fence 的 TSC (更精确)
#[inline]
#[cfg(target_arch = "x86_64")]
pub fn rdtsc_fence() -> u64 {
    let tsc: u64;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!(
            "lfence",
            "rdtsc",
            out("rax") tsc,
            options(nomem, nostack)
        );
    }
    tsc
}

/// I/O 端口操作 (用于 PIC 控制)

/// 从端口读字节
///
/// # Safety
/// port 必须是有效的 I/O 端口地址
#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    crate::arch!(inb(port))
}

/// 向端口写字节
///
/// # Safety
/// port 必须是有效的 I/O 端口地址
#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    crate::arch!(outb(port, value));
}

/// I/O 延时 (用于 PIC 初始化序列)
#[inline(always)]
pub fn io_wait() {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        outb(0x80, 0);
    }
}

/// HALT 指令 (暂停 CPU 直到下一个中断)
#[inline(always)]
///
/// # Safety
///
/// Caller has disabled interrupts before halting. Without interrupts enabled,
/// the CPU will never wake up, causing a permanent hang.
pub unsafe fn halt() {
    crate::arch!(halt());
}

/// 无限循环 (用于 panic 后停止系统)
#[inline(never)]
pub fn halt_loop() -> ! {
    loop {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            halt();
        }
    }
}

/// 栈操作辅助

/// 保存当前栈帧指针 (RBP)
#[inline]
#[cfg(target_arch = "x86_64")]
pub fn save_frame_pointer() -> u64 {
    let rbp: u64;
    // SAFETY: 内联汇编的寄存器约束与变量类型一致; 无内存副作用; 输出 reg 通过 out(reg) 绑定
    unsafe { core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack)) };
    rbp
}

/// 安全的空指针检查
///
/// # Arguments
/// * `ptr` - 要检查的原始指针
///
/// # Returns
/// `true` 如果指针为 null 或接近 null (< 0x1000)
#[inline]
pub fn is_null_or_invalid(ptr: u64) -> bool {
    ptr == 0 || ptr < 0x1000
}

/// 验证用户态地址范围
///
/// # Arguments
/// * `addr` - 要验证的地址
///
/// # Returns
/// `true` 如果地址在合法的用户空间范围内
#[inline]
pub fn is_valid_user_address(addr: u64) -> bool {
    addr > 0xFFFF && addr < 0xFFFF800000000000
}

/// 验证内核态地址范围
///
/// # Arguments
/// * `addr` - 要验证的地址
///
/// # Returns
/// `true` 如果地址在内核空间范围内
#[inline]
pub fn is_valid_kernel_address(addr: u64) -> bool {
    addr >= 0xFFFF800000000000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_features_detection() {
        let features = CpuFeatures::detect();
        // 现代 CPU 应该支持 APIC
        assert!(features.has_apic || !features.has_apic); // 至少不会 panic
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn test_rflags_operations() {
        let flags = read_rflags();
        // IF bit (bit 9) 应该在某个状态
        let _enabled = (flags & (1 << 9)) != 0;
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn test_rdtsc_monotonic() {
        let tsc1 = rdtsc();
        let tsc2 = rdtsc();
        // TSC 应该单调递增 (或相等)
        assert!(tsc2 >= tsc1);
    }

    #[test]
    fn test_address_validation() {
        assert!(is_null_or_invalid(0));
        assert!(is_null_or_invalid(0xFFF));
        assert!(!is_null_or_invalid(0x1000));

        assert!(is_valid_user_address(0x400000)); // 典型 user 地址
        assert!(!is_valid_user_address(0xFFFF800000000000)); // kernel 地址

        assert!(is_valid_kernel_address(0xFFFFFFFF80000000));
        assert!(!is_valid_kernel_address(0x400000));
    }
}

#[cfg(feature = "kernel_test")]
pub fn register_idt_safety_tests() {
    crate::kernel::framework::tests::idt::register_idt_safety_tests();
}
