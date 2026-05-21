//! 架构相关模块
//!
//! ## 架构抽象层 (Architecture Abstraction Layer)
//!
//! 定义了 `Arch` trait 作为所有 CPU 架构的统一接口。
//! 通过 `CurrentArch` 类型别名和 `arch!` 宏实现编译期架构分发。
//!
//! ```text
//! arch/
//! ├── mod.rs         # Arch trait 定义 + CurrentArch + arch! 宏
//! ├── x86_64/
//! │   └── mod.rs     # X8664 结构体 (x86_64 实现)
//! └── aarch64/
//!     └── mod.rs     # Aarch64 结构体 (ARM64 stub)
//! ```
//!
//! ## Phase 1 状态
//! - [x] Arch trait 定义 (完整方法签名)
//! - [x] CurrentArch 类型别名 (编译期架构选择)
//! - [x] arch! 宏 (编译期零开销分发)
//! - [ ] X8664 完整实现 (Phase 2)
//! - [ ] Aarch64 完整实现 (Phase 3+)
//!
//! ## 安全说明
//!
//! Arch trait 方法标记为 `unsafe` 因为它们直接操作硬件:
//! - MMIO/PMIO 操作 (端口读写)
//! - 特权指令 (cli/sti, 写 CR3, invlpg)
//! - 跨地址空间操作 (context_switch)
//!
//! 调用方必须确保:
//! - 中断保存/恢复成对调用
//! - 页表基地址是有效的物理地址
//! - context_switch 在正确的上下文中调用

// ============================================================================
// 模块声明
// ============================================================================

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

// ============================================================================
// Arch Trait 定义 (Phase 1 核心产出)
// ============================================================================

/// 架构抽象 trait — 所有 CPU 架构的统一接口。
///
/// 方法分类:
/// - **中断控制**: `interrupt_disable/restore/enable/is_enabled`
/// - **MMU 操作**: `tlb_flush_page/all`, `read/write_page_table_base`, `read_fault_address`
/// - **上下文切换**: `context_switch`, `enter_user`, `return_to_user`
/// - **CPU 信息**: `cpu_id`, `timestamp`
/// - **内存屏障**: `fence`, `fence_w`
/// - **核间中断**: `send_ipi`, `broadcast_ipi`
/// - **端口 I/O**: `outb/inb/outl/inl` (x86 特有)
/// - **系统控制**: `shutdown`, `reboot`, `halt`
///
/// # Safety
///
/// 实现此 trait 需要对目标架构的特权指令和内存模型有深入理解。
pub trait Arch {
    // --- 中断控制 ---

    /// 禁用中断并返回之前的中断状态标志 (cli / msr daifset)。
    fn interrupt_disable() -> usize;
    /// 恢复之前保存的中断状态 (写 RFLAGS / msr daif)。
    fn interrupt_restore(flags: usize);
    /// 启用中断 (sti / msr daifclr)。
    fn interrupt_enable();
    /// 检查中断是否已启用。
    fn is_interrupt_enabled() -> bool;

    // --- CPU 控制 ---

    /// 暂停 CPU 直到下一次中断 (hlt / wfi)。
    fn halt();

    // --- 内存管理单元 (MMU) ---

    /// 刷新单个虚拟地址的 TLB 条目 (invlpg / tlbi vaae1)。
    fn tlb_flush_page(vaddr: usize);
    /// 刷新整个 TLB (写 CR3 / tlbi vmalle1)。
    fn tlb_flush_all();
    /// 读取当前页表基地址 (mov cr3 / mrs TTBR0_EL1)。
    fn read_page_table_base() -> u64;
    /// 写入页表基地址 (mov to cr3 / msr TTBR0_EL1)。
    fn write_page_table_base(paddr: u64);
    /// 读取触发页错误的地址 (mov cr2 / mrs FAR_EL1)。
    fn read_fault_address() -> usize;

    // --- 上下文切换 ---

    /// 保存当前上下文到 `from`，从 `to` 恢复上下文。
    ///
    /// # Safety
    ///
    /// `from` 和 `to` 必须指向有效的 ProcessContext 内存。
    fn context_switch(from: *mut u8, to: *const u8);

    // --- 用户态切换 ---

    /// 进入用户态执行 (sysret / eret)。
    ///
    /// 此函数不会返回 — 执行流之后将进入用户态入口点。
    fn enter_user(entry: usize, stack: usize, arg: usize) -> !;
    /// 从内核态返回到用户态 (iretq / eret)。
    fn return_to_user();

    // --- CPU 信息 ---

    /// 获取当前 CPU ID (APIC ID / MPIDR_EL1)。
    fn cpu_id() -> u32;
    /// 获取高精度时间戳 (rdtsc / mrs CNTVCT_EL0)。
    fn timestamp() -> u64;

    // --- 内存屏障 ---

    /// 全内存屏障 (mfence / dsb sy)。
    fn fence();
    /// 写内存屏障 (sfence / dmb st)。
    fn fence_w();

    // --- 核间中断 (IPI) ---

    /// 向目标 CPU 发送核间中断。
    fn send_ipi(target_cpu: u32, vector: u8);
    /// 向所有 CPU (不含自身) 广播 IPI。
    fn broadcast_ipi(vector: u8);

    // --- 端口 I/O (x86 特有，其他架构 stub) ---

    /// 向 I/O 端口写入字节 (out dx, al)。
    fn outb(port: u16, value: u8);
    /// 从 I/O 端口读取字节 (in al, dx)。
    fn inb(port: u16) -> u8;
    /// 向 I/O 端口写入双字 (out dx, eax)。
    fn outl(port: u16, value: u32);
    /// 从 I/O 端口读取双字 (in eax, dx)。
    fn inl(port: u16) -> u32;

    // --- 系统控制 ---

    /// 关机 (ACPI / PSCI SYSTEM_OFF)。
    fn shutdown() -> !;
    /// 重启 (8042 / PSCI SYSTEM_RESET)。
    fn reboot() -> !;
}

// ============================================================================
// CurrentArch 类型别名 (编译期架构选择)
// ============================================================================

#[cfg(target_arch = "x86_64")]
/// 当前编译目标的架构类型 — x86_64。
pub type CurrentArch = x86_64::X8664;

#[cfg(target_arch = "aarch64")]
/// 当前编译目标的架构类型 — AArch64。
pub type CurrentArch = aarch64::Aarch64;

// ============================================================================
// arch! 宏 — 编译期零开销架构分发
// ============================================================================

/// 编译期架构分发宏。
///
/// 展开为对 `<CurrentArch as Arch>` 的 trait 方法调用，零运行时开销。
///
/// # 用法
///
/// ```ignore
/// // 无参数调用
/// arch!(tlb_flush_all());
///
/// // 带参数调用
/// arch!(tlb_flush_page(addr));
/// arch!(outb(0x3F8, b'H'));
///
/// // 带返回值
/// let ts = arch!(timestamp());
/// ```
///
/// # 展开示例
///
/// `arch!(tlb_flush_all())` 展开为:
/// `<CurrentArch as Arch>::tlb_flush_all()`
#[macro_export]
macro_rules! arch {
    ($method:ident ( $($arg:expr),* $(,)? )) => {
        <$crate::kernel::arch::CurrentArch as $crate::kernel::arch::Arch>::$method($($arg),*)
    };
    ($method:ident ()) => {
        <$crate::kernel::arch::CurrentArch as $crate::kernel::arch::Arch>::$method()
    };
}