//! Model Specific Register (MSR) 操作
//!
//! 提供 x86-64 MSR 读写的安全封装。
//!
//! ## 安全警告
//!
//! MSR 操作是特权指令，错误的值可能导致系统崩溃或硬件损坏。
//! 此模块的所有函数都是 `unsafe` 的。

use core::arch::asm;

/// 常见 MSR 地址 (枚举替代 magic number)
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum MsrAddress {
    /// IA32_TSC_AUX (用于 rdtscp)
    TscAux = 0xC0000103,
    /// IA32_STAR (syscall 目标)
    Star = 0xC0000080,
    /// IA32_LSTAR (syscall 64-bit 入口)
    LStar = 0xC0000082,
    /// IA32_FMASK (syscall 标志掩码)
    FMask = 0xC0000084,
    /// IA32_FS_BASE
    FsBase = 0xC0000100,
    /// IA32_GS_BASE
    GsBase = 0xC0000101,
    /// IA32_EFER (Extended Feature Enable Register)
    Efer = 0xC0000080,
}

/// 读取 MSR
///
/// # Safety
///
/// - `addr` 必须是有效的 MSR 地址
/// - 调用者必须在 Ring 0
#[inline(always)]
pub unsafe fn rdmsr(addr: MsrAddress) -> u64 {
    let (low, high): (u32, u32);
    asm!(
        "rdmsr",
        in("ecx") addr as u32,
        out("eax") low,
        out("edx") high,
        options(nostack, nomem, preserves_flags),
    );
    ((high as u64) << 32) | (low as u64)
}

/// 写入 MSR
///
/// # Safety
///
/// - `addr` 必须有效
/// - `value` 必须是该 MSR 的合法值
/// - 仅可在初始化阶段调用
#[inline(always)]
pub unsafe fn wrmsr(addr: MsrAddress, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    asm!(
        "wrmsr",
        in("ecx") addr as u32,
        in("eax") low,
        in("edx") high,
        options(nostack, nomem, preserves_flags),
    );
}
