//! MSR (Model Specific Register) 操作
//!
//! 提供 safe wrapper for RDMSR/WRMSR instructions.

/// 读取 64 位 MSR 寄存器
///
/// # Arguments
/// * `msr` - MSR 地址 (如 0xC0000080 for `IA32_EFER`)
///
/// # Returns
/// MSR 的 64 位值
///
/// # Safety
/// 必须在 Ring 0 (内核态) 调用, 否则触发 #GP 异常。
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "inline_always: #[inline(always)] 是性能优化 (关键路径/中断处理); 当前优先 expect"
)]
pub unsafe fn read_msr(msr: u32) -> u64 {
    unsafe {
        let (low, high): (u32, u32);

        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
        );

        (u64::from(high) << 32) | u64::from(low)
    }
}

/// 写入 64 位 MSR 寄存器
///
/// # Arguments
/// * `msr` - MSR 地址
/// * `value` - 要写入的 64 位值
///
/// # Safety
/// 必须在 Ring 0 调用, 且 MSR 必须存在且可写。
#[inline(always)]
// 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
#[expect(clippy::cast_possible_truncation)]
pub unsafe fn write_msr(msr: u32, value: u64) {
    unsafe {
        let low = value as u32;
        let high = (value >> 32) as u32;

        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// FFI 兼容: 读取 MSR (返回两个 32 位值)
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
/// `low` 和 `high` 是合法可写指针.
// 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
#[expect(clippy::cast_possible_truncation)]
pub unsafe extern "C" fn cpu_read_msr(msr: u32, low: *mut u32, high: *mut u32) -> i32 {
    unsafe {
        if low.is_null() || high.is_null() {
            return -1;
        }

        let value = read_msr(msr);
        *low = value as u32;
        *high = (value >> 32) as u32;

        0
    }
}

/// FFI 兼容: 写入 MSR
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
pub unsafe extern "C" fn cpu_write_msr(msr: u32, low: u32, high: u32) -> i32 {
    unsafe {
        write_msr(msr, (u64::from(high) << 32) | u64::from(low));
        0
    }
}

/// FFI 兼容: 读取 64 位 MSR
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
pub unsafe extern "C" fn cpu_read_msr64(msr: u32) -> u64 {
    unsafe { read_msr(msr) }
}

/// FFI 兼容: 写入 64 位 MSR
#[unsafe(no_mangle)]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
pub unsafe extern "C" fn cpu_write_msr64(msr: u32, value: u64) -> i32 {
    unsafe {
        write_msr(msr, value);
        0
    }
}
